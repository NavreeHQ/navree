//! Impersonation: `POST /api/auth/impersonate/{user}`, `DELETE
//! /api/auth/impersonate`, the `act` claim, `Actor`, the guards and the
//! audit trail.

use std::sync::Arc;
use std::time::Duration;

use navree_auth::{JwtService, User};
use navree_contracts::AuditSink;
use navree_testing::{TestApp, TestResponse};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{
    self, PASSWORD, RecordingSink, claims_of, grant_role, refresh, signed_in, tokens,
};

struct Setup {
    app: TestApp,
    sink: Arc<RecordingSink>,
    clock: Arc<navree_contracts::FixedClock>,
    admin: User,
    admin_access: String,
    target: User,
}

async fn setup() -> Setup {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = support::with_access(&clock)
        .config("auth.impersonation.enabled", true)
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (admin, admin_access, _) = signed_in(&app, "admin@example.com").await;
    grant_role(&app, admin.id, "support", &["users.impersonate"]).await;
    let target = support::create_user(&app, "anu@example.com").await;
    Setup {
        app,
        sink,
        clock,
        admin,
        admin_access,
        target,
    }
}

async fn impersonate(app: &TestApp, access: &str, target: Uuid, reason: Value) -> TestResponse {
    app.client()
        .post(&format!("/api/auth/impersonate/{target}"))
        .bearer(access)
        .json(&json!({ "reason": reason }))
        .send()
        .await
}

#[tokio::test]
async fn impersonation_is_off_by_default() {
    let app = support::app().await;
    let (_, access, _) = signed_in(&app, "admin@example.com").await;
    impersonate(&app, &access, Uuid::now_v7(), json!("support"))
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn an_administrator_signs_in_as_the_user() {
    let s = setup().await;
    let response = impersonate(&s.app, &s.admin_access, s.target.id, json!("Ticket #42"))
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(s.target.id))
        .assert_json_path("user.email", json!("anu@example.com"))
        .assert_json_path("impersonator_id", json!(s.admin.id));
    let (access, _) = tokens(&response);
    let jwt = s.app.get::<JwtService>().await;
    let claims = claims_of(&jwt, &access);
    assert_eq!(claims.sub, s.target.id);
    assert_eq!(claims.act.map(|act| act.sub), Some(s.admin.id));
    assert_eq!(claims.auth_time, None, "impersonation is not a sign-in");

    let client = s.app.client();
    client
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("anu@example.com"));
    client
        .get("/api/probe/actor")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(s.target.id))
        .assert_json_path("impersonator_id", json!(s.admin.id))
        .assert_json_path("impersonating", json!(true));
    client
        .get("/api/probe/actor")
        .bearer(&s.admin_access)
        .send()
        .await
        .assert_json_path("impersonating", json!(false));

    let started = s.sink.with_action("auth.impersonation_started");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].actor_id, Some(s.admin.id));
    assert_eq!(started[0].impersonator_id, Some(s.admin.id));
    assert_eq!(started[0].subject_id, Some(s.target.id.to_string()));
    assert_eq!(started[0].metadata["reason"], "Ticket #42");
    assert_eq!(started[0].metadata["session_id"], json!(claims.sid));

    // The user sees the session among theirs.
    let (_, own_access, _) = {
        let response = support::login(&client, "anu@example.com", PASSWORD)
            .await
            .assert_status(200);
        let (a, r) = tokens(&response);
        ((), a, r)
    };
    let sessions = client
        .get("/api/auth/sessions")
        .bearer(&own_access)
        .send()
        .await
        .json_value();
    assert!(
        sessions["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|session| session["impersonator_id"] == json!(s.admin.id)),
        "{sessions}"
    );
}

#[tokio::test]
async fn impersonation_needs_the_permission_a_reason_and_a_fit_target() {
    let s = setup().await;
    let (_, plain_access, _) = signed_in(&s.app, "gauri@example.com").await;
    impersonate(&s.app, &plain_access, s.target.id, json!("curious"))
        .await
        .assert_error(403, None);
    impersonate(&s.app, &s.admin_access, s.target.id, json!(""))
        .await
        .assert_validation_error("reason");
    impersonate(&s.app, &s.admin_access, s.target.id, json!("   "))
        .await
        .assert_validation_error("reason");
    impersonate(&s.app, &s.admin_access, s.admin.id, json!("me"))
        .await
        .assert_error(403, "This user cannot be impersonated: yourself");
    impersonate(&s.app, &s.admin_access, Uuid::now_v7(), json!("ghost"))
        .await
        .assert_error(404, None);

    // Another administrator who may impersonate: refused.
    let other = support::create_user(&s.app, "other-admin@example.com").await;
    grant_role(&s.app, other.id, "support", &[]).await;
    impersonate(&s.app, &s.admin_access, other.id, json!("escalation"))
        .await
        .assert_error(
            403,
            "This user cannot be impersonated: a user who may impersonate others",
        );
}

#[tokio::test]
async fn risky_routes_refuse_impersonation_sessions() {
    let s = setup().await;
    let response = impersonate(&s.app, &s.admin_access, s.target.id, json!("support"))
        .await
        .assert_status(200);
    let (access, _) = tokens(&response);
    let client = s.app.client();
    let forbidden = "Not allowed while impersonating a user";

    client
        .post("/api/probe/no-impersonation")
        .bearer(&access)
        .send()
        .await
        .assert_error(403, forbidden);
    client
        .patch("/api/auth/me/password")
        .bearer(&access)
        .json(&json!({ "current_password": PASSWORD, "password": "another password" }))
        .send()
        .await
        .assert_error(403, forbidden);
    client
        .patch("/api/auth/me")
        .bearer(&access)
        .json(&json!({ "name": "Someone else" }))
        .send()
        .await
        .assert_error(403, forbidden);
    client
        .post("/api/auth/2fa/enable")
        .bearer(&access)
        .send()
        .await
        .assert_error(403, forbidden);
    client
        .post("/api/auth/confirm")
        .bearer(&access)
        .json(&json!({ "password": PASSWORD }))
        .send()
        .await
        .assert_error(403, forbidden);
    impersonate(&s.app, &access, s.admin.id, json!("nested"))
        .await
        .assert_error(403, None);
    // `recent_auth` routes: an impersonation session never signed in.
    client
        .post("/api/probe/sensitive")
        .bearer(&access)
        .send()
        .await
        .assert_error(403, "Please confirm your identity to continue");
    // Reading is fine.
    client
        .get("/api/probe/user")
        .bearer(&access)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn sessions_end_on_request_and_after_their_lifetime() {
    let s = setup().await;
    let response = impersonate(&s.app, &s.admin_access, s.target.id, json!("support"))
        .await
        .assert_status(200);
    let (access, refresh_token) = tokens(&response);
    let client = s.app.client();
    let jwt = s.app.get::<JwtService>().await;

    // Refreshing keeps the impersonator.
    let refreshed = refresh(&client, &refresh_token).await.assert_status(200);
    let (refreshed_access, refresh_token) = tokens(&refreshed);
    assert_eq!(
        claims_of(&jwt, &refreshed_access).act.map(|act| act.sub),
        Some(s.admin.id)
    );
    let refresh_audit = s.sink.with_action("auth.refresh");
    assert_eq!(refresh_audit[0].impersonator_id, Some(s.admin.id));

    // An hour later the session is over, refreshed or not.
    s.clock.advance(Duration::from_secs(3601));
    refresh(&client, &refresh_token)
        .await
        .assert_error(401, "Invalid refresh token");

    // Ending one explicitly.
    s.clock.advance(Duration::from_secs(1));
    let (admin_access, _) = tokens(
        &support::login(&client, "admin@example.com", PASSWORD)
            .await
            .assert_status(200),
    );
    let response = impersonate(&s.app, &admin_access, s.target.id, json!("again"))
        .await
        .assert_status(200);
    let (access2, refresh2) = tokens(&response);
    client
        .delete("/api/auth/impersonate")
        .bearer(&access2)
        .send()
        .await
        .assert_status(204);
    refresh(&client, &refresh2)
        .await
        .assert_error(401, "Invalid refresh token");
    client
        .delete("/api/auth/impersonate")
        .bearer(&admin_access)
        .send()
        .await
        .assert_error(400, "Not impersonating a user");
    let ended = s.sink.with_action("auth.impersonation_ended");
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].actor_id, Some(s.admin.id));
    assert_eq!(ended[0].subject_id, Some(s.target.id.to_string()));
    let _ = access;
}

#[tokio::test]
async fn audit_entries_of_the_session_record_the_impersonator() {
    let s = setup().await;
    let response = impersonate(&s.app, &s.admin_access, s.target.id, json!("support"))
        .await
        .assert_status(200);
    let (access, refresh_token) = tokens(&response);
    s.app
        .client()
        .post("/api/auth/logout")
        .bearer(&access)
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .assert_status(204);
    let logout = s.sink.with_action("auth.logout");
    assert_eq!(logout.len(), 1);
    assert_eq!(logout[0].actor_id, Some(s.target.id));
    assert_eq!(logout[0].impersonator_id, Some(s.admin.id));

    // The administrator's own sign-out names nobody else.
    s.app
        .client()
        .post("/api/auth/logout")
        .bearer(&s.admin_access)
        .send()
        .await
        .assert_status(204);
    let logout = s.sink.with_action("auth.logout");
    assert_eq!(logout[1].impersonator_id, None);
}
