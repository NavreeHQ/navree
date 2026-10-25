//! `POST /api/auth/refresh`: rotation, reuse detection, expiry, storage.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use keel_auth::refresh_token;
use keel_contracts::{AuditKind, AuditSink, Clock, FixedClock};
use keel_crypto::sha256_hex;
use keel_db::Db;
use sea_orm::{ConnectionTrait, EntityTrait, Statement};
use serde_json::json;

use crate::support::{self, RecordingSink, refresh, signed_in, tokens};

#[tokio::test]
async fn refresh_rotates_and_old_token_stops_working() {
    let app = support::app().await;
    let (user, _, first) = signed_in(&app, "ada@example.com").await;
    let client = app.client();

    let response = refresh(&client, &first)
        .await
        .assert_status(200)
        .assert_json_path("token_type", json!("Bearer"))
        .assert_json_path("expires_in", json!(900));
    let (access, second) = tokens(&response);
    assert_ne!(second, first);
    client
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id));

    refresh(&client, &first)
        .await
        .assert_error(401, "Invalid refresh token");

    // The rotation is recorded: the old row points at its successor.
    let db = app.get::<Db>().await;
    let rows = refresh_token::Entity::find().all(db.conn()).await.unwrap();
    assert_eq!(rows.len(), 2);
    let old = rows
        .iter()
        .find(|row| first.starts_with(&row.id.to_string()))
        .unwrap();
    let new = rows
        .iter()
        .find(|row| second.starts_with(&row.id.to_string()))
        .unwrap();
    assert_eq!(old.replaced_by, Some(new.id));
    assert!(old.revoked_at.is_some());
    assert_eq!(old.family_id, new.family_id, "one session, one family");
    assert_eq!(new.user_id, user.id);
}

#[tokio::test]
async fn reused_refresh_token_revokes_family() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::builder()
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (user, _, first) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    // Another session of the same user (another device).
    let other = tokens(
        &support::login(&client, "ada@example.com", support::PASSWORD)
            .await
            .assert_status(200),
    )
    .1;

    let second = tokens(&refresh(&client, &first).await.assert_status(200)).1;
    // An attacker replays the stolen first token.
    refresh(&client, &first)
        .await
        .assert_error(401, "Invalid refresh token");
    // The legitimate holder's newer token is revoked too.
    refresh(&client, &second)
        .await
        .assert_error(401, "Invalid refresh token");
    // The other session is untouched.
    refresh(&client, &other).await.assert_status(200);

    let reuse = sink.with_action("auth.refresh_reuse_detected");
    assert_eq!(reuse.len(), 1, "{:?}", sink.entries());
    assert_eq!(reuse[0].kind, AuditKind::Security);
    assert_eq!(reuse[0].actor_id, Some(user.id));
    assert_eq!(sink.with_action("auth.refresh").len(), 2);
}

#[tokio::test]
async fn invalid_or_expired_refresh_tokens_are_401() {
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = support::builder()
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let (_, _, token) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    for garbage in [
        "nonsense",
        "not-a-uuid.abc",
        &format!("{}.wrong", uuid::Uuid::now_v7()),
    ] {
        refresh(&client, garbage)
            .await
            .assert_error(401, "Invalid refresh token");
    }
    // Right id, wrong secret: rejected without revoking the real token.
    let (id, _) = token.split_once('.').unwrap();
    refresh(&client, &format!("{id}.{}", "x".repeat(48)))
        .await
        .assert_error(401, "Invalid refresh token");
    client
        .post("/api/auth/refresh")
        .json(&json!({}))
        .send()
        .await
        .assert_validation_error("refresh_token");

    clock.advance(Duration::from_secs(2_592_000 + 1));
    refresh(&client, &token)
        .await
        .assert_error(401, "Invalid refresh token");
}

#[tokio::test]
async fn refresh_token_hash_only_at_rest() {
    let app = support::app().await;
    let (_, _, token) = signed_in(&app, "ada@example.com").await;
    let db = app.get::<Db>().await;
    let rows = refresh_token::Entity::find().all(db.conn()).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].token_hash, sha256_hex(token.as_bytes()));
    assert_eq!(rows[0].ip.as_deref(), Some("127.0.0.1"));

    // No column of the table holds the token or its secret part.
    let (_, secret) = token.split_once('.').unwrap();
    let dump = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            "SELECT * FROM refresh_tokens",
        ))
        .await
        .unwrap();
    let text = format!("{dump:?}");
    assert!(!text.contains(secret), "{text}");
}
