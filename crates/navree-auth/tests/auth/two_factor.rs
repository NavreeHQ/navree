//! TOTP two-factor authentication: `/api/auth/2fa/*` and the login
//! challenge.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_auth::{
    AuthError, AuthService, ClientMeta, LoginOutcome, TokenType, TwoFactorProof, User,
    two_factor_enabled,
};
use navree_contracts::{AuditSink, Clock, FixedClock};
use navree_crypto::totp::{Secret, Totp};
use navree_db::Db;
use navree_testing::{TestApp, TestClient, TestResponse};
use navree_orm::Model as _;
use serde_json::{Value, json};

use crate::support::{self, PASSWORD, RecordingSink, login, signed_in, tokens};

/// The TOTP code of `secret` (base32) at `clock`'s time.
fn code_at(secret: &str, clock: &FixedClock) -> String {
    Totp::builder(Secret::from_base32(secret).expect("a base32 secret"))
        .build()
        .expect("a valid TOTP")
        .generate(u64::try_from(clock.now().timestamp()).unwrap())
}

/// A six-digit code that is not valid at `clock`'s time (nor one step
/// before or after).
fn wrong_code(secret: &str, clock: &FixedClock) -> String {
    let totp = Totp::builder(Secret::from_base32(secret).unwrap())
        .build()
        .unwrap();
    let now = u64::try_from(clock.now().timestamp()).unwrap();
    (0..1_000_000)
        .map(|n| format!("{n:06}"))
        .find(|code| totp.check(code, now).is_none())
        .unwrap()
}

/// Moves `clock` to the next 30-second TOTP step.
fn next_step(clock: &FixedClock) {
    clock.advance(Duration::from_secs(30));
}

async fn post(client: &TestClient, path: &str, access: &str, body: Value) -> TestResponse {
    client
        .post(&format!("/api/auth/2fa/{path}"))
        .bearer(access)
        .json(&body)
        .send()
        .await
}

async fn verify(client: &TestClient, body: Value) -> TestResponse {
    client.post("/api/auth/2fa/verify").json(&body).send().await
}

/// The challenge token of a two-factor login.
async fn challenge(client: &TestClient, email: &str) -> String {
    login(client, email, PASSWORD)
        .await
        .assert_status(200)
        .assert_json_path("two_factor_required", json!(true))
        .json_value()["challenge_token"]
        .as_str()
        .expect("a challenge token")
        .to_owned()
}

fn recovery_codes(response: &TestResponse) -> Vec<String> {
    serde_json::from_value(response.json_value()["recovery_codes"].clone())
        .expect("recovery_codes is a list of strings")
}

struct Enabled {
    user: User,
    access: String,
    secret: String,
    recovery_codes: Vec<String>,
}

/// A user with confirmed two-factor authentication; the clock is moved to
/// the next TOTP step afterwards (the confirmation code is used up).
async fn enabled(app: &TestApp, clock: &FixedClock, email: &str) -> Enabled {
    let (user, access, _) = signed_in(app, email).await;
    let client = app.client();
    let setup = post(&client, "enable", &access, json!({}))
        .await
        .assert_status(200)
        .json_value();
    let secret = setup["secret"].as_str().unwrap().to_owned();
    let confirmed = post(
        &client,
        "confirm",
        &access,
        json!({ "code": code_at(&secret, clock) }),
    )
    .await
    .assert_status(200);
    next_step(clock);
    Enabled {
        user,
        access,
        secret,
        recovery_codes: recovery_codes(&confirmed),
    }
}

async fn clocked_app() -> (TestApp, Arc<FixedClock>, Arc<RecordingSink>) {
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let sink = Arc::new(RecordingSink::default());
    let app = support::builder()
        .override_value::<dyn Clock>(clock.clone())
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    (app, clock, sink)
}

async fn stored(app: &TestApp, user: &User) -> User {
    let db = app.get::<Db>().await;
    User::find(&*db, user.id)
        .await
        .unwrap()
        .unwrap()
        .into_inner()
}

#[tokio::test]
async fn two_factor_enable_confirm_and_login_challenge_flow() {
    let (app, clock, sink) = clocked_app().await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();

    // 1. Enable: a new secret, its otpauth URL and a QR code.
    let setup = post(&client, "enable", &access, json!({}))
        .await
        .assert_status(200)
        .json_value();
    let secret = setup["secret"].as_str().unwrap().to_owned();
    assert!(Secret::from_base32(&secret).is_ok(), "{setup}");
    let url = setup["otpauth_url"].as_str().unwrap();
    assert!(
        url.starts_with("otpauth://totp/Navree:ada%40example.com?secret="),
        "{url}"
    );
    assert!(
        url.contains(&secret) && url.contains("issuer=Navree"),
        "{url}"
    );
    assert!(
        setup["qr_svg"].as_str().unwrap().contains("<svg"),
        "{setup}"
    );
    // The secret is stored encrypted; nothing is active yet.
    let row = stored(&app, &user).await;
    let at_rest = row.two_factor_secret.clone().expect("a stored secret");
    assert!(!at_rest.contains(&secret), "{at_rest}");
    assert!(row.two_factor_confirmed_at.is_none());
    let (plain_access, _) = tokens(
        &login(&client, "ada@example.com", PASSWORD)
            .await
            .assert_status(200),
    );
    assert!(!plain_access.is_empty(), "unconfirmed: a plain login");

    // 2. Confirm with a current code: the recovery codes, shown once.
    let confirmed = post(
        &client,
        "confirm",
        &access,
        json!({ "code": code_at(&secret, &clock) }),
    )
    .await
    .assert_status(200);
    let codes = recovery_codes(&confirmed);
    assert_eq!(codes.len(), 8);
    assert!(codes.iter().all(|code| code.len() >= 16), "{codes:?}");
    let row = stored(&app, &user).await;
    assert!(row.two_factor_confirmed_at.is_some());
    let codes_at_rest = row.two_factor_recovery_codes.expect("stored codes");
    assert!(
        codes
            .iter()
            .all(|code| !codes_at_rest.contains(code.as_str())),
        "recovery codes are not stored in plain text"
    );

    // 3. Login now answers with a challenge instead of tokens.
    let response = login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_status(200)
        .assert_json_path("two_factor_required", json!(true))
        .assert_json_path("expires_in", json!(300));
    let body = response.json_value();
    assert!(body.get("access_token").is_none(), "{body}");
    assert!(body.get("refresh_token").is_none(), "{body}");
    let challenge_token = body["challenge_token"].as_str().unwrap().to_owned();

    // 4. The challenge plus a TOTP code gives the token pair.
    next_step(&clock);
    let response = verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": code_at(&secret, &clock) }),
    )
    .await
    .assert_status(200)
    .assert_json_path("user.id", json!(user.id))
    .assert_json_path("token_type", json!("Bearer"));
    let (access, _) = tokens(&response);
    client
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));

    // The challenge is single-use.
    next_step(&clock);
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": code_at(&secret, &clock) }),
    )
    .await
    .assert_error(401, "Invalid token");

    for action in [
        "auth.two_factor_enabled",
        "auth.two_factor_confirmed",
        "auth.two_factor_challenge",
    ] {
        let entries = sink.with_action(action);
        assert_eq!(entries.len(), 1, "{action}: {entries:?}");
        assert_eq!(entries[0].actor_id, Some(user.id), "{action}");
    }
    let logins = sink.with_action("auth.login");
    let last = logins.last().expect("an auth.login entry");
    assert_eq!(last.metadata["two_factor"], "totp", "{last:?}");
    let recorded = serde_json::to_string(&sink.entries()).unwrap();
    assert!(!recorded.contains(&secret), "{recorded}");
}

#[tokio::test]
async fn challenge_token_cannot_be_used_as_access_token() {
    let (app, clock, _) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();
    let challenge_token = challenge(&client, "ada@example.com").await;

    for path in ["/api/auth/me", "/api/probe/whoami"] {
        client
            .get(path)
            .bearer(&challenge_token)
            .send()
            .await
            .assert_error(401, "Invalid token");
    }
    // Nor is an access token a challenge.
    verify(
        &client,
        json!({ "challenge_token": enabled.access, "code": code_at(&enabled.secret, &clock) }),
    )
    .await
    .assert_error(401, "Invalid token");
}

#[tokio::test]
async fn challenge_token_expires_after_five_minutes() {
    let (app, clock, _) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();
    let challenge_token = challenge(&client, "ada@example.com").await;
    clock.advance(Duration::from_secs(300 + 31));
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": code_at(&enabled.secret, &clock) }),
    )
    .await
    .assert_error(401, "Token expired");
}

#[tokio::test]
async fn recovery_code_single_use() {
    let (app, clock, sink) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();
    let first = enabled.recovery_codes[0].clone();

    let challenge_token = challenge(&client, "ada@example.com").await;
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "recovery_code": first.to_uppercase() }),
    )
    .await
    .assert_status(200)
    .assert_json_path("user.id", json!(enabled.user.id));

    let challenge_token = challenge(&client, "ada@example.com").await;
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "recovery_code": first }),
    )
    .await
    .assert_error(401, "Invalid two-factor code")
    .assert_error_code(401, "invalid_two_factor_code");
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "recovery_code": enabled.recovery_codes[1] }),
    )
    .await
    .assert_status(200);

    let used = sink.with_action("auth.two_factor_recovery_code_used");
    assert_eq!(used.len(), 2, "{used:?}");
    let recorded = serde_json::to_string(&sink.entries()).unwrap();
    assert!(!recorded.contains(&first), "{recorded}");
}

#[tokio::test]
async fn wrong_totp_code_rejected() {
    let (app, clock, sink) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();
    let challenge_token = challenge(&client, "ada@example.com").await;

    verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": wrong_code(&enabled.secret, &clock) }),
    )
    .await
    .assert_error(401, "Invalid two-factor code");
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": "abc" }),
    )
    .await
    .assert_error(401, "Invalid two-factor code");
    verify(&client, json!({ "challenge_token": challenge_token }))
        .await
        .assert_validation_error("code");
    let failed = sink.with_action("auth.two_factor_failed");
    assert_eq!(failed.len(), 2, "{failed:?}");
    assert_eq!(failed[0].actor_id, Some(enabled.user.id));

    // A code is accepted once: the same code cannot sign in twice.
    let code = code_at(&enabled.secret, &clock);
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": code }),
    )
    .await
    .assert_status(200);
    let challenge_token = challenge(&client, "ada@example.com").await;
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": code }),
    )
    .await
    .assert_error(401, "Invalid two-factor code");

    // A wrong code does not confirm a new setup either.
    let (_, access, _) = signed_in(&app, "grace@example.com").await;
    let setup = post(&client, "enable", &access, json!({}))
        .await
        .json_value();
    let secret = setup["secret"].as_str().unwrap();
    post(
        &client,
        "confirm",
        &access,
        json!({ "code": wrong_code(secret, &clock) }),
    )
    .await
    .assert_validation_error("code");
}

#[tokio::test]
async fn two_factor_verify_is_throttled() {
    let (app, clock, _) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();
    let challenge_token = challenge(&client, "ada@example.com").await;
    let wrong = wrong_code(&enabled.secret, &clock);
    for _ in 0..5 {
        verify(
            &client,
            json!({ "challenge_token": challenge_token, "code": wrong }),
        )
        .await
        .assert_error(401, "Invalid two-factor code");
    }
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": code_at(&enabled.secret, &clock) }),
    )
    .await
    .assert_error(429, None);
}

/// Parallel wrong codes must not get past the per-minute limit, which is
/// all that protects the 6-digit code space.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_factor_throttle_holds_under_concurrency() {
    let (app, clock, _) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();
    let challenge_token = challenge(&client, "ada@example.com").await;
    let wrong = wrong_code(&enabled.secret, &clock);

    let burst = (0..30).map(|_| {
        let client = client.clone();
        let body = json!({ "challenge_token": challenge_token, "code": wrong });
        async move { verify(&client, body).await }
    });
    let statuses: Vec<u16> = futures::future::join_all(burst)
        .await
        .iter()
        .map(|response| response.status().as_u16())
        .collect();
    let unauthorized = statuses.iter().filter(|s| **s == 401).count();
    assert!(unauthorized <= 5, "{statuses:?}");
    assert_eq!(
        unauthorized + statuses.iter().filter(|s| **s == 429).count(),
        30,
        "{statuses:?}"
    );
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "code": code_at(&enabled.secret, &clock) }),
    )
    .await
    .assert_error(429, None);
}

/// Parallel wrong passwords on a re-authenticated route count like
/// failed logins.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn password_confirmation_throttle_holds_under_concurrency() {
    let (app, clock, _) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();
    let burst = (0..20).map(|n| {
        let client = client.clone();
        let access = enabled.access.clone();
        async move {
            post(
                &client,
                "disable",
                &access,
                json!({ "password": format!("wrong {n}") }),
            )
            .await
        }
    });
    let statuses: Vec<u16> = futures::future::join_all(burst)
        .await
        .iter()
        .map(|response| response.status().as_u16())
        .collect();
    let refused = statuses.iter().filter(|s| **s == 400).count();
    assert!(refused <= 5, "{statuses:?}");
    assert_eq!(
        refused + statuses.iter().filter(|s| **s == 429).count(),
        20,
        "{statuses:?}"
    );
}

/// A correct password confirmation does not use up a failed attempt.
#[tokio::test]
async fn a_correct_password_confirmation_is_not_counted() {
    let (app, clock, _) = clocked_app().await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    for _ in 0..6 {
        post(&client, "disable", &access, json!({ "password": PASSWORD }))
            .await
            .assert_status(204);
    }
    for _ in 0..5 {
        post(
            &client,
            "disable",
            &access,
            json!({ "password": "wrong password" }),
        )
        .await
        .assert_validation_error("password");
    }
    post(&client, "disable", &access, json!({ "password": PASSWORD }))
        .await
        .assert_error(429, None);
    let _ = clock;
}

#[tokio::test]
async fn disable_requires_the_password() {
    let (app, clock, sink) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();

    post(
        &client,
        "disable",
        &enabled.access,
        json!({ "password": "wrong password" }),
    )
    .await
    .assert_validation_error("password");
    post(
        &client,
        "disable",
        &enabled.access,
        json!({ "password": PASSWORD }),
    )
    .await
    .assert_status(204);

    let row = stored(&app, &enabled.user).await;
    assert!(row.two_factor_secret.is_none());
    assert!(row.two_factor_recovery_codes.is_none());
    assert!(row.two_factor_confirmed_at.is_none());
    let (access, _) = tokens(
        &login(&client, "ada@example.com", PASSWORD)
            .await
            .assert_status(200),
    );
    assert!(!access.is_empty());
    assert_eq!(sink.with_action("auth.two_factor_disabled").len(), 1);
}

#[tokio::test]
async fn recovery_codes_can_be_regenerated() {
    let (app, clock, sink) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let client = app.client();

    let fresh = recovery_codes(
        &post(&client, "recovery-codes", &enabled.access, json!({}))
            .await
            .assert_status(200),
    );
    assert_eq!(fresh.len(), 8);
    assert!(
        fresh
            .iter()
            .all(|code| !enabled.recovery_codes.contains(code))
    );

    let challenge_token = challenge(&client, "ada@example.com").await;
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "recovery_code": enabled.recovery_codes[0] }),
    )
    .await
    .assert_error(401, "Invalid two-factor code");
    verify(
        &client,
        json!({ "challenge_token": challenge_token, "recovery_code": fresh[0] }),
    )
    .await
    .assert_status(200);
    assert_eq!(
        sink.with_action("auth.two_factor_recovery_codes_regenerated")
            .len(),
        1
    );

    // Without two-factor authentication there is nothing to regenerate.
    let (_, access, _) = signed_in(&app, "grace@example.com").await;
    post(&client, "recovery-codes", &access, json!({}))
        .await
        .assert_error(400, "Two-factor authentication is not enabled");
}

#[tokio::test]
async fn enable_and_confirm_follow_the_setup_state() {
    let (app, clock, _) = clocked_app().await;
    let client = app.client();
    let (_, access, _) = signed_in(&app, "grace@example.com").await;

    // Confirming before enabling.
    post(&client, "confirm", &access, json!({ "code": "123456" }))
        .await
        .assert_error(400, "Two-factor authentication is not enabled");
    // Enabling twice before confirming replaces the secret.
    let first = post(&client, "enable", &access, json!({}))
        .await
        .assert_status(200)
        .json_value();
    let second = post(&client, "enable", &access, json!({}))
        .await
        .assert_status(200)
        .json_value();
    assert_ne!(first["secret"], second["secret"]);
    let first_secret = first["secret"].as_str().unwrap();
    post(
        &client,
        "confirm",
        &access,
        json!({ "code": code_at(first_secret, &clock) }),
    )
    .await
    .assert_validation_error("code");

    let enabled = enabled(&app, &clock, "ada@example.com").await;
    post(&client, "enable", &enabled.access, json!({}))
        .await
        .assert_error(409, "Two-factor authentication is already enabled");
    // Guests cannot manage two-factor authentication.
    client
        .post("/api/auth/2fa/enable")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn service_login_needs_the_second_factor() {
    let (app, clock, _) = clocked_app().await;
    let enabled = enabled(&app, &clock, "ada@example.com").await;
    let service = app.get::<AuthService>().await;
    let meta = ClientMeta::default();

    let error = service
        .login("ada@example.com", PASSWORD, &meta)
        .await
        .expect_err("a second factor is needed");
    assert!(matches!(error, AuthError::TwoFactorRequired), "{error:?}");

    let LoginOutcome::TwoFactorRequired {
        user,
        challenge_token,
    } = service
        .attempt_login("ada@example.com", PASSWORD, &meta)
        .await
        .unwrap()
    else {
        panic!("expected a two-factor challenge");
    };
    assert_eq!(user.id, enabled.user.id);
    let claims = service
        .jwt()
        .verify(&challenge_token, TokenType::TwoFactorChallenge)
        .unwrap();
    assert_eq!(claims.exp - claims.iat, 300);
    assert!(two_factor_enabled(&user));

    let (user, tokens) = service
        .verify_two_factor(
            &challenge_token,
            TwoFactorProof::Code(code_at(&enabled.secret, &clock)),
            &meta,
        )
        .await
        .unwrap();
    assert_eq!(user.id, enabled.user.id);
    assert_eq!(
        service
            .jwt()
            .verify(&tokens.access_token, TokenType::Access)
            .unwrap()
            .sub,
        user.id
    );
}
