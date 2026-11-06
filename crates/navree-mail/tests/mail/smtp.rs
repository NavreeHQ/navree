//! `SmtpTransport`: URL parsing (no network) and delivery to a local fake
//! SMTP server.

use navree_mail::{MailError, MailTransport, Message, SmtpTls, SmtpTransport};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[test]
fn smtp_transport_builds_from_url() {
    let implicit =
        SmtpTransport::from_url("smtps://mailer:s3cret@smtp.example.com").expect("smtps");
    assert_eq!(implicit.host(), "smtp.example.com");
    assert_eq!(implicit.port(), 465);
    assert_eq!(implicit.tls(), SmtpTls::Implicit);
    let debug = format!("{implicit:?}");
    assert!(debug.contains("smtp.example.com"), "{debug}");
    assert!(!debug.contains("s3cret"), "{debug}");

    let required =
        SmtpTransport::from_url("smtp://mail.example.com:2525?tls=required").expect("starttls");
    assert_eq!((required.port(), required.tls()), (2525, SmtpTls::Required));

    let opportunistic = SmtpTransport::from_url("smtp://mail.example.com?tls=opportunistic")
        .expect("opportunistic");
    assert_eq!(
        (opportunistic.port(), opportunistic.tls()),
        (587, SmtpTls::Opportunistic)
    );

    let plain = SmtpTransport::from_url("smtp://localhost:1025").expect("plain");
    assert_eq!(
        (plain.host(), plain.port(), plain.tls()),
        ("localhost", 1025, SmtpTls::None)
    );
}

#[test]
fn smtp_transport_rejects_bad_urls_without_echoing_them() {
    for url in [
        "smpt://mailer:s3cret@smtp.example.com",
        "smtp://mailer:s3cret@smtp.example.com?tls=sometimes",
        "smtp://mailer:s3cret@:25",
        "not a url s3cret",
    ] {
        let error = SmtpTransport::from_url(url).expect_err(url);
        assert!(matches!(error, MailError::Config(_)), "{url}: {error}");
        assert!(!error.to_string().contains("s3cret"), "{url}: {error}");
    }
}

/// What the fake server received: envelope commands and the DATA section.
struct Session {
    commands: Vec<String>,
    data: String,
}

/// A minimal SMTP server on 127.0.0.1 that accepts one message.
async fn fake_smtp_server() -> (u16, tokio::task::JoinHandle<Session>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        let mut session = Session {
            commands: Vec::new(),
            data: String::new(),
        };
        write
            .write_all(b"220 localhost ESMTP fake\r\n")
            .await
            .expect("greet");
        let mut in_data = false;
        while let Some(line) = lines.next_line().await.expect("read") {
            if in_data {
                if line == "." {
                    write
                        .write_all(b"250 2.0.0 queued\r\n")
                        .await
                        .expect("queued");
                    break;
                }
                session.data.push_str(&line);
                session.data.push('\n');
                continue;
            }
            let command = line.to_ascii_uppercase();
            let reply: &[u8] = if command.starts_with("EHLO") || command.starts_with("HELO") {
                b"250-localhost\r\n250 8BITMIME\r\n"
            } else if command == "DATA" {
                in_data = true;
                b"354 end with <CRLF>.<CRLF>\r\n"
            } else {
                b"250 OK\r\n"
            };
            session.commands.push(line);
            write.write_all(reply).await.expect("reply");
        }
        session
    });
    (port, server)
}

#[tokio::test]
async fn smtp_transport_delivers_to_a_local_server() {
    let (port, server) = fake_smtp_server().await;
    let transport = SmtpTransport::from_url(&format!("smtp://127.0.0.1:{port}")).expect("url");
    let message = Message::new()
        .from("App <app@example.com>")
        .to("ada@example.com")
        .cc("grace@example.com")
        .bcc("hidden@example.com")
        .reply_to("support@example.com")
        .subject("Hello")
        .text("Hello, Ada")
        .html("<p>Hello, Ada</p>");

    transport.send(&message).await.expect("send");
    let session = server.await.expect("server");

    let commands = session.commands.join("\n");
    for expected in [
        "MAIL FROM:<app@example.com>",
        "RCPT TO:<ada@example.com>",
        "RCPT TO:<grace@example.com>",
        "RCPT TO:<hidden@example.com>",
    ] {
        assert!(
            commands.contains(expected),
            "missing {expected} in:\n{commands}"
        );
    }
    let data = &session.data;
    for expected in [
        "From: App <app@example.com>",
        "To: ada@example.com",
        "Cc: grace@example.com",
        "Reply-To: support@example.com",
        "Subject: Hello",
        "multipart/alternative",
        "Hello, Ada",
        "<p>Hello, Ada</p>",
    ] {
        assert!(data.contains(expected), "missing {expected} in:\n{data}");
    }
    assert!(!data.contains("hidden@example.com"), "Bcc leaked:\n{data}");
}

// CONN-102
#[tokio::test]
async fn smtp_transport_sends_attachments_as_multipart_mixed() {
    let (port, server) = fake_smtp_server().await;
    let transport = SmtpTransport::from_url(&format!("smtp://127.0.0.1:{port}")).expect("url");
    let pdf = b"%PDF-1.7 INV-2026-0042".to_vec();
    let message = Message::new()
        .from("Grewal Traders <billing@grewal.example.in>")
        .to("harpreet@example.in")
        .subject("Invoice INV-2026-0042")
        .text("Your invoice is attached.")
        .html("<p>Your invoice is attached.</p>")
        .attach("INV-2026-0042.pdf", "application/pdf", pdf.clone())
        .attach(
            "invite.ics",
            "text/calendar; method=REQUEST",
            b"BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n".to_vec(),
        );

    transport.send(&message).await.expect("send");
    let data = server.await.expect("server").data;

    for expected in [
        "multipart/mixed",
        "multipart/alternative",
        "Your invoice is attached.",
        "Content-Disposition: attachment; filename=\"INV-2026-0042.pdf\"",
        "Content-Type: application/pdf",
        "filename=\"invite.ics\"",
        "text/calendar; method=REQUEST",
    ] {
        assert!(data.contains(expected), "missing {expected} in:\n{data}");
    }
    // The file arrives whole, in whatever transfer encoding its part declares.
    assert_eq!(
        crate::support::attachment_bytes(&data, "INV-2026-0042.pdf"),
        pdf
    );
}

// CONN-102
#[tokio::test]
async fn smtp_transport_refuses_a_message_over_its_limit() {
    let transport = SmtpTransport::from_url("smtp://127.0.0.1:9").expect("url");
    let message = Message::new()
        .from("billing@grewal.example.in")
        .to("harpreet@example.in")
        .subject("Scans")
        .attach(
            "scan.png",
            "image/png",
            vec![0; navree_mail::SMTP_MAX_MESSAGE_BYTES / 4 * 3 + 3],
        );

    let error = transport.send(&message).await.expect_err("too large");

    assert!(
        matches!(
            error,
            MailError::TooLarge {
                transport: "smtp",
                ..
            }
        ),
        "{error}"
    );
}

#[tokio::test]
async fn smtp_transport_reports_connection_failures() {
    // Bind then drop a listener: nothing listens on that port any more.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let transport = SmtpTransport::from_url(&format!("smtp://127.0.0.1:{port}")).expect("url");
    let message = Message::new()
        .from("app@example.com")
        .to("ada@example.com")
        .subject("Hello")
        .text("Hello");

    let error = transport.send(&message).await.expect_err("nothing listens");

    assert!(
        matches!(
            error,
            MailError::Transport {
                transport: "smtp",
                ..
            }
        ),
        "{error}"
    );
}

/// A certificate no public authority signed, for a transport to trust.
const TEST_CERTIFICATE: &str = include_str!("../fixtures/smtp-test-ca.pem");

#[test]
fn smtp_transport_trusts_one_more_certificate_on_every_tls_mode() {
    for (url, port, tls) in [
        ("smtps://mailer:s3cret@127.0.0.1", 465, SmtpTls::Implicit),
        (
            "smtp://mailer:s3cret@127.0.0.1?tls=required",
            587,
            SmtpTls::Required,
        ),
        (
            "smtp://mailer:s3cret@127.0.0.1?tls=opportunistic",
            587,
            SmtpTls::Opportunistic,
        ),
    ] {
        let transport = SmtpTransport::from_url(url)
            .expect(url)
            .with_root_certificate(TEST_CERTIFICATE)
            .expect(url);
        assert_eq!((transport.port(), transport.tls()), (port, tls), "{url}");
        assert!(!format!("{transport:?}").contains("s3cret"));
    }
}

#[test]
fn a_certificate_needs_a_tls_transport_and_a_pem_certificate() {
    let plain = SmtpTransport::from_url("smtp://127.0.0.1:1025").expect("url");
    let error = plain
        .with_root_certificate(TEST_CERTIFICATE)
        .expect_err("plain text has no certificate to trust");
    assert!(matches!(error, MailError::Config(_)), "{error}");
    assert!(error.to_string().contains("no TLS"), "{error}");

    let tls = || SmtpTransport::from_url("smtps://127.0.0.1:465").expect("url");
    for text in [
        "",
        "not a certificate s3cret",
        "-----BEGIN PRIVATE KEY-----\nMIIB\n-----END PRIVATE KEY-----\n",
    ] {
        let error = tls().with_root_certificate(text).expect_err(text);
        assert!(matches!(error, MailError::Config(_)), "{error}");
        assert!(!error.to_string().contains("s3cret"), "{error}");
    }
}

#[test]
fn the_ca_setting_reaches_the_transport_and_survives_the_json_form() {
    use navree_mail::TransportConfig;
    use serde_json::json;

    let table = json!({
        "type": "smtp",
        "url": "smtps://mailer:s3cret@mail.example.com",
        "ca": TEST_CERTIFICATE,
    });
    let config: TransportConfig = serde_json::from_value(table).expect("a transport");
    let transport = config.build().expect("the certificate is trusted");
    assert_eq!(transport.name(), "smtp");
    // The tenant form reads back what was written.
    assert_eq!(
        serde_json::from_value::<TransportConfig>(config.to_json()).expect("reads back"),
        config
    );

    // Without a `ca` nothing is written, and a bad one is a configuration
    // error, as is one on a transport without TLS.
    let plain: TransportConfig =
        serde_json::from_value(json!({ "type": "smtp", "url": "smtps://mail.example.com" }))
            .expect("a transport");
    assert!(plain.to_json().get("ca").is_none());
    let bad = TransportConfig::smtp("smtps://mail.example.com").with_ca("nonsense");
    assert!(matches!(bad.build(), Err(MailError::Config(_))));
    let no_tls = TransportConfig::smtp("smtp://127.0.0.1:1025").with_ca(TEST_CERTIFICATE);
    assert!(matches!(no_tls.build(), Err(MailError::Config(_))));
}
