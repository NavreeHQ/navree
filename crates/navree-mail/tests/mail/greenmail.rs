//! Inbound mail from a real mail server: messages sent to GreenMail over SMTP
//! with `SmtpTransport`, fetched back over IMAP and POP3, and handed to
//! `Inbound::receive` the way the docs show for a mailbox poller ("Your own
//! sources"). The bytes are what a mail server stores, with its own headers
//! added, not a fixture.
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
//! accepts any password; each test uses an address of its own. A missing
//! variable fails the test when `NAVREE_TEST_REQUIRE_SERVICES=1` and prints
//! `SKIPPED` otherwise.
//!
//! The IMAP and POP3 clients below are the smallest that read a mailbox: they
//! belong to the test, not to navree-mail, which has no poller of its own.

use navree_mail::inbound::InboundStatus;
use navree_mail::{Incoming, MailTransport, Message, SmtpTransport};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

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

// ---------------------------------------------------------------------
// IMAP
// ---------------------------------------------------------------------

/// An IMAP session: the commands of a mailbox poller, no TLS.
struct Imap {
    reader: BufReader<TcpStream>,
    tag: u32,
}

/// What a command answered: the untagged lines and the literals in them.
struct Answer {
    lines: Vec<String>,
    literals: Vec<Vec<u8>>,
}

impl Imap {
    async fn connect(address: &str) -> Imap {
        let stream = TcpStream::connect(address)
            .await
            .unwrap_or_else(|error| panic!("cannot connect to IMAP at {address}: {error}"));
        let mut imap = Imap {
            reader: BufReader::new(stream),
            tag: 0,
        };
        let greeting = imap.line().await;
        assert!(greeting.starts_with("* OK"), "{greeting}");
        imap
    }

    async fn line(&mut self) -> String {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).await.expect("read");
        assert!(read > 0, "the server closed the connection");
        line
    }

    /// Sends `command` and reads until its tagged answer, which must be OK.
    async fn run(&mut self, command: &str) -> Answer {
        self.tag += 1;
        let tag = format!("a{}", self.tag);
        self.reader
            .get_mut()
            .write_all(format!("{tag} {command}\r\n").as_bytes())
            .await
            .expect("write");
        let mut answer = Answer {
            lines: Vec::new(),
            literals: Vec::new(),
        };
        loop {
            let line = self.line().await;
            if let Some(status) = line.strip_prefix(&format!("{tag} ")) {
                assert!(status.starts_with("OK"), "`{command}`: {line}");
                return answer;
            }
            // A literal: `... {1234}` and then 1234 bytes.
            let trimmed = line.trim_end();
            if let Some(size) = trimmed
                .strip_suffix('}')
                .and_then(|rest| rest.rsplit_once('{'))
                .and_then(|(_, size)| size.parse::<usize>().ok())
            {
                let mut literal = vec![0; size];
                self.reader.read_exact(&mut literal).await.expect("literal");
                answer.literals.push(literal);
            }
            answer.lines.push(trimmed.to_owned());
        }
    }

    /// The raw messages of INBOX with their UIDs.
    async fn messages(&mut self) -> Vec<(String, Vec<u8>)> {
        let found = self.run("UID SEARCH ALL").await;
        let uids: Vec<String> = found
            .lines
            .iter()
            .find_map(|line| line.strip_prefix("* SEARCH"))
            .map(|rest| rest.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();
        let mut messages = Vec::new();
        for uid in uids {
            let mut fetched = self.run(&format!("UID FETCH {uid} BODY.PEEK[]")).await;
            assert_eq!(fetched.literals.len(), 1, "{:?}", fetched.lines);
            messages.push((uid, fetched.literals.remove(0)));
        }
        messages
    }
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_message_fetched_over_imap_is_parsed_and_routed_by_inbound() {
    let test = "a_message_fetched_over_imap_is_parsed_and_routed_by_inbound";
    let (Some(smtp), Some(imap)) = (
        service("NAVREE_TEST_GREENMAIL_SMTP_URL", test),
        service("NAVREE_TEST_GREENMAIL_IMAP", test),
    ) else {
        return;
    };
    let to = address();
    let subject = unique("Invoice INV-2026-0042");
    send(&smtp, &invoice(&to, &subject)).await;

    let mut session = Imap::connect(&imap).await;
    session.run(&format!("LOGIN \"{to}\" \"any\"")).await;
    session.run("SELECT INBOX").await;
    let messages = session.messages().await;
    session.run("LOGOUT").await;
    assert_eq!(messages.len(), 1, "one message for {to}");
    let (uid, raw) = messages.into_iter().next().unwrap();

    let mail = inbound_app(&["*"], |builder| builder).await;
    let received = mail
        .inbound()
        .await
        .receive(
            Incoming::new("imap", raw.clone())
                .provider_id(uid.clone())
                .recipient(&to),
        )
        .await
        .expect("the message is received");

    assert_eq!(received.status, InboundStatus::Handled, "{received:?}");
    let seen = mail.seen();
    assert_eq!(seen.len(), 1);
    let (pattern, address, mail) = &seen[0];
    assert_eq!((pattern.as_str(), address.as_str()), ("*", to.as_str()));
    assert_eq!(mail.source, "imap");
    assert_eq!(mail.provider_id.as_deref(), Some(uid.as_str()));
    assert_eq!(mail.subject.as_deref(), Some(subject.as_str()));
    assert_eq!(
        mail.from.as_ref().map(|from| from.email.as_str()),
        Some("gurpreet@example.com")
    );
    assert_eq!(mail.cc[0].email, "harpreet@example.com");
    assert_eq!(mail.reply_to[0].email, "accounts@example.com");
    assert!(
        mail.text
            .as_deref()
            .unwrap()
            .contains("Your invoice is attached.")
    );
    assert!(mail.html.as_deref().unwrap().contains("<p>Your invoice"));
    let files: Vec<(&str, &[u8])> = mail
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
    // The server added its own headers on the way in; they are read too.
    assert!(
        mail.headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case("received")),
        "{:?}",
        mail.headers.iter().map(|h| &h.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn gurmukhi_and_rupee_text_survives_a_real_mail_server() {
    let test = "gurmukhi_and_rupee_text_survives_a_real_mail_server";
    let (Some(smtp), Some(imap)) = (
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

    let mut session = Imap::connect(&imap).await;
    session.run(&format!("LOGIN \"{to}\" \"any\"")).await;
    session.run("SELECT INBOX").await;
    let (uid, raw) = session.messages().await.remove(0);
    session.run("LOGOUT").await;

    let mail = inbound_app(&["*"], |builder| builder).await;
    mail.inbound()
        .await
        .receive(Incoming::new("imap", raw).provider_id(uid).recipient(&to))
        .await
        .expect("the message is received");
    let seen = mail.seen();
    let received = &seen[0].2;
    assert_eq!(received.subject.as_deref(), Some(subject.as_str()));
    let text = received.text.as_deref().unwrap().replace(
        "
", "
",
    );
    assert_eq!(text.trim_end(), body);
}

// ---------------------------------------------------------------------
// POP3
// ---------------------------------------------------------------------

/// A POP3 session: USER, PASS, LIST and RETR, no TLS.
struct Pop3 {
    reader: BufReader<TcpStream>,
}

impl Pop3 {
    async fn connect(address: &str) -> Pop3 {
        let stream = TcpStream::connect(address)
            .await
            .unwrap_or_else(|error| panic!("cannot connect to POP3 at {address}: {error}"));
        let mut pop = Pop3 {
            reader: BufReader::new(stream),
        };
        let greeting = pop.line().await;
        assert!(greeting.starts_with("+OK"), "{greeting}");
        pop
    }

    async fn line(&mut self) -> String {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).await.expect("read");
        assert!(read > 0, "the server closed the connection");
        line.trim_end_matches(['\r', '\n']).to_owned()
    }

    /// Sends `command`; its answer must start with `+OK`.
    async fn run(&mut self, command: &str) -> String {
        self.reader
            .get_mut()
            .write_all(format!("{command}\r\n").as_bytes())
            .await
            .expect("write");
        let answer = self.line().await;
        assert!(answer.starts_with("+OK"), "`{command}`: {answer}");
        answer
    }

    /// The multi-line body that follows an answer, up to the lone `.`, with
    /// dot-stuffing undone, as the bytes of one message.
    async fn body(&mut self) -> Vec<u8> {
        let mut message = Vec::new();
        loop {
            let line = self.line().await;
            if line == "." {
                return message;
            }
            let line = line
                .strip_prefix('.')
                .filter(|_| line.starts_with(".."))
                .unwrap_or(&line);
            message.extend_from_slice(line.as_bytes());
            message.extend_from_slice(b"\r\n");
        }
    }
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn a_message_retrieved_over_pop3_is_parsed_and_routed_by_inbound() {
    let test = "a_message_retrieved_over_pop3_is_parsed_and_routed_by_inbound";
    let (Some(smtp), Some(pop3)) = (
        service("NAVREE_TEST_GREENMAIL_SMTP_URL", test),
        service("NAVREE_TEST_GREENMAIL_POP3", test),
    ) else {
        return;
    };
    let to = address();
    let subject = unique("Invoice by POP3");
    send(&smtp, &invoice(&to, &subject)).await;

    let mut session = Pop3::connect(&pop3).await;
    session.run(&format!("USER {to}")).await;
    session.run("PASS any").await;
    let stat = session.run("STAT").await;
    assert!(stat.starts_with("+OK 1 "), "one message waiting: {stat}");
    let unique_id = session.run("UIDL 1").await;
    session.run("RETR 1").await;
    let raw = session.body().await;
    session.run("QUIT").await;

    let mail = inbound_app(&["*"], |builder| builder).await;
    let received = mail
        .inbound()
        .await
        .receive(
            Incoming::new("pop3", raw)
                .provider_id(unique_id.rsplit(' ').next().unwrap_or_default())
                .recipient(&to),
        )
        .await
        .expect("the message is received");

    assert_eq!(received.status, InboundStatus::Handled, "{received:?}");
    let seen = mail.seen();
    let mail = &seen[0].2;
    assert_eq!(mail.source, "pop3");
    assert_eq!(mail.subject.as_deref(), Some(subject.as_str()));
    assert_eq!(mail.attachments.len(), 2);
    assert_eq!(mail.attachments[0].content.as_ref(), PDF);
}
