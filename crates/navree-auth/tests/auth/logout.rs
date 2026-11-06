//! `POST /api/auth/logout`.

use std::sync::Arc;

use navree_contracts::AuditSink;
use serde_json::json;

use crate::support::{self, PASSWORD, RecordingSink, login, refresh, signed_in, tokens};

#[tokio::test]
async fn logout_revokes_refresh_token() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::builder()
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (user, access, token) = signed_in(&app, "anu@example.com").await;
    let client = app.client();
    let other = tokens(
        &login(&client, "anu@example.com", PASSWORD)
            .await
            .assert_status(200),
    )
    .1;

    let response = client
        .post("/api/auth/logout")
        .bearer(&access)
        .json(&json!({ "refresh_token": token }))
        .send()
        .await
        .assert_status(204);
    assert!(response.bytes().is_empty());

    refresh(&client, &token)
        .await
        .assert_error(401, "Invalid refresh token");
    // Only that session ends.
    refresh(&client, &other).await.assert_status(200);

    let entries = sink.with_action("auth.logout");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor_id, Some(user.id));
}

#[tokio::test]
async fn logout_all_revokes_every_session() {
    let app = support::app().await;
    let (_, access, first) = signed_in(&app, "anu@example.com").await;
    let client = app.client();
    let second = tokens(
        &login(&client, "anu@example.com", PASSWORD)
            .await
            .assert_status(200),
    )
    .1;
    // Someone else's session survives.
    let (_, _, grace) = signed_in(&app, "gauri@example.com").await;

    client
        .post("/api/auth/logout")
        .bearer(&access)
        .json(&json!({ "all": true }))
        .send()
        .await
        .assert_status(204);

    refresh(&client, &first).await.assert_status(401);
    refresh(&client, &second).await.assert_status(401);
    refresh(&client, &grace).await.assert_status(200);
}

#[tokio::test]
async fn logout_without_a_body_succeeds() {
    let app = support::app().await;
    let (_, access, token) = signed_in(&app, "anu@example.com").await;
    let client = app.client();
    client
        .post("/api/auth/logout")
        .bearer(&access)
        .send()
        .await
        .assert_status(204);
    // Nothing named, nothing revoked: the refresh token still works.
    refresh(&client, &token).await.assert_status(200);
}

#[tokio::test]
async fn logout_needs_a_bearer_token_and_leaves_other_users_tokens_alone() {
    let app = support::app().await;
    let (_, _, ada_token) = signed_in(&app, "anu@example.com").await;
    let (_, grace_access, _) = signed_in(&app, "gauri@example.com").await;
    let client = app.client();
    client
        .post("/api/auth/logout")
        .json(&json!({ "refresh_token": ada_token }))
        .send()
        .await
        .assert_error(401, None);
    // Gauri cannot end Anu's session.
    client
        .post("/api/auth/logout")
        .bearer(&grace_access)
        .json(&json!({ "refresh_token": ada_token }))
        .send()
        .await
        .assert_status(204);
    refresh(&client, &ada_token).await.assert_status(200);
}

/// Records `end_user_subscriptions` calls.
#[derive(Default)]
struct RecordingBroadcaster(std::sync::Mutex<Vec<uuid::Uuid>>);

impl navree_contracts::Broadcaster for RecordingBroadcaster {
    fn broadcast(
        &self,
        _channels: Vec<String>,
        _event: String,
        _data: serde_json::Value,
    ) -> navree_contracts::BoxFuture<'_, Result<(), navree_contracts::BoxError>> {
        Box::pin(async { Ok(()) })
    }

    fn end_user_subscriptions(
        &self,
        user_id: uuid::Uuid,
    ) -> navree_contracts::BoxFuture<'_, Result<(), navree_contracts::BoxError>> {
        self.0.lock().unwrap().push(user_id);
        Box::pin(async { Ok(()) })
    }
}

/// Signing out ends the user's real-time subscriptions, so channels
/// authorized for the session stop.
#[tokio::test]
async fn logout_ends_the_users_realtime_subscriptions() {
    let broadcaster = Arc::new(RecordingBroadcaster::default());
    let app = support::builder()
        .override_value::<dyn navree_contracts::Broadcaster>(broadcaster.clone())
        .build()
        .await;
    let (user, access, token) = signed_in(&app, "anu@example.com").await;
    assert!(broadcaster.0.lock().unwrap().is_empty());
    app.client()
        .post("/api/auth/logout")
        .bearer(&access)
        .json(&json!({ "refresh_token": token }))
        .send()
        .await
        .assert_status(204);
    assert_eq!(*broadcaster.0.lock().unwrap(), [user.id]);
}
