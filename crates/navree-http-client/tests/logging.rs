//! Request logging: useful, and free of secrets.
//!
//! A separate test binary on purpose: with a single scoped dispatcher,
//! tracing-core caches callsite interest from whichever thread hits a
//! callsite first, so concurrent tests without a subscriber would silence
//! the events this test captures.

use std::io;
use std::sync::{Arc, Mutex};

use http::Method;
use navree_http_client::{FakeResponse, HttpClient};
use tracing_subscriber::fmt::MakeWriter;

/// Collects formatted log lines.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test(start_paused = true)]
async fn logs_requests_without_query_strings_or_tokens() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::GET,
        "*",
        [FakeResponse::new(503), FakeResponse::new(200)],
    );

    client
        .get("https://user:hunter2@api.example.com/reports?api_key=SECRET42")
        .bearer("TOKEN123")
        .send()
        .await
        .unwrap();

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("outgoing request"), "{logs}");
    assert!(logs.contains("https://api.example.com/reports"), "{logs}");
    assert!(logs.contains("status=200"), "{logs}");
    assert!(logs.contains("attempts=2"), "{logs}");
    for secret in ["SECRET42", "TOKEN123", "hunter2"] {
        assert!(
            !logs.contains(secret),
            "`{secret}` leaked into the logs: {logs}"
        );
    }
}
