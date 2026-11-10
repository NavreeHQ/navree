//! [`RedisBackplane`]: navree-realtime's backplane on Redis pub/sub.

use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use navree_kernel::{BoxError, BoxFuture};
use navree_realtime::{Backplane, BackplaneHandler};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::RedisConnection;

/// How long the publisher keeps sending queued messages once the app shuts
/// down (the hub's goodbye among them).
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// The longest pause between two attempts to subscribe again.
const MAX_RESUBSCRIBE_DELAY: Duration = Duration::from_secs(10);

/// navree-realtime's [`Backplane`] on Redis pub/sub
/// (`realtime.backplane = "redis"`): each topic is the Redis channel
/// `<prefix><topic>` (`navree:realtime:<app_id>`).
///
/// Messages are published in order by one task on the shared connection
/// manager; each subscription holds a pub/sub connection of its own and
/// subscribes again, with a growing pause, when it is lost. Redis pub/sub
/// delivers at most once, so a process misses what is published while it is
/// disconnected; the hub's periodic presence snapshots make presence
/// converge again.
#[derive(Clone)]
pub struct RedisBackplane {
    connection: RedisConnection,
    prefix: String,
    outbox: mpsc::UnboundedSender<(String, Bytes)>,
    tracker: TaskTracker,
}

impl RedisBackplane {
    /// The backplane on `connection` with channels under `prefix`. Its
    /// publisher (and every subscription) runs on `tracker` until
    /// `shutdown`.
    pub fn new(
        connection: RedisConnection,
        prefix: &str,
        shutdown: CancellationToken,
        tracker: TaskTracker,
    ) -> Self {
        let (outbox, queued) = mpsc::unbounded_channel();
        tracker.spawn(publish_queued(connection.clone(), queued, shutdown));
        Self {
            connection,
            prefix: prefix.to_owned(),
            outbox,
            tracker,
        }
    }

    /// The Redis channel of `topic`.
    pub fn channel(&self, topic: &str) -> String {
        format!("{}{topic}", self.prefix)
    }
}

impl fmt::Debug for RedisBackplane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisBackplane")
            .field("connection", &self.connection.name())
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl Backplane for RedisBackplane {
    fn publish(&self, topic: &str, message: Bytes) {
        if self.outbox.send((self.channel(topic), message)).is_err() {
            tracing::debug!(target: "navree::redis", topic, "the backplane publisher has stopped; message dropped");
        }
    }

    fn subscribe(
        &self,
        topic: &str,
        handler: BackplaneHandler,
        shutdown: CancellationToken,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        let channel = self.channel(topic);
        Box::pin(async move {
            // The first subscription is awaited, so nothing published after
            // the app booted is missed. A server that is down does not fail
            // the boot: the task below keeps trying (and the `redis` health
            // check reports the server).
            let pubsub = match subscribe_to(&self.connection, &channel).await {
                Ok(pubsub) => Some(pubsub),
                Err(error) => {
                    tracing::warn!(target: "navree::redis", channel = %channel, %error, "cannot subscribe to the backplane yet; retrying in the background");
                    None
                }
            };
            let connection = self.connection.clone();
            self.tracker.spawn(async move {
                let mut pubsub = pubsub;
                let mut delay = Duration::from_millis(100);
                loop {
                    if let Some(mut active) = pubsub.take() {
                        delay = Duration::from_millis(100);
                        let mut messages = active.on_message();
                        loop {
                            tokio::select! {
                                () = shutdown.cancelled() => return,
                                message = messages.next() => match message {
                                    Some(message) => handler(Bytes::from(message.get_payload_bytes().to_vec())),
                                    None => break,
                                },
                            }
                        }
                        tracing::warn!(target: "navree::redis", channel = %channel, "lost the backplane subscription; subscribing again");
                    }
                    tokio::select! {
                        () = shutdown.cancelled() => return,
                        () = tokio::time::sleep(delay) => {}
                    }
                    delay = (delay * 2).min(MAX_RESUBSCRIBE_DELAY);
                    match subscribe_to(&connection, &channel).await {
                        Ok(fresh) => pubsub = Some(fresh),
                        Err(error) => tracing::warn!(target: "navree::redis", channel = %channel, %error, "cannot subscribe to the backplane"),
                    }
                }
            });
            Ok(())
        })
    }
}

/// A pub/sub connection subscribed to `channel`.
async fn subscribe_to(
    connection: &RedisConnection,
    channel: &str,
) -> Result<redis::aio::PubSub, crate::RedisError> {
    let mut pubsub = connection.pubsub().await?;
    pubsub
        .subscribe(channel)
        .await
        .map_err(|source| connection.error(source))?;
    Ok(pubsub)
}

/// Publishes queued messages in order until `shutdown`, then for at most
/// [`SHUTDOWN_GRACE`] more.
async fn publish_queued(
    connection: RedisConnection,
    mut queued: mpsc::UnboundedReceiver<(String, Bytes)>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            next = queued.recv() => match next {
                Some((channel, message)) => publish(&connection, &channel, &message).await,
                None => return,
            },
        }
    }
    while let Ok(Some((channel, message))) =
        tokio::time::timeout(SHUTDOWN_GRACE, queued.recv()).await
    {
        publish(&connection, &channel, &message).await;
    }
}

async fn publish(connection: &RedisConnection, channel: &str, message: &[u8]) {
    let published: Result<i64, redis::RedisError> = redis::cmd("PUBLISH")
        .arg(channel)
        .arg(message)
        .query_async(&mut connection.manager())
        .await;
    if let Err(error) = published {
        tracing::warn!(
            target: "navree::redis",
            channel,
            error = %connection.error(error),
            "cannot publish on the backplane"
        );
    }
}
