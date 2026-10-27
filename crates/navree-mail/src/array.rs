//! [`ArrayTransport`]: records messages in memory, for tests.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_kernel::BoxFuture;

use crate::{MailError, MailTransport, Message};

/// Records every message instead of delivering it (the `array`
/// transport), for tests.
///
/// Clones share the recorded messages, so a test can keep a clone of the
/// transport it hands to the app:
///
/// ```
/// use std::sync::Arc;
/// use navree_mail::{ArrayTransport, MailTransport, Message};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_mail::MailError> {
/// let transport = ArrayTransport::new();
/// let shared: Arc<dyn MailTransport> = Arc::new(transport.clone());
/// shared.send(&Message::new().to("ada@example.com").subject("Hi")).await?;
/// transport.assert_sent(|m| m.subject == "Hi");
/// # Ok(())
/// # }
/// ```
///
/// In an app, either configure `TransportConfig::Array` and read it back
/// with [`Mailer::array_transport`](crate::Mailer::array_transport), or keep
/// a clone and pass it to `override_value::<dyn MailTransport>(..)`.
#[derive(Clone, Debug, Default)]
pub struct ArrayTransport {
    sent: Arc<Mutex<Vec<Message>>>,
}

impl ArrayTransport {
    /// An empty transport.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every message sent so far, oldest first.
    pub fn sent(&self) -> Vec<Message> {
        self.messages().clone()
    }

    /// Forgets every recorded message.
    pub fn clear(&self) {
        self.messages().clear();
    }

    /// Panics unless some sent message satisfies `predicate`. The panic
    /// message lists the recipients and subjects that were sent (not the
    /// bodies).
    #[track_caller]
    pub fn assert_sent(&self, predicate: impl Fn(&Message) -> bool) {
        let sent = self.messages();
        if !sent.iter().any(predicate) {
            panic!(
                "no sent message matches the predicate; sent: {}",
                summary(&sent)
            );
        }
    }

    /// Panics if any message was sent.
    #[track_caller]
    pub fn assert_nothing_sent(&self) {
        let sent = self.messages();
        if !sent.is_empty() {
            panic!("expected no mail, but sent: {}", summary(&sent));
        }
    }

    fn messages(&self) -> MutexGuard<'_, Vec<Message>> {
        self.sent.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl MailTransport for ArrayTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        self.messages().push(message.clone());
        Box::pin(async { Ok(()) })
    }

    fn name(&self) -> &'static str {
        "array"
    }
}

/// `[to: subject], ...` for assertion messages.
fn summary(sent: &[Message]) -> String {
    if sent.is_empty() {
        return "nothing".to_owned();
    }
    sent.iter()
        .map(|m| format!("[{}: {:?}]", m.to.join(", "), m.subject))
        .collect::<Vec<_>>()
        .join(", ")
}
