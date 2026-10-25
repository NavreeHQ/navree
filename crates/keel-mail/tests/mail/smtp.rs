//! `SmtpTransport`: URL parsing (no network) and delivery to a local fake
//! SMTP server.

use keel_mail::{MailError, MailTransport, Message, SmtpTls, SmtpTransport};
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
