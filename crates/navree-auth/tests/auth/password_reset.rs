//! `POST /api/auth/forgot-password` and `POST /api/auth/reset-password`.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_auth::password_reset_token;
use navree_contracts::{AuditSink, Clock, FixedClock};
use navree_crypto::sha256_hex;
use navree_db::Db;
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::{TestApp, TestClient, TestResponse};
use navree_view::ModuleBuilderViewExt;
use navree_orm::Model as _;
use serde_json::json;

use crate::support::{
    self, PASSWORD, RecordingSink, first_url, link_param, login, refresh, sent_mail, signed_in,
};

const NEW_PASSWORD: &str = "a brand new password";
const RESET_URL: &str = "https://app.example.com/reset-password";

async fn forgot(client: &TestClient, email: &str) -> TestResponse {
    client
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": email }))
        .send()
        .await
}

async fn reset(client: &TestClient, email: &str, token: &str, password: &str) -> TestResponse {
    client
        .post("/api/auth/reset-password")
        .json(&json!({ "email": email, "token": token, "password": password }))
        .send()
        .await
}

/// The token of the last reset mail (from its link).
async fn mailed_token(app: &TestApp) -> String {
    let sent = sent_mail(app).await;
    let text = sent
        .last()
        .and_then(|mail| mail.text.clone())
        .expect("a reset mail with a text body");
    let link = first_url(&text).expect("a link in the mail");
    link_param(&link, "token").expect("a token in the link")
}

#[tokio::test]
async fn forgot_password_always_202_and_sends_mail_for_known_email() {
    let app = support::mail_builder()
        .config("auth.password_reset.url", RESET_URL)
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();

    let unknown = forgot(&client, "nobody@example.com")
        .await
        .assert_status(202);
    assert!(
        sent_mail(&app).await.is_empty(),
        "no mail for unknown emails"
    );

    let known = forgot(&client, "ADA@Example.com").await.assert_status(202);
    assert_eq!(
        known.json_value(),
        unknown.json_value(),
        "the response does not tell whether the account exists"
    );
    let sent = sent_mail(&app).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].to, vec!["ada@example.com".to_owned()]);
    assert!(!sent[0].subject.is_empty());
    let text = sent[0].text.clone().expect("a text body");
    let link = first_url(&text).expect("a link");
    assert!(link.starts_with(&format!("{RESET_URL}?")), "{link}");
    assert_eq!(
        link_param(&link, "email").as_deref(),
        Some("ada@example.com")
    );
    let token = link_param(&link, "token").expect("token");
    assert!(token.len() >= 40, "{token}");
    assert!(
        sent[0]
            .html
            .as_deref()
            .is_some_and(|html| html.contains(&token)),
        "the HTML body links too"
    );

    // Only the SHA-256 of the token is stored.
    let db = app.get::<Db>().await;
    let row = password_reset_token::Model::find(&*db, "ada@example.com".to_owned())
        .await
        .unwrap()
        .expect("a reset row");
    assert_eq!(row.token_hash, sha256_hex(token.as_bytes()));

    forgot(&client, "not-an-email")
        .await
        .assert_validation_error("email");
}

#[tokio::test]
async fn forgot_password_without_a_reset_url_mails_the_token() {
    let app = support::mail_builder().build().await;
    support::create_user(&app, "ada@example.com").await;
    forgot(&app.client(), "ada@example.com")
        .await
        .assert_status(202);
    let sent = sent_mail(&app).await;
    let text = sent[0].text.clone().unwrap();
    assert!(first_url(&text).is_none(), "{text}");
    let db = app.get::<Db>().await;
    let row = password_reset_token::Model::find(&*db, "ada@example.com".to_owned())
        .await
        .unwrap()
        .unwrap();
    let token = text
        .split_whitespace()
        .find(|word| sha256_hex(word.as_bytes()) == row.token_hash);
    assert!(token.is_some(), "the token is in the mail: {text}");
}

#[tokio::test]
async fn forgot_password_works_without_a_mailer() {
    let app = support::app().await;
    support::create_user(&app, "ada@example.com").await;
    forgot(&app.client(), "ada@example.com")
        .await
        .assert_status(202);
}

#[tokio::test]
async fn forgot_password_is_not_resent_within_the_throttle_window() {
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = support::mail_builder()
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();
    forgot(&client, "ada@example.com").await.assert_status(202);
    forgot(&client, "ada@example.com").await.assert_status(202);
    assert_eq!(sent_mail(&app).await.len(), 1, "one mail per minute");
    clock.advance(Duration::from_secs(61));
    forgot(&client, "ada@example.com").await.assert_status(202);
    assert_eq!(sent_mail(&app).await.len(), 2);
}

#[tokio::test]
async fn reset_password_with_valid_token_revokes_refresh_tokens() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::mail_builder()
        .config("auth.password_reset.url", RESET_URL)
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (user, _access, refresh_token) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    forgot(&client, "ada@example.com").await.assert_status(202);
    let token = mailed_token(&app).await;

    reset(&client, "Ada@Example.com", &token, NEW_PASSWORD)
        .await
        .assert_status(204);

    refresh(&client, &refresh_token)
        .await
        .assert_error(401, "Invalid refresh token");
    login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_error(401, "Invalid credentials");
    login(&client, "ada@example.com", NEW_PASSWORD)
        .await
        .assert_status(200);

    // The token is single-use.
    reset(&client, "ada@example.com", &token, "yet another password")
        .await
        .assert_error(400, "Invalid or expired password reset token");
    let db = app.get::<Db>().await;
    assert!(
        password_reset_token::Model::find(&*db, "ada@example.com".to_owned())
            .await
            .unwrap()
            .is_none()
    );

    let requested = sink.with_action("auth.password_reset_requested");
    assert_eq!(requested.len(), 1, "{requested:?}");
    assert_eq!(requested[0].actor_id, Some(user.id));
    let done = sink.with_action("auth.password_reset");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].actor_id, Some(user.id));
    let recorded = serde_json::to_string(&sink.entries()).unwrap();
    assert!(!recorded.contains(&token), "{recorded}");
    assert!(!recorded.contains(NEW_PASSWORD), "{recorded}");
}

#[tokio::test]
async fn reset_token_expires() {
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = support::mail_builder()
        .config("auth.password_reset.url", RESET_URL)
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    let client = app.client();

    forgot(&client, "ada@example.com").await.assert_status(202);
    let token = mailed_token(&app).await;
    clock.advance(Duration::from_secs(60 * 60 + 1));
    reset(&client, "ada@example.com", &token, NEW_PASSWORD)
        .await
        .assert_error(400, "Invalid or expired password reset token");
    login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_status(200);

    // A fresh token used within the hour works.
    forgot(&client, "ada@example.com").await.assert_status(202);
    let token = mailed_token(&app).await;
    clock.advance(Duration::from_secs(59 * 60));
    reset(&client, "ada@example.com", &token, NEW_PASSWORD)
        .await
        .assert_status(204);
}

#[tokio::test]
async fn reset_password_rejects_wrong_tokens_and_short_passwords() {
    let app = support::mail_builder()
        .config("auth.password_reset.url", RESET_URL)
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    support::create_user(&app, "grace@example.com").await;
    let client = app.client();
    forgot(&client, "ada@example.com").await.assert_status(202);
    let token = mailed_token(&app).await;

    reset(&client, "ada@example.com", "not-the-token", NEW_PASSWORD)
        .await
        .assert_error(400, "Invalid or expired password reset token");
    reset(&client, "grace@example.com", &token, NEW_PASSWORD)
        .await
        .assert_error(400, "Invalid or expired password reset token");
    reset(&client, "nobody@example.com", &token, NEW_PASSWORD)
        .await
        .assert_error(400, "Invalid or expired password reset token");
    reset(&client, "ada@example.com", &token, "short")
        .await
        .assert_validation_error("password");
    // The failures did not use the token up.
    reset(&client, "ada@example.com", &token, NEW_PASSWORD)
        .await
        .assert_status(204);
}

/// App templates for the account mails.
struct MailTemplates;

impl Module for MailTemplates {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[
            (
                "mail/reset.html",
                "<p>Hi {{ name }}: <a href=\"{{ url }}\">reset</a> within {{ expires_in_minutes }} minutes</p>",
            ),
            ("mail/reset.txt", "Hi {{ name }}, your token is {{ token }} ({{ email }})"),
            ("mail/verify.txt", "Confirm {{ email }}: {{ url }} ({{ expires_in_minutes }} minutes)"),
        ]);
    }
}

#[tokio::test]
async fn account_mails_use_the_configured_templates() {
    let app = support::mail_builder()
        .module(MailTemplates)
        .config("auth.password_reset.url", RESET_URL)
        .config("auth.password_reset.mail_template", "mail/reset")
        .config("auth.verification.mail_template", "mail/verify")
        .config("auth.verification.base_url", "https://app.example.com")
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();

    forgot(&client, "ada@example.com").await.assert_status(202);
    let sent = sent_mail(&app).await;
    let html = sent[0].html.clone().expect("the HTML template");
    // navree-view escapes HTML (also `/` as `&#x2f;`, harmless in attributes).
    assert!(html.starts_with("<p>Hi Ada Lovelace: <a href="), "{html}");
    assert!(html.contains("reset-password?token="), "{html}");
    assert!(html.contains("within 60 minutes"), "{html}");
    let text = sent[0].text.clone().expect("the text template");
    assert!(
        text.starts_with("Hi Ada Lovelace, your token is "),
        "{text}"
    );
    assert!(text.ends_with("(ada@example.com)"), "{text}");

    client
        .post("/api/auth/email/verification")
        .bearer(&access)
        .send()
        .await
        .assert_status(202);
    let sent = sent_mail(&app).await;
    let text = sent[1].text.clone().expect("the text template");
    assert!(
        text.starts_with("Confirm ada@example.com: https://app.example.com/api/auth/email/verify/"),
        "{text}"
    );
    assert!(text.ends_with("(60 minutes)"), "{text}");
    assert!(sent[1].html.is_none());
}
