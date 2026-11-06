//! `SmtpTransport` against Mailpit, a real SMTP server: delivery read back
//! through Mailpit's HTTP API, login with the right and the wrong password,
//! and the TLS modes with a certificate no public authority signed.
//!
//! Needs the Mailpit servers of `scripts/ci-services.sh start`, which prints
//! these variables:
//!
//! - `NAVREE_TEST_SMTP_URL` and `NAVREE_TEST_MAILPIT_URL`: SMTP without login
//!   or TLS, and that server's HTTP API;
//! - `NAVREE_TEST_SMTP_AUTH_URL` and `NAVREE_TEST_MAILPIT_AUTH_URL`: SMTP that
//!   asks for a login (PLAIN or LOGIN), and its HTTP API;
//! - `NAVREE_TEST_SMTP_STARTTLS_URL`: SMTP that requires STARTTLS and a login;
//! - `NAVREE_TEST_SMTP_TLS_URL`: SMTP over TLS from the first byte, with a
//!   login.
//!
//! A test whose variables are not set prints `SKIPPED` and passes, unless
//! `NAVREE_TEST_REQUIRE_SERVICES=1`, which makes it fail.
//!
//! The two TLS servers answer with a self-signed certificate. The transport
//! trusts the public authorities only, so it must refuse them: those tests
//! show that the TLS handshake runs and that the certificate is checked, not
//! that a delivery over TLS works.

use std::error::Error as _;
use std::time::{Duration, Instant};

use navree_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use navree_mail::{MailError, MailTransport, Message, SmtpTransport};
use serde_json::Value;

/// The value of `variable`; `None` after a notice, or a panic when
/// `NAVREE_TEST_REQUIRE_SERVICES=1`.
pub(crate) fn service(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: {variable} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// A client for Mailpit's API on the loopback interface.
pub(crate) fn client() -> HttpClient {
    HttpClient::new(HttpClientOptions {
        retries: 0,
        ssrf: SsrfPolicy {
            block_private: false,
            ..SsrfPolicy::default()
        },
        ..HttpClientOptions::default()
    })
    .expect("a client")
}

/// A subject no other test (or earlier run) used.
pub(crate) fn unique(label: &str) -> String {
    format!("{label} {}", uuid::Uuid::now_v7().simple())
}

/// The message with `subject` in the Mailpit server at `api`, once it is
/// there (Mailpit stores a message before it answers the end of `DATA`; the
/// wait only covers its search index).
pub(crate) async fn received(api: &str, subject: &str) -> Value {
    let http = client();
    let query = format!("subject:\"{subject}\"");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found = http
            .get(format!("{api}/api/v1/search"))
            .query(&[("query", query.as_str())])
            .send()
            .await
            .expect("Mailpit's API answers")
            .error_for_status()
            .expect("the search succeeds")
            .json::<Value>()
            .expect("JSON");
        if let Some(id) = found["messages"][0]["ID"].as_str() {
            return http
                .get(format!("{api}/api/v1/message/{id}"))
                .send()
                .await
                .expect("Mailpit's API answers")
                .error_for_status()
                .expect("the message is there")
                .json::<Value>()
                .expect("JSON");
        }
        assert!(
            Instant::now() < deadline,
            "Mailpit at {api} never received `{subject}`"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The addresses of a recipient list of Mailpit's JSON (`To`, `Cc`, `Bcc`).
pub(crate) fn addresses(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry["Address"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// An error and everything under it, lower case, to look for a word in.
pub(crate) fn whole(error: &MailError) -> String {
    let mut text = format!("{error} {error:?}");
    let mut source = error.source();
    while let Some(inner) = source {
        text.push(' ');
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    text.to_lowercase()
}

pub(crate) fn message(subject: &str) -> Message {
    Message::new()
        .from("Gurpreet Kaur <gurpreet@example.com>")
        .to("simran@example.com")
        .subject(subject)
        .text("Hello, Simran")
}

/// What a client that checks certificates says about a self-signed one.
pub(crate) fn mentions_the_certificate(text: &str) -> bool {
    text.contains("certificate")
        || text.contains("unknownissuer")
        || text.contains("unknown issuer")
}

// CONN-030
#[tokio::test]
async fn mailpit_receives_a_message_with_its_recipients_and_both_bodies() {
    let test = "mailpit_receives_a_message_with_its_recipients_and_both_bodies";
    let (Some(smtp), Some(api)) = (
        service("NAVREE_TEST_SMTP_URL", test),
        service("NAVREE_TEST_MAILPIT_URL", test),
    ) else {
        return;
    };
    let subject = unique("Plain delivery");
    let transport = SmtpTransport::from_url(&smtp).expect("url");
    let message = Message::new()
        .from("Gurpreet Kaur <gurpreet@example.com>")
        .to("simran@example.com")
        .cc("harpreet@example.com")
        .bcc("hidden@example.com")
        .reply_to("support@example.com")
        .subject(subject.as_str())
        .text("Hello, Simran")
        .html("<p>Hello, <b>Simran</b></p>");

    transport
        .send(&message)
        .await
        .expect("Mailpit accepts the message");

    let received = received(&api, &subject).await;
    assert_eq!(received["Subject"], subject.as_str());
    assert_eq!(
        received["From"]["Address"], "gurpreet@example.com",
        "{received}"
    );
    assert_eq!(addresses(&received["To"]), ["simran@example.com"]);
    assert_eq!(addresses(&received["Cc"]), ["harpreet@example.com"]);
    assert_eq!(addresses(&received["ReplyTo"]), ["support@example.com"]);
    // The blind copy reached the server in the envelope only.
    assert_eq!(addresses(&received["Bcc"]), ["hidden@example.com"]);
    assert!(received["Text"].as_str().unwrap().contains("Hello, Simran"));
    assert!(
        received["HTML"]
            .as_str()
            .unwrap()
            .contains("<p>Hello, <b>Simran</b></p>"),
        "{received}"
    );
}

// CONN-030
#[tokio::test]
async fn mailpit_keeps_a_subject_and_body_that_are_not_ascii() {
    let test = "mailpit_keeps_a_subject_and_body_that_are_not_ascii";
    let (Some(smtp), Some(api)) = (
        service("NAVREE_TEST_SMTP_URL", test),
        service("NAVREE_TEST_MAILPIT_URL", test),
    ) else {
        return;
    };
    let subject = unique("Invoice \u{20b9}1,00,000 for Gurpreet");
    let body = "Total: \u{20b9}1,00,000.00\nDue in Ludhiana by 31 March";
    let transport = SmtpTransport::from_url(&smtp).expect("url");
    let message = message(&subject).text(body);

    transport
        .send(&message)
        .await
        .expect("Mailpit accepts the message");

    let received = received(&api, &subject).await;
    assert_eq!(received["Subject"], subject.as_str());
    assert_eq!(
        received["Text"]
            .as_str()
            .unwrap()
            .replace("\r\n", "\n")
            .trim_end(),
        body
    );
}

// CONN-030
#[tokio::test]
async fn the_right_login_delivers_and_check_authenticates() {
    let test = "the_right_login_delivers_and_check_authenticates";
    let (Some(smtp), Some(api)) = (
        service("NAVREE_TEST_SMTP_AUTH_URL", test),
        service("NAVREE_TEST_MAILPIT_AUTH_URL", test),
    ) else {
        return;
    };
    let subject = unique("Login delivery");
    let transport = SmtpTransport::from_url(&smtp).expect("url");

    transport.check().await.expect("the login is accepted");
    transport
        .send(&message(&subject))
        .await
        .expect("Mailpit accepts the message");

    let received = received(&api, &subject).await;
    assert_eq!(received["Subject"], subject.as_str());
}

// CONN-030
#[tokio::test]
async fn a_wrong_password_is_refused_without_echoing_it() {
    let test = "a_wrong_password_is_refused_without_echoing_it";
    let Some(smtp) = service("NAVREE_TEST_SMTP_AUTH_URL", test) else {
        return;
    };
    let wrong = smtp.replace("navree-secret", "not-the-password");
    assert_ne!(wrong, smtp, "the variable carries the test login");
    let transport = SmtpTransport::from_url(&wrong).expect("url");

    let sent = transport
        .send(&message("Wrong password"))
        .await
        .expect_err("the server refuses the login");
    assert!(
        matches!(
            sent,
            MailError::Transport {
                transport: "smtp",
                ..
            }
        ),
        "{sent}"
    );
    // 535: authentication credentials invalid.
    assert!(whole(&sent).contains("535"), "{sent}");
    assert!(!whole(&sent).contains("not-the-password"), "{sent}");

    let checked = transport.check().await.expect_err("check fails too");
    assert!(matches!(checked, MailError::Unusable(_)), "{checked}");
    assert!(!whole(&checked).contains("not-the-password"), "{checked}");
}

// CONN-030
#[tokio::test]
async fn a_server_that_wants_a_login_refuses_mail_without_one() {
    let test = "a_server_that_wants_a_login_refuses_mail_without_one";
    let Some(smtp) = service("NAVREE_TEST_SMTP_AUTH_URL", test) else {
        return;
    };
    // `smtp://navree:navree-secret@127.0.0.1:1026` without the login.
    let (scheme, rest) = smtp.split_once("://").expect("a URL");
    let without = format!("{scheme}://{}", rest.rsplit_once('@').expect("a login").1);
    let transport = SmtpTransport::from_url(&without).expect("url");

    let error = transport
        .send(&message("No login"))
        .await
        .expect_err("the server wants a login");

    // 530: authentication required.
    assert!(whole(&error).contains("530"), "{error}");
}

// CONN-030
#[tokio::test]
async fn starttls_is_negotiated_and_a_certificate_nobody_signed_is_refused() {
    let test = "starttls_is_negotiated_and_a_certificate_nobody_signed_is_refused";
    let Some(smtp) = service("NAVREE_TEST_SMTP_STARTTLS_URL", test) else {
        return;
    };
    assert!(smtp.contains("tls=required"), "{smtp}");
    let transport = SmtpTransport::from_url(&smtp).expect("url");

    let error = transport
        .send(&message("Untrusted certificate"))
        .await
        .expect_err("the certificate is not trusted");

    // The server offered STARTTLS, the handshake ran, and the certificate was
    // the reason it stopped.
    assert!(mentions_the_certificate(&whole(&error)), "{error}");
    let checked = transport.check().await.expect_err("check fails too");
    assert!(mentions_the_certificate(&whole(&checked)), "{checked}");
}

// CONN-030
#[tokio::test]
async fn opportunistic_starttls_does_not_fall_back_to_plain_text_on_a_bad_certificate() {
    let test = "opportunistic_starttls_does_not_fall_back_to_plain_text_on_a_bad_certificate";
    let Some(smtp) = service("NAVREE_TEST_SMTP_STARTTLS_URL", test) else {
        return;
    };
    let opportunistic = smtp.replace("tls=required", "tls=opportunistic");
    assert_ne!(opportunistic, smtp);
    let transport = SmtpTransport::from_url(&opportunistic).expect("url");

    let error = transport
        .send(&message("Opportunistic"))
        .await
        .expect_err("a bad certificate is an error, not a downgrade");

    // Mailpit would also refuse plain text (530 STARTTLS first); the
    // certificate is what stopped the connection.
    assert!(mentions_the_certificate(&whole(&error)), "{error}");
}

// CONN-030
#[tokio::test]
async fn plain_text_to_a_server_that_requires_starttls_is_refused() {
    let test = "plain_text_to_a_server_that_requires_starttls_is_refused";
    let Some(smtp) = service("NAVREE_TEST_SMTP_STARTTLS_URL", test) else {
        return;
    };
    let plain = smtp.split('?').next().expect("a URL").to_owned();
    let transport = SmtpTransport::from_url(&plain).expect("url");

    let error = transport
        .send(&message("Plain text"))
        .await
        .expect_err("the server wants STARTTLS first");

    // 530: must issue a STARTTLS command first.
    let text = whole(&error);
    assert!(text.contains("530") || text.contains("starttls"), "{error}");
}

// CONN-030
#[tokio::test]
async fn tls_from_the_first_byte_checks_the_certificate_too() {
    let test = "tls_from_the_first_byte_checks_the_certificate_too";
    let Some(smtp) = service("NAVREE_TEST_SMTP_TLS_URL", test) else {
        return;
    };
    assert!(smtp.starts_with("smtps://"), "{smtp}");
    let transport = SmtpTransport::from_url(&smtp).expect("url");

    let error = transport
        .send(&message("Implicit TLS"))
        .await
        .expect_err("the certificate is not trusted");

    assert!(mentions_the_certificate(&whole(&error)), "{error}");
}
