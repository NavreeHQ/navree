//! `SmtpTransport` delivering over TLS to Mailpit servers that answer with a
//! certificate no public authority signed, with that certificate trusted.
//!
//! These run in the integration workflow (`.github/workflows/integrations.yml`),
//! which starts the servers with `scripts/integration-services.sh start`; they
//! are `#[ignore]`d in the other suites. Run them with `-- --ignored`. Their
//! variables:
//!
//! - `NAVREE_TEST_TLS_SMTP_CA`: the path of the servers' certificate (PEM);
//! - `NAVREE_TEST_TLS_SMTP_STARTTLS_URL` and `NAVREE_TEST_TLS_SMTP_STARTTLS_API`:
//!   SMTP that requires STARTTLS and a login, and that server's HTTP API;
//! - `NAVREE_TEST_TLS_SMTP_TLS_URL` and `NAVREE_TEST_TLS_SMTP_TLS_API`: SMTP over
//!   TLS from the first byte, with a login, and its HTTP API.
//!
//! A missing variable fails the test when `NAVREE_TEST_REQUIRE_SERVICES=1`
//! and prints `SKIPPED` otherwise (see `mailpit.rs`).

use navree_mail::{MailTransport, Message, SmtpTransport};

use crate::mailpit::{
    addresses, mentions_the_certificate, message, received, service, unique, whole,
};

/// The PEM text of the servers' certificate.
fn certificate(test: &str) -> Option<String> {
    let path = service("NAVREE_TEST_TLS_SMTP_CA", test)?;
    Some(
        std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("{test}: cannot read the certificate at {path}: {error}")
        }),
    )
}

/// The transport for `url`, trusting `pem`.
fn trusting(url: &str, pem: &str) -> SmtpTransport {
    SmtpTransport::from_url(url)
        .expect("url")
        .with_root_certificate(pem)
        .expect("the certificate is accepted")
}

/// The files of [`with_files`].
const PDF: &[u8] = b"%PDF-1.7 INV-2026-0042";
const CSV: &[u8] = b"city,rate\nLudhiana,18\n";

/// A message with an HTML body and two attachments.
fn with_files(subject: &str) -> Message {
    message(subject)
        .html("<p>Hello, <b>Simran</b></p>")
        .attach("invoice.pdf", "application/pdf", PDF.to_vec())
        .attach("rates.csv", "text/csv; charset=utf-8", CSV.to_vec())
}

/// What Mailpit says about `received["Attachments"]`: file names and sizes.
fn attachments(received: &serde_json::Value) -> Vec<(String, u64)> {
    received["Attachments"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .map(|file| {
                    (
                        file["FileName"].as_str().unwrap_or_default().to_owned(),
                        file["Size"].as_u64().unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn starttls_delivers_with_attachments_when_the_certificate_is_trusted() {
    let test = "starttls_delivers_with_attachments_when_the_certificate_is_trusted";
    let (Some(smtp), Some(api), Some(pem)) = (
        service("NAVREE_TEST_TLS_SMTP_STARTTLS_URL", test),
        service("NAVREE_TEST_TLS_SMTP_STARTTLS_API", test),
        certificate(test),
    ) else {
        return;
    };
    assert!(smtp.contains("tls=required"), "{smtp}");
    let subject = unique("STARTTLS delivery");
    let transport = trusting(&smtp, &pem);

    transport
        .check()
        .await
        .expect("the login is accepted over TLS");
    transport
        .send(&with_files(&subject))
        .await
        .expect("the server accepts the message over STARTTLS");

    let received = received(&api, &subject).await;
    assert_eq!(received["Subject"], subject.as_str());
    assert_eq!(addresses(&received["To"]), ["simran@example.com"]);
    assert_eq!(
        attachments(&received),
        [
            ("invoice.pdf".to_owned(), PDF.len() as u64),
            ("rates.csv".to_owned(), CSV.len() as u64)
        ],
        "{received}"
    );
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn opportunistic_starttls_uses_tls_when_the_certificate_is_trusted() {
    let test = "opportunistic_starttls_uses_tls_when_the_certificate_is_trusted";
    let (Some(smtp), Some(api), Some(pem)) = (
        service("NAVREE_TEST_TLS_SMTP_STARTTLS_URL", test),
        service("NAVREE_TEST_TLS_SMTP_STARTTLS_API", test),
        certificate(test),
    ) else {
        return;
    };
    let opportunistic = smtp.replace("tls=required", "tls=opportunistic");
    assert_ne!(opportunistic, smtp);
    let subject = unique("Opportunistic delivery");

    trusting(&opportunistic, &pem)
        .send(&message(&subject))
        .await
        .expect("the server accepts the message");

    // The server requires STARTTLS, so the message cannot have gone in plain
    // text.
    assert_eq!(received(&api, &subject).await["Subject"], subject.as_str());

    // Without the certificate the same transport is refused, not downgraded.
    let error = SmtpTransport::from_url(&opportunistic)
        .expect("url")
        .send(&message(&unique("Untrusted")))
        .await
        .expect_err("a bad certificate is an error, not a downgrade");
    assert!(mentions_the_certificate(&whole(&error)), "{error}");
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn tls_from_the_first_byte_delivers_when_the_certificate_is_trusted() {
    let test = "tls_from_the_first_byte_delivers_when_the_certificate_is_trusted";
    let (Some(smtp), Some(api), Some(pem)) = (
        service("NAVREE_TEST_TLS_SMTP_TLS_URL", test),
        service("NAVREE_TEST_TLS_SMTP_TLS_API", test),
        certificate(test),
    ) else {
        return;
    };
    assert!(smtp.starts_with("smtps://"), "{smtp}");
    let subject = unique("Implicit TLS delivery");
    let transport = trusting(&smtp, &pem);

    transport
        .check()
        .await
        .expect("the login is accepted over TLS");
    transport
        .send(&with_files(&subject))
        .await
        .expect("the server accepts the message over TLS");

    let received = received(&api, &subject).await;
    assert_eq!(received["Subject"], subject.as_str());
    assert_eq!(attachments(&received).len(), 2, "{received}");
}
