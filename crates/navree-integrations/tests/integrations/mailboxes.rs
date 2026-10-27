//! Feature `mail`: Microsoft 365 and Gmail mailboxes of connections watched
//! for new mail, each message received through navree-mail's `Inbound`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use http::Method;
use navree_http_client::FakeResponse;
use navree_integrations::{
    IntegrationsError, IntegrationsOptions, MailboxWatches, MailboxesOptions, NewApiKey,
    NewServiceAccount, Owner,
};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{InboundMail, Incoming, MailModule, MailboxContext, ModuleBuilderMailExt};
use navree_testing::TestAppBuilder;
use navree_view::{ViewModule, ViewOptions};
use serde_json::{Value, json};

use crate::support::{Harness, harness, service_account};

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";
pub(crate) const GRAPH: &str = "https://graph.microsoft.com/v1.0";
pub(crate) const GMAIL: &str = "https://gmail.googleapis.com/gmail/v1";
const NOTIFICATION_URL: &str = "https://app.acme.test/api/mail/inbound/graph";

pub(crate) type Seen = Arc<Mutex<Vec<InboundMail>>>;

/// A catch-all mailbox that records what it receives.
struct Catcher(Seen);

impl Module for Catcher {
    fn register(&self, m: &mut ModuleBuilder) {
        let seen = self.0.clone();
        m.mailbox("*", move |mail: InboundMail, _ctx: MailboxContext| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(mail);
                Ok(())
            }
        });
    }
}

pub(crate) fn with_mail(seen: &Seen) -> impl FnOnce(TestAppBuilder) -> TestAppBuilder {
    let seen = seen.clone();
    move |b| {
        b.config(
            "integrations.mailboxes.graph_notification_url",
            NOTIFICATION_URL,
        )
        .config(
            "integrations.mailboxes.gmail_topic",
            "projects/acme-prod/topics/gmail-inbound",
        )
        .config("integrations.mailboxes.gmail_token", "push-secret")
        .config("mail.from", "app@acme.test")
        .module(ViewModule::for_root(ViewOptions {
            dirs: Vec::new(),
            reload: false,
        }))
        .module(MailModule::from_config())
        .module(Catcher(seen))
    }
}

pub(crate) fn message(subject: &str) -> Vec<u8> {
    Incoming::compose()
        .from("Ada Lovelace <ada@example.com>")
        .to("support@acme.test")
        .subject(subject)
        .text("Hello from outside.")
        .to_bytes()
        .to_vec()
}

pub(crate) async fn watches(h: &Harness) -> Arc<MailboxWatches> {
    h.app.get::<MailboxWatches>().await
}

/// A Microsoft 365 connection and a watched `support@acme.test`
/// (subscription `sub-1`).
pub(crate) async fn graph_watch(h: &Harness) -> navree_integrations::MailboxWatch {
    h.fake.api().on(
        Method::POST,
        &format!("{GRAPH}/subscriptions"),
        FakeResponse::new(201).json(&json!({
            "id": "sub-1",
            "expirationDateTime": "2026-10-30T06:00:00Z",
        })),
    );
    let connection = h.connect_microsoft(Owner::App, TENANT).await;
    watches(h)
        .await
        .watch(connection.id, "Support@Acme.test")
        .await
        .unwrap()
}

#[tokio::test]
async fn a_microsoft_mailbox_is_watched_with_a_graph_subscription() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;

    let watch = graph_watch(&h).await;

    assert_eq!(watch.provider, "microsoft");
    assert_eq!(watch.mailbox, "support@acme.test");
    assert_eq!(watch.subscription_id.as_deref(), Some("sub-1"));
    assert_eq!(watch.status, "active");
    assert_eq!(
        watch.expires_at.map(|at| at.to_rfc3339()).as_deref(),
        Some("2026-10-30T06:00:00+00:00")
    );
    let client_state = watches(&h).await.client_state(watch.id).unwrap();
    h.fake.api().assert_sent(|request| {
        let body: Value = request.json().unwrap_or(Value::Null);
        request.method() == Method::POST
            && request.url().as_str() == format!("{GRAPH}/subscriptions")
            && body["changeType"] == "created"
            && body["notificationUrl"] == NOTIFICATION_URL
            && body["lifecycleNotificationUrl"] == NOTIFICATION_URL
            && body["resource"] == "users/support@acme.test/mailFolders('inbox')/messages"
            && body["clientState"] == client_state.as_str()
    });
    assert_eq!(watches(&h).await.all().await.unwrap(), vec![watch.clone()]);
    assert_eq!(
        watches(&h).await.find(watch.id).await.unwrap().mailbox,
        "support@acme.test"
    );
}

#[tokio::test]
async fn graph_notifications_deliver_new_messages() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = graph_watch(&h).await;
    h.fake.api().on(
        Method::GET,
        &format!("{GRAPH}/users/support@acme.test/messages/AAMk1/$value"),
        FakeResponse::new(200).body(message("Printer on fire")),
    );
    let client_state = watches(&h).await.client_state(watch.id).unwrap();

    h.app
        .client()
        .post("/api/mail/inbound/graph")
        .json(&json!({ "value": [{
            "subscriptionId": "sub-1",
            "clientState": client_state,
            "changeType": "created",
            "resource": "Users/7f3c/Messages/AAMk1",
            "resourceData": { "@odata.type": "#Microsoft.Graph.Message", "id": "AAMk1" },
            "tenantId": TENANT,
        }]}))
        .send()
        .await
        .assert_status(202);

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].source, "graph");
    assert_eq!(seen[0].provider_id.as_deref(), Some("AAMk1"));
    assert_eq!(seen[0].subject.as_deref(), Some("Printer on fire"));
    assert_eq!(seen[0].envelope.recipients, vec!["support@acme.test"]);
    let watch = watches(&h).await.find(watch.id).await.unwrap();
    assert!(watch.last_received_at.is_some());
}

#[tokio::test]
async fn graph_notifications_with_a_wrong_client_state_are_ignored() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    graph_watch(&h).await;

    for client_state in ["forged", ""] {
        h.app
            .client()
            .post("/api/mail/inbound/graph")
            .json(&json!({ "value": [{
                "subscriptionId": "sub-1",
                "clientState": client_state,
                "changeType": "created",
                "resourceData": { "id": "AAMk1" },
            }]}))
            .send()
            .await
            .assert_status(202);
    }
    h.app
        .client()
        .post("/api/mail/inbound/graph")
        .json(&json!({ "value": [{ "subscriptionId": "unknown", "clientState": "x" }] }))
        .send()
        .await
        .assert_status(202);

    assert!(seen.lock().unwrap().is_empty());
    h.fake.api().assert_not_sent(|request| request.url().path().ends_with("$value"));
}

#[tokio::test]
async fn graph_validation_tokens_are_echoed() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;

    let response = h
        .app
        .client()
        .post("/api/mail/inbound/graph?validationToken=Validation%3A%20Testing%20client")
        .send()
        .await
        .assert_status(200);

    assert_eq!(response.text(), "Validation: Testing client");
    assert!(
        response
            .header("content-type")
            .unwrap()
            .starts_with("text/plain")
    );
}

#[tokio::test]
async fn graph_lifecycle_notifications_renew_and_flag_watches() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = graph_watch(&h).await;
    h.fake.api().on(
        Method::PATCH,
        &format!("{GRAPH}/subscriptions/sub-1"),
        FakeResponse::new(200).json(&json!({ "expirationDateTime": "2026-10-31T00:00:00Z" })),
    );
    let client_state = watches(&h).await.client_state(watch.id).unwrap();
    let lifecycle = |event: &str| {
        json!({ "value": [{
            "subscriptionId": "sub-1",
            "clientState": client_state,
            "lifecycleEvent": event,
        }]})
    };

    h.app
        .client()
        .post("/api/mail/inbound/graph")
        .json(&lifecycle("reauthorizationRequired"))
        .send()
        .await
        .assert_status(202);
    let renewed = watches(&h).await.find(watch.id).await.unwrap();
    assert_eq!(
        renewed.expires_at.map(|at| at.to_rfc3339()).as_deref(),
        Some("2026-10-31T00:00:00+00:00")
    );

    h.app
        .client()
        .post("/api/mail/inbound/graph")
        .json(&lifecycle("subscriptionRemoved"))
        .send()
        .await
        .assert_status(202);
    let removed = watches(&h).await.find(watch.id).await.unwrap();
    assert_eq!(removed.status, "failing");
    assert_eq!(removed.subscription_id, None);
}

/// A Google service-account connection (domain-wide delegation) and a
/// watched `ada@acme.test` with cursor `1000`.
pub(crate) async fn gmail_watch(h: &Harness) -> navree_integrations::MailboxWatch {
    h.fake.api().on(
        Method::POST,
        &format!("{GMAIL}/users/ada@acme.test/watch"),
        FakeResponse::new(200).json(&json!({
            "historyId": "1000",
            "expiration": "1793433600000",
        })),
    );
    let integrations = h.integrations().await;
    let connection = integrations
        .connect_service_account(
            Owner::App,
            "google",
            NewServiceAccount::new(service_account(), Some("admin@acme.test".into())),
        )
        .await
        .unwrap();
    watches(h)
        .await
        .watch(connection.id, "ada@acme.test")
        .await
        .unwrap()
}

pub(crate) fn push(mailbox: &str, history: &str) -> Value {
    let data = json!({ "emailAddress": mailbox, "historyId": history }).to_string();
    json!({
        "message": {
            "data": STANDARD.encode(data),
            "messageId": "2070443601311540",
            "publishTime": "2026-10-27T08:00:00Z",
        },
        "subscription": "projects/acme-prod/subscriptions/gmail-inbound-push",
    })
}

#[tokio::test]
async fn gmail_mailboxes_are_watched_and_pushes_read_the_history() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = gmail_watch(&h).await;
    assert_eq!(watch.provider, "google");
    assert_eq!(watch.cursor.as_deref(), Some("1000"));
    assert_eq!(
        watch.expires_at.map(|at| at.timestamp_millis()),
        Some(1_793_433_600_000)
    );
    h.fake.api().assert_sent(|request| {
        let body: Value = request.json().unwrap_or(Value::Null);
        request.url().as_str() == format!("{GMAIL}/users/ada@acme.test/watch")
            && body["topicName"] == "projects/acme-prod/topics/gmail-inbound"
            && body["labelIds"] == json!(["INBOX"])
    });
    assert!(
        h.fake
            .minted()
            .iter()
            .any(|minted| minted.subject.as_deref() == Some("ada@acme.test")),
        "the service account acts as the mailbox's user"
    );

    h.fake.api().on(
        Method::GET,
        &format!("{GMAIL}/users/ada@acme.test/history*"),
        FakeResponse::new(200).json(&json!({
            "history": [
                { "id": "1001", "messagesAdded": [{ "message": { "id": "m1", "labelIds": ["INBOX"] } }] },
                { "id": "1003", "messagesAdded": [{ "message": { "id": "m2" } }, { "message": { "id": "m1" } }] },
            ],
            "historyId": "1005",
        })),
    );
    for (id, subject) in [("m1", "First"), ("m2", "Second")] {
        h.fake.api().on(
            Method::GET,
            &format!("{GMAIL}/users/ada@acme.test/messages/{id}*"),
            FakeResponse::new(200)
                .json(&json!({ "id": id, "raw": URL_SAFE_NO_PAD.encode(message(subject)) })),
        );
    }

    h.app
        .client()
        .post("/api/mail/inbound/gmail?token=push-secret")
        .json(&push("Ada@acme.test", "1005"))
        .send()
        .await
        .assert_status(204);

    let seen = seen.lock().unwrap().clone();
    let subjects: Vec<_> = seen.iter().map(|m| m.subject.clone().unwrap()).collect();
    assert_eq!(subjects, vec!["First", "Second"]);
    assert_eq!(seen[0].source, "gmail");
    assert_eq!(seen[0].provider_id.as_deref(), Some("m1"));
    assert_eq!(seen[0].envelope.recipients, vec!["ada@acme.test"]);
    h.fake.api().assert_sent(|request| {
        request.url().path() == "/gmail/v1/users/ada@acme.test/history"
            && request
                .url()
                .query_pairs()
                .any(|(k, v)| k == "startHistoryId" && v == "1000")
    });
    h.fake.api().assert_sent(|request| {
        request.url().path() == "/gmail/v1/users/ada@acme.test/messages/m1"
            && request
                .url()
                .query_pairs()
                .any(|(k, v)| k == "format" && v == "raw")
    });
    let watch = watches(&h).await.find(watch.id).await.unwrap();
    assert_eq!(watch.cursor.as_deref(), Some("1005"));
}

#[tokio::test]
async fn gmail_pushes_need_the_token() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    gmail_watch(&h).await;

    for path in [
        "/api/mail/inbound/gmail",
        "/api/mail/inbound/gmail?token=wrong",
    ] {
        h.app
            .client()
            .post(path)
            .json(&push("ada@acme.test", "1005"))
            .send()
            .await
            .assert_error_code(401, "invalid_signature");
    }
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_cursor_gmail_forgot_restarts_from_the_latest_history() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = gmail_watch(&h).await;
    h.fake.api().on(
        Method::GET,
        &format!("{GMAIL}/users/ada@acme.test/history*"),
        FakeResponse::new(404).json(&json!({ "error": { "code": 404 } })),
    );

    h.app
        .client()
        .post("/api/mail/inbound/gmail?token=push-secret")
        .json(&push("ada@acme.test", "2000"))
        .send()
        .await
        .assert_status(204);

    let watch = watches(&h).await.find(watch.id).await.unwrap();
    assert_eq!(watch.cursor.as_deref(), Some("2000"));
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn sync_catches_up_on_a_graph_mailbox() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = graph_watch(&h).await;
    h.fake.api().on(
        Method::GET,
        &format!("{GRAPH}/users/support@acme.test/mailFolders/inbox/messages*"),
        FakeResponse::new(200).json(&json!({ "value": [{ "id": "AAMk7" }] })),
    );
    h.fake.api().on(
        Method::GET,
        &format!("{GRAPH}/users/support@acme.test/messages/AAMk7/$value"),
        FakeResponse::new(200).body(message("Missed one")),
    );

    let received = watches(&h).await.sync(watch.id).await.unwrap();

    assert_eq!(received.len(), 1);
    assert_eq!(
        seen.lock().unwrap()[0].subject.as_deref(),
        Some("Missed one")
    );
}

#[tokio::test]
async fn expiring_watches_are_renewed_and_failures_flagged() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = graph_watch(&h).await;
    let watches = watches(&h).await;

    assert_eq!(
        watches.renew_due().await.unwrap(),
        navree_integrations::RenewSummary::default(),
        "nothing is due yet"
    );

    h.advance(Duration::from_secs(48 * 3600));
    h.fake.api().on_sequence(
        Method::PATCH,
        &format!("{GRAPH}/subscriptions/sub-1"),
        [
            FakeResponse::new(200)
                .json(&json!({ "expirationDateTime": "2026-11-01T00:00:00Z" })),
            FakeResponse::new(404).json(&json!({ "error": { "code": "ResourceNotFound" } })),
        ],
    );
    let summary = watches.renew_due().await.unwrap();
    assert_eq!(summary.renewed, 1);
    h.fake.api().assert_sent(|request| {
        let body: Value = request.json().unwrap_or(Value::Null);
        request.method() == Method::PATCH && body["expirationDateTime"].is_string()
    });

    h.advance(Duration::from_secs(5 * 24 * 3600));
    let summary = watches.renew_due().await.unwrap();
    assert_eq!(summary.failed, 1);
    let failed = watches.find(watch.id).await.unwrap();
    assert_eq!(failed.status, "failing");
    assert!(failed.last_error.is_some());
}

#[tokio::test]
async fn unwatching_deletes_the_subscription() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = graph_watch(&h).await;
    h.fake.api().on(
        Method::DELETE,
        &format!("{GRAPH}/subscriptions/sub-1"),
        FakeResponse::new(204),
    );

    watches(&h).await.unwatch(watch.id).await.unwrap();

    h.fake.api().assert_sent(|request| {
        request.method() == Method::DELETE
            && request.url().as_str() == format!("{GRAPH}/subscriptions/sub-1")
    });
    assert!(matches!(
        watches(&h).await.find(watch.id).await,
        Err(IntegrationsError::MailboxWatchNotFound)
    ));
}

#[tokio::test]
async fn watching_again_renews_the_existing_watch() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = graph_watch(&h).await;
    h.fake.api().on(
        Method::PATCH,
        &format!("{GRAPH}/subscriptions/sub-1"),
        FakeResponse::new(200).json(&json!({ "expirationDateTime": "2026-10-31T00:00:00Z" })),
    );

    let again = watches(&h)
        .await
        .watch(watch.connection_id, "support@acme.test")
        .await
        .unwrap();

    assert_eq!(again.id, watch.id);
    assert_eq!(watches(&h).await.all().await.unwrap().len(), 1);
}

#[tokio::test]
async fn only_microsoft_and_google_mailboxes_can_be_watched() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    h.fake.api().on(
        Method::GET,
        "https://acme.edr.example/api/v1/health",
        FakeResponse::new(200).json(&json!({ "status": "ok" })),
    );
    let connection = h
        .integrations()
        .await
        .connect_api_key(
            Owner::App,
            "edr",
            NewApiKey::new("edr-key").api_url("https://acme.edr.example/api"),
        )
        .await
        .unwrap();

    let error = watches(&h)
        .await
        .watch(connection.id, "soc@acme.test")
        .await
        .unwrap_err();

    assert!(
        matches!(error, IntegrationsError::Unsupported { .. }),
        "{error}"
    );
}

#[tokio::test]
async fn the_command_lists_and_renews_watches() {
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    graph_watch(&h).await;

    h.app
        .run_command("integrations:mailboxes", &[])
        .await
        .unwrap();
    h.app
        .run_command("integrations:mailboxes", &["--renew"])
        .await
        .unwrap();
    assert!(
        h.app
            .run_command("integrations:mailboxes", &["--bogus"])
            .await
            .is_err()
    );
}

#[test]
fn mailbox_options_are_checked() {
    let options = IntegrationsOptions {
        mailboxes: MailboxesOptions {
            graph_notification_url: Some("not a url".into()),
            gmail_topic: Some("gmail-inbound".into()),
            renew_before_hours: 0,
            ..MailboxesOptions::default()
        },
        ..IntegrationsOptions::default()
    };

    let errors = options.check().unwrap_err();

    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors[0].contains("graph_notification_url"));
    assert!(errors[1].contains("gmail_topic"));
    assert!(errors[2].contains("renew_before_hours"));
}
