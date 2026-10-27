//! Sessions: `GET /api/auth/sessions`, `DELETE /api/auth/sessions/{id}`,
//! the session claims of access tokens, and step-up confirmation
//! (`POST /api/auth/confirm`, `recent_auth`).

use std::sync::Arc;
use std::time::Duration;

use navree_auth::JwtService;
use navree_contracts::{AuditSink, Clock};
use serde_json::{Value, json};

use crate::support::{
    self, PASSWORD, RecordingSink, claims_of, enable_two_factor, login, refresh, signed_in, tokens,
    totp_code,
};

#[tokio::test]
async fn access_tokens_carry_the_session_and_the_sign_in_time() {
    let clock = support::clock();
    let app = support::clocked(&clock).build().await;
    let (_, access, refresh_token) = signed_in(&app, "ada@example.com").await;
    let jwt = app.get::<JwtService>().await;
    let claims = claims_of(&jwt, &access);
    let session = claims.sid.expect("a session id");
    let signed_in_at = clock.now().timestamp();
    assert_eq!(claims.auth_time, Some(signed_in_at));
    assert_eq!(claims.act, None);

    // A refresh keeps the session and the sign-in time.
    clock.advance(Duration::from_secs(120));
    let refreshed = refresh(&app.client(), &refresh_token)
        .await
        .assert_status(200);
    let (access, _) = tokens(&refreshed);
    let claims = claims_of(&jwt, &access);
    assert_eq!(claims.sid, Some(session));
    assert_eq!(claims.auth_time, Some(signed_in_at));
    assert!(
        refreshed
            .json_value()
            .get("two_factor_setup_required")
            .is_none()
    );
}

#[tokio::test]
async fn sessions_are_listed_with_their_device_and_the_current_one_marked() {
    let app = support::app().await;
    let user = support::create_user(&app, "ada@example.com").await;
    let client = app.client();
    let laptop = client
        .post("/api/auth/login")
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Gecko/20100101 Firefox/131.0",
        )
        .json(&json!({ "email": "ada@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    let (laptop_access, _) = tokens(&laptop);
    let phone = login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_status(200);
    let (phone_access, _) = tokens(&phone);

    let body = client
        .get("/api/auth/sessions")
        .bearer(&laptop_access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    let sessions = body["data"].as_array().expect("a list");
    assert_eq!(sessions.len(), 2, "{body}");
    let jwt = app.get::<JwtService>().await;
    let laptop_session = claims_of(&jwt, &laptop_access).sid.unwrap();
    let current: Vec<&Value> = sessions.iter().filter(|s| s["current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["id"], json!(laptop_session));
    assert_eq!(current[0]["device"], "Firefox on Windows");
    assert_eq!(current[0]["platform"], false);
    assert_eq!(current[0]["impersonator_id"], Value::Null);
    for session in sessions {
        for key in [
            "ip",
            "user_agent",
            "last_used_at",
            "authenticated_at",
            "expires_at",
        ] {
            assert!(session.get(key).is_some(), "{key} in {session}");
        }
    }

    // Another user sees only their own.
    let (_, grace_access, _) = signed_in(&app, "grace@example.com").await;
    let body = client
        .get("/api/auth/sessions")
        .bearer(&grace_access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    let _ = (user, phone_access);
}

#[tokio::test]
async fn revoking_a_session_ends_its_refresh_tokens() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::builder()
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let other = login(&app.client(), "ada@example.com", PASSWORD)
        .await
        .assert_status(200);
    let (other_access, other_refresh) = tokens(&other);
    let jwt = app.get::<JwtService>().await;
    let other_session = claims_of(&jwt, &other_access).sid.unwrap();
    let client = app.client();

    client
        .delete(&format!("/api/auth/sessions/{other_session}"))
        .bearer(&access)
        .send()
        .await
        .assert_status(204);
    refresh(&client, &other_refresh)
        .await
        .assert_error(401, "Invalid refresh token");
    let body = client
        .get("/api/auth/sessions")
        .bearer(&access)
        .send()
        .await
        .json_value();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    // Unknown, already revoked or someone else's: 404.
    client
        .delete(&format!("/api/auth/sessions/{other_session}"))
        .bearer(&access)
        .send()
        .await
        .assert_error(404, None);
    let (_, grace_access, _) = signed_in(&app, "grace@example.com").await;
    let ada_session = claims_of(&jwt, &access).sid.unwrap();
    client
        .delete(&format!("/api/auth/sessions/{ada_session}"))
        .bearer(&grace_access)
        .send()
        .await
        .assert_error(404, None);

    let revoked = sink.with_action("auth.session_revoked");
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].metadata["session_id"], json!(other_session));
}

#[tokio::test]
async fn recent_auth_routes_need_a_recent_sign_in_or_a_confirmation() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = support::clocked(&clock)
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (_, access, refresh_token) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    let sensitive = |token: String| {
        let client = client.clone();
        async move {
            client
                .post("/api/probe/sensitive")
                .bearer(&token)
                .send()
                .await
        }
    };
    sensitive(access.clone()).await.assert_status(200);

    // Eleven minutes later (a refreshed token keeps the old sign-in time).
    clock.advance(Duration::from_secs(11 * 60));
    let (refreshed, refresh_token) =
        tokens(&refresh(&client, &refresh_token).await.assert_status(200));
    let refused = sensitive(refreshed.clone())
        .await
        .assert_error(403, "Please confirm your identity to continue");
    assert!(refused.json_value().get("errors").is_none());

    // A wrong password is a field error; the right one a new token.
    client
        .post("/api/auth/confirm")
        .bearer(&refreshed)
        .json(&json!({ "password": "wrong password!" }))
        .send()
        .await
        .assert_validation_error("password");
    let confirmed = client
        .post("/api/auth/confirm")
        .bearer(&refreshed)
        .json(&json!({ "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("token_type", json!("Bearer"))
        .assert_json_path("expires_in", json!(900))
        .assert_json_path("auth_time", json!(clock.now().timestamp()))
        .json_value();
    let confirmed_access = confirmed["access_token"].as_str().unwrap().to_owned();
    sensitive(confirmed_access.clone()).await.assert_status(200);
    let jwt = app.get::<JwtService>().await;
    assert_eq!(
        claims_of(&jwt, &confirmed_access).sid,
        claims_of(&jwt, &refreshed).sid,
        "the same session"
    );

    // The confirmation is the session's: the next refresh keeps it.
    let (next, _) = tokens(&refresh(&client, &refresh_token).await.assert_status(200));
    assert_eq!(
        claims_of(&jwt, &next).auth_time,
        Some(clock.now().timestamp())
    );
    let audited = sink.with_action("auth.confirmed");
    assert_eq!(audited.len(), 1);
    assert_eq!(audited[0].metadata["method"], "password");
}

#[tokio::test]
async fn confirmation_survives_refreshes() {
    let clock = support::clock();
    let app = support::clocked(&clock).build().await;
    let (_, _, refresh_token) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    clock.advance(Duration::from_secs(20 * 60));
    let (access, refresh_token) =
        tokens(&refresh(&client, &refresh_token).await.assert_status(200));
    client
        .post("/api/auth/confirm")
        .bearer(&access)
        .json(&json!({ "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    let confirmed_at = clock.now().timestamp();
    clock.advance(Duration::from_secs(60));
    let (refreshed, _) = tokens(&refresh(&client, &refresh_token).await.assert_status(200));
    let jwt = app.get::<JwtService>().await;
    assert_eq!(claims_of(&jwt, &refreshed).auth_time, Some(confirmed_at));
    client
        .post("/api/probe/sensitive")
        .bearer(&refreshed)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn confirmation_takes_a_two_factor_code() {
    let clock = support::clock();
    let app = support::clocked(&clock).build().await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    client
        .post("/api/auth/confirm")
        .bearer(&access)
        .json(&json!({ "code": "123456" }))
        .send()
        .await
        .assert_error(400, "Two-factor authentication is not enabled");
    let secret = enable_two_factor(&app, &clock, &user).await;
    client
        .post("/api/auth/confirm")
        .bearer(&access)
        .json(&json!({ "code": "000000" }))
        .send()
        .await
        .assert_validation_error("code");
    client
        .post("/api/auth/confirm")
        .bearer(&access)
        .json(&json!({ "code": totp_code(&secret, &clock) }))
        .send()
        .await
        .assert_status(200);

    // Exactly one of password and code.
    client
        .post("/api/auth/confirm")
        .bearer(&access)
        .json(&json!({}))
        .send()
        .await
        .assert_validation_error("password");
    client
        .post("/api/auth/confirm")
        .bearer(&access)
        .json(&json!({ "password": PASSWORD, "code": "123456" }))
        .send()
        .await
        .assert_validation_error("password");
    // Guests: 401.
    client
        .post("/api/auth/confirm")
        .json(&json!({ "password": PASSWORD }))
        .send()
        .await
        .assert_error(401, None);
}
