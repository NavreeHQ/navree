//! Passwordless sign-in: magic links (`POST /api/auth/magic-link`) and
//! emailed codes (`POST /api/auth/otp`).

use std::sync::Arc;
use std::time::Duration;

use navree_auth::login_token;
use navree_contracts::AuditSink;
use navree_db::Db;
use navree_testing::{TestApp, TestAppBuilder};
use sea_orm::EntityTrait;
use serde_json::{Value, json};

use crate::support::{
    self, RecordingSink, enable_two_factor, link_param, sent_mail, tokens, totp_code,
};

const SENT: &str = "If the email address is registered, a sign-in message has been sent.";

fn mail_app(clock: &Arc<navree_contracts::FixedClock>) -> TestAppBuilder {
    support::mail_builder()
        .override_value::<dyn navree_contracts::Clock>(clock.clone())
        .config("auth.magic_link.enabled", true)
        .config("auth.otp.enabled", true)
}

/// The text body of the last mail.
async fn last_text(app: &TestApp) -> String {
    sent_mail(app)
        .await
        .last()
        .expect("a mail")
        .text
        .clone()
        .expect("a text body")
}

/// The six-digit code in `text`.
fn code_in(text: &str) -> String {
    let start = text.find("code: ").expect("a code") + "code: ".len();
    text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

#[tokio::test]
async fn passwordless_routes_are_off_by_default() {
    let app = support::app().await;
    for path in ["/api/auth/magic-link", "/api/auth/otp"] {
        app.client()
            .post(path)
            .json(&json!({ "email": "ada@example.com" }))
            .send()
            .await
            .assert_error(404, None);
    }
}

#[tokio::test]
async fn a_magic_link_signs_in_once() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = mail_app(&clock)
        .config("auth.magic_link.url", "https://app.example.com/magic")
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();

    client
        .post("/api/auth/magic-link")
        .json(&json!({ "email": "Ada@Example.com" }))
        .send()
        .await
        .assert_status(202)
        .assert_json_path("message", json!(SENT));
    let mails = sent_mail(&app).await;
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].subject, "Your sign-in link");
    let text = last_text(&app).await;
    assert!(
        text.contains("https://app.example.com/magic?token="),
        "{text}"
    );
    let token = link_param(&text, "token").expect("a token in the link");

    let response = client
        .post("/api/auth/magic-link/verify")
        .json(&json!({ "token": token }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("ada@example.com"));
    let (access, _) = tokens(&response);
    assert!(!access.is_empty());
    // The address counts as verified now.
    assert!(response.json_value()["user"]["email_verified_at"].is_string());

    // Once only.
    client
        .post("/api/auth/magic-link/verify")
        .json(&json!({ "token": token }))
        .send()
        .await
        .assert_error(401, "Invalid or expired sign-in code");
    let logins = sink.with_action("auth.login");
    assert_eq!(logins.len(), 1);
    assert_eq!(logins[0].metadata["method"], "magic_link");
    assert_eq!(sink.with_action("auth.magic_link_sent").len(), 1);
}

#[tokio::test]
async fn magic_links_expire_are_throttled_and_say_nothing_about_unknown_addresses() {
    let clock = support::clock();
    let app = mail_app(&clock).build().await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();
    let request = |email: &'static str| {
        let client = client.clone();
        async move {
            client
                .post("/api/auth/magic-link")
                .json(&json!({ "email": email }))
                .send()
                .await
        }
    };

    request("nobody@example.com")
        .await
        .assert_status(202)
        .assert_json_path("message", json!(SENT));
    assert!(sent_mail(&app).await.is_empty());

    request("ada@example.com").await.assert_status(202);
    request("ada@example.com").await.assert_status(202);
    assert_eq!(sent_mail(&app).await.len(), 1, "one mail per minute");
    // Without a URL the mail carries the token itself.
    let text = last_text(&app).await;
    let token = text
        .split("Your sign-in token: ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("the token")
        .to_owned();

    clock.advance(Duration::from_secs(901));
    client
        .post("/api/auth/magic-link/verify")
        .json(&json!({ "token": token }))
        .send()
        .await
        .assert_error(401, "Invalid or expired sign-in code");

    // A new link replaces the old one.
    request("ada@example.com").await.assert_status(202);
    let db = app.get::<Db>().await;
    let rows = login_token::Entity::find().all(db.conn()).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "magic_link");
    assert!(!rows[0].token_hash.contains(&token));

    for bad in ["", "garbage", "0190a1b2-0000-7000-8000-000000000000.short"] {
        client
            .post("/api/auth/magic-link/verify")
            .json(&json!({ "token": bad }))
            .send()
            .await
            .assert_status(if bad.is_empty() { 400 } else { 401 });
    }
}

#[tokio::test]
async fn an_emailed_code_signs_in_and_wrong_guesses_burn_it() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = mail_app(&clock)
        .config("auth.otp.max_attempts", 3)
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();
    let send_code = || {
        let client = client.clone();
        async move {
            client
                .post("/api/auth/otp")
                .json(&json!({ "email": "ada@example.com" }))
                .send()
                .await
                .assert_status(202)
        }
    };
    let verify = |code: String| {
        let client = client.clone();
        async move {
            client
                .post("/api/auth/otp/verify")
                .json(&json!({ "email": "ada@example.com", "code": code }))
                .send()
                .await
        }
    };

    send_code().await;
    let mail = sent_mail(&app).await;
    assert_eq!(mail[0].subject, "Your sign-in code");
    let code = code_in(&last_text(&app).await);
    assert_eq!(code.len(), 6);
    verify(code.clone())
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("ada@example.com"));
    verify(code)
        .await
        .assert_error(401, "Invalid or expired sign-in code");

    // Three wrong guesses burn the next code.
    clock.advance(Duration::from_secs(61));
    send_code().await;
    let code = code_in(&last_text(&app).await);
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..3 {
        verify(wrong.to_owned())
            .await
            .assert_error(401, "Invalid or expired sign-in code");
    }
    verify(code)
        .await
        .assert_error(401, "Invalid or expired sign-in code");
    let failed = sink.with_action("auth.login_failed");
    assert!(failed.iter().all(|entry| entry.metadata["method"] == "otp"));
    assert!(failed.len() >= 4);

    // Unknown addresses: the same answers.
    client
        .post("/api/auth/otp")
        .json(&json!({ "email": "nobody@example.com" }))
        .send()
        .await
        .assert_status(202)
        .assert_json_path("message", json!(SENT));
    client
        .post("/api/auth/otp/verify")
        .json(&json!({ "email": "nobody@example.com", "code": "123456" }))
        .send()
        .await
        .assert_error(401, "Invalid or expired sign-in code");
}

#[tokio::test]
async fn code_guesses_are_throttled_per_address_and_ip() {
    let clock = support::clock();
    let app = mail_app(&clock)
        .config("auth.login_throttle_per_minute", 2)
        .config("auth.otp.max_attempts", 10)
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();
    client
        .post("/api/auth/otp")
        .json(&json!({ "email": "ada@example.com" }))
        .send()
        .await
        .assert_status(202);
    let mut statuses = Vec::new();
    for _ in 0..3 {
        let status = client
            .post("/api/auth/otp/verify")
            .json(&json!({ "email": "ada@example.com", "code": "99999999" }))
            .send()
            .await
            .status()
            .as_u16();
        statuses.push(status);
    }
    assert_eq!(statuses, [401, 401, 429]);
}

#[tokio::test]
async fn passwordless_sign_ins_keep_the_second_factor() {
    let clock = support::clock();
    let app = mail_app(&clock).build().await;
    let user = support::create_user(&app, "ada@example.com").await;
    let secret = enable_two_factor(&app, &clock, &user).await;
    let client = app.client();
    client
        .post("/api/auth/otp")
        .json(&json!({ "email": "ada@example.com" }))
        .send()
        .await
        .assert_status(202);
    let code = code_in(&last_text(&app).await);
    let challenge: Value = client
        .post("/api/auth/otp/verify")
        .json(&json!({ "email": "ada@example.com", "code": code }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("two_factor_required", json!(true))
        .json_value();
    client
        .post("/api/auth/2fa/verify")
        .json(&json!({
            "challenge_token": challenge["challenge_token"],
            "code": totp_code(&secret, &clock),
        }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("ada@example.com"));
}
