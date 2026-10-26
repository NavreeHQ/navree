//! The array, log, failover and SMTP transports.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use navis_kernel::BoxFuture;
use navis_mail::{ArrayTransport, FailoverTransport, MailError, MailTransport, Message};

fn hello() -> Message {
    Message::new()
        .from("App <app@example.com>")
        .to("ada@example.com")
        .subject("Hello")
        .text("Hello, Ada")
}

/// A transport that always fails, counting its attempts.
#[derive(Default)]
struct Broken {
    attempts: AtomicUsize,
}

impl MailTransport for Broken {
    fn send<'a>(&'a self, _message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(MailError::Transport {
                transport: "broken",
                source: "connection refused".into(),
            })
        })
    }
}

#[tokio::test]
async fn array_transport_records() {
    let transport = ArrayTransport::new();
    let message = hello();

    transport.send(&message).await.expect("send");

    assert_eq!(transport.sent(), vec![message]);
    transport.assert_sent(|m| m.subject == "Hello" && m.to == ["ada@example.com"]);

    // Clones share the recorded messages, so a test can keep one.
    let clone = transport.clone();
    clone.clear();
    assert!(transport.sent().is_empty());
    transport.assert_nothing_sent();
}

#[test]
#[should_panic(expected = "no sent message matches")]
fn array_transport_assert_sent_panics_when_nothing_matches() {
    let transport = ArrayTransport::new();
    transport.assert_sent(|m| m.subject == "Hello");
}

#[tokio::test]
async fn failover_uses_second_on_error() {
    let broken = Arc::new(Broken::default());
    let backup = ArrayTransport::new();
    let failover = FailoverTransport::new(vec![
        broken.clone() as Arc<dyn MailTransport>,
        Arc::new(backup.clone()),
    ]);

    failover.send(&hello()).await.expect("the backup delivers");

    assert_eq!(broken.attempts.load(Ordering::SeqCst), 1);
    assert_eq!(backup.sent(), vec![hello()]);
}

#[tokio::test]
async fn failover_stops_at_the_first_transport_that_succeeds() {
    let primary = ArrayTransport::new();
    let backup = Arc::new(Broken::default());
    let failover = FailoverTransport::new(vec![
        Arc::new(primary.clone()) as Arc<dyn MailTransport>,
        backup.clone(),
    ]);

    failover.send(&hello()).await.expect("the primary delivers");

    assert_eq!(primary.sent().len(), 1);
    assert_eq!(backup.attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failover_fails_with_every_error_when_all_transports_fail() {
    let failover = FailoverTransport::new(vec![
        Arc::new(Broken::default()) as Arc<dyn MailTransport>,
        Arc::new(Broken::default()),
    ]);

    let error = failover.send(&hello()).await.expect_err("all fail");

    match &error {
        MailError::AllTransportsFailed { errors } => assert_eq!(errors.len(), 2),
        other => panic!("unexpected error: {other}"),
    }
    assert!(error.to_string().contains("connection refused"), "{error}");
}
