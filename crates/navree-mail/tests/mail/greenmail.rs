//! Inbound mail from a real mail server: messages sent to GreenMail over SMTP
//! with `SmtpTransport`, read back over IMAP and POP3 by `Inbound::poll`, which
//! hands them to `Inbound::receive`. The bytes are what a mail server stores,
//! with its own headers added, not a fixture.
//!
//! These run in the integration workflow, which starts GreenMail with
//! `scripts/integration-services.sh start mail`; they are `#[ignore]`d in the
//! other suites. Run them with `-- --ignored`. The variables:
//!
//! - `NAVREE_TEST_GREENMAIL_SMTP_URL`: its SMTP server (`smtp://127.0.0.1:13025`);
//! - `NAVREE_TEST_GREENMAIL_IMAP`: its IMAP server (`127.0.0.1:13143`);
//! - `NAVREE_TEST_GREENMAIL_POP3`: its POP3 server (`127.0.0.1:13110`).
//!
//! GreenMail makes a mailbox for every address it receives mail for, and
//! accepts any password; each test uses an address of its own. The poller
//! logs in with that address, without TLS (GreenMail's TLS ports use a
//! certificate of its own, which the fake servers of `inbound/poll.rs`
//! cover). A missing variable fails the test when
//! `NAVREE_TEST_REQUIRE_SERVICES=1` and prints `SKIPPED` otherwise.

use navree_mail::inbound::{InboundStatus, MailboxPoller, PollAfter, PollSecurity};
use navree_mail::{MailTransport, Message, SmtpTransport};

use crate::inbound::inbound_app;
use crate::mailpit::{service, unique};

const PDF: &[u8] = b"%PDF-1.7 INV-2026-0042";
const CSV: &[u8] = "city,rate\nLudhiana,18\nAmritsar,12\n".as_bytes();

/// A mailbox address no other test uses.
fn address() -> String {
    format!("support-{}@example.test", uuid::Uuid::now_v7().simple())
}

/// The message these tests send: text and HTML, a Cc, two attachments.
fn invoice(to: &str, subject: &str) -> Message {
    Message::new()
        .from("Gurpreet Kaur <gurpreet@example.com>")
        .to(to)
        .cc("harpreet@example.com")
        .reply_to("accounts@example.com")
        .subject(subject)
        .text("Your invoice is attached.")
        .html("<p>Your invoice is attached.</p>")
        .attach("INV-2026-0042.pdf", "application/pdf", PDF.to_vec())
        .attach("rates.csv", "text/csv; charset=utf-8", CSV.to_vec())
}

async fn send(smtp: &str, message: &Message) {
    SmtpTransport::from_url(smtp)
        .expect("url")
        .send(message)
        .await
        .expect("GreenMail accepts the message");
}

/// `host:port` as the host and the port.
fn endpoint(address: &str) -> (&str, u16) {
    let (host, port) = address.rsplit_once(':').expect("host:port");
    (host, port.parse().expect("a port"))
}

fn imap(address: &str, mailbox: &str) -> MailboxPoller {
    let (host, port) = endpoint(address);
    MailboxPoller::imap(host)
        .port(port)
        .security(PollSecurity::None)
        .login(mailbox, "any")
}

fn pop3(address: &str, mailbox: &str) -> MailboxPoller {
    let (host, port) = endpoint(address);
    MailboxPoller::pop3(host)
        .port(port)
        .security(PollSecurity::None)
        .login(mailbox, "any")
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_message_polled_over_imap_is_parsed_and_routed_by_inbound() {
    let test = "a_message_polled_over_imap_is_parsed_and_routed_by_inbound";
    let (Some(smtp), Some(imap_address)) = (
        service("NAVREE_TEST_GREENMAIL_SMTP_URL", test),
        service("NAVREE_TEST_GREENMAIL_IMAP", test),
    ) else {
        return;
    };
    let to = address();
    let subject = unique("Invoice INV-2026-0042");
    send(&smtp, &invoice(&to, &subject)).await;
    let mail = inbound_app(&["*"], |builder| builder).await;
    let inbound = mail.inbound().await;

    let report = inbound
        .poll(&imap(&imap_address, &to))
        .await
        .expect("the mailbox is read");

    assert_eq!(report.received.len(), 1, "one message for {to}: {report:?}");
    assert_eq!(report.received[0].status, InboundStatus::Handled);
    assert!(
        report.refused.is_empty() && report.failed.is_empty(),
        "{report:?}"
    );
    let seen = mail.seen();
    assert_eq!(seen.len(), 1);
    let (pattern, address, mail_seen) = &seen[0];
    assert_eq!((pattern.as_str(), address.as_str()), ("*", to.as_str()));
    assert_eq!(mail_seen.source, "imap");
    assert!(mail_seen.provider_id.is_some(), "the IMAP UID");
    assert_eq!(mail_seen.subject.as_deref(), Some(subject.as_str()));
    assert_eq!(
        mail_seen.from.as_ref().map(|from| from.email.as_str()),
        Some("gurpreet@example.com")
    );
    assert_eq!(mail_seen.cc[0].email, "harpreet@example.com");
    assert_eq!(mail_seen.reply_to[0].email, "accounts@example.com");
    assert!(
        mail_seen
            .text
            .as_deref()
            .unwrap()
            .contains("Your invoice is attached.")
    );
    assert!(
        mail_seen
            .html
            .as_deref()
            .unwrap()
            .contains("<p>Your invoice")
    );
    let files: Vec<(&str, &[u8])> = mail_seen
        .attachments
        .iter()
        .map(|file| {
            (
                file.filename.as_deref().unwrap_or(""),
                file.content.as_ref(),
            )
        })
        .collect();
    assert_eq!(
        files,
        [("INV-2026-0042.pdf", PDF), ("rates.csv", CSV)],
        "the attachments arrive whole"
    );
    // Every header the server stored is read, whatever it added on the way in.
    let names: Vec<_> = mail_seen
        .headers
        .iter()
        .map(|h| h.name.to_lowercase())
        .collect();
    for expected in ["from", "to", "subject", "message-id", "mime-version"] {
        assert!(names.iter().any(|name| name == expected), "{names:?}");
    }

    // The message was flagged \Seen, so the next poll finds nothing.
    let again = inbound
        .poll(&imap(&imap_address, &to))
        .await
        .expect("the mailbox is read again");
    assert!(again.received.is_empty(), "{again:?}");
    assert_eq!(mail.seen().len(), 1);
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn an_imap_message_can_be_deleted_from_the_server_once_received() {
    let test = "an_imap_message_can_be_deleted_from_the_server_once_received";
    let (Some(smtp), Some(imap_address)) = (
        service("NAVREE_TEST_GREENMAIL_SMTP_URL", test),
        service("NAVREE_TEST_GREENMAIL_IMAP", test),
    ) else {
        return;
    };
    let to = address();
    send(&smtp, &invoice(&to, &unique("Invoice to delete"))).await;
    let mail = inbound_app(&["*"], |builder| builder).await;
    let inbound = mail.inbound().await;

    let first = inbound
        .poll(&imap(&imap_address, &to).after(PollAfter::Delete))
        .await
        .expect("the mailbox is read");
    assert_eq!(first.received.len(), 1, "{first:?}");

    // Nothing is left to read.
    let again = inbound
        .poll(&imap(&imap_address, &to))
        .await
        .expect("the mailbox is read again");
    assert!(again.received.is_empty(), "{again:?}");
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn gurmukhi_and_rupee_text_survives_a_real_mail_server() {
    let test = "gurmukhi_and_rupee_text_survives_a_real_mail_server";
    let (Some(smtp), Some(imap_address)) = (
        service("NAVREE_TEST_GREENMAIL_SMTP_URL", test),
        service("NAVREE_TEST_GREENMAIL_IMAP", test),
    ) else {
        return;
    };
    let to = address();
    let subject = unique(
        "\u{a38}\u{a24} \u{a38}\u{a4d}\u{a30}\u{a40} \u{a05}\u{a15}\u{a3e}\u{a32}: \u{20b9}1,00,000",
    );
    let body =
        "\u{a07}\u{a28}\u{a35}\u{a4c}\u{a07}\u{a38}: \u{20b9}1,00,000.00\nLudhiana, 31 March";
    send(
        &smtp,
        &Message::new()
            .from("Gurpreet Kaur <gurpreet@example.com>")
            .to(to.as_str())
            .subject(subject.as_str())
            .text(body),
    )
    .await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    mail.inbound()
        .await
        .poll(&imap(&imap_address, &to))
        .await
        .expect("the mailbox is read");

    let seen = mail.seen();
    let received = &seen[0].2;
    assert_eq!(received.subject.as_deref(), Some(subject.as_str()));
    let text = received.text.as_deref().unwrap().replace("\r\n", "\n");
    assert_eq!(text.trim_end(), body);
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_message_polled_over_pop3_is_parsed_routed_and_deleted() {
    let test = "a_message_polled_over_pop3_is_parsed_routed_and_deleted";
    let (Some(smtp), Some(pop3_address)) = (
        service("NAVREE_TEST_GREENMAIL_SMTP_URL", test),
        service("NAVREE_TEST_GREENMAIL_POP3", test),
    ) else {
        return;
    };
    let to = address();
    let subject = unique("Invoice by POP3");
    send(&smtp, &invoice(&to, &subject)).await;
    let mail = inbound_app(&["*"], |builder| builder).await;
    let inbound = mail.inbound().await;

    let report = inbound
        .poll(&pop3(&pop3_address, &to))
        .await
        .expect("the mailbox is read");

    assert_eq!(report.received.len(), 1, "{report:?}");
    assert_eq!(report.received[0].status, InboundStatus::Handled);
    let seen = mail.seen();
    let received = &seen[0].2;
    assert_eq!(received.source, "pop3");
    assert!(received.provider_id.is_some(), "the POP3 unique id");
    assert_eq!(received.subject.as_deref(), Some(subject.as_str()));
    assert_eq!(received.attachments.len(), 2);
    assert_eq!(received.attachments[0].content.as_ref(), PDF);

    // POP3 has no flag: the message was deleted, so a second poll finds none.
    let again = inbound
        .poll(&pop3(&pop3_address, &to))
        .await
        .expect("the mailbox is read again");
    assert!(again.received.is_empty(), "{again:?}");
}
