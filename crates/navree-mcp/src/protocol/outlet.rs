//! [`Outlet`]: where messages to one client stream go (a request's SSE
//! answer, a session's `GET` stream, the stdio writer), through a bounded
//! queue so a client that reads slowly cannot grow the server's memory.

use serde_json::Value;
use tokio::sync::mpsc;

/// How many messages wait for a slow client before notifications are
/// dropped (answers wait for room instead).
pub(crate) const QUEUE: usize = 64;

/// The sending side of one stream of messages to the client.
#[derive(Clone, Debug)]
pub(crate) struct Outlet {
    tx: mpsc::Sender<Value>,
}

/// Why a notification was not queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotSent {
    /// The client is not reading: the queue is full and the notification
    /// was dropped.
    Full,
    /// The stream is gone.
    Closed,
}

impl std::fmt::Display for NotSent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NotSent::Full => "the client's stream is full",
            NotSent::Closed => "the client's stream is closed",
        })
    }
}

impl Outlet {
    /// An outlet and the receiving end of its queue.
    pub(crate) fn channel() -> (Outlet, mpsc::Receiver<Value>) {
        let (tx, rx) = mpsc::channel(QUEUE);
        (Outlet { tx }, rx)
    }

    /// Queues a notification without waiting: dropped when the queue is
    /// full (notifications tell the client to look again, so a missed one
    /// costs a stale view, not data).
    pub(crate) fn notify(&self, message: Value) -> Result<(), NotSent> {
        self.tx.try_send(message).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => NotSent::Full,
            mpsc::error::TrySendError::Closed(_) => NotSent::Closed,
        })
    }

    /// Queues a message that must arrive (an answer), waiting for room;
    /// false when the stream is gone.
    pub(crate) async fn deliver(&self, message: Value) -> bool {
        self.tx.send(message).await.is_ok()
    }

    /// Whether the receiving end is gone.
    pub(crate) fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn a_full_queue_drops_notifications_but_keeps_its_bound() {
        let (outlet, mut rx) = Outlet::channel();
        for n in 0..QUEUE {
            outlet.notify(json!(n)).unwrap();
        }
        assert_eq!(outlet.notify(json!("one too many")), Err(NotSent::Full));
        assert_eq!(rx.recv().await, Some(json!(0)));
        // An answer waits for room and arrives.
        assert!(outlet.deliver(json!("answer")).await);
        drop(rx);
        assert!(outlet.is_closed());
        assert_eq!(outlet.notify(json!(1)), Err(NotSent::Closed));
        assert!(!outlet.deliver(json!(2)).await);
    }
}
