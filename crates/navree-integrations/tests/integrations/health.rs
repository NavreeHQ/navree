//! Health checks: `Integrations::check`, `check_due`, the commands and the
//! schedule.

use std::time::Duration;

use http::Method;
use navree_http_client::FakeResponse;
use navree_integrations::{
    ConnectCallback, ConnectOptions, ConnectionRestored, ConnectionStatus, ConnectionUnhealthy,
    FakeGrant, Owner,
};
use serde_json::json;

use crate::support::{Harness, harness, start};

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

async fn slack(h: &Harness) -> uuid::Uuid {
    h.fake.on_code(
        "slack",
        "code",
        FakeGrant::new()
            .access_token("xoxb-1")
            .never_expires()
            .external_id("T0123ABCD"),
    );
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "slack", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    integrations
        .complete(
            Owner::App,
            "slack",
            &ConnectCallback::code(state, "code"),
            None,
        )
        .await
        .unwrap()
        .connection
        .id
}

#[tokio::test]
async fn a_check_mints_a_new_token_and_records_the_result() {
    let h = harness(|b| b).await;
    let connection = h.connect_microsoft(Owner::App, TENANT).await;
    let integrations = h.integrations().await;
    integrations.token(connection.id).await.unwrap();

    let health = integrations.check(connection.id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Healthy);
    assert_eq!(health.failures, 0);
    assert_eq!(health.error, None);
    assert_eq!(health.checked_at, start());
    // A check always asks the provider, even with a fresh token cached.
    assert_eq!(h.fake.minted().len(), 2);
    let stored = integrations.find(connection.id).await.unwrap();
    assert_eq!(stored.last_checked_at, Some(start()));
}

#[tokio::test]
async fn a_refusal_is_unhealthy_at_once_and_a_success_restores_it() {
    let h = harness(|b| b).await;
    let connection = h.connect_microsoft(Owner::App, TENANT).await;
    let integrations = h.integrations().await;
    h.fake.refuse_tokens("microsoft", "unauthorized_client");
    let health = integrations.check(connection.id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Unhealthy);
    assert_eq!(health.failures, 1);
    assert_eq!(health.error.as_deref(), Some("unauthorized_client"));
    let events = h.inbox.of::<ConnectionUnhealthy>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].owner, Owner::App);
    assert_eq!(events[0].scope_set, "default");

    // Still failing: no second event.
    integrations.check(connection.id).await.unwrap();
    assert_eq!(h.inbox.of::<ConnectionUnhealthy>().len(), 1);
    assert_eq!(integrations.find(connection.id).await.unwrap().failures, 2);

    h.fake.restore_tokens("microsoft");
    let health = integrations.check(connection.id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Healthy);
    assert_eq!(health.failures, 0);
    assert_eq!(h.inbox.of::<ConnectionRestored>().len(), 1);
}

#[tokio::test]
async fn an_unreachable_provider_is_unhealthy_after_the_threshold() {
    let h = harness(|b| b.config("integrations.unhealthy_after", 3)).await;
    let connection = h.connect_microsoft(Owner::App, TENANT).await;
    let integrations = h.integrations().await;
    h.fake.fail_tokens("microsoft");
    for failures in 1..=2 {
        let health = integrations.check(connection.id).await.unwrap();
        assert_eq!(health.status, ConnectionStatus::Healthy);
        assert_eq!(health.failures, failures);
        assert_eq!(health.error.as_deref(), Some("connection refused (fake)"));
    }
    assert!(h.inbox.of::<ConnectionUnhealthy>().is_empty());
    let health = integrations.check(connection.id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Unhealthy);
    assert_eq!(h.inbox.of::<ConnectionUnhealthy>()[0].failures, 3);
}

#[tokio::test]
async fn slack_connections_are_probed_with_auth_test() {
    let h = harness(|b| b).await;
    let id = slack(&h).await;
    let integrations = h.integrations().await;
    h.fake.api().on_sequence(
        Method::POST,
        "https://slack.com/api/auth.test",
        [
            FakeResponse::new(200).json(&json!({ "ok": true, "team_id": "T0123ABCD" })),
            FakeResponse::new(200).json(&json!({ "ok": false, "error": "invalid_auth" })),
        ],
    );
    assert_eq!(
        integrations.check(id).await.unwrap().status,
        ConnectionStatus::Healthy
    );
    h.fake
        .api()
        .assert_sent(|request| request.header("authorization") == Some("Bearer xoxb-1"));
    let health = integrations.check(id).await.unwrap();
    assert_eq!(health.status, ConnectionStatus::Unhealthy);
    assert_eq!(health.error.as_deref(), Some("invalid_auth"));
    // A bot token without rotation is not minted again by checks.
    assert!(h.fake.minted().is_empty());
}

#[tokio::test]
async fn due_connections_are_checked_once_per_interval() {
    let h = harness(|b| b.config("integrations.check_interval_mins", 60)).await;
    let first = h.connect_microsoft(Owner::App, TENANT).await;
    let integrations = h.integrations().await;

    // Never checked: due.
    let summary = integrations.check_due().await.unwrap();
    assert_eq!(
        (summary.checked, summary.healthy, summary.unhealthy),
        (1, 1, 0)
    );
    h.advance(Duration::from_secs(30 * 60));
    assert_eq!(integrations.check_due().await.unwrap().checked, 0);
    h.advance(Duration::from_secs(31 * 60));
    assert_eq!(integrations.check_due().await.unwrap().checked, 1);
    // `check_all` checks everything.
    assert_eq!(integrations.check_all().await.unwrap().checked, 1);
    assert_eq!(
        integrations.find(first.id).await.unwrap().last_checked_at,
        Some(start() + chrono::TimeDelta::seconds(61 * 60))
    );
}

#[tokio::test]
async fn an_interval_of_zero_turns_scheduled_checks_off() {
    let h = harness(|b| b.config("integrations.check_interval_mins", 0)).await;
    h.connect_microsoft(Owner::App, TENANT).await;
    let integrations = h.integrations().await;
    assert_eq!(integrations.check_due().await.unwrap().checked, 0);
    assert_eq!(integrations.check_all().await.unwrap().checked, 1);
}

#[tokio::test]
async fn the_commands_check_and_list_connections() {
    let h = harness(|b| b).await;
    let connection = h.connect_microsoft(Owner::App, TENANT).await;
    h.app
        .run_command("integrations:check", &["--due"])
        .await
        .unwrap();
    assert!(
        h.integrations()
            .await
            .find(connection.id)
            .await
            .unwrap()
            .last_checked_at
            .is_some()
    );
    let id = connection.id.to_string();
    h.app
        .run_command("integrations:check", &["--id", &id])
        .await
        .unwrap();
    h.app.run_command("integrations:check", &[]).await.unwrap();
    h.app.run_command("integrations:list", &[]).await.unwrap();
    assert!(
        h.app
            .run_command("integrations:check", &["--bogus"])
            .await
            .is_err()
    );
    assert!(
        h.app
            .run_command("integrations:check", &["--id", "nope"])
            .await
            .is_err()
    );
    assert_eq!(h.fake.minted().len(), 3);
}

#[tokio::test]
async fn deleting_a_workspace_forgets_its_connections() {
    let h = harness(crate::support::with_workspaces).await;
    let agra = h.workspace(crate::support::OTHER, "agra").await;
    h.connect_microsoft(Owner::Workspace(agra.id), TENANT).await;
    let workspaces = h.app.get::<navree_workspace::Workspaces>().await;
    let uow = h.app.get::<navree_db::Db>().await.uow(None, None).await;
    workspaces.delete(&uow, agra.id).await.unwrap();
    uow.commit().await.unwrap();
    assert!(h.integrations().await.all().await.unwrap().is_empty());
}
