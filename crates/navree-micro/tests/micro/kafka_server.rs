//! A small Kafka broker speaking the APIs the Kafka transport uses:
//! Metadata, CreateTopics, Produce, ListOffsets, Fetch (with long polling),
//! FindCoordinator, the KIP-848 ConsumerGroupHeartbeat with incremental
//! assignment (a partition moves only once its owner released it),
//! OffsetCommit, OffsetFetch and SASL PLAIN. The transport runs against it
//! with its own client code, without a Kafka cluster. Like Kafka, it
//! handles the requests of one connection one at a time.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use kafka_protocol::messages::consumer_group_heartbeat_response::{Assignment, TopicPartitions};
use kafka_protocol::messages::create_topics_response::CreatableTopicResult;
use kafka_protocol::messages::fetch_response::{FetchableTopicResponse, PartitionData};
use kafka_protocol::messages::find_coordinator_response::Coordinator;
use kafka_protocol::messages::list_offsets_response::{
    ListOffsetsPartitionResponse, ListOffsetsTopicResponse,
};
use kafka_protocol::messages::metadata_response::{
    MetadataResponseBroker, MetadataResponsePartition, MetadataResponseTopic,
};
use kafka_protocol::messages::offset_commit_response::{
    OffsetCommitResponsePartition, OffsetCommitResponseTopic,
};
use kafka_protocol::messages::offset_fetch_response::{
    OffsetFetchResponseGroup, OffsetFetchResponsePartitions, OffsetFetchResponseTopics,
};
use kafka_protocol::messages::produce_response::{PartitionProduceResponse, TopicProduceResponse};
use kafka_protocol::messages::{
    ApiKey, BrokerId, ConsumerGroupHeartbeatRequest, ConsumerGroupHeartbeatResponse,
    CreateTopicsRequest, CreateTopicsResponse, FetchRequest, FetchResponse, FindCoordinatorRequest,
    FindCoordinatorResponse, ListOffsetsRequest, ListOffsetsResponse, MetadataRequest,
    MetadataResponse, OffsetCommitRequest, OffsetCommitResponse, OffsetFetchRequest,
    OffsetFetchResponse, ProduceRequest, ProduceResponse, RequestHeader, ResponseHeader,
    SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest, SaslHandshakeResponse,
    TopicName,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use kafka_protocol::records::{
    Compression, Record, RecordBatchDecoder, RecordBatchEncoder, RecordEncodeOptions,
};
use navree_kernel::BoxError;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The one broker's node id.
const NODE: i32 = 0;

/// The heartbeat interval the coordinator asks for.
const HEARTBEAT_MS: i32 = 100;

/// A member that stops heartbeating leaves the group after this long.
const SESSION_TIMEOUT: Duration = Duration::from_secs(3);

/// Kafka error codes the broker answers with.
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const OFFSET_OUT_OF_RANGE: i16 = 1;
const CORRUPT_MESSAGE: i16 = 2;
const UNKNOWN_MEMBER_ID: i16 = 25;
const UNSUPPORTED_SASL_MECHANISM: i16 = 33;
const TOPIC_ALREADY_EXISTS: i16 = 36;
const SASL_AUTHENTICATION_FAILED: i16 = 58;

/// The requests the broker serves.
enum RequestKind {
    SaslHandshake(SaslHandshakeRequest),
    SaslAuthenticate(SaslAuthenticateRequest),
    Metadata(MetadataRequest),
    CreateTopics(CreateTopicsRequest),
    Produce(ProduceRequest),
    ListOffsets(ListOffsetsRequest),
    Fetch(FetchRequest),
    FindCoordinator(FindCoordinatorRequest),
    ConsumerGroupHeartbeat(ConsumerGroupHeartbeatRequest),
    OffsetCommit(OffsetCommitRequest),
    OffsetFetch(OffsetFetchRequest),
}

impl RequestKind {
    /// Decodes the body of a request to `api` at `version`; `Ok(None)` for
    /// an API the broker does not serve.
    fn decode(
        api: ApiKey,
        bytes: &mut Bytes,
        version: i16,
    ) -> Result<Option<RequestKind>, BoxError> {
        Ok(Some(match api {
            ApiKey::SaslHandshake => RequestKind::SaslHandshake(Decodable::decode(bytes, version)?),
            ApiKey::SaslAuthenticate => {
                RequestKind::SaslAuthenticate(Decodable::decode(bytes, version)?)
            }
            ApiKey::Metadata => RequestKind::Metadata(Decodable::decode(bytes, version)?),
            ApiKey::CreateTopics => RequestKind::CreateTopics(Decodable::decode(bytes, version)?),
            ApiKey::Produce => RequestKind::Produce(Decodable::decode(bytes, version)?),
            ApiKey::ListOffsets => RequestKind::ListOffsets(Decodable::decode(bytes, version)?),
            ApiKey::Fetch => RequestKind::Fetch(Decodable::decode(bytes, version)?),
            ApiKey::FindCoordinator => {
                RequestKind::FindCoordinator(Decodable::decode(bytes, version)?)
            }
            ApiKey::ConsumerGroupHeartbeat => {
                RequestKind::ConsumerGroupHeartbeat(Decodable::decode(bytes, version)?)
            }
            ApiKey::OffsetCommit => RequestKind::OffsetCommit(Decodable::decode(bytes, version)?),
            ApiKey::OffsetFetch => RequestKind::OffsetFetch(Decodable::decode(bytes, version)?),
            _ => return Ok(None),
        }))
    }
}

/// The responses the broker sends.
enum ResponseKind {
    SaslHandshake(SaslHandshakeResponse),
    SaslAuthenticate(SaslAuthenticateResponse),
    Metadata(MetadataResponse),
    CreateTopics(CreateTopicsResponse),
    Produce(ProduceResponse),
    ListOffsets(ListOffsetsResponse),
    Fetch(FetchResponse),
    FindCoordinator(FindCoordinatorResponse),
    ConsumerGroupHeartbeat(ConsumerGroupHeartbeatResponse),
    OffsetCommit(OffsetCommitResponse),
    OffsetFetch(OffsetFetchResponse),
}

impl ResponseKind {
    fn encode(&self, out: &mut BytesMut, version: i16) -> Result<(), BoxError> {
        let encoded = match self {
            ResponseKind::SaslHandshake(r) => r.encode(out, version),
            ResponseKind::SaslAuthenticate(r) => r.encode(out, version),
            ResponseKind::Metadata(r) => r.encode(out, version),
            ResponseKind::CreateTopics(r) => r.encode(out, version),
            ResponseKind::Produce(r) => r.encode(out, version),
            ResponseKind::ListOffsets(r) => r.encode(out, version),
            ResponseKind::Fetch(r) => r.encode(out, version),
            ResponseKind::FindCoordinator(r) => r.encode(out, version),
            ResponseKind::ConsumerGroupHeartbeat(r) => r.encode(out, version),
            ResponseKind::OffsetCommit(r) => r.encode(out, version),
            ResponseKind::OffsetFetch(r) => r.encode(out, version),
        };
        Ok(encoded?)
    }
}

/// A running broker; stops (and drops its connections) when dropped.
pub struct MiniKafka {
    addr: SocketAddr,
    state: Arc<State>,
    stop: CancellationToken,
}

/// A topic as created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopicSettings {
    pub partitions: usize,
    pub replication_factor: i16,
    pub retention_ms: Option<String>,
}

impl MiniKafka {
    /// Starts a broker on a free local port.
    pub async fn start() -> MiniKafka {
        Self::launch(None).await
    }

    /// Starts a broker that requires SASL PLAIN with these credentials.
    pub async fn with_plain(username: &str, password: &str) -> MiniKafka {
        Self::launch(Some((username.to_owned(), password.to_owned()))).await
    }

    async fn launch(sasl: Option<(String, String)>) -> MiniKafka {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(State {
            sasl,
            inner: Mutex::default(),
            produced: Notify::new(),
        });
        let stop = CancellationToken::new();
        let shared = state.clone();
        let stopping = stop.clone();
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    accepted = listener.accept() => accepted,
                    () = stopping.cancelled() => return,
                };
                let Ok((stream, _)) = accepted else {
                    return;
                };
                tokio::spawn(serve(stream, addr, shared.clone(), stopping.clone()));
            }
        });
        MiniKafka { addr, state, stop }
    }

    /// `127.0.0.1:<port>`, the bootstrap broker.
    pub fn url(&self) -> String {
        self.addr.to_string()
    }

    /// The topics, by name.
    pub fn topics(&self) -> Vec<String> {
        self.state.lock().topics.keys().cloned().collect()
    }

    /// How topic `name` was created.
    pub fn topic(&self, name: &str) -> Option<TopicSettings> {
        self.state
            .lock()
            .topics
            .get(name)
            .map(|topic| TopicSettings {
                partitions: topic.partitions.len(),
                replication_factor: topic.replication_factor,
                retention_ms: topic.retention_ms.clone(),
            })
    }

    /// The records of `topic`, every partition, as (headers, value).
    pub fn records(&self, topic: &str) -> Vec<(BTreeMap<String, String>, String)> {
        let inner = self.state.lock();
        let Some(topic) = inner.topics.get(topic) else {
            return Vec::new();
        };
        topic
            .partitions
            .iter()
            .flatten()
            .map(|record| {
                let headers = record
                    .headers
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.to_string(),
                            value.as_ref().map_or_else(String::new, |v| {
                                String::from_utf8_lossy(v).into_owned()
                            }),
                        )
                    })
                    .collect();
                let value = record
                    .value
                    .as_ref()
                    .map_or_else(String::new, |v| String::from_utf8_lossy(v).into_owned());
                (headers, value)
            })
            .collect()
    }

    /// The offsets group `group` committed for `topic`, by partition.
    pub fn committed(&self, group: &str, topic: &str) -> BTreeMap<i32, i64> {
        let inner = self.state.lock();
        inner
            .groups
            .get(group)
            .map(|group| {
                group
                    .offsets
                    .iter()
                    .filter(|((t, _), _)| t == topic)
                    .map(|((_, partition), offset)| (*partition, *offset))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The members of `group`.
    pub fn members(&self, group: &str) -> usize {
        self.state
            .lock()
            .groups
            .get(group)
            .map_or(0, |group| group.members.len())
    }

    /// Requests the broker did not understand, or refused to serve.
    pub fn problems(&self) -> Vec<String> {
        self.state.lock().problems.clone()
    }
}

impl Drop for MiniKafka {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

struct State {
    sasl: Option<(String, String)>,
    inner: Mutex<Inner>,
    produced: Notify,
}

impl State {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn problem(&self, problem: String) {
        self.lock().problems.push(problem);
    }
}

#[derive(Default)]
struct Inner {
    topics: BTreeMap<String, Topic>,
    groups: HashMap<String, Group>,
    problems: Vec<String>,
}

struct Topic {
    id: Uuid,
    partitions: Vec<Vec<Record>>,
    replication_factor: i16,
    retention_ms: Option<String>,
}

#[derive(Default)]
struct Group {
    epoch: i32,
    members: BTreeMap<String, Member>,
    offsets: HashMap<(String, i32), i64>,
}

struct Member {
    topics: Vec<String>,
    owned: BTreeSet<(String, i32)>,
    seen: Instant,
}

fn name(text: &str) -> TopicName {
    TopicName(StrBytes::from_string(text.to_owned()))
}

fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

fn len_i32(len: usize) -> i32 {
    i32::try_from(len).unwrap_or(i32::MAX)
}

/// Serves one connection: one request at a time, in order.
async fn serve(stream: TcpStream, addr: SocketAddr, state: Arc<State>, stop: CancellationToken) {
    let _ = stream.set_nodelay(true);
    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(64 << 20)
        .new_codec();
    let mut framed = Framed::new(stream, codec);
    let mut authenticated = state.sasl.is_none();
    loop {
        let frame = tokio::select! {
            frame = framed.next() => frame,
            () = stop.cancelled() => return,
        };
        let Some(Ok(frame)) = frame else {
            return;
        };
        let mut bytes = frame.freeze();
        if bytes.len() < 4 {
            return;
        }
        let key = i16::from_be_bytes([bytes[0], bytes[1]]);
        let version = i16::from_be_bytes([bytes[2], bytes[3]]);
        let Ok(api) = ApiKey::try_from(key) else {
            state.problem(format!("unknown API key {key}"));
            return;
        };
        let Ok(header) = RequestHeader::decode(&mut bytes, api.request_header_version(version))
        else {
            state.problem(format!("{api:?} v{version}: a malformed header"));
            return;
        };
        let request = match RequestKind::decode(api, &mut bytes, version) {
            Ok(Some(request)) => request,
            Ok(None) => {
                state.problem(format!("an unsupported request: {api:?} v{version}"));
                return;
            }
            Err(error) => {
                state.problem(format!("{api:?} v{version} did not decode: {error}"));
                return;
            }
        };
        let response = tokio::select! {
            response = handle(&state, addr, request, &mut authenticated) => response,
            () = stop.cancelled() => return,
        };
        let Some(response) = response else {
            return;
        };
        let mut out = BytesMut::new();
        let encoded = ResponseHeader::default()
            .with_correlation_id(header.correlation_id)
            .encode(&mut out, api.response_header_version(version))
            .map_err(BoxError::from)
            .and_then(|()| response.encode(&mut out, version));
        if let Err(error) = encoded {
            state.problem(format!(
                "{api:?} v{version}: the response did not encode: {error}"
            ));
            return;
        }
        if framed.send(out.freeze()).await.is_err() {
            return;
        }
    }
}

/// Answers one request; `None` closes the connection.
async fn handle(
    state: &State,
    addr: SocketAddr,
    request: RequestKind,
    authenticated: &mut bool,
) -> Option<ResponseKind> {
    match request {
        RequestKind::SaslHandshake(request) => {
            let supported = state.sasl.is_some() && request.mechanism.as_str() == "PLAIN";
            Some(ResponseKind::SaslHandshake(
                SaslHandshakeResponse::default()
                    .with_error_code(if supported {
                        0
                    } else {
                        UNSUPPORTED_SASL_MECHANISM
                    })
                    .with_mechanisms(vec![StrBytes::from_static_str("PLAIN")]),
            ))
        }
        RequestKind::SaslAuthenticate(request) => {
            let expected = state
                .sasl
                .as_ref()
                .map(|(username, password)| format!("\0{username}\0{password}"));
            if expected.as_deref().map(str::as_bytes) == Some(request.auth_bytes.as_ref()) {
                *authenticated = true;
                Some(ResponseKind::SaslAuthenticate(
                    SaslAuthenticateResponse::default(),
                ))
            } else {
                Some(ResponseKind::SaslAuthenticate(
                    SaslAuthenticateResponse::default()
                        .with_error_code(SASL_AUTHENTICATION_FAILED)
                        .with_error_message(Some(StrBytes::from_static_str(
                            "Authentication failed: Invalid username or password",
                        ))),
                ))
            }
        }
        _ if !*authenticated => {
            state.problem("a request before SASL authentication".to_owned());
            None
        }
        RequestKind::Metadata(request) => {
            Some(ResponseKind::Metadata(metadata(state, addr, request)))
        }
        RequestKind::CreateTopics(request) => {
            Some(ResponseKind::CreateTopics(create_topics(state, request)))
        }
        RequestKind::Produce(request) => Some(ResponseKind::Produce(produce(state, request))),
        RequestKind::ListOffsets(request) => {
            Some(ResponseKind::ListOffsets(list_offsets(state, request)))
        }
        RequestKind::Fetch(request) => Some(ResponseKind::Fetch(fetch(state, request).await)),
        RequestKind::FindCoordinator(request) => Some(ResponseKind::FindCoordinator(
            find_coordinator(addr, request),
        )),
        RequestKind::ConsumerGroupHeartbeat(request) => Some(ResponseKind::ConsumerGroupHeartbeat(
            heartbeat(state, request),
        )),
        RequestKind::OffsetCommit(request) => {
            Some(ResponseKind::OffsetCommit(offset_commit(state, request)))
        }
        RequestKind::OffsetFetch(request) => {
            Some(ResponseKind::OffsetFetch(offset_fetch(state, request)))
        }
    }
}

fn metadata(state: &State, addr: SocketAddr, request: MetadataRequest) -> MetadataResponse {
    let inner = state.lock();
    let names: Vec<String> = match request.topics {
        None => inner.topics.keys().cloned().collect(),
        Some(topics) => topics
            .into_iter()
            .filter_map(|topic| topic.name.map(|name| name.0.to_string()))
            .collect(),
    };
    let topics = names
        .into_iter()
        .map(|topic_name| match inner.topics.get(&topic_name) {
            Some(topic) => MetadataResponseTopic::default()
                .with_name(Some(name(&topic_name)))
                .with_topic_id(topic.id)
                .with_partitions(
                    (0..topic.partitions.len())
                        .map(|partition| {
                            MetadataResponsePartition::default()
                                .with_partition_index(len_i32(partition))
                                .with_leader_id(BrokerId(NODE))
                                .with_replica_nodes(vec![BrokerId(NODE)])
                                .with_isr_nodes(vec![BrokerId(NODE)])
                        })
                        .collect(),
                ),
            None => MetadataResponseTopic::default()
                .with_error_code(UNKNOWN_TOPIC_OR_PARTITION)
                .with_name(Some(name(&topic_name))),
        })
        .collect();
    MetadataResponse::default()
        .with_brokers(vec![
            MetadataResponseBroker::default()
                .with_node_id(BrokerId(NODE))
                .with_host(StrBytes::from_string(addr.ip().to_string()))
                .with_port(i32::from(addr.port())),
        ])
        .with_cluster_id(Some(StrBytes::from_static_str("mini-kafka")))
        .with_controller_id(BrokerId(NODE))
        .with_topics(topics)
}

fn create_topics(state: &State, request: CreateTopicsRequest) -> CreateTopicsResponse {
    let mut inner = state.lock();
    let topics = request
        .topics
        .into_iter()
        .map(|topic| {
            let topic_name = topic.name.0.to_string();
            if let Some(existing) = inner.topics.get(&topic_name) {
                return CreatableTopicResult::default()
                    .with_name(topic.name)
                    .with_topic_id(existing.id)
                    .with_error_code(TOPIC_ALREADY_EXISTS)
                    .with_error_message(Some(StrBytes::from_string(format!(
                        "Topic '{topic_name}' already exists."
                    ))));
            }
            let partitions = topic.num_partitions.max(1);
            let id = Uuid::new_v4();
            let retention_ms = topic
                .configs
                .iter()
                .find(|config| config.name.as_str() == "retention.ms")
                .and_then(|config| config.value.as_ref().map(ToString::to_string));
            inner.topics.insert(
                topic_name,
                Topic {
                    id,
                    partitions: (0..partitions).map(|_| Vec::new()).collect(),
                    replication_factor: topic.replication_factor,
                    retention_ms,
                },
            );
            CreatableTopicResult::default()
                .with_name(topic.name)
                .with_topic_id(id)
                .with_num_partitions(partitions)
                .with_replication_factor(topic.replication_factor)
        })
        .collect();
    CreateTopicsResponse::default().with_topics(topics)
}

fn produce(state: &State, request: ProduceRequest) -> ProduceResponse {
    let mut inner = state.lock();
    let mut responses = Vec::new();
    for topic in request.topic_data {
        let topic_name = topic.name.0.to_string();
        let mut partitions = Vec::new();
        for data in topic.partition_data {
            let response = PartitionProduceResponse::default().with_index(data.index);
            let log = inner.topics.get_mut(&topic_name).and_then(|topic| {
                usize::try_from(data.index)
                    .ok()
                    .and_then(|index| topic.partitions.get_mut(index))
            });
            let Some(log) = log else {
                partitions.push(
                    response
                        .with_error_code(UNKNOWN_TOPIC_OR_PARTITION)
                        .with_base_offset(-1),
                );
                continue;
            };
            let base = len_i64(log.len());
            let mut records = data.records.unwrap_or_default();
            let mut corrupt = false;
            while !records.is_empty() {
                match RecordBatchDecoder::decode(&mut records) {
                    Ok(set) => {
                        for mut record in set.records {
                            record.offset = len_i64(log.len());
                            log.push(record);
                        }
                    }
                    Err(_) => {
                        corrupt = true;
                        break;
                    }
                }
            }
            partitions.push(if corrupt {
                response
                    .with_error_code(CORRUPT_MESSAGE)
                    .with_base_offset(-1)
            } else {
                response.with_base_offset(base)
            });
        }
        responses.push(
            TopicProduceResponse::default()
                .with_name(topic.name)
                .with_partition_responses(partitions),
        );
    }
    drop(inner);
    state.produced.notify_waiters();
    ProduceResponse::default().with_responses(responses)
}

fn list_offsets(state: &State, request: ListOffsetsRequest) -> ListOffsetsResponse {
    let inner = state.lock();
    let topics = request
        .topics
        .into_iter()
        .map(|topic| {
            let log = inner.topics.get(topic.name.0.as_str());
            let partitions = topic
                .partitions
                .into_iter()
                .map(|asked| {
                    let response = ListOffsetsPartitionResponse::default()
                        .with_partition_index(asked.partition_index)
                        .with_timestamp(-1);
                    let len = log.and_then(|topic| {
                        usize::try_from(asked.partition_index)
                            .ok()
                            .and_then(|index| topic.partitions.get(index))
                            .map(Vec::len)
                    });
                    match len {
                        // -2: the earliest offset; anything else, the end.
                        Some(len) => response.with_offset(if asked.timestamp == -2 {
                            0
                        } else {
                            len_i64(len)
                        }),
                        None => response
                            .with_error_code(UNKNOWN_TOPIC_OR_PARTITION)
                            .with_offset(-1),
                    }
                })
                .collect();
            ListOffsetsTopicResponse::default()
                .with_name(topic.name)
                .with_partitions(partitions)
        })
        .collect();
    ListOffsetsResponse::default().with_topics(topics)
}

/// Waits up to `max_wait_ms` for records at any asked position.
async fn fetch(state: &State, request: FetchRequest) -> FetchResponse {
    let wait = Duration::from_millis(u64::try_from(request.max_wait_ms.max(0)).unwrap_or(0));
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let notified = state.produced.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let (response, any) = fetch_now(state, &request);
        if any || tokio::time::Instant::now() >= deadline {
            return response;
        }
        let _ = tokio::time::timeout_at(deadline, notified).await;
    }
}

/// The records at the asked positions, and whether there were any.
fn fetch_now(state: &State, request: &FetchRequest) -> (FetchResponse, bool) {
    let inner = state.lock();
    let mut any = false;
    let responses = request
        .topics
        .iter()
        .map(|topic| {
            let log = inner.topics.get(topic.topic.0.as_str());
            let partitions = topic
                .partitions
                .iter()
                .map(|asked| {
                    let data = PartitionData::default()
                        .with_partition_index(asked.partition)
                        .with_aborted_transactions(Some(Vec::new()))
                        .with_preferred_read_replica(BrokerId(-1));
                    let records = log.and_then(|topic| {
                        usize::try_from(asked.partition)
                            .ok()
                            .and_then(|index| topic.partitions.get(index))
                    });
                    let Some(records) = records else {
                        return data
                            .with_error_code(UNKNOWN_TOPIC_OR_PARTITION)
                            .with_high_watermark(-1);
                    };
                    let high = len_i64(records.len());
                    if asked.fetch_offset < 0 || asked.fetch_offset > high {
                        return data
                            .with_error_code(OFFSET_OUT_OF_RANGE)
                            .with_high_watermark(high);
                    }
                    let from = usize::try_from(asked.fetch_offset).unwrap_or(0);
                    let batch: Vec<&Record> = records[from..].iter().take(500).collect();
                    let mut bytes = BytesMut::new();
                    if !batch.is_empty() {
                        any = true;
                        RecordBatchEncoder::encode(
                            &mut bytes,
                            batch,
                            &RecordEncodeOptions {
                                version: 2,
                                compression: Compression::None,
                            },
                        )
                        .expect("records encode");
                    }
                    data.with_high_watermark(high)
                        .with_last_stable_offset(high)
                        .with_log_start_offset(0)
                        .with_records(Some(Bytes::from(bytes)))
                })
                .collect();
            FetchableTopicResponse::default()
                .with_topic(topic.topic.clone())
                .with_topic_id(log.map_or(Uuid::nil(), |topic| topic.id))
                .with_partitions(partitions)
        })
        .collect();
    (FetchResponse::default().with_responses(responses), any)
}

fn find_coordinator(addr: SocketAddr, request: FindCoordinatorRequest) -> FindCoordinatorResponse {
    FindCoordinatorResponse::default().with_coordinators(
        request
            .coordinator_keys
            .into_iter()
            .map(|key| {
                Coordinator::default()
                    .with_key(key)
                    .with_node_id(BrokerId(NODE))
                    .with_host(StrBytes::from_string(addr.ip().to_string()))
                    .with_port(i32::from(addr.port()))
            })
            .collect(),
    )
}

/// KIP-848: members join with epoch 0 and leave with -1; each topic's
/// partitions are spread over the members subscribed to it, and a
/// partition another member still owns is held back until it reports it
/// released.
fn heartbeat(
    state: &State,
    request: ConsumerGroupHeartbeatRequest,
) -> ConsumerGroupHeartbeatResponse {
    let mut inner = state.lock();
    let Inner { topics, groups, .. } = &mut *inner;
    let now = Instant::now();
    let group = groups.entry(request.group_id.0.to_string()).or_default();
    let before = group.members.len();
    group
        .members
        .retain(|_, member| now.duration_since(member.seen) < SESSION_TIMEOUT);
    if group.members.len() != before {
        group.epoch += 1;
    }
    let member_id = request.member_id.to_string();
    let response = ConsumerGroupHeartbeatResponse::default()
        .with_member_id(Some(request.member_id.clone()))
        .with_heartbeat_interval_ms(HEARTBEAT_MS);
    if request.member_epoch == -1 {
        if group.members.remove(&member_id).is_some() {
            group.epoch += 1;
        }
        return response.with_member_epoch(-1);
    }
    if !group.members.contains_key(&member_id) {
        if request.member_epoch != 0 {
            return response.with_error_code(UNKNOWN_MEMBER_ID);
        }
        group.epoch += 1;
        group.members.insert(
            member_id.clone(),
            Member {
                topics: Vec::new(),
                owned: BTreeSet::new(),
                seen: now,
            },
        );
    }
    let topic_of = |id: Uuid| {
        topics
            .iter()
            .find(|(_, topic)| topic.id == id)
            .map(|(name, _)| name.clone())
    };
    if let Some(member) = group.members.get_mut(&member_id) {
        member.seen = now;
        if let Some(names) = &request.subscribed_topic_names {
            member.topics = names.iter().map(|name| name.0.to_string()).collect();
        }
        if let Some(owned) = &request.topic_partitions {
            member.owned = owned
                .iter()
                .filter_map(|owned| {
                    topic_of(owned.topic_id).map(|name| (name, owned.partitions.clone()))
                })
                .flat_map(|(name, partitions)| {
                    partitions.into_iter().map(move |p| (name.clone(), p))
                })
                .collect();
        }
    }
    let subscribed = group
        .members
        .get(&member_id)
        .map(|m| m.topics.clone())
        .unwrap_or_default();
    let mut assigned: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    for topic_name in subscribed {
        let Some(topic) = topics.get(&topic_name) else {
            continue;
        };
        let subscribers: Vec<&String> = group
            .members
            .iter()
            .filter(|(_, member)| member.topics.contains(&topic_name))
            .map(|(id, _)| id)
            .collect();
        let Some(me) = subscribers.iter().position(|id| **id == member_id) else {
            continue;
        };
        for partition in 0..topic.partitions.len() {
            if partition % subscribers.len() != me {
                continue;
            }
            let key = (topic_name.clone(), len_i32(partition));
            let held = group
                .members
                .iter()
                .any(|(id, member)| *id != member_id && member.owned.contains(&key));
            if !held {
                assigned.entry(topic_name.clone()).or_default().push(key.1);
            }
        }
    }
    let assignment = Assignment::default().with_topic_partitions(
        assigned
            .into_iter()
            .filter_map(|(topic_name, partitions)| {
                topics.get(&topic_name).map(|topic| {
                    TopicPartitions::default()
                        .with_topic_id(topic.id)
                        .with_partitions(partitions)
                })
            })
            .collect(),
    );
    response
        .with_member_epoch(group.epoch)
        .with_assignment(Some(assignment))
}

fn offset_commit(state: &State, request: OffsetCommitRequest) -> OffsetCommitResponse {
    let mut inner = state.lock();
    let group = inner
        .groups
        .entry(request.group_id.0.to_string())
        .or_default();
    let known = group.members.contains_key(request.member_id.as_str());
    let topics = request
        .topics
        .into_iter()
        .map(|topic| {
            let topic_name = topic.name.0.to_string();
            let partitions = topic
                .partitions
                .into_iter()
                .map(|partition| {
                    let response = OffsetCommitResponsePartition::default()
                        .with_partition_index(partition.partition_index);
                    if known {
                        group.offsets.insert(
                            (topic_name.clone(), partition.partition_index),
                            partition.committed_offset,
                        );
                        response
                    } else {
                        response.with_error_code(UNKNOWN_MEMBER_ID)
                    }
                })
                .collect();
            OffsetCommitResponseTopic::default()
                .with_name(topic.name)
                .with_partitions(partitions)
        })
        .collect();
    OffsetCommitResponse::default().with_topics(topics)
}

fn offset_fetch(state: &State, request: OffsetFetchRequest) -> OffsetFetchResponse {
    let inner = state.lock();
    let groups = request
        .groups
        .into_iter()
        .map(|asked| {
            let offsets = inner
                .groups
                .get(asked.group_id.0.as_str())
                .map(|group| &group.offsets);
            let topics = asked
                .topics
                .unwrap_or_default()
                .into_iter()
                .map(|topic| {
                    let topic_name = topic.name.0.to_string();
                    let partitions = topic
                        .partition_indexes
                        .iter()
                        .map(|partition| {
                            let committed = offsets
                                .and_then(|offsets| {
                                    offsets.get(&(topic_name.clone(), *partition)).copied()
                                })
                                .unwrap_or(-1);
                            OffsetFetchResponsePartitions::default()
                                .with_partition_index(*partition)
                                .with_committed_offset(committed)
                                .with_committed_leader_epoch(-1)
                        })
                        .collect();
                    OffsetFetchResponseTopics::default()
                        .with_name(topic.name)
                        .with_partitions(partitions)
                })
                .collect();
            OffsetFetchResponseGroup::default()
                .with_group_id(asked.group_id)
                .with_topics(topics)
        })
        .collect();
    OffsetFetchResponse::default().with_groups(groups)
}
