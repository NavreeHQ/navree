//! [`Backplane`]: how the `serve` processes of an app share broadcasts and
//! presence, and the in-process [`MemoryBackplane`].

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use navree_kernel::{BoxError, BoxFuture};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Receives the messages of a [`Backplane`] subscription, in the order each
/// process published them. It runs on the backplane's own task and must
/// not block.
pub type BackplaneHandler = Arc<dyn Fn(Bytes) + Send + Sync>;

/// A publish/subscribe channel between the processes of an app, so that
/// several `serve` processes behind a load balancer share broadcasts,
/// presence members and user disconnections (`realtime.backplane`).
///
/// [`Realtime`](crate::Realtime) owns the messages (versioned JSON); a
/// backplane only moves bytes. navree-redis's Redis backplane (Redis
/// pub/sub) is the production one; [`MemoryBackplane`] connects the hubs
/// of several apps in one process, for tests.
///
/// Delivery is at most once: a process that is disconnected from the
/// backplane misses what was published meanwhile. The hub resends its
/// presence members every 30 seconds, so presence converges again.
pub trait Backplane: Send + Sync + 'static {
    /// Queues `message` for every subscriber of `topic` (this process's
    /// own subscription included). Must not block or fail: errors are the
    /// backplane's to log and retry. Messages from one process arrive in
    /// the order they were published.
    fn publish(&self, topic: &str, message: Bytes);

    /// Delivers every message published on `topic` to `handler` until
    /// `shutdown` is cancelled. Resolves once the subscription is active,
    /// so nothing published after it is missed.
    fn subscribe(
        &self,
        topic: &str,
        handler: BackplaneHandler,
        shutdown: CancellationToken,
    ) -> BoxFuture<'_, Result<(), BoxError>>;
}

/// An in-process [`Backplane`]: every subscriber in this process receives
/// every message of its topic. Clones share the same subscribers, so give
/// one clone to each app (`AppBuilder::override_value::<dyn Backplane>`)
/// to connect their hubs, as two `serve` processes on one Redis would be.
///
/// ```
/// use std::sync::Arc;
///
/// use navree_realtime::{Backplane, MemoryBackplane};
///
/// let backplane = MemoryBackplane::new();
/// let shared: Arc<dyn Backplane> = Arc::new(backplane.clone());
/// # let _ = shared;
/// ```
#[derive(Clone, Default)]
pub struct MemoryBackplane {
    topics: Arc<Mutex<Topics>>,
}

#[derive(Default)]
struct Topics {
    next_id: u64,
    subscribers: HashMap<String, Vec<(u64, mpsc::UnboundedSender<Bytes>)>>,
}

impl MemoryBackplane {
    /// A backplane with no subscribers.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of active subscriptions to `topic`.
    pub fn subscribers(&self, topic: &str) -> usize {
        self.topics().subscribers.get(topic).map_or(0, Vec::len)
    }

    fn topics(&self) -> std::sync::MutexGuard<'_, Topics> {
        self.topics.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn unsubscribe(&self, topic: &str, id: u64) {
        let mut topics = self.topics();
        if let Some(subscribers) = topics.subscribers.get_mut(topic) {
            subscribers.retain(|(subscriber, _)| *subscriber != id);
            if subscribers.is_empty() {
                topics.subscribers.remove(topic);
            }
        }
    }
}

impl fmt::Debug for MemoryBackplane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let topics = self.topics();
        f.debug_struct("MemoryBackplane")
            .field("topics", &topics.subscribers.len())
            .finish()
    }
}

impl Backplane for MemoryBackplane {
    fn publish(&self, topic: &str, message: Bytes) {
        let mut topics = self.topics();
        if let Some(subscribers) = topics.subscribers.get_mut(topic) {
            // A subscriber whose task ended is dropped here.
            subscribers.retain(|(_, sender)| sender.send(message.clone()).is_ok());
        }
    }

    fn subscribe(
        &self,
        topic: &str,
        handler: BackplaneHandler,
        shutdown: CancellationToken,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        let (sender, mut receiver) = mpsc::unbounded_channel::<Bytes>();
        let id = {
            let mut topics = self.topics();
            topics.next_id += 1;
            let id = topics.next_id;
            topics
                .subscribers
                .entry(topic.to_owned())
                .or_default()
                .push((id, sender));
            id
        };
        let backplane = self.clone();
        let topic = topic.to_owned();
        // Messages are handed over on a task of their own, so a handler
        // never runs inside the publisher's call (and its locks).
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    message = receiver.recv() => match message {
                        Some(message) => handler(message),
                        None => break,
                    },
                }
            }
            backplane.unsubscribe(&topic, id);
        });
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn collector() -> (BackplaneHandler, mpsc::UnboundedReceiver<Bytes>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let handler: BackplaneHandler = Arc::new(move |message| {
            let _ = sender.send(message);
        });
        (handler, receiver)
    }

    async fn next(receiver: &mut mpsc::UnboundedReceiver<Bytes>) -> Option<Bytes> {
        tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .ok()
            .flatten()
    }

    #[tokio::test]
    async fn subscribers_of_a_topic_receive_its_messages_in_order() {
        let backplane = MemoryBackplane::new();
        let token = CancellationToken::new();
        let (first, mut first_rx) = collector();
        let (second, mut second_rx) = collector();
        let (other, mut other_rx) = collector();
        backplane.subscribe("t", first, token.clone()).await.unwrap();
        backplane
            .clone()
            .subscribe("t", second, token.clone())
            .await
            .unwrap();
        backplane.subscribe("u", other, token.clone()).await.unwrap();
        assert_eq!(backplane.subscribers("t"), 2);

        backplane.publish("t", Bytes::from_static(b"1"));
        backplane.publish("t", Bytes::from_static(b"2"));
        for receiver in [&mut first_rx, &mut second_rx] {
            assert_eq!(next(receiver).await.as_deref(), Some(&b"1"[..]));
            assert_eq!(next(receiver).await.as_deref(), Some(&b"2"[..]));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), other_rx.recv())
                .await
                .is_err(),
            "another topic receives nothing"
        );
    }

    #[tokio::test]
    async fn a_cancelled_subscription_stops_receiving() {
        let backplane = MemoryBackplane::new();
        let token = CancellationToken::new();
        let (handler, mut receiver) = collector();
        backplane.subscribe("t", handler, token.clone()).await.unwrap();
        token.cancel();
        for _ in 0..50 {
            if backplane.subscribers("t") == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(backplane.subscribers("t"), 0);
        backplane.publish("t", Bytes::from_static(b"late"));
        assert!(next(&mut receiver).await.is_none());
        assert!(format!("{backplane:?}").contains("MemoryBackplane"));
    }
}
