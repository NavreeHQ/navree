//! [`RedisTransport`] (`redis` feature): Redis Streams with consumer groups.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use navree_kernel::BoxFuture;
use redis::aio::{ConnectionManager, ConnectionManagerConfig, MultiplexedConnection};
use redis::{AsyncConnectionConfig, Cmd, RedisError, Value};
use tokio::sync::{OnceCell, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::envelope::headers;
use crate::options::RedisOptions;
use crate::transport::LocalRetry;
use crate::{
    DeadLetter, Delivery, DeliveryActions, Envelope, MicroError, MicroTransport, PatternKind,
    Subscription, has_wildcard,
};

/// The stream field that holds the payload; every other field is a header.
pub(crate) const PAYLOAD_FIELD: &str = "payload";

/// Entries one read takes.
const READ_COUNT: usize = 32;

/// Entries one reply stream keeps; replies are read as they arrive.
const REPLY_MAX_LEN: usize = 1_000;

/// How long a reply stream outlives its last reply.
const REPLY_TTL_SECS: u64 = 600;

/// The time limit of one non-blocking command.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Redis Streams: each pattern is a stream, `<prefix><pattern>`
/// (`navree:deal.won`), and each subscribing service is a consumer group
/// named after `micro.service`, whose instances are its consumers. An entry
/// holds the headers as fields and the payload in the field `payload`;
/// `XADD` trims every stream to about `micro.redis.max_len` entries.
///
/// - **Events.** An instance acknowledges an entry with `XACK` once the
///   handler's unit of work committed. An entry an instance took and never
///   acknowledged (it crashed, or the handler failed) is claimed with
///   `XAUTOCLAIM` once it has been idle `micro.redis.claim_after_ms`
///   (raised to the handler's time limit), and the claim is the next
///   attempt. A failed event sets its entry's idle time with
///   `XCLAIM .. IDLE` so that the claim happens when its backoff is over.
/// - **Requests.** A request is an entry in the pattern's stream, read by
///   the answering service's group, with `navree-reply-to` naming the
///   asking instance's reply stream (`<prefix>replies.<instance>`) and
///   `navree-correlation-id` pairing the reply with it. A pattern no
///   service has subscribed to fails at once with
///   [`MicroError::NoResponders`].
/// - **Broadcast** handlers read the stream with `XREAD`, without a group:
///   every instance gets every event, at most once.
/// - **Dead letters** (`micro.events.dead_letter = "transport"`) go to the
///   stream `<prefix>dead`.
///
/// Needs Redis 6.2 or later (`XAUTOCLAIM`). Patterns are full names: Redis
/// has no wildcard subscriptions.
pub struct RedisTransport {
    client: redis::Client,
    url: String,
    options: RedisOptions,
    service: String,
    instance: String,
    connect_timeout: Duration,
    commands: OnceCell<ConnectionManager>,
    replies: OnceCell<Arc<Pending>>,
    connected: Arc<AtomicBool>,
    closed: CancellationToken,
}

impl RedisTransport {
    /// A transport for `url` (`redis://` or `rediss://`) that listens as
    /// `service` (its consumer group) and consumes as `instance`. Connects
    /// on first use, giving up after `connect_timeout`. `MicroModule` builds
    /// it from `micro.redis`; build one yourself for
    /// [`Transport::custom`](crate::Transport::custom) or the
    /// [conformance suite](crate::conformance).
    pub fn new(
        url: impl Into<String>,
        options: &RedisOptions,
        service: &str,
        instance: &str,
        connect_timeout: Duration,
    ) -> Result<Self, MicroError> {
        let url = url.into();
        let client = redis::Client::open(url.as_str()).map_err(|error| {
            MicroError::transport(format!(
                "micro.url `{}` is not a Redis URL: {error}",
                crate::options::redact_url(&url)
            ))
        })?;
        if options.max_len == 0 {
            return Err(MicroError::transport("micro.redis.max_len must be at least 1"));
        }
        Ok(Self {
            client,
            url,
            options: options.clone(),
            service: service.to_owned(),
            instance: instance.to_owned(),
            connect_timeout,
            commands: OnceCell::new(),
            replies: OnceCell::new(),
            connected: Arc::new(AtomicBool::new(true)),
            closed: CancellationToken::new(),
        })
    }

    /// `<prefix><pattern>`.
    fn key(&self, pattern: &str) -> String {
        format!("{}{pattern}", self.options.prefix)
    }

    /// This instance's reply stream.
    fn reply_key(&self) -> String {
        format!("{}replies.{}", self.options.prefix, self.instance)
    }

    /// The connection for commands, made on first use; it reconnects by
    /// itself.
    async fn commands(&self) -> Result<ConnectionManager, MicroError> {
        let manager = self
            .commands
            .get_or_try_init(|| async {
                let config = ConnectionManagerConfig::new()
                    .set_connection_timeout(Some(self.connect_timeout))
                    .set_response_timeout(Some(COMMAND_TIMEOUT))
                    .set_min_delay(Duration::from_millis(100))
                    .set_max_delay(Duration::from_secs(30));
                let connecting = ConnectionManager::new_with_config(self.client.clone(), config);
                match tokio::time::timeout(self.connect_timeout, connecting).await {
                    Ok(Ok(manager)) => Ok(manager),
                    Ok(Err(error)) => Err(MicroError::transport(format!("cannot connect to Redis: {error}"))),
                    Err(_) => Err(MicroError::transport(format!(
                        "connecting to Redis took longer than {:?}",
                        self.connect_timeout
                    ))),
                }
            })
            .await?;
        Ok(manager.clone())
    }

    /// The reply reader of this instance, started on the first request.
    async fn replies(&self) -> Result<Arc<Pending>, MicroError> {
        let pending = self
            .replies
            .get_or_try_init(|| async {
                let reader = Reader::new(self.client.clone(), self.connect_timeout);
                let connection = reader.connect().await?;
                let pending = Arc::new(Pending::default());
                tokio::spawn(read_replies(
                    reader,
                    connection,
                    self.reply_key(),
                    self.options.block_ms,
                    pending.clone(),
                    self.closed.clone(),
                ));
                Ok::<_, MicroError>(pending)
            })
            .await?;
        Ok(pending.clone())
    }

    /// Runs `cmd` on the command connection, keeping the connection state.
    async fn run(&self, cmd: &Cmd) -> Result<Value, MicroError> {
        let mut connection = self.commands().await?;
        query(&mut connection, cmd, &self.connected).await
    }

    /// Creates the consumer group of this service on `key` when missing;
    /// returns whether it was created.
    async fn ensure_group(&self, key: &str) -> Result<bool, MicroError> {
        let mut connection = self.commands().await?;
        ensure_group(&mut connection, key, &self.service, &self.connected).await
    }
}

impl Drop for RedisTransport {
    fn drop(&mut self) {
        self.closed.cancel();
    }
}

impl fmt::Debug for RedisTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisTransport")
            .field("url", &crate::options::redact_url(&self.url))
            .field("prefix", &self.options.prefix)
            .field("service", &self.service)
            .field("instance", &self.instance)
            .finish_non_exhaustive()
    }
}

impl MicroTransport for RedisTransport {
    fn name(&self) -> &'static str {
        "redis"
    }

    fn request(&self, mut envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        Box::pin(async move {
            let pattern = envelope.pattern().to_owned();
            let key = self.key(&pattern);
            if !self.has_group(&key).await? {
                return Err(MicroError::NoResponders { pattern });
            }
            let pending = self.replies().await?;
            let correlation = Uuid::now_v7().to_string();
            envelope.set_header(headers::REPLY_TO, self.reply_key());
            envelope.set_header(headers::CORRELATION_ID, correlation.clone());
            let (sender, reply) = oneshot::channel();
            pending.insert(correlation.clone(), sender);
            if let Err(error) = self.run(&xadd(&key, self.options.max_len, &envelope)).await {
                pending.remove(&correlation);
                return Err(error);
            }
            let wait = envelope.remaining();
            let outcome = match wait {
                Some(wait) => tokio::time::timeout(wait, reply).await.ok(),
                None => Some(reply.await),
            };
            match outcome {
                Some(Ok(reply)) => Ok(reply),
                Some(Err(_)) => Err(MicroError::ShutDown),
                None => {
                    pending.remove(&correlation);
                    Err(MicroError::Timeout {
                        pattern,
                        after: wait.unwrap_or_default(),
                    })
                }
            }
        })
    }

    fn publish(&self, envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let key = self.key(envelope.pattern());
            self.run(&xadd(&key, self.options.max_len, &envelope)).await?;
            Ok(())
        })
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let mut grouped = Vec::new();
            let mut broadcast = Vec::new();
            for (index, subscription) in subscriptions.iter().enumerate() {
                if has_wildcard(&subscription.pattern) {
                    return Err(MicroError::transport(format!(
                        "the redis transport needs full pattern names, not `{}`",
                        subscription.pattern
                    )));
                }
                let key = self.key(&subscription.pattern);
                if subscription.is_broadcast() {
                    broadcast.push(Stream { index, key, claim_after: Duration::ZERO, kind: subscription.kind });
                } else {
                    self.ensure_group(&key).await?;
                    let claim_after = Duration::from_millis(self.options.claim_after_ms).max(subscription.ack_wait);
                    grouped.push(Stream { index, key, claim_after, kind: subscription.kind });
                }
            }
            let commands = self.commands().await?;
            let reader = Reader::new(self.client.clone(), self.connect_timeout);
            let group = Group {
                commands,
                connected: self.connected.clone(),
                group: self.service.clone(),
                consumer: self.instance.clone(),
            };
            let mut tasks = JoinSet::new();
            if !grouped.is_empty() {
                let streams = Arc::new(grouped);
                tasks.spawn(read_group(
                    reader.clone(),
                    group.clone(),
                    streams.clone(),
                    self.options.block_ms,
                    deliveries.clone(),
                ));
                tasks.spawn(claim(group.clone(), streams, deliveries.clone()));
            }
            if !broadcast.is_empty() {
                tasks.spawn(read_broadcast(reader, group.clone(), broadcast, self.options.block_ms, deliveries.clone()));
            }
            tokio::select! {
                () = deliveries.closed() => {}
                () = self.closed.cancelled() => {}
            }
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            Ok(())
        })
    }

    fn setup(&self, subscriptions: &[Subscription]) -> BoxFuture<'_, Result<Vec<String>, MicroError>> {
        let keys: Vec<String> = subscriptions
            .iter()
            .filter(|subscription| !subscription.is_broadcast())
            .map(|subscription| self.key(&subscription.pattern))
            .collect();
        Box::pin(async move {
            let mut lines = Vec::new();
            for key in keys {
                let created = self.ensure_group(&key).await?;
                lines.push(if created {
                    format!("created consumer group {} on {key}", self.service)
                } else {
                    format!("consumer group {} on {key} exists", self.service)
                });
            }
            Ok(lines)
        })
    }

    fn supports_wildcards(&self) -> bool {
        false
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn dead_letter(&self, letter: DeadLetter) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let mut envelope = letter.envelope();
            envelope.set_header("navree-error", single_line(&letter.error));
            envelope.set_header("navree-attempts", letter.attempts.to_string());
            envelope.set_header("navree-dead-service", letter.service.clone());
            let key = format!("{}dead", self.options.prefix);
            self.run(&xadd(&key, self.options.max_len, &envelope)).await?;
            Ok(())
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.closed.cancel();
        })
    }
}

impl RedisTransport {
    /// Whether some service's group reads `key`.
    async fn has_group(&self, key: &str) -> Result<bool, MicroError> {
        let mut cmd = redis::cmd("XINFO");
        cmd.arg("GROUPS").arg(key);
        let mut connection = self.commands().await?;
        match cmd.query_async::<Value>(&mut connection).await {
            Ok(Value::Array(groups) | Value::Set(groups)) => {
                self.connected.store(true, Ordering::SeqCst);
                Ok(!groups.is_empty())
            }
            Ok(_) => Ok(false),
            Err(error) if is_no_such_key(&error) => Ok(false),
            Err(error) => Err(failed(&self.connected, &error)),
        }
    }
}

/// One stream a listener reads.
#[derive(Clone, Debug)]
struct Stream {
    /// The subscription's index.
    index: usize,
    key: String,
    /// When an unacknowledged entry may be claimed.
    claim_after: Duration,
    kind: PatternKind,
}

/// The group a listener reads as, and the command connection.
#[derive(Clone)]
struct Group {
    commands: ConnectionManager,
    connected: Arc<AtomicBool>,
    group: String,
    consumer: String,
}

/// Makes the dedicated connections that blocking reads use.
#[derive(Clone)]
struct Reader {
    client: redis::Client,
    connect_timeout: Duration,
}

impl Reader {
    fn new(client: redis::Client, connect_timeout: Duration) -> Self {
        Self { client, connect_timeout }
    }

    /// A connection without a response timeout, for `BLOCK` reads.
    async fn connect(&self) -> Result<MultiplexedConnection, MicroError> {
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(Some(self.connect_timeout))
            .set_response_timeout(None);
        match tokio::time::timeout(
            self.connect_timeout,
            self.client.get_multiplexed_async_connection_with_config(&config),
        )
        .await
        {
            Ok(Ok(connection)) => Ok(connection),
            Ok(Err(error)) => Err(MicroError::transport(format!("cannot connect to Redis: {error}"))),
            Err(_) => Err(MicroError::transport(format!(
                "connecting to Redis took longer than {:?}",
                self.connect_timeout
            ))),
        }
    }

    /// A connection, retried with backoff (100 ms doubling to 30 s) until
    /// it is made or the listener stops.
    async fn reconnect(&self, deliveries: &mpsc::Sender<Delivery>) -> Option<MultiplexedConnection> {
        let mut attempt = 0u32;
        loop {
            match self.connect().await {
                Ok(connection) => return Some(connection),
                Err(error) => {
                    tracing::warn!(target: "navree::micro", %error, "reconnecting to Redis");
                    attempt = attempt.saturating_add(1);
                    tokio::select! {
                        () = tokio::time::sleep(backoff(attempt)) => {}
                        () = deliveries.closed() => return None,
                    }
                }
            }
        }
    }
}

/// Reconnect pauses: 100 ms doubling to 30 seconds.
fn backoff(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(16);
    Duration::from_millis(100u64.saturating_mul(1u64 << exponent)).min(Duration::from_secs(30))
}

/// The requests waiting for their replies, by correlation id.
#[derive(Default)]
struct Pending(Mutex<HashMap<String, oneshot::Sender<Envelope>>>);

impl Pending {
    fn insert(&self, correlation: String, sender: oneshot::Sender<Envelope>) {
        lock(&self.0).insert(correlation, sender);
    }

    fn remove(&self, correlation: &str) -> Option<oneshot::Sender<Envelope>> {
        lock(&self.0).remove(correlation)
    }
}

/// Reads this instance's reply stream and hands each reply to its request,
/// until the transport closes.
async fn read_replies(
    reader: Reader,
    mut connection: MultiplexedConnection,
    key: String,
    block_ms: u64,
    pending: Arc<Pending>,
    closed: CancellationToken,
) {
    let mut last = "0-0".to_owned();
    let mut attempt = 0u32;
    loop {
        let mut cmd = redis::cmd("XREAD");
        cmd.arg("COUNT")
            .arg(READ_COUNT * 4)
            .arg("BLOCK")
            .arg(block_ms.max(1))
            .arg("STREAMS")
            .arg(&key)
            .arg(&last);
        let read = tokio::select! {
            read = cmd.query_async::<Value>(&mut connection) => read,
            () = closed.cancelled() => return,
        };
        match read {
            Ok(value) => {
                attempt = 0;
                for (_, entries) in stream_entries(value) {
                    for entry in entries {
                        last.clone_from(&entry.id);
                        let envelope = entry.envelope("");
                        if let Some(correlation) = envelope.header(headers::CORRELATION_ID)
                            && let Some(sender) = pending.remove(correlation)
                        {
                            let _ = sender.send(envelope);
                        }
                    }
                }
                if last != "0-0" {
                    // Replies are read once: drop the ones behind us.
                    let mut trim = redis::cmd("XTRIM");
                    trim.arg(&key).arg("MINID").arg(&last);
                    let _ = trim.query_async::<Value>(&mut connection).await;
                }
            }
            Err(error) => {
                tracing::warn!(target: "navree::micro", %error, "reading replies from Redis failed");
                attempt = attempt.saturating_add(1);
                tokio::select! {
                    () = tokio::time::sleep(backoff(attempt)) => {}
                    () = closed.cancelled() => return,
                }
                if let Ok(fresh) = reader.connect().await {
                    connection = fresh;
                }
            }
        }
    }
}

/// Reads new entries for the group with `XREADGROUP` and hands them to the
/// listener, until it stops.
async fn read_group(
    reader: Reader,
    group: Group,
    streams: Arc<Vec<Stream>>,
    block_ms: u64,
    deliveries: mpsc::Sender<Delivery>,
) {
    let by_key: HashMap<&str, &Stream> = streams.iter().map(|s| (s.key.as_str(), s)).collect();
    'connect: loop {
        let Some(mut connection) = reader.reconnect(&deliveries).await else {
            return;
        };
        loop {
            let mut cmd = redis::cmd("XREADGROUP");
            cmd.arg("GROUP")
                .arg(&group.group)
                .arg(&group.consumer)
                .arg("COUNT")
                .arg(READ_COUNT)
                .arg("BLOCK")
                .arg(block_ms.max(1))
                .arg("STREAMS");
            for stream in streams.iter() {
                cmd.arg(&stream.key);
            }
            for _ in streams.iter() {
                cmd.arg(">");
            }
            let read = tokio::select! {
                read = cmd.query_async::<Value>(&mut connection) => read,
                () = deliveries.closed() => return,
            };
            let value = match read {
                Ok(value) => {
                    group.connected.store(true, Ordering::SeqCst);
                    value
                }
                Err(error) if error.code() == Some("NOGROUP") => {
                    // The stream was deleted: create the groups again.
                    let mut commands = group.commands.clone();
                    for stream in streams.iter() {
                        let _ = ensure_group(&mut commands, &stream.key, &group.group, &group.connected).await;
                    }
                    continue;
                }
                Err(error) => {
                    tracing::warn!(target: "navree::micro", %error, "reading from Redis failed");
                    failed(&group.connected, &error);
                    continue 'connect;
                }
            };
            for (key, entries) in stream_entries(value) {
                let Some(stream) = by_key.get(key.as_str()) else {
                    continue;
                };
                for entry in entries {
                    let delivery = group.delivery(stream, entry, 1);
                    if deliveries.send(delivery).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

/// Claims the group's entries that stayed unacknowledged past their
/// stream's `claim_after` (a crashed instance's, or failed events whose
/// backoff is over) and hands them to the listener as their next attempt.
async fn claim(group: Group, streams: Arc<Vec<Stream>>, deliveries: mpsc::Sender<Delivery>) {
    let shortest = streams
        .iter()
        .map(|stream| stream.claim_after)
        .min()
        .unwrap_or(Duration::from_secs(1));
    let poll = (shortest / 10).clamp(Duration::from_millis(20), Duration::from_secs(1));
    let mut commands = group.commands.clone();
    loop {
        tokio::select! {
            () = tokio::time::sleep(poll) => {}
            () = deliveries.closed() => return,
        }
        for stream in streams.iter() {
            let mut cmd = redis::cmd("XAUTOCLAIM");
            cmd.arg(&stream.key)
                .arg(&group.group)
                .arg(&group.consumer)
                .arg(millis(stream.claim_after))
                .arg("0-0")
                .arg("COUNT")
                .arg(READ_COUNT);
            let claimed = match query(&mut commands, &cmd, &group.connected).await {
                Ok(value) => autoclaimed(value),
                Err(error) => {
                    tracing::debug!(target: "navree::micro", %error, key = %stream.key, "XAUTOCLAIM failed");
                    continue;
                }
            };
            for entry in claimed {
                let attempt = delivered(&mut commands, &stream.key, &group.group, &entry.id, &group.connected)
                    .await
                    .unwrap_or(2);
                let delivery = group.delivery(stream, entry, attempt);
                if deliveries.send(delivery).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Reads broadcast streams with `XREAD` (no group: every instance gets
/// every entry, at most once), from the entries added after it started.
async fn read_broadcast(
    reader: Reader,
    group: Group,
    streams: Vec<Stream>,
    block_ms: u64,
    deliveries: mpsc::Sender<Delivery>,
) {
    let mut commands = group.commands.clone();
    let mut last: Vec<String> = Vec::with_capacity(streams.len());
    for stream in &streams {
        let mut cmd = redis::cmd("XREVRANGE");
        cmd.arg(&stream.key).arg("+").arg("-").arg("COUNT").arg(1);
        let id = query(&mut commands, &cmd, &group.connected)
            .await
            .ok()
            .and_then(|value| entries(value).into_iter().next())
            .map_or_else(|| "0-0".to_owned(), |entry| entry.id);
        last.push(id);
    }
    'connect: loop {
        let Some(mut connection) = reader.reconnect(&deliveries).await else {
            return;
        };
        loop {
            let mut cmd = redis::cmd("XREAD");
            cmd.arg("COUNT")
                .arg(READ_COUNT)
                .arg("BLOCK")
                .arg(block_ms.max(1))
                .arg("STREAMS");
            for stream in &streams {
                cmd.arg(&stream.key);
            }
            for id in &last {
                cmd.arg(id);
            }
            let read = tokio::select! {
                read = cmd.query_async::<Value>(&mut connection) => read,
                () = deliveries.closed() => return,
            };
            let value = match read {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(target: "navree::micro", %error, "reading from Redis failed");
                    continue 'connect;
                }
            };
            for (key, entries) in stream_entries(value) {
                let Some(position) = streams.iter().position(|stream| stream.key == key) else {
                    continue;
                };
                for entry in entries {
                    last[position].clone_from(&entry.id);
                    let envelope = entry.envelope(&key);
                    let delivery = LocalRetry::delivery(deliveries.clone(), envelope, streams[position].index);
                    if deliveries.send(delivery).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

impl Group {
    /// The delivery of a group entry.
    fn delivery(&self, stream: &Stream, entry: Entry, attempt: u32) -> Delivery {
        let id = entry.id.clone();
        let envelope = entry.envelope(&stream.key);
        let reply_to = match stream.kind {
            PatternKind::Message => envelope.header(headers::REPLY_TO).map(str::to_owned),
            PatternKind::Event => None,
        };
        let actions = EntryActions {
            commands: self.commands.clone(),
            connected: self.connected.clone(),
            key: stream.key.clone(),
            group: self.group.clone(),
            consumer: self.consumer.clone(),
            id,
            claim_after: stream.claim_after,
            reply_to,
        };
        Delivery::new(envelope, stream.index, attempt, actions)
    }
}

/// Acknowledges a group entry, sets it up for a later claim, or replies to
/// the request it holds.
struct EntryActions {
    commands: ConnectionManager,
    connected: Arc<AtomicBool>,
    key: String,
    group: String,
    consumer: String,
    id: String,
    claim_after: Duration,
    reply_to: Option<String>,
}

impl EntryActions {
    fn xack(&self) -> Cmd {
        let mut cmd = redis::cmd("XACK");
        cmd.arg(&self.key).arg(&self.group).arg(&self.id);
        cmd
    }

    /// `XCLAIM` to ourselves with the given idle time, without counting a
    /// delivery.
    fn set_idle(&self, idle: Duration) -> Cmd {
        let mut cmd = redis::cmd("XCLAIM");
        cmd.arg(&self.key)
            .arg(&self.group)
            .arg(&self.consumer)
            .arg(0)
            .arg(&self.id)
            .arg("IDLE")
            .arg(millis(idle))
            .arg("JUSTID");
        cmd
    }
}

impl DeliveryActions for EntryActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            let mut commands = self.commands.clone();
            query(&mut commands, &self.xack(), &self.connected).await?;
            Ok(())
        })
    }

    fn retry_in(self: Box<Self>, delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            let mut commands = self.commands.clone();
            if delay < self.claim_after {
                // Idle for `claim_after - delay` now: claimable in `delay`.
                query(&mut commands, &self.set_idle(self.claim_after - delay), &self.connected).await?;
                return Ok(());
            }
            // A backoff longer than the claim time: keep the entry ours
            // until what is left of it is shorter, then hand it over. If
            // this process dies first, the entry is claimed earlier, which
            // at-least-once delivery allows.
            let this = *self;
            tokio::spawn(async move {
                let mut commands = this.commands.clone();
                let due = tokio::time::Instant::now() + delay;
                loop {
                    let left = due.saturating_duration_since(tokio::time::Instant::now());
                    if left < this.claim_after {
                        let _ = query(&mut commands, &this.set_idle(this.claim_after - left), &this.connected).await;
                        return;
                    }
                    let _ = query(&mut commands, &this.set_idle(Duration::ZERO), &this.connected).await;
                    tokio::time::sleep((this.claim_after / 2).min(left - this.claim_after + Duration::from_millis(1))).await;
                }
            });
            Ok(())
        })
    }

    fn reply(self: Box<Self>, reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            let mut commands = self.commands.clone();
            if let Some(reply_to) = &self.reply_to {
                let mut pipe = redis::pipe();
                pipe.add_command(xadd(reply_to, REPLY_MAX_LEN, &reply))
                    .ignore()
                    .cmd("EXPIRE")
                    .arg(reply_to)
                    .arg(REPLY_TTL_SECS)
                    .ignore();
                if let Err(error) = pipe.query_async::<()>(&mut commands).await {
                    return Err(failed(&self.connected, &error));
                }
            }
            query(&mut commands, &self.xack(), &self.connected).await?;
            Ok(())
        })
    }
}

/// `XADD key MAXLEN ~ max_len * <headers> payload <payload>`.
fn xadd(key: &str, max_len: usize, envelope: &Envelope) -> Cmd {
    let mut cmd = redis::cmd("XADD");
    cmd.arg(key).arg("MAXLEN").arg("~").arg(max_len).arg("*");
    for (name, value) in envelope.headers() {
        if name != PAYLOAD_FIELD {
            cmd.arg(name).arg(single_line(value));
        }
    }
    cmd.arg(PAYLOAD_FIELD).arg(envelope.payload().as_ref());
    cmd
}

/// Creates `group` on `key` (and the stream) when missing, reading new
/// entries only; returns whether it was created.
async fn ensure_group(
    connection: &mut ConnectionManager,
    key: &str,
    group: &str,
    connected: &AtomicBool,
) -> Result<bool, MicroError> {
    let mut cmd = redis::cmd("XGROUP");
    cmd.arg("CREATE").arg(key).arg(group).arg("$").arg("MKSTREAM");
    match cmd.query_async::<Value>(connection).await {
        Ok(_) => Ok(true),
        Err(error) if error.code() == Some("BUSYGROUP") => Ok(false),
        Err(error) => Err(failed(connected, &error)),
    }
}

/// How many times the group delivered entry `id` (`XPENDING`).
async fn delivered(
    connection: &mut ConnectionManager,
    key: &str,
    group: &str,
    id: &str,
    connected: &AtomicBool,
) -> Option<u32> {
    let mut cmd = redis::cmd("XPENDING");
    cmd.arg(key).arg(group).arg(id).arg(id).arg(1);
    let value = query(connection, &cmd, connected).await.ok()?;
    let Value::Array(rows) = value else {
        return None;
    };
    let Some(Value::Array(row)) = rows.into_iter().next() else {
        return None;
    };
    match row.get(3) {
        Some(Value::Int(count)) => u32::try_from(*count).ok(),
        _ => None,
    }
}

/// Runs a command, keeping track of whether the connection works.
async fn query(connection: &mut ConnectionManager, cmd: &Cmd, connected: &AtomicBool) -> Result<Value, MicroError> {
    match cmd.query_async::<Value>(connection).await {
        Ok(value) => {
            connected.store(true, Ordering::SeqCst);
            Ok(value)
        }
        Err(error) => Err(failed(connected, &error)),
    }
}

/// A Redis error as a transport error; marks the connection down when the
/// error is about the connection.
fn failed(connected: &AtomicBool, error: &RedisError) -> MicroError {
    if error.is_io_error() || error.is_connection_dropped() || error.is_connection_refusal() || error.is_timeout() {
        connected.store(false, Ordering::SeqCst);
    }
    MicroError::transport(format!("the Redis command failed: {error}"))
}

/// `ERR no such key` (`XINFO GROUPS` on a missing stream).
fn is_no_such_key(error: &RedisError) -> bool {
    error.to_string().to_ascii_lowercase().contains("no such key")
}

/// One stream entry: its id and its fields.
#[derive(Debug, Default)]
struct Entry {
    id: String,
    fields: Vec<(String, Bytes)>,
}

impl Entry {
    /// The envelope it holds; the pattern comes from `navree-pattern`, else
    /// the stream's key.
    fn envelope(self, key: &str) -> Envelope {
        let mut payload = Bytes::new();
        let mut pairs = Vec::with_capacity(self.fields.len());
        for (name, value) in self.fields {
            if name == PAYLOAD_FIELD {
                payload = value;
            } else {
                pairs.push((name, String::from_utf8_lossy(&value).into_owned()));
            }
        }
        let pattern = pairs
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(headers::PATTERN))
            .map_or_else(|| key.to_owned(), |(_, value)| value.clone());
        let mut envelope = Envelope::new(pattern, payload);
        for (name, value) in pairs {
            envelope.set_header(&name, value);
        }
        envelope
    }
}

/// The entries of an `XREAD`/`XREADGROUP` reply, per stream key; nil (a
/// read that timed out) is none.
fn stream_entries(value: Value) -> Vec<(String, Vec<Entry>)> {
    let streams: Vec<(Value, Value)> = match value {
        Value::Array(streams) => streams
            .into_iter()
            .filter_map(|stream| match stream {
                Value::Array(pair) if pair.len() == 2 => {
                    let mut pair = pair.into_iter();
                    Some((pair.next()?, pair.next()?))
                }
                _ => None,
            })
            .collect(),
        Value::Map(streams) => streams,
        _ => Vec::new(),
    };
    streams
        .into_iter()
        .filter_map(|(key, entries_value)| Some((text(&key)?, entries(entries_value))))
        .collect()
}

/// A list of `[id, [field, value, ...]]` entries; nil entries (deleted
/// while pending) are skipped.
fn entries(value: Value) -> Vec<Entry> {
    let Value::Array(items) = value else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| {
            let Value::Array(parts) = item else {
                return None;
            };
            let mut parts = parts.into_iter();
            let id = text(&parts.next()?)?;
            let fields = match parts.next()? {
                Value::Array(flat) => {
                    let mut fields = Vec::with_capacity(flat.len() / 2);
                    let mut flat = flat.into_iter();
                    while let (Some(name), Some(value)) = (flat.next(), flat.next()) {
                        fields.push((text(&name)?.to_ascii_lowercase(), bytes(value)?));
                    }
                    fields
                }
                Value::Map(map) => map
                    .into_iter()
                    .filter_map(|(name, value)| Some((text(&name)?.to_ascii_lowercase(), bytes(value)?)))
                    .collect(),
                _ => return None,
            };
            Some(Entry { id, fields })
        })
        .collect()
}

/// The entries an `XAUTOCLAIM` claimed: `[cursor, entries, deleted ids]`.
fn autoclaimed(value: Value) -> Vec<Entry> {
    match value {
        Value::Array(parts) => parts.into_iter().nth(1).map(entries).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::BulkString(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        Value::SimpleString(text) => Some(text.clone()),
        Value::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

fn bytes(value: Value) -> Option<Bytes> {
    match value {
        Value::BulkString(bytes) => Some(Bytes::from(bytes)),
        Value::SimpleString(text) => Some(Bytes::from(text)),
        Value::Int(n) => Some(Bytes::from(n.to_string())),
        Value::Nil => Some(Bytes::new()),
        _ => None,
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Header values are one line.
fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulk(text: &str) -> Value {
        Value::BulkString(text.as_bytes().to_vec())
    }

    #[test]
    fn entries_are_written_as_header_fields_and_a_payload() {
        let envelope = Envelope::new("deal.won", r#"{"id":1}"#)
            .with_header(headers::MESSAGE_ID, "m1")
            .with_header("x-note", "a\nb");
        let packed = xadd("navree:deal.won", 500, &envelope).get_packed_command();
        let text = String::from_utf8_lossy(&packed);
        for part in ["XADD", "navree:deal.won", "MAXLEN", "~", "500", "navree-message-id", "m1", "a b", "payload", r#"{"id":1}"#] {
            assert!(text.contains(part), "{part} missing from {text}");
        }
    }

    #[test]
    fn read_replies_parse_into_envelopes() {
        let value = Value::Array(vec![Value::Array(vec![
            bulk("navree:deal.won"),
            Value::Array(vec![
                Value::Array(vec![
                    bulk("1-0"),
                    Value::Array(vec![
                        bulk("navree-pattern"),
                        bulk("deal.won"),
                        bulk("Navree-Message-Id"),
                        bulk("m1"),
                        bulk("payload"),
                        bulk("{}"),
                    ]),
                ]),
                Value::Nil,
            ]),
        ])]);
        let mut streams = stream_entries(value);
        assert_eq!(streams.len(), 1);
        let (key, mut entries) = streams.remove(0);
        assert_eq!(key, "navree:deal.won");
        assert_eq!(entries.len(), 1, "nil entries are skipped");
        let entry = entries.remove(0);
        assert_eq!(entry.id, "1-0");
        let envelope = entry.envelope("navree:deal.won");
        assert_eq!(envelope.pattern(), "deal.won");
        assert_eq!(envelope.message_id(), Some("m1"));
        assert_eq!(envelope.payload().as_ref(), b"{}");
        assert!(stream_entries(Value::Nil).is_empty(), "a read that timed out");
    }

    #[test]
    fn an_entry_without_a_pattern_header_takes_the_key() {
        let entry = Entry {
            id: "1-0".into(),
            fields: vec![("payload".into(), Bytes::from_static(b"1"))],
        };
        assert_eq!(entry.envelope("deal.won").pattern(), "deal.won");
    }

    #[test]
    fn autoclaim_replies_give_their_entries() {
        let value = Value::Array(vec![
            bulk("0-0"),
            Value::Array(vec![Value::Array(vec![bulk("5-0"), Value::Array(vec![bulk("payload"), bulk("x")])])]),
            Value::Array(vec![]),
        ]);
        let claimed = autoclaimed(value);
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, "5-0");
    }

    #[test]
    fn reconnects_back_off_from_100_ms_to_30_seconds() {
        assert_eq!(backoff(1), Duration::from_millis(100));
        assert_eq!(backoff(3), Duration::from_millis(400));
        assert_eq!(backoff(40), Duration::from_secs(30));
    }

    #[test]
    fn bad_urls_and_settings_are_refused() {
        let options = RedisOptions::default();
        let error = RedisTransport::new("http://x", &options, "s", "s-1", Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("not a Redis URL"), "{error}");
        let error = RedisTransport::new(
            "redis://127.0.0.1:6379",
            &RedisOptions { max_len: 0, ..RedisOptions::default() },
            "s",
            "s-1",
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(error.to_string().contains("micro.redis.max_len"), "{error}");
        let transport = RedisTransport::new("redis://u:secret@127.0.0.1:6379/0", &options, "deals", "deals-1", Duration::from_secs(1)).unwrap();
        assert!(!format!("{transport:?}").contains("secret"));
        assert_eq!(transport.key("deal.won"), "navree:deal.won");
        assert_eq!(transport.reply_key(), "navree:replies.deals-1");
    }
}
