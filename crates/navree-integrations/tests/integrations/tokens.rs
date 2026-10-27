//! Tokens: cached while fresh, refreshed once under a lock, refusals turn a
//! connection unhealthy.

use std::time::Duration;

use navree_integrations::{
    ConnectCallback, ConnectOptions, ConnectionRestored, ConnectionStatus, ConnectionUnhealthy,
    FakeGrant, IntegrationsError, Owner,
};

use crate::support::{Harness, harness};

/// A HubSpot connection whose token lasts `expires_in` seconds.
async fn hubspot(h: &Harness, expires_in: i64) -> uuid::Uuid {
    h.fake.on_code(
        "hubspot",
        "code",
        FakeGrant::new()
            .access_token("first")
            .refresh_token("refresh")
            .expires_in(expires_in),
    );
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "hubspot", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    integrations
        .complete(Owner::App, "hubspot", &ConnectCallback::code(state, "code"), None)
        .await
        .unwrap()
        .connection
        .id
}

#[tokio::test]
async fn concurrent_requests_refresh_a_connection_once() {
    let h = harness(|b| b).await;
    let id = hubspot(&h, 600).await;
    h.advance(Duration::from_secs(600));
    let integrations = h.integrations().await;
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let integrations = integrations.clone();
        tasks.push(tokio::spawn(async move {
            integrations.token(id).await.unwrap().expose().clone()
        }));
    }
    let mut tokens = Vec::new();
    for task in tasks {
        tokens.push(task.await.unwrap());
    }
    tokens.dedup();
    assert_eq!(tokens.len(), 1, "{tokens:?}");
    assert_ne!(tokens[0], "first");
    assert_eq!(h.fake.minted().len(), 1);
}

#[tokio::test]
async fn a_token_is_refreshed_a_minute_before_it_expires() {
    let h = harness(|b| b).await;
    let id = hubspot(&h, 3600).await;
    let integrations = h.integrations().await;
    h.advance(Duration::from_secs(3600 - 61));
    assert_eq!(integrations.token(id).await.unwrap().expose(), "first");
    h.advance(Duration::from_secs(2));
    assert_ne!(integrations.token(id).await.unwrap().expose(), "first");
    assert_eq!(h.fake.minted().len(), 1);
}

#[tokio::test]
async fn a_refused_refresh_turns_the_connection_unhealthy_until_it_works_again() {
    let h = harness(|b| b).await;
    let id = hubspot(&h, 60).await;
    let integrations = h.integrations().await;
    h.fake.refuse_tokens("hubspot", "invalid_grant");
    h.advance(Duration::from_secs(60));

    let error = integrations.token(id).await.unwrap_err();
    assert!(matches!(error, IntegrationsError::Reauthorize { .. }), "{error:?}");
    assert_eq!(error.to_string(), "HubSpot needs to be connected again");
    let connection = integrations.find(id).await.unwrap();
    assert_eq!(connection.status, ConnectionStatus::Unhealthy);
    assert_eq!(connection.failures, 1);
    assert_eq!(connection.last_error.as_deref(), Some("invalid_grant"));
    // Once, on the change.
    assert!(integrations.token(id).await.is_err());
    let unhealthy = h.inbox.of::<ConnectionUnhealthy>();
    assert_eq!(unhealthy.len(), 1);
    assert_eq!(unhealthy[0].connection_id, id);
    assert_eq!(unhealthy[0].error, "invalid_grant");
    assert_eq!(unhealthy[0].provider, "hubspot");

    h.fake.restore_tokens("hubspot");
    integrations.token(id).await.unwrap();
    let connection = integrations.find(id).await.unwrap();
    assert_eq!(connection.status, ConnectionStatus::Healthy);
    assert_eq!(connection.failures, 0);
    assert_eq!(connection.last_error, None);
    assert_eq!(h.inbox.of::<ConnectionRestored>().len(), 1);
}

#[tokio::test]
async fn an_unreachable_provider_is_not_a_refusal() {
    let h = harness(|b| b).await;
    let id = hubspot(&h, 60).await;
    let integrations = h.integrations().await;
    h.fake.fail_tokens("hubspot");
    h.advance(Duration::from_secs(60));
    let error = integrations.token(id).await.unwrap_err();
    assert!(matches!(error, IntegrationsError::Unavailable { .. }), "{error:?}");
    assert_eq!(
        integrations.find(id).await.unwrap().status,
        ConnectionStatus::Healthy
    );
    assert!(h.inbox.of::<ConnectionUnhealthy>().is_empty());
}

#[tokio::test]
async fn app_only_tokens_are_minted_again_when_they_expire() {
    let h = harness(|b| b).await;
    let connection = h
        .connect_microsoft(Owner::App, "9188040d-6c67-4c5b-b112-36a304b66dad")
        .await;
    let integrations = h.integrations().await;
    let first = integrations.token(connection.id).await.unwrap();
    integrations.token(connection.id).await.unwrap();
    assert_eq!(h.fake.minted().len(), 1);
    h.advance(Duration::from_secs(3600));
    let second = integrations.token(connection.id).await.unwrap();
    assert_ne!(first.expose(), second.expose());
    assert_eq!(h.fake.minted().len(), 2);
}

#[tokio::test]
async fn an_expired_token_without_a_refresh_token_needs_a_new_connection() {
    let h = harness(|b| b).await;
    h.fake.on_code(
        "hubspot",
        "short",
        FakeGrant::new().access_token("only").expires_in(60),
    );
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "hubspot", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    let id = integrations
        .complete(Owner::App, "hubspot", &ConnectCallback::code(state, "short"), None)
        .await
        .unwrap()
        .connection
        .id;
    h.advance(Duration::from_secs(120));
    assert!(matches!(
        integrations.token(id).await,
        Err(IntegrationsError::Reauthorize { .. })
    ));
    assert!(h.fake.minted().is_empty());
}
