//! The platform audience: `POST /api/auth/platform/login`, `POST
//! /api/auth/platform/2fa/verify`, platform sessions and their guard.

use std::sync::Arc;

use navree_auth::{JwtService, TokenType};
use navree_contracts::{AuditSink, FixedClock};
use navree_testing::{TestApp, TestClient, TestResponse};
use serde_json::{Value, json};

use crate::support::{
    self, PASSWORD, RecordingSink, claims_of, enable_two_factor, grant_role, refresh, signed_in,
    tokens, totp_code,
};

async fn platform_app(clock: &Arc<FixedClock>, sink: Arc<RecordingSink>) -> TestApp {
    support::with_access(clock)
        .config("auth.platform.enabled", true)
        .override_value::<dyn AuditSink>(sink)
        .build()
        .await
}

async fn platform_login(client: &TestClient, email: &str, password: &str) -> TestResponse {
    client
        .post("/api/auth/platform/login")
        .json(&json!({ "email": email, "password": password }))
        .send()
        .await
}

#[tokio::test]
async fn platform_routes_are_off_by_default() {
    let app = support::app().await;
    platform_login(&app.client(), "ada@example.com", PASSWORD)
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn a_platform_administrator_signs_in_with_two_factor() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = platform_app(&clock, sink.clone()).await;
    let admin = support::create_user(&app, "root@example.com").await;
    grant_role(&app, admin.id, "platform-admin", &[]).await;
    let client = app.client();

    // Without two-factor authentication: refused.
    platform_login(&client, "root@example.com", PASSWORD)
        .await
        .assert_error(403, "Two-factor authentication required");

    let secret = enable_two_factor(&app, &clock, &admin).await;
    let challenge: Value = platform_login(&client, "root@example.com", PASSWORD)
        .await
        .assert_status(200)
        .assert_json_path("two_factor_required", json!(true))
        .json_value();
    let challenge_token = challenge["challenge_token"].as_str().unwrap().to_owned();

    // The platform challenge is not an app challenge.
    client
        .post("/api/auth/2fa/verify")
        .json(&json!({ "challenge_token": challenge_token, "code": totp_code(&secret, &clock) }))
        .send()
        .await
        .assert_error(401, "Invalid token");

    let response = client
        .post("/api/auth/platform/2fa/verify")
        .json(&json!({ "challenge_token": challenge_token, "code": totp_code(&secret, &clock) }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("root@example.com"));
    let (access, refresh_token) = tokens(&response);
    let jwt = app.get::<JwtService>().await;
    assert_eq!(claims_of(&jwt, &access).typ, TokenType::Platform);

    client
        .get("/api/probe/platform")
        .bearer(&access)
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/probe/platform-admin")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("root@example.com"));

    // Refreshing keeps the platform audience.
    let (refreshed, _) = tokens(&refresh(&client, &refresh_token).await.assert_status(200));
    assert_eq!(claims_of(&jwt, &refreshed).typ, TokenType::Platform);
    let body = client
        .get("/api/auth/sessions")
        .bearer(&refreshed)
        .send()
        .await
        .json_value();
    assert_eq!(body["data"][0]["platform"], true);

    let audited = sink.with_action("auth.platform_login");
    assert_eq!(audited.len(), 1);
    assert_eq!(audited[0].actor_id, Some(admin.id));
}

#[tokio::test]
async fn app_sessions_and_other_users_never_reach_the_platform() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = platform_app(&clock, sink.clone()).await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    enable_two_factor(&app, &clock, &user).await;
    let client = app.client();

    client
        .get("/api/probe/platform")
        .bearer(&access)
        .send()
        .await
        .assert_error(403, "A platform session is required");
    client
        .get("/api/probe/platform-admin")
        .bearer(&access)
        .send()
        .await
        .assert_error(403, "A platform session is required");

    // Not an administrator, or a wrong password: the same answer.
    platform_login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_error(401, "Invalid credentials");
    platform_login(&client, "nobody@example.com", PASSWORD)
        .await
        .assert_error(401, "Invalid credentials");
    let failed = sink.with_action("auth.login_failed");
    assert!(
        failed
            .iter()
            .all(|entry| entry.metadata["flow"] == "platform")
    );
}

#[tokio::test]
async fn platform_sign_ins_have_their_own_throttle() {
    let clock = support::clock();
    let app = support::with_access(&clock)
        .config("auth.platform.enabled", true)
        .config("auth.platform.login_throttle_per_minute", 2)
        .build()
        .await;
    support::create_user(&app, "root@example.com").await;
    let client = app.client();
    let mut statuses = Vec::new();
    for _ in 0..3 {
        statuses.push(
            platform_login(&client, "root@example.com", "wrong password!")
                .await
                .status()
                .as_u16(),
        );
    }
    assert_eq!(statuses, [401, 401, 429]);
    // The app's own sign-in is unaffected.
    support::login(&client, "root@example.com", PASSWORD)
        .await
        .assert_status(200);
}

#[tokio::test]
async fn losing_the_role_ends_the_platform_session_at_refresh() {
    let clock = support::clock();
    let app = platform_app(&clock, Arc::new(RecordingSink::default())).await;
    let admin = support::create_user(&app, "root@example.com").await;
    grant_role(&app, admin.id, "platform-admin", &[]).await;
    let secret = enable_two_factor(&app, &clock, &admin).await;
    let client = app.client();
    let challenge: Value = platform_login(&client, "root@example.com", PASSWORD)
        .await
        .assert_status(200)
        .json_value();
    let (_, refresh_token) = tokens(
        &client
            .post("/api/auth/platform/2fa/verify")
            .json(&json!({
                "challenge_token": challenge["challenge_token"],
                "code": totp_code(&secret, &clock),
            }))
            .send()
            .await
            .assert_status(200),
    );

    let db = app.get::<navree_db::Db>().await;
    let access = app.get::<navree_access::AccessService>().await;
    let uow = db.uow(None, None).await;
    access
        .remove_role(&uow, admin.id, "platform-admin")
        .await
        .unwrap();
    uow.commit().await.unwrap();
    refresh(&client, &refresh_token)
        .await
        .assert_error(401, "Invalid refresh token");
}
