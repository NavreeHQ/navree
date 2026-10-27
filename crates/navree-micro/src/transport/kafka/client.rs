//! A small Kafka client on kafka-protocol: connections (TLS, SASL),
//! metadata, keyed produce, list offsets, fetch, topic creation and the
//! group coordinator's APIs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use kafka_protocol::error::ResponseError;
use kafka_protocol::indexmap::IndexMap;
use kafka_protocol::messages::create_topics_request::{CreatableTopic, CreatableTopicConfig};
use kafka_protocol::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_protocol::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_protocol::messages::metadata_request::MetadataRequestTopic;
use kafka_protocol::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_protocol::messages::{
    ApiKey, BrokerId, CreateTopicsRequest, FetchRequest, FetchResponse, ListOffsetsRequest,
    MetadataRequest, ProduceRequest, RequestHeader, ResponseHeader, SaslAuthenticateRequest,
    SaslHandshakeRequest, TopicName,
};
use kafka_protocol::protocol::{Decodable, Encodable, Request, StrBytes};
use kafka_protocol::records::{
    Compression, Record, RecordBatchDecoder, RecordBatchEncoder, RecordEncodeOptions, TimestampType,
};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_rustls::TlsConnector;
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use uuid::Uuid;

use super::sasl;
use crate::MicroError;
use crate::options::SaslOptions;

/// Versions this client speaks: all within what Kafka 4.0 supports.
pub(crate) mod version {
    pub(crate) const METADATA: i16 = 12;
    pub(crate) const PRODUCE: i16 = 9;
    pub(crate) const FETCH: i16 = 12;
    pub(crate) const LIST_OFFSETS: i16 = 7;
    pub(crate) const FIND_COORDINATOR: i16 = 4;
    pub(crate) const HEARTBEAT: i16 = 0;
    pub(crate) const OFFSET_COMMIT: i16 = 9;
    pub(crate) const OFFSET_FETCH: i16 = 9;
    pub(crate) const CREATE_TOPICS: i16 = 7;
    pub(crate) const SASL_HANDSHAKE: i16 = 1;
    pub(crate) const SASL_AUTHENTICATE: i16 = 2;
}

/// Kafka error codes this client acts on.
pub(crate) mod code {
    pub(crate) const OFFSET_OUT_OF_RANGE: i16 = 1;
    pub(crate) const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
    pub(crate) const LEADER_NOT_AVAILABLE: i16 = 5;
    pub(crate) const NOT_LEADER_OR_FOLLOWER: i16 = 6;
    pub(crate) const COORDINATOR_LOAD_IN_PROGRESS: i16 = 14;
    pub(crate) const COORDINATOR_NOT_AVAILABLE: i16 = 15;
    pub(crate) const NOT_COORDINATOR: i16 = 16;
    pub(crate) const UNKNOWN_MEMBER_ID: i16 = 25;
    pub(crate) const TOPIC_ALREADY_EXISTS: i16 = 36;
    pub(crate) const FENCED_MEMBER_EPOCH: i16 = 110;
    pub(crate) const STALE_MEMBER_EPOCH: i16 = 113;
}

/// `ListOffsets` timestamps.
pub(crate) const LATEST: i64 = -1;
pub(crate) const EARLIEST: i64 = -2;

/// The largest frame read.
const MAX_FRAME: usize = 128 * 1024 * 1024;

/// How long one request may wait for its response (beyond a fetch's own
/// wait).
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// The text of a Kafka error code.
pub(crate) fn describe(code: i16) -> String {
    ResponseError::try_from_code(code).map_or_else(
        || "no error".to_owned(),
        |error| format!("{error} ({code})"),
    )
}

trait Io: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

type Waiters = Arc<Mutex<HashMap<i32, oneshot::Sender<BytesMut>>>>;

/// One connection to a broker; requests are pipelined and matched to their
/// responses by correlation id.
pub(crate) struct Conn {
    writer:
        tokio::sync::Mutex<FramedWrite<tokio::io::WriteHalf<Box<dyn Io>>, LengthDelimitedCodec>>,
    waiters: Waiters,
    next: AtomicI32,
    client_id: StrBytes,
    alive: Arc<AtomicBool>,
}

impl Conn {
    /// Opens a connection to `addr`, with TLS and SASL when configured.
    pub(crate) async fn open(addr: &str, settings: &ConnSettings) -> Result<Arc<Conn>, MicroError> {
        let opening = async {
            let tcp = TcpStream::connect(addr).await.map_err(|error| {
                MicroError::transport(format!("cannot connect to Kafka at {addr}: {error}"))
            })?;
            let _ = tcp.set_nodelay(true);
            let io: Box<dyn Io> = match &settings.tls {
                Some(connector) => {
                    let host = addr
                        .rsplit_once(':')
                        .map_or(addr, |(host, _)| host)
                        .trim_matches(['[', ']']);
                    let name = ServerName::try_from(host.to_owned()).map_err(|error| {
                        MicroError::transport(format!("bad TLS server name {host}: {error}"))
                    })?;
                    Box::new(connector.connect(name, tcp).await.map_err(|error| {
                        MicroError::transport(format!(
                            "the TLS handshake with {addr} failed: {error}"
                        ))
                    })?)
                }
                None => Box::new(tcp),
            };
            let (read, write) = tokio::io::split(io);
            let codec = || {
                LengthDelimitedCodec::builder()
                    .max_frame_length(MAX_FRAME)
                    .new_codec()
            };
            let reader = FramedRead::new(read, codec());
            let writer = FramedWrite::new(write, codec());
            let waiters: Waiters = Arc::default();
            let alive = Arc::new(AtomicBool::new(true));
            tokio::spawn(read_responses(reader, waiters.clone(), alive.clone()));
            let conn = Arc::new(Conn {
                writer: tokio::sync::Mutex::new(writer),
                waiters,
                next: AtomicI32::new(1),
                client_id: StrBytes::from_string(settings.client_id.clone()),
                alive,
            });
            if let Some(credentials) = &settings.sasl {
                sasl::authenticate(&conn, credentials).await?;
            }
            Ok(conn)
        };
        match tokio::time::timeout(settings.connect_timeout, opening).await {
            Ok(outcome) => outcome,
            Err(_) => Err(MicroError::transport(format!(
                "connecting to Kafka at {addr} took longer than {:?}",
                settings.connect_timeout
            ))),
        }
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Sends `request` (at `version`) and decodes its response, waiting up
    /// to `extra` beyond the usual response time.
    pub(crate) async fn send<R: Request>(
        &self,
        request: &R,
        version: i16,
        extra: Duration,
    ) -> Result<R::Response, MicroError> {
        let api_key =
            ApiKey::try_from(R::KEY).map_err(|_| MicroError::transport("an unknown Kafka API"))?;
        let correlation = self.next.fetch_add(1, Ordering::SeqCst);
        let header = RequestHeader::default()
            .with_request_api_key(R::KEY)
            .with_request_api_version(version)
            .with_correlation_id(correlation)
            .with_client_id(Some(self.client_id.clone()));
        let mut buf = BytesMut::new();
        header
            .encode(&mut buf, api_key.request_header_version(version))
            .and_then(|()| request.encode(&mut buf, version))
            .map_err(|error| {
                MicroError::transport(format!(
                    "encoding a Kafka {api_key:?} request failed: {error}"
                ))
            })?;
        let (sender, response) = oneshot::channel();
        lock(&self.waiters).insert(correlation, sender);
        {
            let mut writer = self.writer.lock().await;
            if let Err(error) = writer.send(buf.freeze()).await {
                lock(&self.waiters).remove(&correlation);
                self.alive.store(false, Ordering::SeqCst);
                return Err(MicroError::transport(format!(
                    "the Kafka connection broke: {error}"
                )));
            }
        }
        let mut frame = match tokio::time::timeout(RESPONSE_TIMEOUT + extra, response).await {
            Ok(Ok(frame)) => frame,
            Ok(Err(_)) => return Err(MicroError::transport("the Kafka connection broke")),
            Err(_) => {
                lock(&self.waiters).remove(&correlation);
                return Err(MicroError::transport(format!(
                    "Kafka did not answer a {api_key:?} request in time"
                )));
            }
        };
        ResponseHeader::decode(&mut frame, api_key.response_header_version(version)).map_err(
            |error| MicroError::transport(format!("a malformed Kafka response header: {error}")),
        )?;
        R::Response::decode(&mut frame, version).map_err(|error| {
            MicroError::transport(format!("a malformed Kafka {api_key:?} response: {error}"))
        })
    }
}

/// Hands each response frame to its request.
async fn read_responses(
    mut reader: FramedRead<tokio::io::ReadHalf<Box<dyn Io>>, LengthDelimitedCodec>,
    waiters: Waiters,
    alive: Arc<AtomicBool>,
) {
    while let Some(Ok(frame)) = reader.next().await {
        if frame.len() < 4 {
            break;
        }
        let correlation = i32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        if let Some(waiter) = lock(&waiters).remove(&correlation) {
            let _ = waiter.send(frame);
        }
    }
    alive.store(false, Ordering::SeqCst);
    // Fail the requests still waiting.
    lock(&waiters).clear();
}

/// What a connection needs.
#[derive(Clone)]
pub(crate) struct ConnSettings {
    pub(crate) client_id: String,
    pub(crate) tls: Option<TlsConnector>,
    pub(crate) sasl: Option<SaslOptions>,
    pub(crate) connect_timeout: Duration,
}

/// A partition's leader.
#[derive(Clone, Debug)]
pub(crate) struct PartitionMeta {
    pub(crate) partition: i32,
    pub(crate) leader: i32,
}

/// What metadata says about a topic.
#[derive(Clone, Debug)]
pub(crate) struct TopicMeta {
    pub(crate) id: Uuid,
    pub(crate) partitions: Vec<PartitionMeta>,
}

#[derive(Default)]
struct Meta {
    brokers: HashMap<i32, String>,
    topics: HashMap<String, TopicMeta>,
}

/// The brokers, their connections and the cluster's metadata.
///
/// A broker handles the requests of one connection one at a time, so a
/// fetch that waits for records holds up everything behind it: each
/// reader fetches on connections of its own ([`lane`](Cluster::lane)), and
/// produce, metadata and group requests share the others.
pub(crate) struct Cluster {
    bootstrap: Vec<String>,
    settings: ConnSettings,
    conns: tokio::sync::Mutex<HashMap<String, Arc<Conn>>>,
    fetch_conns: tokio::sync::Mutex<HashMap<(String, u64), Arc<Conn>>>,
    meta: Mutex<Meta>,
    round_robin: AtomicUsize,
    lanes: AtomicU64,
}

impl Cluster {
    pub(crate) fn new(bootstrap: Vec<String>, settings: ConnSettings) -> Self {
        Self {
            bootstrap,
            settings,
            conns: tokio::sync::Mutex::new(HashMap::new()),
            fetch_conns: tokio::sync::Mutex::new(HashMap::new()),
            meta: Mutex::new(Meta::default()),
            round_robin: AtomicUsize::new(0),
            lanes: AtomicU64::new(1),
        }
    }

    /// A new fetch lane: the connections one reader fetches on.
    pub(crate) fn lane(&self) -> u64 {
        self.lanes.fetch_add(1, Ordering::Relaxed)
    }

    /// A live connection to `addr` for the fetches of `lane`.
    async fn fetch_conn(&self, addr: &str, lane: u64) -> Result<Arc<Conn>, MicroError> {
        let mut conns = self.fetch_conns.lock().await;
        let key = (addr.to_owned(), lane);
        if let Some(conn) = conns.get(&key)
            && conn.is_alive()
        {
            return Ok(conn.clone());
        }
        let conn = Conn::open(addr, &self.settings).await?;
        conns.insert(key, conn.clone());
        Ok(conn)
    }

    /// A live connection to `addr`.
    pub(crate) async fn conn(&self, addr: &str) -> Result<Arc<Conn>, MicroError> {
        let mut conns = self.conns.lock().await;
        if let Some(conn) = conns.get(addr)
            && conn.is_alive()
        {
            return Ok(conn.clone());
        }
        let conn = Conn::open(addr, &self.settings).await?;
        conns.insert(addr.to_owned(), conn.clone());
        Ok(conn)
    }

    /// A connection to any broker: the known ones, then the bootstrap list.
    pub(crate) async fn any(&self) -> Result<Arc<Conn>, MicroError> {
        let mut candidates: Vec<String> = lock(&self.meta).brokers.values().cloned().collect();
        candidates.extend(self.bootstrap.iter().cloned());
        let mut last = None;
        for addr in candidates {
            match self.conn(&addr).await {
                Ok(conn) => return Ok(conn),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| MicroError::transport("micro.url names no Kafka broker")))
    }

    /// Whether some connection is up (or none was needed yet).
    pub(crate) fn is_connected(&self) -> bool {
        match self.conns.try_lock() {
            Ok(conns) => conns.is_empty() || conns.values().any(|conn| conn.is_alive()),
            Err(_) => true,
        }
    }

    /// Refreshes the metadata of `topics`; returns those that exist.
    pub(crate) async fn refresh(
        &self,
        topics: &[String],
    ) -> Result<HashMap<String, TopicMeta>, MicroError> {
        let request = MetadataRequest::default()
            .with_topics(Some(
                topics
                    .iter()
                    .map(|topic| MetadataRequestTopic::default().with_name(Some(topic_name(topic))))
                    .collect(),
            ))
            .with_allow_auto_topic_creation(false);
        let conn = self.any().await?;
        let response = conn
            .send(&request, version::METADATA, Duration::ZERO)
            .await?;
        let mut meta = lock(&self.meta);
        for broker in response.brokers {
            meta.brokers
                .insert(broker.node_id.0, address(broker.host.as_str(), broker.port));
        }
        let mut found = HashMap::new();
        for topic in response.topics {
            let Some(name) = topic.name else {
                continue;
            };
            let name = name.0.to_string();
            if topic.error_code != 0 {
                meta.topics.remove(&name);
                continue;
            }
            let mut partitions: Vec<PartitionMeta> = topic
                .partitions
                .iter()
                .map(|partition| PartitionMeta {
                    partition: partition.partition_index,
                    leader: partition.leader_id.0,
                })
                .collect();
            partitions.sort_by_key(|partition| partition.partition);
            let entry = TopicMeta {
                id: topic.topic_id,
                partitions,
            };
            meta.topics.insert(name.clone(), entry.clone());
            found.insert(name, entry);
        }
        Ok(found)
    }

    /// The metadata of `topic`, refreshed when unknown; `None` when it
    /// does not exist.
    pub(crate) async fn topic(&self, topic: &str) -> Result<Option<TopicMeta>, MicroError> {
        if let Some(known) = lock(&self.meta).topics.get(topic).cloned() {
            return Ok(Some(known));
        }
        Ok(self.refresh(&[topic.to_owned()]).await?.remove(topic))
    }

    /// The name of the topic with id `id`, among the known ones.
    pub(crate) fn topic_name_of(&self, id: Uuid) -> Option<String> {
        lock(&self.meta)
            .topics
            .iter()
            .find(|(_, meta)| meta.id == id)
            .map(|(name, _)| name.clone())
    }

    /// A connection to the leader of `topic`/`partition`.
    pub(crate) async fn leader(
        &self,
        topic: &str,
        partition: i32,
    ) -> Result<(i32, Arc<Conn>), MicroError> {
        let meta = self.topic(topic).await?.ok_or_else(|| {
            MicroError::transport(format!("the Kafka topic {topic} does not exist"))
        })?;
        let leader = meta
            .partitions
            .iter()
            .find(|p| p.partition == partition)
            .map(|p| p.leader)
            .ok_or_else(|| {
                MicroError::transport(format!("{topic} has no partition {partition}"))
            })?;
        let addr = lock(&self.meta)
            .brokers
            .get(&leader)
            .cloned()
            .ok_or_else(|| MicroError::transport(format!("Kafka broker {leader} is unknown")))?;
        Ok((leader, self.conn(&addr).await?))
    }

    /// A connection to broker `node`.
    pub(crate) async fn broker(&self, node: i32) -> Result<Arc<Conn>, MicroError> {
        let addr = lock(&self.meta)
            .brokers
            .get(&node)
            .cloned()
            .ok_or_else(|| MicroError::transport(format!("Kafka broker {node} is unknown")))?;
        self.conn(&addr).await
    }

    /// Forgets the metadata of `topic`, so the next use refreshes it.
    pub(crate) fn forget(&self, topic: &str) {
        lock(&self.meta).topics.remove(topic);
    }

    /// Produces one record; returns its partition and offset. Fails with
    /// [`ProduceError::NoTopic`] when the topic does not exist.
    pub(crate) async fn produce(
        &self,
        topic: &str,
        record: OutRecord,
    ) -> Result<(i32, i64), ProduceError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let meta = self.topic(topic).await?.ok_or(ProduceError::NoTopic)?;
            if meta.partitions.is_empty() {
                return Err(ProduceError::NoTopic);
            }
            let count = meta.partitions.len();
            let index = match &record.key {
                Some(key) => usize::try_from(murmur2(key) & 0x7fff_ffff).unwrap_or(0) % count,
                None => self.round_robin.fetch_add(1, Ordering::Relaxed) % count,
            };
            let partition = meta.partitions[index].partition;
            let (_, conn) = self.leader(topic, partition).await?;
            let mut records = BytesMut::new();
            RecordBatchEncoder::encode(
                &mut records,
                [&record.to_record()],
                &RecordEncodeOptions {
                    version: 2,
                    compression: Compression::None,
                },
            )
            .map_err(|error| {
                MicroError::transport(format!("encoding a Kafka record failed: {error}"))
            })?;
            let request = ProduceRequest::default()
                .with_acks(-1)
                .with_timeout_ms(30_000)
                .with_topic_data(vec![
                    TopicProduceData::default()
                        .with_name(topic_name(topic))
                        .with_partition_data(vec![
                            PartitionProduceData::default()
                                .with_index(partition)
                                .with_records(Some(records.freeze())),
                        ]),
                ]);
            let response = conn
                .send(&request, version::PRODUCE, Duration::ZERO)
                .await?;
            let outcome = response
                .responses
                .first()
                .and_then(|topic| topic.partition_responses.first())
                .map(|partition| (partition.error_code, partition.base_offset));
            match outcome {
                Some((0, offset)) => return Ok((partition, offset)),
                Some((code::UNKNOWN_TOPIC_OR_PARTITION, _)) if attempt >= 3 => {
                    return Err(ProduceError::NoTopic);
                }
                Some((
                    code::NOT_LEADER_OR_FOLLOWER
                    | code::LEADER_NOT_AVAILABLE
                    | code::UNKNOWN_TOPIC_OR_PARTITION,
                    _,
                )) if attempt < 3 => {
                    self.forget(topic);
                    tokio::time::sleep(Duration::from_millis(100 * attempt)).await;
                }
                Some((error, _)) => {
                    return Err(MicroError::transport(format!(
                        "Kafka refused the record for {topic}: {}",
                        describe(error)
                    ))
                    .into());
                }
                None => {
                    return Err(MicroError::transport(
                        "Kafka answered the produce request with nothing",
                    )
                    .into());
                }
            }
        }
    }

    /// The offset at `timestamp` ([`LATEST`] or [`EARLIEST`]) of each
    /// partition of `topic`.
    pub(crate) async fn offsets(
        &self,
        topic: &str,
        partitions: &[i32],
        timestamp: i64,
    ) -> Result<HashMap<i32, i64>, MicroError> {
        let mut by_leader: HashMap<i32, Vec<i32>> = HashMap::new();
        for &partition in partitions {
            let (leader, _) = self.leader(topic, partition).await?;
            by_leader.entry(leader).or_default().push(partition);
        }
        let mut offsets = HashMap::new();
        for (leader, partitions) in by_leader {
            let conn = self.broker(leader).await?;
            let request = ListOffsetsRequest::default()
                .with_replica_id(BrokerId(-1))
                .with_topics(vec![
                    ListOffsetsTopic::default()
                        .with_name(topic_name(topic))
                        .with_partitions(
                            partitions
                                .iter()
                                .map(|&partition| {
                                    ListOffsetsPartition::default()
                                        .with_partition_index(partition)
                                        .with_current_leader_epoch(-1)
                                        .with_timestamp(timestamp)
                                })
                                .collect(),
                        ),
                ]);
            let response = conn
                .send(&request, version::LIST_OFFSETS, Duration::ZERO)
                .await?;
            for topic in response.topics {
                for partition in topic.partitions {
                    if partition.error_code == 0 {
                        offsets.insert(partition.partition_index, partition.offset);
                    } else {
                        self.forget(topic.name.0.as_str());
                        return Err(MicroError::transport(format!(
                            "listing the offsets of {} failed: {}",
                            topic.name.0.as_str(),
                            describe(partition.error_code)
                        )));
                    }
                }
            }
        }
        Ok(offsets)
    }

    /// Fetches from one leader, on a connection of `lane`: `positions`
    /// lists topic, partition and offset. Waits up to `max_wait` for
    /// records.
    pub(crate) async fn fetch(
        &self,
        leader: i32,
        positions: &[(String, i32, i64)],
        max_wait: Duration,
        lane: u64,
    ) -> Result<FetchResponse, MicroError> {
        let addr = lock(&self.meta)
            .brokers
            .get(&leader)
            .cloned()
            .ok_or_else(|| MicroError::transport(format!("Kafka broker {leader} is unknown")))?;
        let conn = self.fetch_conn(&addr, lane).await?;
        let mut topics: Vec<FetchTopic> = Vec::new();
        for (topic, partition, offset) in positions {
            let entry = FetchPartition::default()
                .with_partition(*partition)
                .with_current_leader_epoch(-1)
                .with_fetch_offset(*offset)
                .with_last_fetched_epoch(-1)
                .with_log_start_offset(-1)
                .with_partition_max_bytes(4 * 1024 * 1024);
            match topics.iter_mut().find(|t| t.topic.0.as_str() == topic) {
                Some(existing) => existing.partitions.push(entry),
                None => topics.push(
                    FetchTopic::default()
                        .with_topic(topic_name(topic))
                        .with_partitions(vec![entry]),
                ),
            }
        }
        let request = FetchRequest::default()
            .with_replica_id(BrokerId(-1))
            .with_max_wait_ms(i32::try_from(max_wait.as_millis()).unwrap_or(i32::MAX))
            .with_min_bytes(1)
            .with_max_bytes(32 * 1024 * 1024)
            .with_isolation_level(1)
            .with_session_id(0)
            .with_session_epoch(-1)
            .with_topics(topics);
        conn.send(&request, version::FETCH, max_wait).await
    }

    /// Creates `topics` (name, partitions, replication factor, retention in
    /// milliseconds); returns each name with whether it was created (false:
    /// it existed).
    pub(crate) async fn create_topics(
        &self,
        topics: &[(String, i32, i16, Option<u64>)],
    ) -> Result<Vec<(String, bool)>, MicroError> {
        if topics.is_empty() {
            return Ok(Vec::new());
        }
        let request = CreateTopicsRequest::default()
            .with_timeout_ms(30_000)
            .with_topics(
                topics
                    .iter()
                    .map(|(name, partitions, replication, retention)| {
                        let configs = retention
                            .map(|millis| {
                                vec![
                                    CreatableTopicConfig::default()
                                        .with_name(StrBytes::from_static_str("retention.ms"))
                                        .with_value(Some(StrBytes::from_string(
                                            millis.to_string(),
                                        ))),
                                ]
                            })
                            .unwrap_or_default();
                        CreatableTopic::default()
                            .with_name(topic_name(name))
                            .with_num_partitions(*partitions)
                            .with_replication_factor(*replication)
                            .with_configs(configs)
                    })
                    .collect(),
            );
        let conn = self.any().await?;
        let response = conn
            .send(&request, version::CREATE_TOPICS, Duration::ZERO)
            .await?;
        let mut outcome = Vec::new();
        for topic in response.topics {
            let name = topic.name.0.to_string();
            match topic.error_code {
                0 => outcome.push((name.clone(), true)),
                code::TOPIC_ALREADY_EXISTS => outcome.push((name.clone(), false)),
                error => {
                    return Err(MicroError::transport(format!(
                        "creating the Kafka topic {name} failed: {}{}",
                        describe(error),
                        topic
                            .error_message
                            .map(|m| format!(": {m}"))
                            .unwrap_or_default()
                    )));
                }
            }
            self.forget(&name);
        }
        Ok(outcome)
    }
}

/// Why a produce failed.
#[derive(Debug)]
pub(crate) enum ProduceError {
    /// The topic does not exist.
    NoTopic,
    /// Anything else.
    Failed(MicroError),
}

impl From<MicroError> for ProduceError {
    fn from(error: MicroError) -> Self {
        ProduceError::Failed(error)
    }
}

/// A record to produce.
#[derive(Clone, Debug, Default)]
pub(crate) struct OutRecord {
    pub(crate) key: Option<Bytes>,
    pub(crate) headers: Vec<(String, Bytes)>,
    pub(crate) value: Bytes,
}

impl OutRecord {
    fn to_record(&self) -> Record {
        let mut headers = IndexMap::new();
        for (name, value) in &self.headers {
            headers.insert(StrBytes::from_string(name.clone()), Some(value.clone()));
        }
        Record {
            transactional: false,
            control: false,
            delete_horizon: false,
            partition_leader_epoch: -1,
            producer_id: -1,
            producer_epoch: -1,
            timestamp_type: TimestampType::Creation,
            offset: 0,
            sequence: -1,
            timestamp: crate::envelope::now_millis(),
            key: self.key.clone(),
            value: Some(self.value.clone()),
            headers,
        }
    }
}

/// A fetched record.
#[derive(Clone, Debug)]
pub(crate) struct InRecord {
    pub(crate) offset: i64,
    pub(crate) key: Option<Bytes>,
    pub(crate) headers: Vec<(String, Bytes)>,
    pub(crate) value: Bytes,
}

/// The records of a fetched partition at or after `from`, skipping control
/// records.
pub(crate) fn records(data: Option<Bytes>, from: i64) -> Result<Vec<InRecord>, MicroError> {
    let Some(mut data) = data else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    while !data.is_empty() {
        let set = match RecordBatchDecoder::decode(&mut data) {
            Ok(set) => set,
            // A partial batch at the end of a fetch.
            Err(_) if !out.is_empty() => break,
            Err(error) => {
                return Err(MicroError::transport(format!(
                    "a Kafka record batch did not decode: {error}"
                )));
            }
        };
        for record in set.records {
            if record.control || record.offset < from {
                continue;
            }
            out.push(InRecord {
                offset: record.offset,
                key: record.key,
                headers: record
                    .headers
                    .into_iter()
                    .filter_map(|(name, value)| Some((name.to_string(), value?)))
                    .collect(),
                value: record.value.unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

pub(crate) fn topic_name(name: &str) -> TopicName {
    TopicName(StrBytes::from_string(name.to_owned()))
}

/// `host:port`, with brackets for IPv6 hosts.
pub(crate) fn address(host: &str, port: i32) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Kafka's default partitioner hash (murmur2), so keys land on the same
/// partitions as with the Java client.
pub(crate) fn murmur2(data: &[u8]) -> i32 {
    const SEED: u32 = 0x9747_b28c;
    const M: u32 = 0x5bd1_e995;
    const R: u32 = 24;
    let length = data.len();
    let mut h: u32 = SEED ^ u32::try_from(length).unwrap_or(u32::MAX);
    let chunks = length / 4;
    for i in 0..chunks {
        let at = i * 4;
        let mut k = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    let tail = chunks * 4;
    match length % 4 {
        3 => {
            h ^= u32::from(data[tail + 2]) << 16;
            h ^= u32::from(data[tail + 1]) << 8;
            h ^= u32::from(data[tail]);
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= u32::from(data[tail + 1]) << 8;
            h ^= u32::from(data[tail]);
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= u32::from(data[tail]);
            h = h.wrapping_mul(M);
        }
        _ => {}
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    i32::from_ne_bytes(h.to_ne_bytes())
}

/// Sends a SASL handshake.
pub(crate) async fn sasl_handshake(conn: &Conn, mechanism: &str) -> Result<(), MicroError> {
    let request =
        SaslHandshakeRequest::default().with_mechanism(StrBytes::from_string(mechanism.to_owned()));
    let response = conn
        .send(&request, version::SASL_HANDSHAKE, Duration::ZERO)
        .await?;
    if response.error_code != 0 {
        let offered: Vec<String> = response
            .mechanisms
            .iter()
            .map(ToString::to_string)
            .collect();
        return Err(MicroError::transport(format!(
            "Kafka refused SASL {mechanism}: {} (it offers {})",
            describe(response.error_code),
            offered.join(", ")
        )));
    }
    Ok(())
}

/// One SASL round trip.
pub(crate) async fn sasl_authenticate(conn: &Conn, bytes: Bytes) -> Result<Bytes, MicroError> {
    let request = SaslAuthenticateRequest::default().with_auth_bytes(bytes);
    let response = conn
        .send(&request, version::SASL_AUTHENTICATE, Duration::ZERO)
        .await?;
    if response.error_code != 0 {
        return Err(MicroError::transport(format!(
            "Kafka SASL authentication failed: {}{}",
            describe(response.error_code),
            response
                .error_message
                .map(|m| format!(": {m}"))
                .unwrap_or_default()
        )));
    }
    Ok(response.auth_bytes)
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_hash_like_the_java_client() {
        assert_eq!(murmur2(b"21"), -973_932_308);
        assert_eq!(murmur2(b"foobar"), -790_332_482);
        assert_eq!(murmur2(b"a-little-bit-long-string"), -985_981_536);
        assert_eq!(murmur2(b"a-little-bit-longer-string"), -1_486_304_829);
        assert_eq!(
            murmur2(b"lkjh234lh9fiuh90y23oiuhsafujhadof229phr9h19h89h8"),
            -58_897_971
        );
        assert_eq!(murmur2(b"abc"), 479_470_107);
    }

    #[test]
    fn records_round_trip_through_a_batch() {
        let out = OutRecord {
            key: Some(Bytes::from_static(b"deal-1")),
            headers: vec![("navree-pattern".into(), Bytes::from_static(b"deal.won"))],
            value: Bytes::from_static(b"{}"),
        };
        let mut buf = BytesMut::new();
        RecordBatchEncoder::encode(
            &mut buf,
            [&out.to_record()],
            &RecordEncodeOptions {
                version: 2,
                compression: Compression::None,
            },
        )
        .unwrap();
        let back = records(Some(buf.freeze()), 0).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].key.as_deref(), Some(&b"deal-1"[..]));
        assert_eq!(back[0].headers[0].0, "navree-pattern");
        assert_eq!(back[0].value.as_ref(), b"{}");
        assert!(records(None, 0).unwrap().is_empty());
    }

    #[test]
    fn addresses_bracket_ipv6_hosts() {
        assert_eq!(address("kafka-1", 9092), "kafka-1:9092");
        assert_eq!(address("::1", 9092), "[::1]:9092");
        assert!(describe(3).contains("(3)"));
    }
}
