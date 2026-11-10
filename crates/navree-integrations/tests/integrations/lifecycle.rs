//! The connection lifecycle on whichever server the suite runs on
//! (`NAVREE_TEST_DATABASE`, see `servers`), so the migration and every
//! statement of the `IntegrationConnection` model run on each: the upsert of
//! a reconnect, the owner chain's `OR` groups, token and health updates, the
//! `IS NULL OR <` lookup of due checks, deletes, and timestamps that come
//! back as they were written. With the `mail` feature, the same for the
//! `MailboxWatch` model.

use std::time::Duration;

use chrono::TimeDelta;
use navree_db::Db;
use navree_integrations::{
    ConnectCallback, ConnectOptions, ConnectionStatus, CredentialKind, FakeGrant,
    IntegrationConnection, Owner,
};
use navree_orm::Model as _;
use uuid::Uuid;

use crate::support::{Harness, OTHER, harness, start, with_workspaces};

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

/// Connects `owner` to HubSpot with a token that lasts ten minutes.
async fn connect_hubspot(h: &Harness, owner: Owner, code: &str, token: &str) -> Uuid {
    h.fake.on_code(
        "hubspot",
        code,
        FakeGrant::new()
            .access_token(token)
            .refresh_token("refresh-secret")
            .expires_in(600)
            .external_id("hub-portal-1"),
    );
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(owner, "hubspot", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    integrations
        .complete(owner, "hubspot", &ConnectCallback::code(state, code), None)
        .await
        .unwrap()
        .connection
        .id
}

/// Connect, inherit, reconnect, refresh, check, list and disconnect.
#[tokio::test]
async fn the_connection_lifecycle() {
    let h = harness(|b| with_workspaces(b).config("integrations.check_interval_mins", 60)).await;
    let db = h.app.get::<Db>().await;
    let integrations = h.integrations().await;

    let agra = h.workspace(OTHER, "agra").await;
    let group = h.organization(OTHER, "agra-group", &[agra.id]).await;
    let workspace = Owner::Workspace(agra.id);
    let organization = Owner::Organization(group);

    // The organization's admin consent; the workspace's own OAuth install.
    let microsoft = h.connect_microsoft(organization, TENANT).await;
    assert_eq!(microsoft.kind, CredentialKind::AppOnly);
    assert_eq!(microsoft.external_id.as_deref(), Some(TENANT));
    assert_eq!(microsoft.owner(), organization);
    assert_eq!(
        (microsoft.created_at, microsoft.updated_at),
        (start(), start())
    );
    let hubspot = connect_hubspot(&h, workspace, "code-1", "first").await;

    // The workspace sees its own connection, then its organization's.
    let visible = integrations.connections(workspace).await.unwrap();
    let names: Vec<(&str, Owner)> = visible
        .iter()
        .map(|info| (info.provider.as_str(), info.owner()))
        .collect();
    assert_eq!(names, [("hubspot", workspace), ("microsoft", organization)]);
    let inherited = integrations
        .connection(workspace, "microsoft", "default")
        .await
        .unwrap();
    assert_eq!(inherited.id, microsoft.id);
    assert_eq!(
        integrations.connections(organization).await.unwrap().len(),
        1
    );

    // Connecting again replaces the row in place.
    h.advance(Duration::from_secs(60));
    let again = connect_hubspot(&h, workspace, "code-2", "second").await;
    assert_eq!(again, hubspot);
    let row = integrations.find(hubspot).await.unwrap();
    assert_eq!(row.created_at, start());
    assert_eq!(row.updated_at, start() + TimeDelta::seconds(60));
    assert_eq!(
        integrations.token(hubspot).await.unwrap().expose(),
        "second"
    );

    // The credentials and token are stored encrypted.
    let stored = IntegrationConnection::find(&*db, hubspot)
        .await
        .unwrap()
        .unwrap();
    assert!(!stored.credentials.contains("refresh-secret"));
    assert!(!stored.access_token.as_deref().unwrap().contains("second"));
    assert_eq!(
        stored.token_expires_at,
        Some(start() + TimeDelta::seconds(660))
    );

    // An expired token is refreshed once and stored.
    h.advance(Duration::from_secs(600));
    let refreshed = integrations.token(hubspot).await.unwrap().expose().clone();
    assert_ne!(refreshed, "second");
    assert_eq!(
        integrations.token(hubspot).await.unwrap().expose(),
        &refreshed
    );
    assert_eq!(h.fake.minted().len(), 1);

    // Checks: a refusal turns a connection unhealthy, a success restores it.
    h.fake.refuse_tokens("microsoft", "unauthorized_client");
    let health = integrations.check(microsoft.id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Unhealthy);
    let unhealthy = IntegrationConnection::query()
        .where_(IntegrationConnection::STATUS.eq("unhealthy"))
        .pluck(&*db, IntegrationConnection::ID)
        .await
        .unwrap();
    assert_eq!(unhealthy, [microsoft.id]);
    h.fake.restore_tokens("microsoft");
    let health = integrations.check(microsoft.id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Healthy);
    let checked_at = start() + TimeDelta::seconds(660);
    assert_eq!(
        integrations
            .find(microsoft.id)
            .await
            .unwrap()
            .last_checked_at,
        Some(checked_at)
    );

    // Due checks: HubSpot was never checked; Microsoft is due an hour on.
    let summary = integrations.check_due().await.unwrap();
    assert_eq!((summary.checked, summary.healthy), (1, 1));
    assert_eq!(integrations.check_due().await.unwrap().checked, 0);
    h.advance(Duration::from_secs(61 * 60));
    assert_eq!(integrations.check_due().await.unwrap().checked, 2);

    // Every connection, by owner type, owner, provider and scope set.
    let all = integrations.all().await.unwrap();
    assert_eq!(
        all.iter().map(|info| info.id).collect::<Vec<_>>(),
        [microsoft.id, hubspot]
    );

    integrations.disconnect(hubspot).await.unwrap();
    assert!(integrations.find(hubspot).await.is_err());
    assert_eq!(integrations.forget(organization).await.unwrap(), 1);
    assert_eq!(IntegrationConnection::query().count(&*db).await.unwrap(), 0);
}

/// Feature `mail`: the mailbox watches' migration and statements: insert, the lookup by Graph subscription id (a nullable
/// column), cursor and expiry updates, the lookup by provider and mailbox,
/// and deletes.
#[tokio::test]
async fn the_mailbox_watch_lifecycle() {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use http::Method;
    use navree_http_client::FakeResponse;
    use navree_integrations::MailboxWatch;
    use serde_json::json;

    use crate::mailboxes::{
        GMAIL, GRAPH, Seen, gmail_watch, graph_watch, message, push, watches, with_mail,
    };

    let seen = Seen::default();
    let h = harness(|b| with_mail(&seen)(b)).await;
    let db = h.app.get::<Db>().await;
    let watches = watches(&h).await;

    // Microsoft 365: a subscription, a notification, a renewal.
    let graph = graph_watch(&h).await;
    h.fake.api().on(
        Method::GET,
        &format!("{GRAPH}/users/support@acme.test/messages/AAMk1/$value"),
        FakeResponse::new(200).body(message("On a real server")),
    );
    let received = watches
        .graph_notifications(&json!({ "value": [{
            "subscriptionId": "sub-1",
            "clientState": watches.client_state(graph.id).unwrap(),
            "changeType": "created",
            "resourceData": { "id": "AAMk1" },
        }]}))
        .await
        .unwrap();
    assert_eq!(received.len(), 1);
    let stored = watches.find(graph.id).await.unwrap();
    assert_eq!(stored.created_at, start());
    assert_eq!(stored.last_received_at, Some(start()));
    assert_eq!(stored.subscription_id.as_deref(), Some("sub-1"));

    // Gmail: the cursor moves.
    let gmail = gmail_watch(&h).await;
    h.fake.api().on(
        Method::GET,
        &format!("{GMAIL}/users/anu@acme.test/history*"),
        FakeResponse::new(200).json(&json!({
            "history": [{ "messagesAdded": [{ "message": { "id": "m1" } }] }],
            "historyId": "1005",
        })),
    );
    h.fake.api().on(
        Method::GET,
        &format!("{GMAIL}/users/anu@acme.test/messages/m1*"),
        FakeResponse::new(200)
            .json(&json!({ "id": "m1", "raw": URL_SAFE_NO_PAD.encode(message("Gmail")) })),
    );
    assert_eq!(
        watches
            .gmail_push(&push("anu@acme.test", "1005"))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        watches.find(gmail.id).await.unwrap().cursor.as_deref(),
        Some("1005")
    );
    assert_eq!(seen.lock().unwrap().len(), 2);

    // One row per watched mailbox; unwatching deletes them.
    assert_eq!(MailboxWatch::query().count(&*db).await.unwrap(), 2);
    h.fake.api().on(
        Method::DELETE,
        &format!("{GRAPH}/subscriptions/sub-1"),
        FakeResponse::new(204),
    );
    watches.unwatch(graph.id).await.unwrap();
    watches.unwatch(gmail.id).await.unwrap();
    assert_eq!(MailboxWatch::query().count(&*db).await.unwrap(), 0);
}
