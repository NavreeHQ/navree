//! `LogTransport`: the envelope at info level, bodies only at debug level.

use std::io;
use std::sync::{Arc, Mutex};

use navree_mail::{LogTransport, MailTransport, Message};
use tracing::Level;
use tracing_subscriber::fmt::MakeWriter;

/// Collects formatted log output.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().expect("lock").clone()).expect("utf-8")
    }
}

impl io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Captured {
        self.clone()
    }
}

/// Sends a password-reset message through `LogTransport` with logging at
/// `level` and returns the log output.
async fn log_at(level: Level) -> String {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_max_level(level)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let message = Message::new()
        .from("app@example.com")
        .to("anu@example.com")
        .subject("Reset your password")
        .text("https://app.example.com/reset?token=tok_secret_123");

    LogTransport.send(&message).await.expect("send");

    captured.contents()
}

#[tokio::test(flavor = "current_thread")]
async fn log_transport_logs_the_envelope_but_not_the_body_at_info() {
    let output = log_at(Level::INFO).await;

    assert!(output.contains("anu@example.com"), "{output}");
    assert!(output.contains("Reset your password"), "{output}");
    assert!(!output.contains("tok_secret_123"), "{output}");
}

#[tokio::test(flavor = "current_thread")]
async fn log_transport_logs_the_body_at_debug() {
    let output = log_at(Level::DEBUG).await;

    assert!(output.contains("tok_secret_123"), "{output}");
}
