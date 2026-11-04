//! [`KafkaTransport`] (`kafka` feature): Kafka 4.0 or later, through a
//! small client of Navree's own on kafka-protocol.

mod client;
mod sasl;

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use kafka_protocol::messages::consumer_group_heartbeat_request::TopicPartitions;
use kafka_protocol::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_protocol::messages::offset_fetch_request::{
    OffsetFetchRequestGroup, OffsetFetchRequestTopics,
};
use kafka_protocol::messages::{
    ConsumerGroupHeartbeatRequest, FindCoordinatorRequest, GroupId, OffsetCommitRequest,
    OffsetFetchRequest,
};
use kafka_protocol::protocol::StrBytes;
use navree_kernel::BoxFuture;
use tokio::sync::{OnceCell, mpsc, oneshot};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use self::client::{
    Cluster, Conn, ConnSettings, EARLIEST, InRecord, LATEST, OutRecord, ProduceError, code,
    describe, lock, records, topic_name, version,
};
use crate::envelope::headers;
use crate::options::KafkaOptions;
use crate::transport::LocalRetry;
use crate::transport::broker::{client_config, reconnect_delay};
use crate::{
    DeadLetter, Delivery, DeliveryActions, Envelope, MicroError, MicroTransport, PatternKind,
    Subscribed, Subscription, TlsOptions, has_wildcard,
};

/// How long one fetch waits for records.
const FETCH_WAIT: Duration = Duration::from_millis(500);

/// How often handled offsets are committed.
const COMMIT_EVERY: Duration = Duration::from_secs(1);

/// How long a revoked partition waits for the events it handed out.
const REVOKE_TIMEOUT: Duration = Duration::from_secs(30);

/// The rebalance timeout sent to the coordinator.
const REBALANCE_TIMEOUT_MS: i32 = 60_000;

/// The header of a retried event: when it is due, in Unix milliseconds.
const DUE: &str = "navree-due";

/// Kafka: each pattern is a topic (`deal.won`), and each subscribing
/// service is a consumer group named after `micro.service`, using Kafka's
/// consumer group protocol (KIP-848, Kafka 4.0 or later), where the broker
/// assigns partitions.
///
/// - **Events.** The event's key is the record key (hashed like the Java
///   client), so events with the same key land on the same partition and
///   keep their order. Offsets are committed once every earlier record of
///   the partition was handled; a revoked partition is released after the
///   events it handed out are done. A new group starts at the end of the
///   topics as they were when the listener started (`micro.kafka.start_from
///   = "latest"`), or at their beginning (`"earliest"`).
/// - **Retries.** A failed event is copied to the service's retry topic,
///   `<service>.retry`, with its attempt and the time it becomes due, and
///   handled again from there, so it never blocks its partition.
/// - **Requests** are records on the pattern's topic, read by the answering
///   service's group; replies go to the asking service's reply topic,
///   `<service>.replies`, which each instance reads whole. A pattern whose
///   topic does not exist fails at once with [`MicroError::NoResponders`].
/// - **Broadcast** handlers read the topic's partitions themselves, from
///   the end, without a group.
/// - **Dead letters** (`micro.events.dead_letter = "transport"`) go to
///   `<service>.dead`.
///
/// `micro:setup` (and listening) create missing topics with
/// `micro.kafka.partitions`, `replication_factor` and `retention_hours`.
/// Publishing an event creates its topic when missing. Record batches
/// compressed with gzip or snappy are read; lz4 and zstd are not.
pub struct KafkaTransport {
    cluster: Arc<Cluster>,
    brokers: Vec<String>,
    options: KafkaOptions,
    service: String,
    instance: String,
    replies: OnceCell<Arc<Replies>>,
    closed: CancellationToken,
}

impl KafkaTransport {
    /// A transport for the bootstrap brokers in `url` (comma-separated
    /// `host:port`) that listens as `service` (its consumer group). Connects
    /// on first use, giving up after `connect_timeout`. `MicroModule`
    /// builds it from `micro.kafka`; build one yourself for
    /// [`Transport::custom`](crate::Transport::custom) or the
    /// [conformance suite](crate::conformance).
    pub fn new(
        url: impl Into<String>,
        options: &KafkaOptions,
        service: &str,
        instance: &str,
        connect_timeout: Duration,
    ) -> Result<Self, MicroError> {
        let url = url.into();
        let brokers: Vec<String> = url
            .split(',')
            .map(str::trim)
            .filter(|broker| !broker.is_empty())
            .map(|broker| {
                broker
                    .strip_prefix("kafka://")
                    .unwrap_or(broker)
                    .trim_end_matches('/')
                    .to_owned()
            })
            .collect();
        if brokers.is_empty() {
            return Err(MicroError::transport("micro.url names no Kafka broker"));
        }
        if !matches!(options.start_from.as_str(), "latest" | "earliest") {
            return Err(MicroError::transport(format!(
                "micro.kafka.start_from must be `latest` or `earliest`, not `{}`",
                options.start_from
            )));
        }
        if options.partitions < 1 || options.replication_factor < 1 {
            return Err(MicroError::transport(
                "micro.kafka.partitions and micro.kafka.replication_factor must be at least 1",
            ));
        }
        if let Some(sasl) = &options.sasl
            && !matches!(
                sasl.mechanism.as_str(),
                "PLAIN" | "SCRAM-SHA-256" | "SCRAM-SHA-512"
            )
        {
            return Err(MicroError::transport(format!(
                "micro.kafka.sasl.mechanism must be PLAIN, SCRAM-SHA-256 or SCRAM-SHA-512, not `{}`",
                sasl.mechanism
            )));
        }
        let tls = if options.tls {
            Some(TlsConnector::from(client_config(
                &TlsOptions::default(),
                "micro.kafka",
            )?))
        } else {
            None
        };
        let settings = ConnSettings {
            client_id: instance.to_owned(),
            tls,
            sasl: options.sasl.clone(),
            connect_timeout,
        };
        Ok(Self {
            cluster: Arc::new(Cluster::new(brokers.clone(), settings)),
            brokers,
            options: options.clone(),
            service: service.to_owned(),
            instance: instance.to_owned(),
            replies: OnceCell::new(),
            closed: CancellationToken::new(),
        })
    }

    fn retry_topic(&self) -> String {
        format!("{}.retry", self.service)
    }

    fn dead_topic(&self) -> String {
        format!("{}.dead", self.service)
    }

    fn reply_topic(&self) -> String {
        format!("{}.replies", self.service)
    }

    fn retention_ms(&self) -> Option<u64> {
        Some(self.options.retention_hours.saturating_mul(3_600_000))
    }

    /// Creates `topics` when missing; returns each with whether it was
    /// created.
    async fn ensure_topics(&self, topics: &[String]) -> Result<Vec<(String, bool)>, MicroError> {
        let existing = self.cluster.refresh(topics).await?;
        let missing: Vec<(String, i32, i16, Option<u64>)> = topics
            .iter()
            .filter(|topic| !existing.contains_key(*topic))
            .map(|topic| {
                (
                    topic.clone(),
                    self.options.partitions,
                    self.options.replication_factor,
                    self.retention_ms(),
                )
            })
            .collect();
        let created = self.cluster.create_topics(&missing).await?;
        let mut lines: Vec<(String, bool)> = topics
            .iter()
            .filter(|topic| existing.contains_key(*topic))
            .map(|topic| (topic.clone(), false))
            .collect();
        lines.extend(created);
        if !missing.is_empty() {
            // Wait for the new topics' leaders.
            for _ in 0..50 {
                let known = self.cluster.refresh(topics).await?;
                if topics.iter().all(|topic| {
                    known
                        .get(topic)
                        .is_some_and(|meta| !meta.partitions.is_empty())
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        Ok(lines)
    }

    /// Produces `envelope` to `topic`, creating the topic when missing.
    async fn produce_creating(&self, topic: &str, record: OutRecord) -> Result<(), MicroError> {
        match self.cluster.produce(topic, record.clone()).await {
            Ok(_) => Ok(()),
            Err(ProduceError::NoTopic) => {
                self.ensure_topics(&[topic.to_owned()]).await?;
                match self.cluster.produce(topic, record).await {
                    Ok(_) => Ok(()),
                    Err(ProduceError::NoTopic) => Err(MicroError::transport(format!(
                        "the Kafka topic {topic} could not be created"
                    ))),
                    Err(ProduceError::Failed(error)) => Err(error),
                }
            }
            Err(ProduceError::Failed(error)) => Err(error),
        }
    }

    /// The reply reader of this instance, positioned at the end of the
    /// reply topic before the first request goes out.
    async fn replies(&self) -> Result<Arc<Replies>, MicroError> {
        let replies = self
            .replies
            .get_or_try_init(|| async {
                let topic = self.reply_topic();
                self.ensure_topics(std::slice::from_ref(&topic)).await?;
                let reader = Reader::start(self.cluster.clone(), vec![topic]).await?;
                let replies = Arc::new(Replies::default());
                tokio::spawn(read_replies(reader, replies.clone(), self.closed.clone()));
                Ok::<_, MicroError>(replies)
            })
            .await?;
        Ok(replies.clone())
    }
}

impl Drop for KafkaTransport {
    fn drop(&mut self) {
        self.closed.cancel();
    }
}

impl fmt::Debug for KafkaTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KafkaTransport")
            .field("brokers", &self.brokers)
            .field("service", &self.service)
            .field("instance", &self.instance)
            .field("tls", &self.options.tls)
            .finish_non_exhaustive()
    }
}

impl MicroTransport for KafkaTransport {
    fn name(&self) -> &'static str {
        "kafka"
    }

    fn request(&self, mut envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        Box::pin(async move {
            let pattern = envelope.pattern().to_owned();
            let replies = self.replies().await?;
            let correlation = Uuid::now_v7().to_string();
            envelope.set_header(headers::REPLY_TO, self.reply_topic());
            envelope.set_header(headers::CORRELATION_ID, correlation.clone());
            let (sender, reply) = oneshot::channel();
            lock(&replies.0).insert(correlation.clone(), sender);
            let forget = || {
                lock(&replies.0).remove(&correlation);
            };
            match self.cluster.produce(&pattern, out_record(&envelope)).await {
                Ok(_) => {}
                Err(ProduceError::NoTopic) => {
                    forget();
                    return Err(MicroError::NoResponders { pattern });
                }
                Err(ProduceError::Failed(error)) => {
                    forget();
                    return Err(error);
                }
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
                    forget();
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
            self.produce_creating(envelope.pattern(), out_record(&envelope))
                .await
        })
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        self.listen_subscribed(subscriptions, deliveries, Subscribed::detached())
    }

    /// Signals once the topics exist and where each new group starts is
    /// read, so an event published after the signal is delivered. While the
    /// broker can't be reached it signals at once (nothing can be published
    /// then either) and keeps trying in the background.
    fn listen_subscribed(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
        subscribed: Subscribed,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            if let Some(wild) = subscriptions.iter().find(|s| has_wildcard(&s.pattern)) {
                return Err(MicroError::transport(format!(
                    "the kafka transport needs full pattern names, not `{}`",
                    wild.pattern
                )));
            }
            let grouped: Vec<(usize, &Subscription)> = subscriptions
                .iter()
                .enumerate()
                .filter(|(_, s)| !s.is_broadcast())
                .collect();
            let broadcast: Vec<(usize, &Subscription)> = subscriptions
                .iter()
                .enumerate()
                .filter(|(_, s)| s.is_broadcast())
                .collect();
            let has_events = grouped.iter().any(|(_, s)| s.kind == PatternKind::Event);
            let retry_topic = has_events.then(|| self.retry_topic());
            let mut topics: Vec<String> = subscriptions.iter().map(|s| s.pattern.clone()).collect();
            topics.extend(retry_topic.clone());
            topics.sort();
            topics.dedup();
            let mut subscribed = Some(subscribed);
            let mut attempt = 0u32;
            loop {
                match self.ensure_topics(&topics).await {
                    Ok(_) => break,
                    Err(error) => {
                        tracing::warn!(target: "navree::micro", %error, "preparing the Kafka topics failed; retrying");
                        if let Some(subscribed) = subscribed.take() {
                            subscribed.signal();
                        }
                        attempt = attempt.saturating_add(1);
                        tokio::select! {
                            () = tokio::time::sleep(reconnect_delay(attempt)) => {}
                            () = deliveries.closed() => return Ok(()),
                            () = self.closed.cancelled() => return Ok(()),
                        }
                    }
                }
            }
            let stop = CancellationToken::new();
            let mut tasks = tokio::task::JoinSet::new();
            if !broadcast.is_empty() {
                let topics: Vec<String> =
                    broadcast.iter().map(|(_, s)| s.pattern.clone()).collect();
                let by_topic: HashMap<String, usize> = broadcast
                    .iter()
                    .map(|(i, s)| (s.pattern.clone(), *i))
                    .collect();
                let reader = Reader::start(self.cluster.clone(), topics).await?;
                tasks.spawn(read_broadcast(
                    reader,
                    by_topic,
                    deliveries.clone(),
                    stop.clone(),
                ));
            }
            let group = if grouped.is_empty() {
                None
            } else {
                let group = Arc::new(
                    GroupConsumer::new(self, &grouped, retry_topic.clone(), deliveries.clone())
                        .await?,
                );
                tasks.spawn(group.clone().run_membership(stop.clone()));
                tasks.spawn(group.clone().run_fetching(stop.clone()));
                Some(group)
            };
            if let Some(subscribed) = subscribed {
                subscribed.signal();
            }
            tokio::select! {
                () = deliveries.closed() => {}
                () = self.closed.cancelled() => {}
            }
            stop.cancel();
            while tasks.join_next().await.is_some() {}
            if let Some(group) = group {
                group.leave().await;
            }
            Ok(())
        })
    }

    fn setup(
        &self,
        subscriptions: &[Subscription],
    ) -> BoxFuture<'_, Result<Vec<String>, MicroError>> {
        let mut topics: Vec<String> = subscriptions.iter().map(|s| s.pattern.clone()).collect();
        if subscriptions
            .iter()
            .any(|s| s.kind == PatternKind::Event && !s.is_broadcast())
        {
            topics.push(self.retry_topic());
            topics.push(self.dead_topic());
        }
        topics.push(self.reply_topic());
        topics.sort();
        topics.dedup();
        Box::pin(async move {
            Ok(self
                .ensure_topics(&topics)
                .await?
                .into_iter()
                .map(|(topic, created)| {
                    if created {
                        format!("created topic {topic}")
                    } else {
                        format!("topic {topic} exists")
                    }
                })
                .collect())
        })
    }

    fn supports_wildcards(&self) -> bool {
        false
    }

    fn is_connected(&self) -> bool {
        self.cluster.is_connected()
    }

    fn dead_letter(&self, letter: DeadLetter) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let mut envelope = letter.envelope();
            envelope.set_header("navree-error", single_line(&letter.error));
            envelope.set_header("navree-attempts", letter.attempts.to_string());
            self.produce_creating(&format!("{}.dead", letter.service), out_record(&envelope))
                .await
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.closed.cancel();
        })
    }
}

/// The record of an envelope: its key, headers and payload.
fn out_record(envelope: &Envelope) -> OutRecord {
    OutRecord {
        key: envelope
            .key()
            .map(|key| Bytes::copy_from_slice(key.as_bytes())),
        headers: envelope
            .headers()
            .iter()
            .map(|(name, value)| (name.clone(), Bytes::copy_from_slice(value.as_bytes())))
            .collect(),
        value: envelope.payload().clone(),
    }
}

/// The envelope of a record; the pattern is `navree-pattern`, else the
/// topic.
fn from_record(topic: &str, record: &InRecord) -> Envelope {
    let pattern = record
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(headers::PATTERN))
        .map_or_else(
            || topic.to_owned(),
            |(_, value)| String::from_utf8_lossy(value).into_owned(),
        );
    let mut envelope = Envelope::new(pattern, record.value.clone());
    for (name, value) in &record.headers {
        envelope.set_header(name, String::from_utf8_lossy(value).into_owned());
    }
    if envelope.key().is_none()
        && let Some(key) = &record.key
    {
        envelope.set_header(headers::KEY, String::from_utf8_lossy(key).into_owned());
    }
    envelope
}

/// Requests waiting for their replies, by correlation id.
#[derive(Default)]
struct Replies(Mutex<HashMap<String, oneshot::Sender<Envelope>>>);

/// Reads every partition of some topics from their end, without a group.
struct Reader {
    cluster: Arc<Cluster>,
    positions: Vec<(String, i32, i64)>,
    lane: u64,
}

impl Reader {
    async fn start(cluster: Arc<Cluster>, topics: Vec<String>) -> Result<Reader, MicroError> {
        let mut positions = Vec::new();
        for topic in topics {
            let Some(meta) = cluster.topic(&topic).await? else {
                continue;
            };
            let partitions: Vec<i32> = meta.partitions.iter().map(|p| p.partition).collect();
            let offsets = cluster.offsets(&topic, &partitions, LATEST).await?;
            for partition in partitions {
                positions.push((
                    topic.clone(),
                    partition,
                    offsets.get(&partition).copied().unwrap_or(0),
                ));
            }
        }
        let lane = cluster.lane();
        Ok(Reader {
            cluster,
            positions,
            lane,
        })
    }

    /// The next records, per topic and partition.
    async fn next(&mut self) -> Result<Vec<(String, InRecord)>, MicroError> {
        let mut by_leader: HashMap<i32, Vec<(String, i32, i64)>> = HashMap::new();
        for (topic, partition, offset) in &self.positions {
            let (leader, _) = self.cluster.leader(topic, *partition).await?;
            by_leader
                .entry(leader)
                .or_default()
                .push((topic.clone(), *partition, *offset));
        }
        let fetches = by_leader
            .into_iter()
            .map(|(leader, positions)| {
                let cluster = self.cluster.clone();
                let lane = self.lane;
                async move {
                    (
                        positions.clone(),
                        cluster.fetch(leader, &positions, FETCH_WAIT, lane).await,
                    )
                }
            })
            .collect::<Vec<_>>();
        let mut out = Vec::new();
        for (asked, fetched) in futures_util::future::join_all(fetches).await {
            let response = fetched?;
            for topic in response.responses {
                let name = topic.topic.0.to_string();
                for partition in topic.partitions {
                    let Some(position) = self
                        .positions
                        .iter_mut()
                        .find(|(t, p, _)| *t == name && *p == partition.partition_index)
                    else {
                        continue;
                    };
                    if partition.error_code == code::OFFSET_OUT_OF_RANGE {
                        position.2 = partition.high_watermark.max(0);
                        continue;
                    }
                    if partition.error_code != 0 {
                        self.cluster.forget(&name);
                        continue;
                    }
                    let from = asked
                        .iter()
                        .find(|(t, p, _)| *t == name && *p == partition.partition_index)
                        .map_or(position.2, |(_, _, offset)| *offset);
                    for record in records(partition.records, from)? {
                        position.2 = position.2.max(record.offset + 1);
                        out.push((name.clone(), record));
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Hands each reply to its request.
async fn read_replies(mut reader: Reader, replies: Arc<Replies>, closed: CancellationToken) {
    let mut attempt = 0u32;
    loop {
        let next = tokio::select! {
            next = reader.next() => next,
            () = closed.cancelled() => return,
        };
        match next {
            Ok(records) => {
                attempt = 0;
                for (topic, record) in records {
                    let envelope = from_record(&topic, &record);
                    if let Some(correlation) = envelope.header(headers::CORRELATION_ID)
                        && let Some(sender) = lock(&replies.0).remove(correlation)
                    {
                        let _ = sender.send(envelope);
                    }
                }
            }
            Err(error) => {
                tracing::warn!(target: "navree::micro", %error, "reading Kafka replies failed");
                attempt = attempt.saturating_add(1);
                tokio::select! {
                    () = tokio::time::sleep(reconnect_delay(attempt)) => {}
                    () = closed.cancelled() => return,
                }
            }
        }
    }
}

/// Hands broadcast events to this instance's listener, at most once.
async fn read_broadcast(
    mut reader: Reader,
    by_topic: HashMap<String, usize>,
    deliveries: mpsc::Sender<Delivery>,
    stop: CancellationToken,
) {
    let mut attempt = 0u32;
    loop {
        let next = tokio::select! {
            next = reader.next() => next,
            () = stop.cancelled() => return,
        };
        match next {
            Ok(records) => {
                attempt = 0;
                for (topic, record) in records {
                    let Some(&index) = by_topic.get(&topic) else {
                        continue;
                    };
                    let envelope = from_record(&topic, &record);
                    if deliveries
                        .send(LocalRetry::delivery(deliveries.clone(), envelope, index))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            Err(error) => {
                tracing::warn!(target: "navree::micro", %error, "reading Kafka broadcast events failed");
                attempt = attempt.saturating_add(1);
                tokio::select! {
                    () = tokio::time::sleep(reconnect_delay(attempt)) => {}
                    () = stop.cancelled() => return,
                }
            }
        }
    }
}

/// A partition this member owns.
#[derive(Debug)]
struct Part {
    /// The next offset to fetch.
    position: i64,
    /// Offsets handed out and not yet done.
    pending: BTreeSet<i64>,
    /// The last committed offset.
    committed: Option<i64>,
    /// When the coordinator took it away (it is released once its pending
    /// offsets are done).
    revoking: Option<Instant>,
    /// Tells this ownership from an earlier one of the same partition.
    generation: u64,
}

impl Part {
    /// What to commit: the first offset not yet done.
    fn commit_position(&self) -> i64 {
        self.pending.first().copied().unwrap_or(self.position)
    }
}

#[derive(Default)]
struct GroupState {
    epoch: i32,
    member_id: String,
    coordinator: Option<Arc<Conn>>,
    parts: HashMap<(String, i32), Part>,
    generation: u64,
}

/// This instance's membership of its service's consumer group, and the
/// fetching of the partitions the coordinator gives it.
struct GroupConsumer {
    cluster: Arc<Cluster>,
    group: String,
    topics: Vec<String>,
    retry_topic: Option<String>,
    /// Topic (the pattern) to subscription index and kind.
    by_topic: HashMap<String, (usize, PatternKind)>,
    start_from: i64,
    /// Where partitions start when the group has no committed offset (the
    /// end of the topics when the listener started).
    start: HashMap<(String, i32), i64>,
    deliveries: mpsc::Sender<Delivery>,
    state: Mutex<GroupState>,
    /// The connections this consumer fetches on.
    lane: u64,
}

impl GroupConsumer {
    async fn new(
        transport: &KafkaTransport,
        grouped: &[(usize, &Subscription)],
        retry_topic: Option<String>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> Result<GroupConsumer, MicroError> {
        let mut topics: Vec<String> = grouped.iter().map(|(_, s)| s.pattern.clone()).collect();
        topics.extend(retry_topic.clone());
        let start_from = if transport.options.start_from == "earliest" {
            EARLIEST
        } else {
            LATEST
        };
        let mut start = HashMap::new();
        for topic in &topics {
            let Some(meta) = transport.cluster.topic(topic).await? else {
                continue;
            };
            let partitions: Vec<i32> = meta.partitions.iter().map(|p| p.partition).collect();
            for (partition, offset) in transport
                .cluster
                .offsets(topic, &partitions, start_from)
                .await?
            {
                start.insert((topic.clone(), partition), offset);
            }
        }
        Ok(GroupConsumer {
            cluster: transport.cluster.clone(),
            group: transport.service.clone(),
            topics,
            retry_topic,
            by_topic: grouped
                .iter()
                .map(|(i, s)| (s.pattern.clone(), (*i, s.kind)))
                .collect(),
            start_from,
            start,
            deliveries,
            state: Mutex::new(GroupState {
                member_id: Uuid::new_v4().simple().to_string(),
                ..GroupState::default()
            }),
            lane: transport.cluster.lane(),
        })
    }

    /// The group coordinator.
    async fn coordinator(&self) -> Result<Arc<Conn>, MicroError> {
        if let Some(conn) = lock(&self.state).coordinator.clone()
            && conn.is_alive()
        {
            return Ok(conn);
        }
        let request = FindCoordinatorRequest::default()
            .with_key_type(0)
            .with_coordinator_keys(vec![StrBytes::from_string(self.group.clone())]);
        let response = self
            .cluster
            .any()
            .await?
            .send(&request, version::FIND_COORDINATOR, Duration::ZERO)
            .await?;
        let coordinator = response
            .coordinators
            .into_iter()
            .next()
            .ok_or_else(|| MicroError::transport("Kafka named no group coordinator"))?;
        if coordinator.error_code != 0 {
            return Err(MicroError::transport(format!(
                "finding the coordinator of group {} failed: {}",
                self.group,
                describe(coordinator.error_code)
            )));
        }
        let conn = self
            .cluster
            .conn(&client::address(
                coordinator.host.as_str(),
                coordinator.port,
            ))
            .await?;
        lock(&self.state).coordinator = Some(conn.clone());
        Ok(conn)
    }

    /// Heartbeats, follows the assignment, releases revoked partitions and
    /// commits offsets, until `stop`.
    async fn run_membership(self: Arc<Self>, stop: CancellationToken) {
        let mut next_heartbeat = Instant::now();
        let mut last_commit = Instant::now();
        let mut failures = 0u32;
        loop {
            if Instant::now() >= next_heartbeat {
                let epoch = lock(&self.state).epoch;
                match self.heartbeat(epoch).await {
                    Ok(Some(every)) => {
                        failures = 0;
                        next_heartbeat = Instant::now() + every;
                    }
                    Ok(None) => next_heartbeat = Instant::now(),
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        tracing::warn!(target: "navree::micro", %error, group = %self.group, "the Kafka heartbeat failed");
                        lock(&self.state).coordinator = None;
                        next_heartbeat = Instant::now() + reconnect_delay(failures);
                    }
                }
            }
            if self.release_revoked().await {
                // Tell the coordinator at once.
                next_heartbeat = Instant::now();
            }
            if last_commit.elapsed() >= COMMIT_EVERY {
                self.commit().await;
                last_commit = Instant::now();
            }
            let pause = next_heartbeat
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100));
            tokio::select! {
                () = tokio::time::sleep(pause) => {}
                () = stop.cancelled() => return,
            }
        }
    }

    /// One heartbeat at `epoch`; returns when to beat again, or `None` to
    /// beat again at once.
    async fn heartbeat(&self, epoch: i32) -> Result<Option<Duration>, MicroError> {
        let coordinator = self.coordinator().await?;
        let (member_id, owned) = {
            let state = lock(&self.state);
            let mut by_topic: HashMap<String, Vec<i32>> = HashMap::new();
            for (topic, partition) in state.parts.keys() {
                by_topic.entry(topic.clone()).or_default().push(*partition);
            }
            (state.member_id.clone(), by_topic)
        };
        let mut owned_partitions = Vec::new();
        for (topic, mut partitions) in owned {
            let Some(meta) = self.cluster.topic(&topic).await? else {
                continue;
            };
            partitions.sort_unstable();
            owned_partitions.push(
                TopicPartitions::default()
                    .with_topic_id(meta.id)
                    .with_partitions(partitions),
            );
        }
        let request = ConsumerGroupHeartbeatRequest::default()
            .with_group_id(GroupId(StrBytes::from_string(self.group.clone())))
            .with_member_id(StrBytes::from_string(member_id))
            .with_member_epoch(epoch)
            .with_rebalance_timeout_ms(REBALANCE_TIMEOUT_MS)
            .with_subscribed_topic_names(Some(self.topics.iter().map(|t| topic_name(t)).collect()))
            .with_topic_partitions(Some(owned_partitions));
        let response = coordinator
            .send(&request, version::HEARTBEAT, Duration::ZERO)
            .await?;
        match response.error_code {
            0 => {}
            code::UNKNOWN_MEMBER_ID | code::FENCED_MEMBER_EPOCH => {
                // Fenced: give everything up without committing, and join
                // again.
                let mut state = lock(&self.state);
                state.parts.clear();
                state.epoch = 0;
                return Ok(None);
            }
            code::NOT_COORDINATOR
            | code::COORDINATOR_NOT_AVAILABLE
            | code::COORDINATOR_LOAD_IN_PROGRESS => {
                lock(&self.state).coordinator = None;
                return Err(MicroError::transport(describe(response.error_code)));
            }
            code::STALE_MEMBER_EPOCH => return Ok(Some(Duration::from_millis(200))),
            other => {
                return Err(MicroError::transport(format!(
                    "the group coordinator refused the heartbeat: {}{}",
                    describe(other),
                    response
                        .error_message
                        .map(|m| format!(": {m}"))
                        .unwrap_or_default()
                )));
            }
        }
        {
            let mut state = lock(&self.state);
            state.epoch = response.member_epoch;
            if let Some(member_id) = response.member_id
                && !member_id.is_empty()
            {
                state.member_id = member_id.to_string();
            }
        }
        if let Some(assignment) = response.assignment {
            let mut target = BTreeSet::new();
            for assigned in assignment.topic_partitions {
                let name = match self.cluster.topic_name_of(assigned.topic_id) {
                    Some(name) => Some(name),
                    None => {
                        self.cluster.refresh(&self.topics).await?;
                        self.cluster.topic_name_of(assigned.topic_id)
                    }
                };
                if let Some(name) = name {
                    for partition in assigned.partitions {
                        target.insert((name.clone(), partition));
                    }
                }
            }
            self.assign(target).await?;
        }
        let every = u64::try_from(response.heartbeat_interval_ms.max(100)).unwrap_or(3_000);
        Ok(Some(Duration::from_millis(every)))
    }

    /// Follows a new assignment: revokes what left it, starts what joined.
    async fn assign(&self, target: BTreeSet<(String, i32)>) -> Result<(), MicroError> {
        let added: Vec<(String, i32)> = {
            let mut state = lock(&self.state);
            for (key, part) in &mut state.parts {
                if target.contains(key) {
                    part.revoking = None;
                } else if part.revoking.is_none() {
                    part.revoking = Some(Instant::now());
                }
            }
            target
                .iter()
                .filter(|key| !state.parts.contains_key(*key))
                .cloned()
                .collect()
        };
        if added.is_empty() {
            return Ok(());
        }
        let committed = self.committed(&added).await?;
        let mut starts = HashMap::new();
        for (topic, partition) in &added {
            let start = match committed.get(&(topic.clone(), *partition)) {
                Some(offset) if *offset >= 0 => *offset,
                _ => match self.start.get(&(topic.clone(), *partition)) {
                    Some(offset) => *offset,
                    None => self
                        .cluster
                        .offsets(topic, &[*partition], self.start_from)
                        .await?
                        .get(partition)
                        .copied()
                        .unwrap_or(0),
                },
            };
            starts.insert(
                (topic.clone(), *partition),
                (start, committed.get(&(topic.clone(), *partition)).copied()),
            );
        }
        let mut state = lock(&self.state);
        for (key, (position, committed)) in starts {
            if target.contains(&key) && !state.parts.contains_key(&key) {
                state.generation += 1;
                let generation = state.generation;
                state.parts.insert(
                    key,
                    Part {
                        position,
                        pending: BTreeSet::new(),
                        committed: committed.filter(|offset| *offset >= 0),
                        revoking: None,
                        generation,
                    },
                );
            }
        }
        Ok(())
    }

    /// The group's committed offsets of `partitions`.
    async fn committed(
        &self,
        partitions: &[(String, i32)],
    ) -> Result<HashMap<(String, i32), i64>, MicroError> {
        let coordinator = self.coordinator().await?;
        let (member_id, epoch) = {
            let state = lock(&self.state);
            (state.member_id.clone(), state.epoch)
        };
        let mut by_topic: HashMap<String, Vec<i32>> = HashMap::new();
        for (topic, partition) in partitions {
            by_topic.entry(topic.clone()).or_default().push(*partition);
        }
        let request = OffsetFetchRequest::default().with_groups(vec![
            OffsetFetchRequestGroup::default()
                .with_group_id(GroupId(StrBytes::from_string(self.group.clone())))
                .with_member_id(Some(StrBytes::from_string(member_id)))
                .with_member_epoch(epoch)
                .with_topics(Some(
                    by_topic
                        .into_iter()
                        .map(|(topic, partitions)| {
                            OffsetFetchRequestTopics::default()
                                .with_name(topic_name(&topic))
                                .with_partition_indexes(partitions)
                        })
                        .collect(),
                )),
        ]);
        let response = coordinator
            .send(&request, version::OFFSET_FETCH, Duration::ZERO)
            .await?;
        let mut offsets = HashMap::new();
        for group in response.groups {
            for topic in group.topics {
                for partition in topic.partitions {
                    if partition.error_code == 0 {
                        offsets.insert(
                            (topic.name.0.to_string(), partition.partition_index),
                            partition.committed_offset,
                        );
                    }
                }
            }
        }
        Ok(offsets)
    }

    /// Commits the handled offsets that moved.
    async fn commit(&self) {
        let (member_id, epoch, due) = {
            let state = lock(&self.state);
            let due: Vec<(String, i32, i64)> = state
                .parts
                .iter()
                .filter(|(_, part)| part.committed != Some(part.commit_position()))
                .map(|((topic, partition), part)| {
                    (topic.clone(), *partition, part.commit_position())
                })
                .collect();
            (state.member_id.clone(), state.epoch, due)
        };
        if due.is_empty() || epoch <= 0 {
            return;
        }
        let Ok(coordinator) = self.coordinator().await else {
            return;
        };
        let mut by_topic: HashMap<String, Vec<(i32, i64)>> = HashMap::new();
        for (topic, partition, offset) in &due {
            by_topic
                .entry(topic.clone())
                .or_default()
                .push((*partition, *offset));
        }
        let request = OffsetCommitRequest::default()
            .with_group_id(GroupId(StrBytes::from_string(self.group.clone())))
            .with_generation_id_or_member_epoch(epoch)
            .with_member_id(StrBytes::from_string(member_id))
            .with_topics(
                by_topic
                    .into_iter()
                    .map(|(topic, partitions)| {
                        OffsetCommitRequestTopic::default()
                            .with_name(topic_name(&topic))
                            .with_partitions(
                                partitions
                                    .into_iter()
                                    .map(|(partition, offset)| {
                                        OffsetCommitRequestPartition::default()
                                            .with_partition_index(partition)
                                            .with_committed_offset(offset)
                                            .with_committed_leader_epoch(-1)
                                    })
                                    .collect(),
                            )
                    })
                    .collect(),
            );
        match coordinator
            .send(&request, version::OFFSET_COMMIT, Duration::ZERO)
            .await
        {
            Ok(response) => {
                let mut state = lock(&self.state);
                for topic in response.topics {
                    for partition in topic.partitions {
                        if partition.error_code != 0 {
                            tracing::debug!(target: "navree::micro", error = %describe(partition.error_code), "a Kafka offset commit was refused");
                            continue;
                        }
                        let key = (topic.name.0.to_string(), partition.partition_index);
                        let committed = due
                            .iter()
                            .find(|(t, p, _)| *t == key.0 && *p == key.1)
                            .map(|(_, _, o)| *o);
                        if let Some(part) = state.parts.get_mut(&key) {
                            part.committed = committed;
                        }
                    }
                }
            }
            Err(error) => {
                tracing::debug!(target: "navree::micro", %error, "committing Kafka offsets failed")
            }
        }
    }

    /// Releases the revoked partitions whose events are done (or that
    /// waited long enough), committing first; returns whether any was
    /// released.
    async fn release_revoked(&self) -> bool {
        let ready: Vec<(String, i32)> = {
            let state = lock(&self.state);
            state
                .parts
                .iter()
                .filter(|(_, part)| {
                    part.revoking.is_some_and(|since| {
                        part.pending.is_empty() || since.elapsed() >= REVOKE_TIMEOUT
                    })
                })
                .map(|(key, _)| key.clone())
                .collect()
        };
        if ready.is_empty() {
            return false;
        }
        self.commit().await;
        let mut state = lock(&self.state);
        for key in ready {
            state.parts.remove(&key);
        }
        true
    }

    /// Commits, then leaves the group.
    async fn leave(&self) {
        self.commit().await;
        let _ = self.heartbeat(-1).await;
    }

    /// Fetches the owned partitions and hands their records to the
    /// listener, until `stop`.
    async fn run_fetching(self: Arc<Self>, stop: CancellationToken) {
        let mut failures = 0u32;
        loop {
            if stop.is_cancelled() {
                return;
            }
            let positions: Vec<(String, i32, i64, u64)> = lock(&self.state)
                .parts
                .iter()
                .filter(|(_, part)| part.revoking.is_none())
                .map(|((topic, partition), part)| {
                    (topic.clone(), *partition, part.position, part.generation)
                })
                .collect();
            if positions.is_empty() {
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(50)) => continue,
                    () = stop.cancelled() => return,
                }
            }
            let fetched = tokio::select! {
                fetched = self.fetch_round(&positions) => fetched,
                () = stop.cancelled() => return,
            };
            match fetched {
                Ok(records) => {
                    failures = 0;
                    for (topic, partition, generation, record) in records {
                        if !self.take(&topic, partition, generation, record.offset) {
                            continue;
                        }
                        let Some(delivery) = self.delivery(&topic, partition, generation, &record)
                        else {
                            continue;
                        };
                        match delivery {
                            Handout::Now(delivery) => {
                                let sent = tokio::select! {
                                    sent = self.deliveries.send(delivery) => sent.is_ok(),
                                    () = stop.cancelled() => return,
                                };
                                if !sent {
                                    return;
                                }
                            }
                            Handout::At(due, delivery) => {
                                let deliveries = self.deliveries.clone();
                                tokio::spawn(async move {
                                    tokio::time::sleep(
                                        due.saturating_duration_since(Instant::now()),
                                    )
                                    .await;
                                    let _ = deliveries.send(delivery).await;
                                });
                            }
                        }
                    }
                }
                Err(error) => {
                    failures = failures.saturating_add(1);
                    tracing::warn!(target: "navree::micro", %error, "fetching from Kafka failed");
                    tokio::select! {
                        () = tokio::time::sleep(reconnect_delay(failures)) => {}
                        () = stop.cancelled() => return,
                    }
                }
            }
        }
    }

    /// One fetch from every leader at once.
    async fn fetch_round(
        &self,
        positions: &[(String, i32, i64, u64)],
    ) -> Result<Vec<(String, i32, u64, InRecord)>, MicroError> {
        let mut by_leader: HashMap<i32, Vec<(String, i32, i64)>> = HashMap::new();
        for (topic, partition, offset, _) in positions {
            let (leader, _) = self.cluster.leader(topic, *partition).await?;
            by_leader
                .entry(leader)
                .or_default()
                .push((topic.clone(), *partition, *offset));
        }
        let fetches = by_leader
            .into_iter()
            .map(|(leader, asked)| {
                let cluster = self.cluster.clone();
                let lane = self.lane;
                async move { cluster.fetch(leader, &asked, FETCH_WAIT, lane).await }
            })
            .collect::<Vec<_>>();
        let mut out = Vec::new();
        for fetched in futures_util::future::join_all(fetches).await {
            let response = fetched?;
            for topic in response.responses {
                let name = topic.topic.0.to_string();
                for partition in topic.partitions {
                    let index = partition.partition_index;
                    let Some((_, _, from, generation)) = positions
                        .iter()
                        .find(|(t, p, _, _)| *t == name && *p == index)
                    else {
                        continue;
                    };
                    match partition.error_code {
                        0 => {}
                        code::OFFSET_OUT_OF_RANGE => {
                            let reset = self
                                .cluster
                                .offsets(&name, &[index], self.start_from)
                                .await?
                                .get(&index)
                                .copied()
                                .unwrap_or(0);
                            if let Some(part) =
                                lock(&self.state).parts.get_mut(&(name.clone(), index))
                                && part.generation == *generation
                            {
                                part.position = reset;
                            }
                            continue;
                        }
                        _ => {
                            self.cluster.forget(&name);
                            continue;
                        }
                    }
                    for record in records(partition.records, *from)? {
                        out.push((name.clone(), index, *generation, record));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Records `offset` as handed out, when the partition is still ours
    /// and the offset is the next one.
    fn take(&self, topic: &str, partition: i32, generation: u64, offset: i64) -> bool {
        let mut state = lock(&self.state);
        match state.parts.get_mut(&(topic.to_owned(), partition)) {
            Some(part)
                if part.generation == generation
                    && part.revoking.is_none()
                    && offset >= part.position =>
            {
                part.pending.insert(offset);
                part.position = offset + 1;
                true
            }
            _ => false,
        }
    }

    /// Marks `offset` done.
    fn done(&self, topic: &str, partition: i32, generation: u64, offset: i64) {
        let mut state = lock(&self.state);
        if let Some(part) = state.parts.get_mut(&(topic.to_owned(), partition))
            && part.generation == generation
        {
            part.pending.remove(&offset);
        }
    }

    /// The delivery of a record, now or (a retried event) when it is due.
    fn delivery(
        self: &Arc<Self>,
        topic: &str,
        partition: i32,
        generation: u64,
        record: &InRecord,
    ) -> Option<Handout> {
        let envelope = from_record(topic, record);
        let handle = Handle {
            group: self.clone(),
            topic: topic.to_owned(),
            partition,
            generation,
            offset: record.offset,
        };
        if Some(topic) == self.retry_topic.as_deref() {
            let pattern = envelope.pattern().to_owned();
            let Some(&(index, PatternKind::Event)) = self.by_topic.get(&pattern) else {
                // Not ours any more.
                handle.done();
                return None;
            };
            let attempt = envelope.attempt();
            let due = envelope
                .header(DUE)
                .and_then(|due| due.parse::<i64>().ok())
                .map(|due| due - crate::envelope::now_millis())
                .filter(|left| *left > 0)
                .map_or_else(Instant::now, |left| {
                    Instant::now() + Duration::from_millis(u64::try_from(left).unwrap_or(0))
                });
            let actions = EventActions {
                handle,
                envelope: envelope.clone(),
                attempt,
                retry_topic: self.retry_topic.clone(),
            };
            return Some(Handout::At(
                due,
                Delivery::new(envelope, index, attempt, actions),
            ));
        }
        let Some(&(index, kind)) = self.by_topic.get(topic) else {
            handle.done();
            return None;
        };
        Some(Handout::Now(match kind {
            PatternKind::Event => {
                let attempt = envelope.attempt();
                let actions = EventActions {
                    handle,
                    envelope: envelope.clone(),
                    attempt,
                    retry_topic: self.retry_topic.clone(),
                };
                Delivery::new(envelope, index, attempt, actions)
            }
            PatternKind::Message => {
                let actions = RequestActions {
                    handle,
                    reply_to: envelope.header(headers::REPLY_TO).map(str::to_owned),
                };
                Delivery::new(envelope, index, 1, actions)
            }
        }))
    }
}

/// A delivery to hand out now, or at a time (a retried event).
enum Handout {
    Now(Delivery),
    At(Instant, Delivery),
}

/// The record a delivery came from.
struct Handle {
    group: Arc<GroupConsumer>,
    topic: String,
    partition: i32,
    generation: u64,
    offset: i64,
}

impl Handle {
    fn done(&self) {
        self.group
            .done(&self.topic, self.partition, self.generation, self.offset);
    }
}

/// An event: done once handled; a retry is a copy on the retry topic.
struct EventActions {
    handle: Handle,
    envelope: Envelope,
    attempt: u32,
    retry_topic: Option<String>,
}

impl DeliveryActions for EventActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        self.handle.done();
        Box::pin(async { Ok(()) })
    }

    fn retry_in(self: Box<Self>, delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            let Some(retry_topic) = &self.retry_topic else {
                self.handle.done();
                return Ok(());
            };
            let mut envelope = self.envelope.clone();
            envelope.set_header(headers::ATTEMPT, (self.attempt + 1).to_string());
            let due = crate::envelope::now_millis()
                .saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX));
            envelope.set_header(DUE, due.to_string());
            match self
                .handle
                .group
                .cluster
                .produce(retry_topic, out_record(&envelope))
                .await
            {
                Ok(_) => {
                    self.handle.done();
                    Ok(())
                }
                // Left pending: it comes back after a restart or a
                // rebalance rather than being lost.
                Err(ProduceError::NoTopic) => Err(MicroError::transport(format!(
                    "the retry topic {retry_topic} does not exist"
                ))),
                Err(ProduceError::Failed(error)) => Err(error),
            }
        })
    }

    fn reply(self: Box<Self>, _reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        self.ack()
    }
}

/// A request: the reply goes to the asking service's reply topic.
struct RequestActions {
    handle: Handle,
    reply_to: Option<String>,
}

impl DeliveryActions for RequestActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        self.handle.done();
        Box::pin(async { Ok(()) })
    }

    fn retry_in(self: Box<Self>, _delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        self.ack()
    }

    fn reply(self: Box<Self>, reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            let outcome = match &self.reply_to {
                Some(topic) => match self
                    .handle
                    .group
                    .cluster
                    .produce(topic, out_record(&reply))
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(ProduceError::NoTopic) => Err(MicroError::transport(format!(
                        "the reply topic {topic} does not exist"
                    ))),
                    Err(ProduceError::Failed(error)) => Err(error),
                },
                None => Ok(()),
            };
            self.handle.done();
            outcome
        })
    }
}

/// Header values are one line.
fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_envelopes_round_trip() {
        let envelope = Envelope::new("deal.won", "{}")
            .with_header(headers::MESSAGE_ID, "m1")
            .with_header(headers::KEY, "deal-1");
        let record = out_record(&envelope);
        assert_eq!(record.key.as_deref(), Some(&b"deal-1"[..]));
        let back = from_record(
            "deal.won",
            &InRecord {
                offset: 3,
                key: record.key.clone(),
                headers: record.headers.clone(),
                value: record.value.clone(),
            },
        );
        assert_eq!(back.pattern(), "deal.won");
        assert_eq!(back.message_id(), Some("m1"));
        assert_eq!(back.key(), Some("deal-1"));
        let bare = from_record(
            "deal.lost",
            &InRecord {
                offset: 0,
                key: Some(Bytes::from_static(b"k")),
                headers: Vec::new(),
                value: Bytes::new(),
            },
        );
        assert_eq!(bare.pattern(), "deal.lost");
        assert_eq!(bare.key(), Some("k"), "the record key is the ordering key");
    }

    #[test]
    fn commits_wait_for_the_first_offset_not_done() {
        let mut part = Part {
            position: 10,
            pending: BTreeSet::new(),
            committed: None,
            revoking: None,
            generation: 1,
        };
        assert_eq!(part.commit_position(), 10);
        part.pending.extend([7, 9]);
        assert_eq!(part.commit_position(), 7);
        part.pending.remove(&7);
        assert_eq!(part.commit_position(), 9);
    }

    #[test]
    fn settings_are_checked() {
        let options = KafkaOptions::default();
        let transport = KafkaTransport::new(
            " kafka-1:9092, kafka://kafka-2:9092 ",
            &options,
            "deals",
            "deals-1",
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(transport.brokers, ["kafka-1:9092", "kafka-2:9092"]);
        assert_eq!(transport.retry_topic(), "deals.retry");
        assert_eq!(transport.dead_topic(), "deals.dead");
        assert_eq!(transport.reply_topic(), "deals.replies");
        assert!(!transport.supports_wildcards());
        assert!(KafkaTransport::new(" , ", &options, "s", "s-1", Duration::from_secs(1)).is_err());
        let bad = KafkaOptions {
            start_from: "middle".into(),
            ..KafkaOptions::default()
        };
        let error =
            KafkaTransport::new("k:9092", &bad, "s", "s-1", Duration::from_secs(1)).unwrap_err();
        assert!(
            error.to_string().contains("micro.kafka.start_from"),
            "{error}"
        );
        let sasl = KafkaOptions {
            sasl: Some(crate::SaslOptions {
                mechanism: "GSSAPI".into(),
                username: "u".into(),
                password: "p".to_owned().into(),
            }),
            ..KafkaOptions::default()
        };
        let error =
            KafkaTransport::new("k:9092", &sasl, "s", "s-1", Duration::from_secs(1)).unwrap_err();
        assert!(
            error.to_string().contains("micro.kafka.sasl.mechanism"),
            "{error}"
        );
    }
}
