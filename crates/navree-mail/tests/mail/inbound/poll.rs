//! `Inbound::poll` against IMAP and POP3 servers that run in this process:
//! each is a script of the commands the poller must send and the bytes a real
//! server answers with, and the commands it received are checked afterwards.
//! The TLS tests serve a certificate made for the test; the poller trusts it
//! only when it is told to.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use navree_kernel::{BoxError, Module, ModuleBuilder};
use navree_mail::inbound::{InboundError, MailboxPoller, PollAfter, PollSecurity};
use navree_mail::{InboundMail, MailboxContext, ModuleBuilderMailExt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use super::{fixture, inbound_app};

/// One command a fake server expects, and what it answers.
struct Step {
    /// The command as the client sends it, an IMAP tag left out.
    expect: &'static str,
    /// The answer; `{tag}` is the command's IMAP tag.
    reply: Vec<u8>,
    /// TLS starts after this answer (`STARTTLS`, `STLS`).
    starts_tls: bool,
}

fn step(expect: &'static str, reply: impl Into<Vec<u8>>) -> Step {
    Step {
        expect,
        reply: reply.into(),
        starts_tls: false,
    }
}

fn tls_step(expect: &'static str, reply: impl Into<Vec<u8>>) -> Step {
    Step {
        starts_tls: true,
        ..step(expect, reply)
    }
}

/// How a fake server is reached.
enum Wire {
    Plain,
    /// TLS from the first byte.
    Tls(TlsAcceptor),
    /// Plain text, then TLS after the step that `starts_tls`.
    Upgrade(TlsAcceptor),
}

/// A certificate for 127.0.0.1: the acceptor of the server and its PEM.
fn certificate() -> (TlsAcceptor, String) {
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
    let pem = certified.cert.pem();
    let cert = CertificateDer::from(certified.cert.der().to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert], key)
    .unwrap();
    (TlsAcceptor::from(Arc::new(config)), pem)
}

/// Answers the commands of one client from `steps`; returns the stream when
/// a step starts TLS, and None when the conversation ends.
async fn converse<S>(
    mut stream: BufReader<S>,
    imap: bool,
    steps: &mut VecDeque<Step>,
    log: &mut Vec<String>,
) -> Option<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let mut line = String::new();
        if stream.read_line(&mut line).await.unwrap_or(0) == 0 {
            return None;
        }
        let line = line.trim_end().to_owned();
        let (tag, command) = if imap {
            line.split_once(' ').unwrap_or((line.as_str(), ""))
        } else {
            ("", line.as_str())
        };
        log.push(command.to_owned());
        let matches = steps
            .front()
            .is_some_and(|step| command.starts_with(step.expect));
        if !matches {
            let refusal = if imap {
                format!("{tag} BAD unexpected command\r\n")
            } else {
                "-ERR unexpected command\r\n".to_owned()
            };
            stream.get_mut().write_all(refusal.as_bytes()).await.ok()?;
            continue;
        }
        let step = steps.pop_front().unwrap();
        // The reply may hold bytes that are not UTF-8 (a message): keep them.
        let bytes = replace_tag(&step.reply, tag);
        stream.get_mut().write_all(&bytes).await.ok()?;
        stream.get_mut().flush().await.ok()?;
        if step.starts_tls {
            return Some(stream.into_inner());
        }
        if command == "LOGOUT" || command == "QUIT" {
            return None;
        }
    }
}

/// `reply` with `{tag}` replaced, byte for byte otherwise.
fn replace_tag(reply: &[u8], tag: &str) -> Vec<u8> {
    let marker = b"{tag}";
    let mut out = Vec::with_capacity(reply.len());
    let mut i = 0;
    while i < reply.len() {
        if reply[i..].starts_with(marker) {
            out.extend_from_slice(tag.as_bytes());
            i += marker.len();
        } else {
            out.push(reply[i]);
            i += 1;
        }
    }
    out
}

/// Starts a fake server on a free port; the handle gives the commands it
/// received, in order, once the conversation is over.
async fn serve(
    wire: Wire,
    imap: bool,
    greeting: &'static str,
    steps: Vec<Step>,
) -> (u16, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let mut steps: VecDeque<Step> = steps.into();
        let mut log = Vec::new();
        let (tcp, _) = listener.accept().await.unwrap();
        match wire {
            Wire::Plain => {
                let mut stream = BufReader::new(tcp);
                stream
                    .get_mut()
                    .write_all(greeting.as_bytes())
                    .await
                    .unwrap();
                converse(stream, imap, &mut steps, &mut log).await;
            }
            Wire::Tls(acceptor) => {
                // A client that does not trust the certificate stops here.
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return log;
                };
                let mut stream = BufReader::new(tls);
                stream
                    .get_mut()
                    .write_all(greeting.as_bytes())
                    .await
                    .unwrap();
                converse(stream, imap, &mut steps, &mut log).await;
            }
            Wire::Upgrade(acceptor) => {
                let mut stream = BufReader::new(tcp);
                stream
                    .get_mut()
                    .write_all(greeting.as_bytes())
                    .await
                    .unwrap();
                let Some(tcp) = converse(stream, imap, &mut steps, &mut log).await else {
                    return log;
                };
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return log;
                };
                converse(BufReader::new(tls), imap, &mut steps, &mut log).await;
            }
        }
        log
    });
    (port, handle)
}

async fn imap_server(wire: Wire, steps: Vec<Step>) -> (u16, JoinHandle<Vec<String>>) {
    serve(wire, true, "* OK IMAP4rev1 ready\r\n", steps).await
}

async fn pop3_server(wire: Wire, steps: Vec<Step>) -> (u16, JoinHandle<Vec<String>>) {
    serve(wire, false, "+OK POP3 ready\r\n", steps).await
}

/// An IMAP answer to `UID FETCH` with `raw` as the message.
fn fetch_reply(uid: u32, raw: &[u8]) -> Vec<u8> {
    let mut reply = format!("* 1 FETCH (UID {uid} BODY[] {{{}}}\r\n", raw.len()).into_bytes();
    reply.extend_from_slice(raw);
    reply.extend_from_slice(b")\r\n{tag} OK fetched\r\n");
    reply
}

/// A POP3 answer to `RETR` with `raw` as the message: lines ended with
/// CR LF, those that start with a dot stuffed with another.
fn retr_reply(raw: &[u8]) -> Vec<u8> {
    let mut reply = format!("+OK {} octets\r\n", raw.len()).into_bytes();
    for line in raw.split_inclusive(|byte| *byte == b'\n') {
        let line = line
            .strip_suffix(b"\r\n")
            .or_else(|| line.strip_suffix(b"\n"))
            .unwrap_or(line);
        if line.starts_with(b".") {
            reply.push(b'.');
        }
        reply.extend_from_slice(line);
        reply.extend_from_slice(b"\r\n");
    }
    reply.extend_from_slice(b".\r\n");
    reply
}

/// `raw` with every line ended by CR LF, as POP3 delivers a message.
fn crlf(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in raw.split_inclusive(|byte| *byte == b'\n') {
        let line = line
            .strip_suffix(b"\r\n")
            .or_else(|| line.strip_suffix(b"\n"))
            .unwrap_or(line);
        out.extend_from_slice(line);
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn imap_poller(port: u16, security: PollSecurity) -> MailboxPoller {
    MailboxPoller::imap("127.0.0.1")
        .port(port)
        .security(security)
        .login("ada@acme.test", "s3cret")
        .timeout(Duration::from_secs(10))
}

fn pop3_poller(port: u16, security: PollSecurity) -> MailboxPoller {
    MailboxPoller::pop3("127.0.0.1")
        .port(port)
        .security(security)
        .login("ada@acme.test", "s3cret")
        .timeout(Duration::from_secs(10))
}

/// The commands of a session that logs in, opens the inbox and lists
/// `uids`.
fn imap_opening(uids: &str) -> Vec<Step> {
    vec![
        step(
            "LOGIN \"ada@acme.test\" \"s3cret\"",
            "{tag} OK logged in\r\n",
        ),
        step(
            "SELECT \"INBOX\"",
            "* 2 EXISTS\r\n{tag} OK [READ-WRITE] selected\r\n",
        ),
        step(
            "UID SEARCH UNSEEN UNDELETED",
            format!("* SEARCH {uids}\r\n{{tag}} OK done\r\n"),
        ),
    ]
}

fn imap_closing() -> Step {
    step("LOGOUT", "* BYE bye\r\n{tag} OK logged out\r\n")
}

#[tokio::test]
async fn an_imap_mailbox_is_read_received_and_flagged_seen() {
    let first = fixture("reply.eml");
    let second = fixture("auto-reply.eml");
    let mut steps = imap_opening("4 7");
    steps.push(step("UID FETCH 4 BODY.PEEK[]", fetch_reply(4, &first)));
    steps.push(step(
        "UID STORE 4 +FLAGS.SILENT (\\Seen)",
        "{tag} OK stored\r\n",
    ));
    steps.push(step("UID FETCH 7 BODY.PEEK[]", fetch_reply(7, &second)));
    steps.push(step(
        "UID STORE 7 +FLAGS.SILENT (\\Seen)",
        "{tag} OK stored\r\n",
    ));
    steps.push(imap_closing());
    let (port, server) = imap_server(Wire::Plain, steps).await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let report = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::None))
        .await
        .expect("the mailbox is read");

    assert_eq!(report.received.len(), 2, "{report:?}");
    assert!(
        report.refused.is_empty() && report.failed.is_empty(),
        "{report:?}"
    );
    assert_eq!(report.left, 0);
    let seen = mail.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].2.source, "imap");
    assert_eq!(seen[0].2.provider_id.as_deref(), Some("4"));
    assert_eq!(seen[0].2.raw().as_ref(), first.as_slice());
    assert_eq!(seen[1].2.provider_id.as_deref(), Some("7"));
    assert_eq!(seen[1].2.raw().as_ref(), second.as_slice());
    assert_eq!(
        server.await.unwrap(),
        [
            "LOGIN \"ada@acme.test\" \"s3cret\"",
            "SELECT \"INBOX\"",
            "UID SEARCH UNSEEN UNDELETED",
            "UID FETCH 4 BODY.PEEK[]",
            "UID STORE 4 +FLAGS.SILENT (\\Seen)",
            "UID FETCH 7 BODY.PEEK[]",
            "UID STORE 7 +FLAGS.SILENT (\\Seen)",
            "LOGOUT",
        ]
    );
}

#[tokio::test]
async fn an_imap_message_can_be_deleted_once_received() {
    let raw = fixture("reply.eml");
    let mut steps = imap_opening("4");
    steps.push(step("UID FETCH 4 BODY.PEEK[]", fetch_reply(4, &raw)));
    steps.push(step(
        "UID STORE 4 +FLAGS.SILENT (\\Deleted)",
        "{tag} OK stored\r\n",
    ));
    steps.push(step("EXPUNGE", "* 1 EXPUNGE\r\n{tag} OK expunged\r\n"));
    steps.push(imap_closing());
    let (port, server) = imap_server(Wire::Plain, steps).await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let report = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::None).after(PollAfter::Delete))
        .await
        .expect("the mailbox is read");

    assert_eq!(report.received.len(), 1);
    let commands = server.await.unwrap();
    assert!(commands.contains(&"EXPUNGE".to_owned()), "{commands:?}");
}

#[tokio::test]
async fn an_imap_server_with_a_private_certificate_is_trusted_only_when_told_to() {
    let raw = fixture("reply.eml");
    let (acceptor, pem) = certificate();
    let mut steps = imap_opening("4");
    steps.push(step("UID FETCH 4 BODY.PEEK[]", fetch_reply(4, &raw)));
    steps.push(step(
        "UID STORE 4 +FLAGS.SILENT (\\Seen)",
        "{tag} OK stored\r\n",
    ));
    steps.push(imap_closing());
    let (port, server) = imap_server(Wire::Tls(acceptor.clone()), steps).await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let report = mail
        .inbound()
        .await
        .poll(
            &imap_poller(port, PollSecurity::Tls)
                .root_certificate(&pem)
                .expect("a certificate"),
        )
        .await
        .expect("the mailbox is read over TLS");
    assert_eq!(report.received.len(), 1);
    assert_eq!(server.await.unwrap().len(), 6);

    // Without the certificate the handshake fails and the password is never sent.
    let (port, server) = imap_server(Wire::Tls(acceptor), imap_opening("4")).await;
    let error = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::Tls))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, InboundError::Mailbox { reason, .. } if reason.contains("TLS")),
        "{error}"
    );
    assert!(!error.to_string().contains("s3cret"));
    assert!(
        server.await.unwrap().is_empty(),
        "no command reached the server"
    );
}

#[tokio::test]
async fn an_imap_server_is_secured_with_starttls_before_the_login() {
    let raw = fixture("reply.eml");
    let (acceptor, pem) = certificate();
    let mut steps = vec![tls_step("STARTTLS", "{tag} OK begin TLS\r\n")];
    steps.extend(imap_opening("4"));
    steps.push(step("UID FETCH 4 BODY.PEEK[]", fetch_reply(4, &raw)));
    steps.push(step(
        "UID STORE 4 +FLAGS.SILENT (\\Seen)",
        "{tag} OK stored\r\n",
    ));
    steps.push(imap_closing());
    let (port, server) = imap_server(Wire::Upgrade(acceptor), steps).await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let report = mail
        .inbound()
        .await
        .poll(
            &imap_poller(port, PollSecurity::StartTls)
                .root_certificate(&pem)
                .expect("a certificate"),
        )
        .await
        .expect("the mailbox is read");

    assert_eq!(report.received.len(), 1);
    let commands = server.await.unwrap();
    assert_eq!(commands[0], "STARTTLS");
    assert!(commands[1].starts_with("LOGIN"), "{commands:?}");
}

#[tokio::test]
async fn a_server_that_refuses_starttls_is_not_logged_in_to() {
    let (port, server) = imap_server(
        Wire::Plain,
        vec![step("STARTTLS", "{tag} NO STARTTLS is not available\r\n")],
    )
    .await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let error = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::StartTls))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("STARTTLS"), "{error}");
    assert_eq!(server.await.unwrap(), ["STARTTLS"]);
}

#[tokio::test]
async fn a_refused_login_is_an_error_that_does_not_quote_the_password() {
    let (port, _server) = imap_server(
        Wire::Plain,
        vec![step(
            "LOGIN",
            "{tag} NO [AUTHENTICATIONFAILED] invalid credentials\r\n",
        )],
    )
    .await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let error = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::None))
        .await
        .unwrap_err();

    let text = error.to_string();
    assert!(text.contains("login was refused"), "{text}");
    assert!(text.contains("invalid credentials"), "{text}");
    assert!(!text.contains("s3cret"), "{text}");
}

#[tokio::test]
async fn a_message_whose_mailbox_fails_stays_for_the_next_poll() {
    struct Failing;
    impl Module for Failing {
        fn register(&self, m: &mut ModuleBuilder) {
            m.mailbox(
                "ada@acme.test",
                |_mail: InboundMail, _ctx: MailboxContext| async move {
                    Err::<(), BoxError>("database unavailable".into())
                },
            );
        }
    }
    let raw = fixture("reply.eml");
    let mut steps = imap_opening("4");
    steps.push(step("UID FETCH 4 BODY.PEEK[]", fetch_reply(4, &raw)));
    steps.push(imap_closing());
    let (port, server) = imap_server(Wire::Plain, steps).await;
    let mail = inbound_app(&[], |builder| builder.module(Failing)).await;

    let report = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::None))
        .await
        .expect("the poll itself works");

    assert!(report.received.is_empty());
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert_eq!(report.failed[0].id, "4");
    assert!(report.failed[0].reason.contains("database unavailable"));
    let commands = server.await.unwrap();
    assert!(
        commands
            .iter()
            .all(|command| !command.starts_with("UID STORE")),
        "the message was flagged: {commands:?}"
    );
}

#[tokio::test]
async fn a_message_over_the_size_limit_is_refused_flagged_and_never_kept() {
    let raw = fixture("reply.eml");
    let mut steps = imap_opening("4");
    steps.push(step("UID FETCH 4 BODY.PEEK[]", fetch_reply(4, &raw)));
    steps.push(step(
        "UID STORE 4 +FLAGS.SILENT (\\Seen)",
        "{tag} OK stored\r\n",
    ));
    steps.push(imap_closing());
    let (port, server) = imap_server(Wire::Plain, steps).await;
    let mail = inbound_app(&["*"], |builder| {
        builder.config("mail.inbound.max_size", 200)
    })
    .await;

    let report = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::None).after(PollAfter::Delete))
        .await
        .expect("the poll works");

    assert!(report.received.is_empty());
    assert_eq!(report.refused.len(), 1, "{report:?}");
    assert!(report.refused[0].reason.contains("mail.inbound.max_size"));
    assert!(mail.seen().is_empty());
    // Flagged seen, not deleted, even though the poller deletes what it takes.
    let commands = server.await.unwrap();
    assert!(commands.contains(&"UID STORE 4 +FLAGS.SILENT (\\Seen)".to_owned()));
    assert!(!commands.contains(&"EXPUNGE".to_owned()));
}

#[tokio::test]
async fn a_poll_reads_no_more_than_max_messages() {
    let raw = fixture("reply.eml");
    let mut steps = imap_opening("4 5 6");
    steps.push(step("UID FETCH 4 BODY.PEEK[]", fetch_reply(4, &raw)));
    steps.push(step(
        "UID STORE 4 +FLAGS.SILENT (\\Seen)",
        "{tag} OK stored\r\n",
    ));
    steps.push(imap_closing());
    let (port, _server) = imap_server(Wire::Plain, steps).await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let report = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::None).max_messages(1))
        .await
        .expect("the poll works");

    assert_eq!(report.received.len(), 1);
    assert_eq!(report.left, 2);
}

#[tokio::test]
async fn a_pop3_mailbox_is_read_and_each_message_deleted() {
    // Lines that start with a dot, and a byte that is not UTF-8.
    let dots: &[u8] = b"From: grace@example.com\r\nTo: ada@acme.test\r\nSubject: dots\r\n\r\n.start of a line\r\n..two dots\r\nend\r\n";
    let bytes: &[u8] =
        b"From: grace@example.com\r\nTo: ada@acme.test\r\nSubject: bytes\r\n\r\nzol\xff\r\n";
    let steps = vec![
        step("USER ada@acme.test", "+OK\r\n"),
        step("PASS s3cret", "+OK logged in\r\n"),
        step("UIDL", "+OK\r\n1 uid-aaa\r\n2 uid-bbb\r\n.\r\n"),
        step("RETR 1", retr_reply(dots)),
        step("DELE 1", "+OK deleted\r\n"),
        step("RETR 2", retr_reply(bytes)),
        step("DELE 2", "+OK deleted\r\n"),
        step("QUIT", "+OK bye\r\n"),
    ];
    let (port, server) = pop3_server(Wire::Plain, steps).await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let report = mail
        .inbound()
        .await
        .poll(&pop3_poller(port, PollSecurity::None))
        .await
        .expect("the mailbox is read");

    assert_eq!(report.received.len(), 2, "{report:?}");
    let seen = mail.seen();
    assert_eq!(seen[0].2.source, "pop3");
    assert_eq!(seen[0].2.provider_id.as_deref(), Some("uid-aaa"));
    assert_eq!(seen[0].2.raw().as_ref(), crlf(dots).as_slice());
    assert_eq!(seen[1].2.raw().as_ref(), crlf(bytes).as_slice());
    assert_eq!(
        server.await.unwrap(),
        [
            "USER ada@acme.test",
            "PASS s3cret",
            "UIDL",
            "RETR 1",
            "DELE 1",
            "RETR 2",
            "DELE 2",
            "QUIT",
        ]
    );
}

#[tokio::test]
async fn a_pop3_server_without_uidl_is_read_by_message_number() {
    let raw = fixture("reply.eml");
    let steps = vec![
        step("USER", "+OK\r\n"),
        step("PASS", "+OK\r\n"),
        step("UIDL", "-ERR not supported\r\n"),
        step("LIST", "+OK 1 messages\r\n1 881\r\n.\r\n"),
        step("RETR 1", retr_reply(&raw)),
        step("DELE 1", "+OK deleted\r\n"),
        step("QUIT", "+OK bye\r\n"),
    ];
    let (port, _server) = pop3_server(Wire::Plain, steps).await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let report = mail
        .inbound()
        .await
        .poll(&pop3_poller(port, PollSecurity::None))
        .await
        .expect("the mailbox is read");

    assert_eq!(report.received.len(), 1, "{report:?}");
    assert_eq!(mail.seen()[0].2.provider_id.as_deref(), Some("1"));
}

#[tokio::test]
async fn a_pop3_server_is_secured_with_stls_before_the_login() {
    let raw = fixture("reply.eml");
    let (acceptor, pem) = certificate();
    let steps = vec![
        tls_step("STLS", "+OK begin TLS\r\n"),
        step("USER", "+OK\r\n"),
        step("PASS", "+OK\r\n"),
        step("UIDL", "+OK\r\n1 uid-aaa\r\n.\r\n"),
        step("RETR 1", retr_reply(&raw)),
        step("DELE 1", "+OK deleted\r\n"),
        step("QUIT", "+OK bye\r\n"),
    ];
    let (port, server) = pop3_server(Wire::Upgrade(acceptor), steps).await;
    let mail = inbound_app(&["*"], |builder| builder).await;

    let report = mail
        .inbound()
        .await
        .poll(
            &pop3_poller(port, PollSecurity::StartTls)
                .root_certificate(&pem)
                .expect("a certificate"),
        )
        .await
        .expect("the mailbox is read");

    assert_eq!(report.received.len(), 1);
    assert_eq!(server.await.unwrap()[0], "STLS");
}

#[tokio::test]
async fn a_pop3_message_that_is_too_large_is_left_on_the_server() {
    let raw = fixture("reply.eml");
    let steps = vec![
        step("USER", "+OK\r\n"),
        step("PASS", "+OK\r\n"),
        step("UIDL", "+OK\r\n1 uid-aaa\r\n.\r\n"),
        step("RETR 1", retr_reply(&raw)),
        step("QUIT", "+OK bye\r\n"),
    ];
    let (port, server) = pop3_server(Wire::Plain, steps).await;
    let mail = inbound_app(&["*"], |builder| {
        builder.config("mail.inbound.max_size", 200)
    })
    .await;

    let report = mail
        .inbound()
        .await
        .poll(&pop3_poller(port, PollSecurity::None))
        .await
        .expect("the poll works");

    assert_eq!(report.refused.len(), 1, "{report:?}");
    assert!(mail.seen().is_empty());
    let commands = server.await.unwrap();
    assert!(
        commands.iter().all(|command| !command.starts_with("DELE")),
        "{commands:?}"
    );
}

#[tokio::test]
async fn a_poller_that_cannot_work_is_a_configuration_error() {
    let mail = inbound_app(&["*"], |builder| builder).await;
    let inbound = mail.inbound().await;

    for poller in [
        MailboxPoller::imap("127.0.0.1"),
        pop3_poller(110, PollSecurity::None).after(PollAfter::MarkSeen),
        imap_poller(143, PollSecurity::None).max_messages(0),
        MailboxPoller::imap("127.0.0.1").login("ada", "line\nbreak"),
    ] {
        let error = inbound.poll(&poller).await.unwrap_err();
        assert!(matches!(error, InboundError::Config(_)), "{error}");
    }
}

#[tokio::test]
async fn a_server_that_cannot_be_reached_is_a_mailbox_error() {
    let mail = inbound_app(&["*"], |builder| builder).await;
    // A port nothing listens on: bind one and drop it.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address: SocketAddr = listener.local_addr().unwrap();
        address.port()
    };

    let error = mail
        .inbound()
        .await
        .poll(&imap_poller(port, PollSecurity::None))
        .await
        .unwrap_err();

    assert!(matches!(error, InboundError::Mailbox { .. }), "{error}");
}
