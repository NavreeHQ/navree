//! Passwordless sign-in: magic links (`POST /api/auth/magic-link`) and
//! emailed codes (`POST /api/auth/otp`).

use std::sync::Arc;
use std::time::Duration;

use navree_auth::{User, login_token, refresh_token};
use navree_contracts::AuditSink;
use navree_db::Db;
use navree_orm::Model as _;
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::{Value, json};

use crate::support::{
    self, PASSWORD, RecordingSink, enable_two_factor, link_param, sent_mail, tokens, totp_code,
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

/// The account `id` as it is stored now.
async fn stored(app: &TestApp, id: uuid::Uuid) -> User {
    let db = app.get::<Db>().await;
    User::find(&*db, id)
        .await
        .unwrap()
        .expect("the account")
        .into_inner()
}

#[tokio::test]
async fn passwordless_routes_are_off_by_default() {
    let app = support::app().await;
    for path in ["/api/auth/magic-link", "/api/auth/otp"] {
        app.client()
            .post(path)
            .json(&json!({ "email": "anu@example.com" }))
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
    support::create_user(&app, "anu@example.com").await;
    let client = app.client();

    client
        .post("/api/auth/magic-link")
        .json(&json!({ "email": "Anu@Example.com" }))
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
        .assert_json_path("user.email", json!("anu@example.com"));
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
    support::create_user(&app, "anu@example.com").await;
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

    request("anu@example.com").await.assert_status(202);
    request("anu@example.com").await.assert_status(202);
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
    request("anu@example.com").await.assert_status(202);
    let db = app.get::<Db>().await;
    let rows = login_token::Model::all(&*db).await.unwrap().into_vec();
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
    support::create_user(&app, "anu@example.com").await;
    let client = app.client();
    let send_code = || {
        let client = client.clone();
        async move {
            client
                .post("/api/auth/otp")
                .json(&json!({ "email": "anu@example.com" }))
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
                .json(&json!({ "email": "anu@example.com", "code": code }))
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
        .assert_json_path("user.email", json!("anu@example.com"));
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
    support::create_user(&app, "anu@example.com").await;
    let client = app.client();
    client
        .post("/api/auth/otp")
        .json(&json!({ "email": "anu@example.com" }))
        .send()
        .await
        .assert_status(202);
    let mut statuses = Vec::new();
    for _ in 0..3 {
        let status = client
            .post("/api/auth/otp/verify")
            .json(&json!({ "email": "anu@example.com", "code": "99999999" }))
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
    let user = support::create_user(&app, "anu@example.com").await;
    let secret = enable_two_factor(&app, &clock, &user).await;
    let client = app.client();
    client
        .post("/api/auth/otp")
        .json(&json!({ "email": "anu@example.com" }))
        .send()
        .await
        .assert_status(202);
    let code = code_in(&last_text(&app).await);
    let challenge: Value = client
        .post("/api/auth/otp/verify")
        .json(&json!({ "email": "anu@example.com", "code": code }))
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
        .assert_json_path("user.email", json!("anu@example.com"));
}

/// A link or code mailed to an account's address signs in only while the
/// account holds that address: once it moves to another, neither signs in,
/// no session starts and the new address stays unverified.
#[tokio::test]
async fn a_link_mailed_before_an_address_change_does_not_prove_the_new_address() {
    let clock = support::clock();
    let app = mail_app(&clock)
        .config("auth.magic_link.url", "https://app.example.com/magic")
        .build()
        .await;
    let (user, access, _) = support::signed_in(&app, "harpreet@sutlej.in").await;
    let client = app.client();
    for path in ["/api/auth/magic-link", "/api/auth/otp"] {
        client
            .post(path)
            .json(&json!({ "email": "harpreet@sutlej.in" }))
            .send()
            .await
            .assert_status(202);
    }
    let mails = sent_mail(&app).await;
    let text_of = |subject: &str| {
        mails
            .iter()
            .find(|mail| mail.subject == subject)
            .and_then(|mail| mail.text.clone())
            .expect("the mail and its text body")
    };
    let token = link_param(&text_of("Your sign-in link"), "token")
        .expect("a token in the link");
    let code = code_in(&text_of("Your sign-in code"));

    client
        .patch("/api/auth/me")
        .bearer(&access)
        .json(&json!({ "email": "simran@sutlej.in", "current_password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email_verified_at", Value::Null);
    let db = app.get::<Db>().await;
    let unused = login_token::Model::query()
        .where_(login_token::Model::USER_ID.eq(user.id))
        .where_null(login_token::Model::USED_AT)
        .count(&*db)
        .await
        .unwrap();
    assert_eq!(unused, 0, "the change ends the link and the code");
    let sessions = || {
        refresh_token::Model::query()
            .where_(refresh_token::Model::USER_ID.eq(user.id))
    };
    let before = sessions().count(&*db).await.unwrap();

    client
        .post("/api/auth/magic-link/verify")
        .json(&json!({ "token": token }))
        .send()
        .await
        .assert_error(401, "Invalid or expired sign-in code");
    for email in ["simran@sutlej.in", "harpreet@sutlej.in"] {
        client
            .post("/api/auth/otp/verify")
            .json(&json!({ "email": email, "code": code }))
            .send()
            .await
            .assert_error(401, "Invalid or expired sign-in code");
    }

    let account = stored(&app, user.id).await;
    assert_eq!(account.email, "simran@sutlej.in");
    assert_eq!(account.email_verified_at, None);
    assert_eq!(
        sessions().count(&*db).await.unwrap(),
        before,
        "no session was started"
    );
}

/// The positive side: a link outlives a change of name, and a code mailed to
/// the account's new address signs in and verifies that address.
#[tokio::test]
async fn a_link_or_code_mailed_to_the_address_held_now_still_proves_it() {
    let clock = support::clock();
    let app = mail_app(&clock)
        .config("auth.magic_link.url", "https://app.example.com/magic")
        .build()
        .await;
    let (_, access, _) = support::signed_in(&app, "harpreet@sutlej.in").await;
    let client = app.client();
    let update_profile = |body: Value| {
        let (client, access) = (client.clone(), access.clone());
        async move {
            client
                .patch("/api/auth/me")
                .bearer(&access)
                .json(&body)
                .send()
                .await
                .assert_status(200)
        }
    };

    client
        .post("/api/auth/magic-link")
        .json(&json!({ "email": "harpreet@sutlej.in" }))
        .send()
        .await
        .assert_status(202);
    let token = link_param(&last_text(&app).await, "token")
        .expect("a token in the link");
    update_profile(json!({ "name": "Harpreet Kaur" })).await;
    let signed_in = client
        .post("/api/auth/magic-link/verify")
        .json(&json!({ "token": token }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("harpreet@sutlej.in"))
        .json_value();
    assert!(signed_in["user"]["email_verified_at"].is_string());

    let moved = json!({ "email": "harpreet.kaur@sutlej.in", "current_password": PASSWORD });
    update_profile(moved)
        .await
        .assert_json_path("email_verified_at", Value::Null);
    client
        .post("/api/auth/otp")
        .json(&json!({ "email": "harpreet.kaur@sutlej.in" }))
        .send()
        .await
        .assert_status(202);
    let code = code_in(&last_text(&app).await);
    let signed_in = client
        .post("/api/auth/otp/verify")
        .json(&json!({ "email": "harpreet.kaur@sutlej.in", "code": code }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("harpreet.kaur@sutlej.in"))
        .json_value();
    assert!(signed_in["user"]["email_verified_at"].is_string());
}
