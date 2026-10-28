//! `POST /api/auth/login`: credentials, throttling, auditing, rehashing.

use std::sync::Arc;

use navree_auth::{AuthError, AuthService, ClientMeta, user};
use navree_contracts::{AuditKind, AuditSink};
use navree_crypto::{Hasher, HashingOptions};
use navree_db::Db;
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
use serde_json::json;

use crate::support::{self, PASSWORD, RecordingSink, login, tokens};

#[tokio::test]
async fn login_returns_token_pair_and_user() {
    let app = support::app().await;
    let user = support::create_user(&app, "ada@example.com").await;
    let response = login(&app.client(), "ADA@Example.com", PASSWORD)
        .await
        .assert_status(200)
        .assert_json_path("user.__type", json!("User"))
        .assert_json_path("user.id", json!(user.id))
        .assert_json_path("user.email", json!("ada@example.com"))
        .assert_json_path("token_type", json!("Bearer"))
        .assert_json_path("expires_in", json!(900));
    let (access, refresh) = tokens(&response);
    assert_eq!(access.split('.').count(), 3, "a JWT");
    let (id, secret) = refresh.split_once('.').expect("<id>.<secret>");
    assert!(uuid::Uuid::parse_str(id).is_ok(), "{refresh}");
    assert_eq!(secret.len(), 48);
    assert!(secret.chars().all(|c| c.is_ascii_alphanumeric()));
}

#[tokio::test]
async fn bad_credentials_401_and_audited() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::builder()
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let user = support::create_user(&app, "ada@example.com").await;
    let client = app.client();

    login(&client, "ada@example.com", "wrong password")
        .await
        .assert_error(401, "Invalid credentials")
        .assert_error_code(401, "invalid_credentials");
    login(&client, "nobody@example.com", PASSWORD)
        .await
        .assert_error_code(401, "invalid_credentials");

    let failed = sink.with_action("auth.login_failed");
    assert_eq!(failed.len(), 2, "{failed:?}");
    assert_eq!(failed[0].kind, AuditKind::Security);
    assert_eq!(failed[0].actor_id, Some(user.id), "known email: the user");
    assert_eq!(failed[1].actor_id, None, "unknown email: no actor");
    assert_eq!(failed[0].ip.as_deref(), Some("127.0.0.1"));
    assert!(failed[0].request_id.is_some());
    let recorded = serde_json::to_string(&failed).unwrap();
    assert!(!recorded.contains("wrong password"), "{recorded}");
    assert!(!recorded.contains(PASSWORD), "{recorded}");
    assert!(sink.with_action("auth.login").is_empty());

    login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_status(200);
    let ok = sink.with_action("auth.login");
    assert_eq!(ok.len(), 1);
    assert_eq!(ok[0].kind, AuditKind::Auth);
    assert_eq!(ok[0].actor_id, Some(user.id));
}

#[tokio::test]
async fn malformed_login_input_is_400() {
    let app = support::app().await;
    app.client()
        .post("/api/auth/login")
        .json(&json!({ "email": "ada@example.com" }))
        .send()
        .await
        .assert_validation_error("password");
}

#[tokio::test]
async fn login_throttled_after_5_attempts() {
    let app = support::app().await;
    support::create_user(&app, "ada@example.com").await;
    support::create_user(&app, "grace@example.com").await;
    let client = app.client();
    for _ in 0..5 {
        login(&client, "ada@example.com", "wrong password")
            .await
            .assert_error(401, "Invalid credentials");
    }
    let throttled = login(&client, "ada@example.com", "wrong password")
        .await
        .assert_error(429, None);
    let retry_after: u64 = throttled
        .header("retry-after")
        .expect("Retry-After")
        .parse()
        .unwrap();
    assert!((1..=60).contains(&retry_after), "{retry_after}");
    // Even the right password waits.
    login(&client, "ADA@example.com", PASSWORD)
        .await
        .assert_error(429, None);
    // Another account from the same address is not affected.
    login(&client, "grace@example.com", PASSWORD)
        .await
        .assert_status(200);
}

/// A burst of parallel logins must not get more guesses through than the
/// limit: each attempt is counted before the password is checked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn login_throttle_holds_under_concurrency() {
    let app = support::app().await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();

    let burst = (0..30).map(|n| {
        let client = client.clone();
        async move { login(&client, "ada@example.com", &format!("wrong {n}")).await }
    });
    let statuses: Vec<u16> = futures::future::join_all(burst)
        .await
        .iter()
        .map(|response| response.status().as_u16())
        .collect();
    let unauthorized = statuses.iter().filter(|s| **s == 401).count();
    let throttled = statuses.iter().filter(|s| **s == 429).count();
    assert!(unauthorized <= 5, "{statuses:?}");
    assert_eq!(unauthorized + throttled, 30, "{statuses:?}");

    // A second burst carrying the right password is refused as a whole.
    let burst = (0..10).map(|n| {
        let client = client.clone();
        let password = if n == 5 {
            PASSWORD.to_owned()
        } else {
            format!("wrong {n}")
        };
        async move { login(&client, "ada@example.com", &password).await }
    });
    let statuses: Vec<u16> = futures::future::join_all(burst)
        .await
        .iter()
        .map(|response| response.status().as_u16())
        .collect();
    assert!(statuses.iter().all(|s| *s == 429), "{statuses:?}");
}

/// A throttled login is a security event for the audit log too: once per
/// window (a flood of refusals is one entry), with the email and address.
#[tokio::test]
async fn throttled_logins_are_audited_once_per_window() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::builder()
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();
    for _ in 0..5 {
        login(&client, "ada@example.com", "wrong password")
            .await
            .assert_status(401);
    }
    assert!(sink.with_action("auth.throttled").is_empty());
    for _ in 0..3 {
        login(&client, "Ada@example.com", "wrong password")
            .await
            .assert_status(429);
    }
    let throttled = sink.with_action("auth.throttled");
    assert_eq!(throttled.len(), 1, "{throttled:?}");
    assert_eq!(throttled[0].kind, AuditKind::Security);
    assert_eq!(
        throttled[0].metadata,
        json!({ "flow": "login", "email": "ada@example.com" })
    );
    assert_eq!(throttled[0].ip.as_deref(), Some("127.0.0.1"));
    assert!(throttled[0].request_id.is_some());
}

#[tokio::test]
async fn a_successful_login_resets_the_failed_attempts() {
    let app = support::app().await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();
    for _ in 0..4 {
        login(&client, "ada@example.com", "wrong password")
            .await
            .assert_status(401);
    }
    login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_status(200);
    for _ in 0..5 {
        login(&client, "ada@example.com", "wrong password")
            .await
            .assert_status(401);
    }
    login(&client, "ada@example.com", "wrong password")
        .await
        .assert_status(429);
}

/// Guesses from the addresses of one IPv6 network share the throttle (the
/// network is a /64 unless `http.rate_limit_ipv6_prefix` says otherwise),
/// so rotating through a /64 does not give an attacker fresh guesses.
#[tokio::test]
async fn login_throttle_counts_ipv6_clients_by_network() {
    let app = support::app().await;
    support::create_user(&app, "ada@example.com").await;
    let auth = app.get::<AuthService>().await;
    let from = |ip: &str| ClientMeta {
        ip: Some(ip.to_owned()),
        ..ClientMeta::default()
    };
    for n in 0..5 {
        let meta = from(&format!("2001:db8:1:2::{n:x}"));
        let error = auth
            .attempt_login("ada@example.com", "wrong password", &meta)
            .await
            .unwrap_err();
        assert!(matches!(error, AuthError::InvalidCredentials), "{error:?}");
    }
    // A sixth address of the same /64 is refused before the password is checked.
    let sixth = from("2001:db8:1:2:aaaa:bbbb:cccc:dddd");
    let error = auth
        .attempt_login("ada@example.com", PASSWORD, &sixth)
        .await
        .unwrap_err();
    assert!(
        matches!(error, AuthError::TooManyAttempts { .. }),
        "{error:?}"
    );
    // Another /64, and an IPv4 client, have their own budgets.
    let other = from("2001:db8:1:3::1");
    assert!(
        auth.attempt_login("ada@example.com", PASSWORD, &other)
            .await
            .is_ok()
    );
    let v4 = from("192.0.2.9");
    assert!(
        auth.attempt_login("ada@example.com", PASSWORD, &v4)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn throttle_can_be_turned_off() {
    let app = support::builder()
        .config("auth.login_throttle_per_minute", 0)
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();
    for _ in 0..7 {
        login(&client, "ada@example.com", "wrong password")
            .await
            .assert_status(401);
    }
}

#[tokio::test]
async fn login_rehashes_an_outdated_password_hash() {
    let app = support::app().await;
    let user = support::create_user(&app, "ada@example.com").await;
    // A hash made with other cost parameters (as after a config change).
    let old_hasher = Hasher::new(&HashingOptions {
        memory_kib: 2048,
        iterations: 1,
        ..HashingOptions::default()
    })
    .unwrap();
    let old_hash = old_hasher.hash(PASSWORD).await.unwrap();
    let db = app.get::<Db>().await;
    let mut row = user.clone().into_active_model();
    row.password_hash = Set(old_hash.clone());
    row.update(db.conn()).await.unwrap();

    login(&app.client(), "ada@example.com", PASSWORD)
        .await
        .assert_status(200);

    let stored = user::Entity::find_by_id(user.id)
        .one(db.conn())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(stored.password_hash, old_hash, "rehashed");
    let hasher = app.get::<Hasher>().await;
    assert!(!hasher.needs_rehash(&stored.password_hash));
    assert!(
        hasher
            .verify(PASSWORD, &stored.password_hash)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_malformed_stored_hash_is_a_failed_login() {
    let app = support::app().await;
    let user = support::create_user(&app, "ada@example.com").await;
    let db = app.get::<Db>().await;
    let mut row = user.into_active_model();
    row.password_hash = Set("not-a-phc-string".to_owned());
    row.update(db.conn()).await.unwrap();
    login(&app.client(), "ada@example.com", PASSWORD)
        .await
        .assert_error(401, "Invalid credentials");
}
