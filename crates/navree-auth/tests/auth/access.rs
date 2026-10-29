//! Bearer access tokens: the JWT strategy, `GET /api/auth/me`, the
//! `CurrentUser` extractor and public routes.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_auth::{JwtService, TokenType, User};
use navree_contracts::{Clock, FixedClock};
use navree_db::Db;
use navree_orm::Model as _;
use serde_json::{Value, json};

use crate::support::{self, signed_in};

#[tokio::test]
async fn bearer_access_token_sets_principal() {
    let app = support::app().await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    app.client()
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id))
        .assert_json_path("via", json!("Token"))
        .assert_json_path("token_abilities", Value::Null);
    app.client()
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("__type", json!("User"))
        .assert_json_path("id", json!(user.id))
        .assert_json_path("name", json!("Ada Lovelace"))
        .assert_json_path("email", json!("ada@example.com"));
}

#[tokio::test]
async fn expired_or_tampered_access_token_is_401() {
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = support::builder()
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    client
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200);

    // Tampered: another signature.
    let mut tampered = access.clone();
    let last = tampered.pop().unwrap();
    tampered.push(if last == 'A' { 'B' } else { 'A' });
    client
        .get("/api/auth/me")
        .bearer(&tampered)
        .send()
        .await
        .assert_error(401, "Invalid token");
    client
        .get("/api/auth/me")
        .bearer("not.a.jwt")
        .send()
        .await
        .assert_error(401, "Invalid token")
        .assert_error_code(401, "invalid_token");

    // Expired: 15 minutes plus the 30 s leeway later.
    clock.advance(Duration::from_secs(900 + 31));
    let expired = client
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_error(401, "Token expired")
        .assert_error_code(401, "token_expired");
    assert!(
        expired
            .header("www-authenticate")
            .is_some_and(|value| value.starts_with("Bearer")),
        "a WWW-Authenticate challenge"
    );
    // Never silently anonymous, not even on a public route.
    client
        .get("/api/probe/ping")
        .bearer(&access)
        .send()
        .await
        .assert_error(401, "Token expired");
}

#[tokio::test]
async fn a_two_factor_challenge_token_is_not_an_access_token() {
    let app = support::app().await;
    let (user, _, _) = signed_in(&app, "ada@example.com").await;
    let challenge = app
        .get::<JwtService>()
        .await
        .sign(
            user.id,
            TokenType::TwoFactorChallenge,
            Duration::from_secs(300),
        )
        .unwrap();
    app.client()
        .get("/api/auth/me")
        .bearer(&challenge)
        .send()
        .await
        .assert_error(401, "Invalid token");
}

#[tokio::test]
async fn me_401_when_guest() {
    let app = support::app().await;
    app.client()
        .get("/api/auth/me")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    app.client()
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_error(401, "Unauthorized");
}

#[tokio::test]
async fn public_route_works_without_token() {
    let app = support::app().await;
    app.client()
        .get("/api/probe/ping")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("guest", json!(true));
    // A valid token on a public route authenticates the request.
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    app.client()
        .get("/api/probe/ping")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("guest", json!(false));
    // Another authorization scheme is not the JWT strategy's business.
    app.client()
        .get("/api/probe/ping")
        .header("authorization", "Basic YWRhOnNlY3JldA==")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("guest", json!(true));
}

#[tokio::test]
async fn current_user_is_401_once_the_user_is_deleted() {
    let app = support::app().await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    app.client()
        .get("/api/probe/user")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("ada@example.com"));
    let db = app.get::<Db>().await;
    User::query().where_key(user.id).delete(&*db).await.unwrap();
    app.client()
        .get("/api/probe/user")
        .bearer(&access)
        .send()
        .await
        .assert_error(401, None);
}
