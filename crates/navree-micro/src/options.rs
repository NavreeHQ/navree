//! [`MicroOptions`]: the `micro` configuration section.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use navree_config::{ConfigSection, ConfigTable};
use navree_contracts::{Backoff, Secret};
use serde::Deserialize;

use crate::{Codec, Transport, TransportKind};

/// Options of `MicroModule`: the `micro` configuration section.
///
/// ```toml
/// [micro]
/// service = "deals"                  # this service's name
/// transport = "nats"                 # tcp, nats, kafka, rabbitmq, redis, mqtt or memory
/// url = "nats://127.0.0.1:4222"
/// codec = "json"                     # or "msgpack"
/// request_timeout_ms = 5000          # the default deadline of client.send
/// connect_timeout_ms = 5000
/// concurrency = 64                   # messages one process handles at once
/// listen_with_serve = true
/// max_message_bytes = 1048576
/// forward_auth = true
/// outbox = true                      # with DatabaseModule
/// inbox = true                       # with DatabaseModule
/// inbox_retention_hours = 72
/// outbox_poll_ms = 1000
/// # auto_setup = true                # default: debug builds
///
/// [micro.events]
/// max_attempts = 5
/// backoff = { type = "exponential", base_secs = 1, max_secs = 300 }
/// timeout_secs = 120
/// # dead_letter = "database"         # default: database with DatabaseModule, else transport
/// ```
///
/// As environment variables: `APP_MICRO__SERVICE=deals`,
/// `APP_MICRO__TRANSPORT=nats`, `APP_MICRO__URL=nats://nats.internal:4222`.
///
/// `Debug` never shows a credential: URL passwords are masked, and the
/// NATS token, the Kafka SASL password and the MQTT password are
/// [`Secret`]s.
#[derive(Clone, Deserialize, ConfigSection)]
#[config(key = "micro", docs = "/docs/microservices/introduction/", validate = Self::check_values)]
#[serde(default)]
pub struct MicroOptions {
    /// The service's name: its consumer group, its client id and the
    /// `navree-service` header of what it sends. Empty: `app.name`, else
    /// the binary's name.
    pub service: String,
    /// The transport (required): `tcp`, `nats`, `kafka`, `rabbitmq`, `redis`,
    /// `mqtt` or `memory`.
    #[config(one_of = "tcp, nats, kafka, rabbitmq, redis, mqtt, memory")]
    #[config(default = "")]
    pub transport: Transport,
    /// Where the broker is; the transport's local default when unset.
    pub url: Option<String>,
    /// How payloads are encoded.
    #[config(one_of = "json, msgpack")]
    pub codec: Codec,
    /// The deadline of a `send` that sets none.
    pub request_timeout_ms: u64,
    /// How long one connection attempt to the broker may take.
    pub connect_timeout_ms: u64,
    /// Messages one process handles at once, over every pattern.
    pub concurrency: usize,
    /// Whether `serve` starts the listener too.
    pub listen_with_serve: bool,
    /// The largest encoded payload, sent or received.
    pub max_message_bytes: usize,
    /// Forward the current request's `Authorization` header on `send`.
    pub forward_auth: bool,
    /// Publish events emitted inside a unit of work through the outbox
    /// (with `DatabaseModule`).
    pub outbox: bool,
    /// Skip messages already handled, by message id (with
    /// `DatabaseModule`).
    pub inbox: bool,
    /// How long handled message ids (and `dedupe` replies) are kept.
    pub inbox_retention_hours: u64,
    /// How often the outbox relay looks for events a crashed process left
    /// behind.
    pub outbox_poll_ms: u64,
    /// Declare the streams, topics and queues the app needs when the
    /// listener starts. Default: on in debug builds.
    pub auto_setup: bool,
    /// Retries and dead letters of events.
    #[config(table)]
    pub events: EventsOptions,
    /// TCP settings.
    #[config(table)]
    pub tcp: TcpOptions,
    /// NATS settings.
    #[config(table)]
    pub nats: NatsOptions,
    /// Kafka settings.
    #[config(table)]
    pub kafka: KafkaOptions,
    /// RabbitMQ settings.
    #[config(table)]
    pub rabbitmq: RabbitmqOptions,
    /// Redis Streams settings.
    #[config(table)]
    pub redis: RedisOptions,
    /// MQTT settings.
    #[config(table)]
    pub mqtt: MqttOptions,
    /// Named clients for other transports (`micro.clients.<name>`), which
    /// only send.
    pub clients: BTreeMap<String, ClientOptions>,
}

impl Default for MicroOptions {
    fn default() -> Self {
        Self {
            service: String::new(),
            transport: Transport::default(),
            url: None,
            codec: Codec::Json,
            request_timeout_ms: 5_000,
            connect_timeout_ms: 5_000,
            concurrency: 64,
            listen_with_serve: true,
            max_message_bytes: 1_048_576,
            forward_auth: true,
            outbox: true,
            inbox: true,
            inbox_retention_hours: 72,
            outbox_poll_ms: 1_000,
            auto_setup: cfg!(debug_assertions),
            events: EventsOptions::default(),
            tcp: TcpOptions::default(),
            nats: NatsOptions::default(),
            kafka: KafkaOptions::default(),
            rabbitmq: RabbitmqOptions::default(),
            redis: RedisOptions::default(),
            mqtt: MqttOptions::default(),
            clients: BTreeMap::new(),
        }
    }
}

/// The URL's password is masked.
impl fmt::Debug for MicroOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MicroOptions")
            .field("service", &self.service)
            .field("transport", &self.transport)
            .field("url", &self.url.as_deref().map(redact_url))
            .field("codec", &self.codec)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field("connect_timeout_ms", &self.connect_timeout_ms)
            .field("concurrency", &self.concurrency)
            .field("listen_with_serve", &self.listen_with_serve)
            .field("max_message_bytes", &self.max_message_bytes)
            .field("forward_auth", &self.forward_auth)
            .field("outbox", &self.outbox)
            .field("inbox", &self.inbox)
            .field("inbox_retention_hours", &self.inbox_retention_hours)
            .field("outbox_poll_ms", &self.outbox_poll_ms)
            .field("auto_setup", &self.auto_setup)
            .field("events", &self.events)
            .field("tcp", &self.tcp)
            .field("nats", &self.nats)
            .field("kafka", &self.kafka)
            .field("rabbitmq", &self.rabbitmq)
            .field("redis", &self.redis)
            .field("mqtt", &self.mqtt)
            .field("clients", &self.clients)
            .finish()
    }
}

/// `MicroModule::for_root(Transport::nats(..))`: the transport alone.
impl From<Transport> for MicroOptions {
    fn from(transport: Transport) -> Self {
        MicroOptions {
            transport,
            ..MicroOptions::default()
        }
    }
}

impl MicroOptions {
    /// [`check`](MicroOptions::check) with the problems joined (the boot
    /// calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }

    /// Every problem with these options, each naming its key.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        match self.transport.kind() {
            None => errors.push(
                "micro.transport is required: tcp, nats, kafka, rabbitmq, redis, mqtt or memory"
                    .to_owned(),
            ),
            Some(kind) => {
                if let Err(error) = check_compiled(kind, "micro.transport") {
                    errors.push(error);
                }
            }
        }
        if self.request_timeout_ms == 0 {
            errors.push("micro.request_timeout_ms must be greater than 0".to_owned());
        }
        if self.connect_timeout_ms == 0 {
            errors.push("micro.connect_timeout_ms must be greater than 0".to_owned());
        }
        if self.concurrency == 0 {
            errors.push("micro.concurrency must be greater than 0".to_owned());
        }
        if self.max_message_bytes == 0 {
            errors.push("micro.max_message_bytes must be greater than 0".to_owned());
        }
        if self.outbox_poll_ms == 0 {
            errors.push("micro.outbox_poll_ms must be greater than 0".to_owned());
        }
        if self.inbox_retention_hours == 0 {
            errors.push("micro.inbox_retention_hours must be greater than 0".to_owned());
        }
        if !self.service.is_empty() && !valid_service(&self.service) {
            errors.push(format!(
                "micro.service `{}` may only hold letters, digits, `_` and `-`",
                self.service
            ));
        }
        if self.events.max_attempts == 0 {
            errors.push("micro.events.max_attempts must be at least 1".to_owned());
        }
        if self.events.timeout_secs == 0 {
            errors.push("micro.events.timeout_secs must be greater than 0".to_owned());
        }
        if self.tcp.pool_size == 0 {
            errors.push("micro.tcp.pool_size must be at least 1".to_owned());
        }
        if self.tcp.tls.cert.is_some() != self.tcp.tls.key.is_some() {
            errors.push("micro.tcp.tls.cert and micro.tcp.tls.key go together".to_owned());
        }
        for (prefix, peer) in &self.tcp.routes {
            if prefix.is_empty() || peer.is_empty() {
                errors.push(format!(
                    "micro.tcp.routes: `{prefix}` = `{peer}` needs a pattern prefix and a host:port"
                ));
            }
        }
        if !matches!(self.kafka.start_from.as_str(), "latest" | "earliest") {
            errors.push(format!(
                "micro.kafka.start_from must be `latest` or `earliest`, not `{}`",
                self.kafka.start_from
            ));
        }
        if let Some(sasl) = &self.kafka.sasl
            && !matches!(
                sasl.mechanism.as_str(),
                "PLAIN" | "SCRAM-SHA-256" | "SCRAM-SHA-512"
            )
        {
            errors.push(format!(
                "micro.kafka.sasl.mechanism must be PLAIN, SCRAM-SHA-256 or SCRAM-SHA-512, not `{}`",
                sasl.mechanism
            ));
        }
        if !matches!(self.rabbitmq.queue_type.as_str(), "quorum" | "classic") {
            errors.push(format!(
                "micro.rabbitmq.queue_type must be `quorum` or `classic`, not `{}`",
                self.rabbitmq.queue_type
            ));
        }
        if self.rabbitmq.prefetch == 0 {
            errors.push("micro.rabbitmq.prefetch must be at least 1".to_owned());
        }
        if self.redis.max_len == 0 {
            errors.push("micro.redis.max_len must be at least 1".to_owned());
        }
        for (name, client) in &self.clients {
            if client.transport.kind().is_none() {
                errors.push(format!("micro.clients.{name}.transport is required"));
            } else if let Some(kind) = client.transport.kind()
                && let Err(error) = check_compiled(kind, &format!("micro.clients.{name}.transport"))
            {
                errors.push(error);
            }
            if client.request_timeout_ms == Some(0) {
                errors.push(format!(
                    "micro.clients.{name}.request_timeout_ms must be greater than 0"
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The URL the transport connects to: the constructor's, else
    /// `micro.url`.
    pub(crate) fn transport_url(&self) -> Option<String> {
        self.transport
            .url()
            .map(str::to_owned)
            .or_else(|| self.url.clone())
    }
}

/// A transport whose driver is behind a feature that is off fails the boot,
/// naming the feature.
fn check_compiled(kind: TransportKind, key: &str) -> Result<(), String> {
    let compiled = match kind {
        TransportKind::Tcp | TransportKind::Memory | TransportKind::Custom => true,
        TransportKind::Nats => cfg!(feature = "nats"),
        TransportKind::Kafka => cfg!(feature = "kafka"),
        TransportKind::Rabbitmq => cfg!(feature = "rabbitmq"),
        TransportKind::Redis => cfg!(feature = "redis"),
        TransportKind::Mqtt => cfg!(feature = "mqtt"),
    };
    if compiled {
        Ok(())
    } else {
        Err(format!(
            "{key} = \"{kind}\" needs the `{}` feature of navree",
            kind.feature()
        ))
    }
}

/// A service name: letters, digits, `_` and `-`.
pub(crate) fn valid_service(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Where exhausted events go (`micro.events.dead_letter`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeadLetterMode {
    /// A row in `micro_dead_letters` (needs `DatabaseModule`).
    Database,
    /// The broker's dead-letter destination.
    Transport,
    /// An `error` log line on target `navree::micro`; the event is dropped.
    Log,
}

/// Retries and dead letters of events (`micro.events`).
#[derive(Clone, Debug, Deserialize, ConfigTable)]
#[serde(default)]
pub struct EventsOptions {
    /// Attempts before an event is dead-lettered.
    pub max_attempts: u32,
    /// The delay before each retry, the same `Backoff` jobs use.
    pub backoff: Backoff,
    /// The time limit of one attempt.
    pub timeout_secs: u64,
    /// Where exhausted events go; default: `database` with
    /// `DatabaseModule`, else `transport`.
    #[config(one_of = "database, transport, log")]
    pub dead_letter: Option<DeadLetterMode>,
}

impl Default for EventsOptions {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            backoff: Backoff::default(),
            timeout_secs: 120,
            dead_letter: None,
        }
    }
}

/// TLS files (`.tls.ca`, `.tls.cert`, `.tls.key`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct TlsOptions {
    /// A CA certificate (PEM).
    pub ca: Option<PathBuf>,
    /// This service's certificate chain (PEM).
    pub cert: Option<PathBuf>,
    /// This service's private key (PEM).
    pub key: Option<PathBuf>,
}

/// TCP settings (`micro.tcp`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct TcpOptions {
    /// Where this service listens.
    pub addr: String,
    /// Pattern prefix (`deals.*`, or a full pattern) to `host:port`; the
    /// longest matching prefix wins.
    pub routes: BTreeMap<String, String>,
    /// Connections kept open to each peer.
    pub pool_size: usize,
    /// `cert` and `key` turn on TLS; `ca` turns on mutual TLS.
    #[config(table)]
    pub tls: TlsOptions,
}

impl Default for TcpOptions {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:4000".to_owned(),
            routes: BTreeMap::new(),
            pool_size: 4,
            tls: TlsOptions::default(),
        }
    }
}

/// NATS settings (`micro.nats`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct NatsOptions {
    /// Events through JetStream (at least once); `false` uses core
    /// publish-subscribe (at most once).
    pub jetstream: bool,
    /// The stream that stores events.
    pub stream: String,
    /// Stream replicas.
    pub replicas: usize,
    /// How long the stream keeps events.
    pub max_age_hours: u64,
    /// How long an unacknowledged event waits before redelivery (raised
    /// to the handler's time limit).
    pub ack_wait_secs: u64,
    /// A `.creds` file (JWT and NKey). Not supported: NKey signing needs
    /// ed25519-dalek, which the crypto policy keeps out, so a value here
    /// fails at connect. Use `token`, a user and password in the URL, or
    /// TLS client certificates.
    #[config(public)]
    pub credentials: Option<PathBuf>,
    /// A token, for servers with token authentication
    /// (`APP_MICRO__NATS__TOKEN`). Never shown in `Debug`.
    pub token: Option<Secret<String>>,
    /// TLS files.
    #[config(table)]
    pub tls: TlsOptions,
}

impl Default for NatsOptions {
    fn default() -> Self {
        Self {
            jetstream: true,
            stream: "NAVREE".to_owned(),
            replicas: 1,
            max_age_hours: 168,
            ack_wait_secs: 30,
            credentials: None,
            token: None,
            tls: TlsOptions::default(),
        }
    }
}

/// SASL credentials (`micro.kafka.sasl`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
pub struct SaslOptions {
    /// `PLAIN`, `SCRAM-SHA-256` or `SCRAM-SHA-512`.
    #[config(one_of = "PLAIN, SCRAM-SHA-256, SCRAM-SHA-512")]
    pub mechanism: String,
    /// The user name.
    #[serde(default)]
    pub username: String,
    /// The password (`APP_MICRO__KAFKA__SASL__PASSWORD`). Never shown in
    /// `Debug`.
    #[serde(default)]
    pub password: Secret<String>,
}

/// Kafka settings (`micro.kafka`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct KafkaOptions {
    /// Partitions of the topics `micro:setup` creates.
    pub partitions: i32,
    /// Replicas of those topics.
    pub replication_factor: i16,
    /// How long those topics keep records.
    pub retention_hours: u64,
    /// Connect with TLS, trusting the public authorities.
    pub tls: bool,
    /// TLS with files of your own: `ca`, an authority to trust instead of the
    /// public ones (a broker whose certificate a private authority signed), and
    /// `cert` and `key`, for a broker that asks for a client certificate.
    /// Setting `ca` or `cert` connects with TLS.
    #[config(table)]
    pub tls_files: TlsOptions,
    /// SASL credentials.
    #[config(table)]
    pub sasl: Option<SaslOptions>,
    /// Where a new consumer group starts: `latest` or `earliest`.
    #[config(one_of = "latest, earliest")]
    pub start_from: String,
}

impl Default for KafkaOptions {
    fn default() -> Self {
        Self {
            partitions: 6,
            replication_factor: 1,
            retention_hours: 168,
            tls: false,
            tls_files: TlsOptions::default(),
            sasl: None,
            start_from: "latest".to_owned(),
        }
    }
}

/// RabbitMQ settings (`micro.rabbitmq`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct RabbitmqOptions {
    /// The topic exchange.
    pub exchange: String,
    /// `quorum` (replicated) or `classic`.
    #[config(one_of = "quorum, classic")]
    pub queue_type: String,
    /// Unacknowledged messages one instance holds per queue.
    pub prefetch: u16,
    /// TLS files (`ca` for `amqps://` URLs signed by a private CA).
    #[config(table)]
    pub tls: TlsOptions,
}

impl Default for RabbitmqOptions {
    fn default() -> Self {
        Self {
            exchange: "navree".to_owned(),
            queue_type: "quorum".to_owned(),
            prefetch: 32,
            tls: TlsOptions::default(),
        }
    }
}

/// Redis Streams settings (`micro.redis`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct RedisOptions {
    /// The prefix of every stream key.
    pub prefix: String,
    /// About how many entries each stream keeps.
    pub max_len: usize,
    /// When another instance takes over an unacknowledged entry (raised to
    /// the handler's time limit).
    pub claim_after_ms: u64,
    /// How long one read waits for new entries.
    pub block_ms: u64,
}

impl Default for RedisOptions {
    fn default() -> Self {
        Self {
            prefix: "navree:".to_owned(),
            max_len: 100_000,
            claim_after_ms: 60_000,
            block_ms: 5_000,
        }
    }
}

/// MQTT settings (`micro.mqtt`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct MqttOptions {
    /// The topic prefix.
    pub prefix: String,
    /// The client id; `<service>-<instance>` when unset.
    pub client_id: Option<String>,
    /// The keep-alive interval.
    pub keep_alive_secs: u64,
    /// How long the broker keeps the session of a disconnected instance.
    pub session_expiry_secs: u32,
    /// The user name.
    pub username: Option<String>,
    /// The password (`APP_MICRO__MQTT__PASSWORD`). Never shown in `Debug`.
    pub password: Option<Secret<String>>,
    /// TLS files.
    #[config(table)]
    pub tls: TlsOptions,
}

impl Default for MqttOptions {
    fn default() -> Self {
        Self {
            prefix: "navree".to_owned(),
            client_id: None,
            keep_alive_secs: 30,
            session_expiry_secs: 3_600,
            username: None,
            password: None,
            tls: TlsOptions::default(),
        }
    }
}

/// A named client (`micro.clients.<name>`): the keys of the `micro` section
/// a client uses. Unset keys take the app's values. `Debug` masks the URL's
/// password.
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct ClientOptions {
    /// The transport (required).
    pub transport: Transport,
    /// Where the broker (or, for TCP, the peer) is.
    pub url: Option<String>,
    /// The codec.
    pub codec: Option<Codec>,
    /// The default deadline of `send`.
    pub request_timeout_ms: Option<u64>,
    /// TCP settings.
    pub tcp: Option<TcpOptions>,
    /// NATS settings.
    pub nats: Option<NatsOptions>,
    /// Kafka settings.
    pub kafka: Option<KafkaOptions>,
    /// RabbitMQ settings.
    pub rabbitmq: Option<RabbitmqOptions>,
    /// Redis Streams settings.
    pub redis: Option<RedisOptions>,
    /// MQTT settings.
    pub mqtt: Option<MqttOptions>,
}

impl fmt::Debug for ClientOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientOptions")
            .field("transport", &self.transport)
            .field("url", &self.url.as_deref().map(redact_url))
            .field("codec", &self.codec)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field("tcp", &self.tcp)
            .field("nats", &self.nats)
            .field("kafka", &self.kafka)
            .field("rabbitmq", &self.rabbitmq)
            .field("redis", &self.redis)
            .field("mqtt", &self.mqtt)
            .finish()
    }
}

/// A URL with its password masked, for logs.
pub(crate) fn redact_url(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(scheme), Some(at)) if at > scheme => {
            let credentials = &url[scheme + 3..at];
            match credentials.split_once(':') {
                Some((user, _)) => format!("{}{user}:***{}", &url[..scheme + 3], &url[at..]),
                None => url.to_owned(),
            }
        }
        _ => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;
    use serde_json::json;

    use super::*;

    fn parse(value: serde_json::Value) -> MicroOptions {
        serde_json::from_value(value).expect("options parse")
    }

    #[test]
    fn defaults_match_the_documented_table() {
        let options = MicroOptions::default();
        assert_eq!(options.codec, Codec::Json);
        assert_eq!(options.request_timeout_ms, 5_000);
        assert_eq!(options.connect_timeout_ms, 5_000);
        assert_eq!(options.concurrency, 64);
        assert!(options.listen_with_serve && options.forward_auth);
        assert!(options.outbox && options.inbox);
        assert_eq!(options.max_message_bytes, 1_048_576);
        assert_eq!(options.inbox_retention_hours, 72);
        assert_eq!(options.outbox_poll_ms, 1_000);
        assert_eq!(options.auto_setup, cfg!(debug_assertions));
        assert_eq!(options.events.max_attempts, 5);
        assert_eq!(options.events.timeout_secs, 120);
        assert_eq!(
            options.events.backoff,
            Backoff::Exponential {
                base_secs: 1,
                max_secs: 300
            }
        );
        assert_eq!(options.tcp.addr, "127.0.0.1:4000");
        assert_eq!(options.tcp.pool_size, 4);
        assert_eq!(options.nats.stream, "NAVREE");
        assert_eq!(options.nats.ack_wait_secs, 30);
        assert_eq!(options.kafka.partitions, 6);
        assert_eq!(options.kafka.start_from, "latest");
        assert_eq!(options.rabbitmq.exchange, "navree");
        assert_eq!(options.rabbitmq.queue_type, "quorum");
        assert_eq!(options.rabbitmq.prefetch, 32);
        assert_eq!(options.redis.prefix, "navree:");
        assert_eq!(options.redis.max_len, 100_000);
        assert_eq!(options.redis.claim_after_ms, 60_000);
        assert_eq!(options.mqtt.prefix, "navree");
        assert_eq!(options.mqtt.session_expiry_secs, 3_600);
    }

    #[test]
    fn the_transport_is_required() {
        let errors = MicroOptions::default().check().unwrap_err();
        assert!(
            errors[0].starts_with("micro.transport is required"),
            "{errors:?}"
        );
        assert!(MicroOptions::from(Transport::memory()).check().is_ok());
    }

    #[test]
    fn parses_the_documented_section() {
        let options = parse(json!({
            "service": "deals",
            "transport": "tcp",
            "codec": "msgpack",
            "request_timeout_ms": 2000,
            "tcp": {
                "addr": "0.0.0.0:4000",
                "routes": { "deals.*": "deals.internal:4000" },
            },
            "events": {
                "max_attempts": 10,
                "backoff": { "type": "fixed", "secs": 2 },
                "dead_letter": "log",
            },
            "clients": { "billing": { "transport": "tcp", "url": "billing.internal:4000", "request_timeout_ms": 2000 } },
        }));
        assert_eq!(options.service, "deals");
        assert_eq!(options.transport.kind(), Some(TransportKind::Tcp));
        assert_eq!(options.codec, Codec::Msgpack);
        assert_eq!(options.tcp.routes["deals.*"], "deals.internal:4000");
        assert_eq!(options.events.backoff, Backoff::Fixed { secs: 2 });
        assert_eq!(options.events.dead_letter, Some(DeadLetterMode::Log));
        assert_eq!(
            options.clients["billing"].url.as_deref(),
            Some("billing.internal:4000")
        );
        assert!(options.check().is_ok(), "{:?}", options.check());
    }

    #[test]
    fn errors_name_their_keys() {
        let mut options = MicroOptions::from(Transport::memory());
        options.request_timeout_ms = 0;
        options.concurrency = 0;
        options.service = "deals service".into();
        options.events.max_attempts = 0;
        options.tcp.pool_size = 0;
        options.kafka.start_from = "middle".into();
        options.rabbitmq.queue_type = "stream".into();
        options
            .clients
            .insert("billing".into(), ClientOptions::default());
        let errors = options.check().unwrap_err().join("\n");
        for key in [
            "micro.request_timeout_ms",
            "micro.concurrency",
            "micro.service",
            "micro.events.max_attempts",
            "micro.tcp.pool_size",
            "micro.kafka.start_from",
            "micro.rabbitmq.queue_type",
            "micro.clients.billing.transport",
        ] {
            assert!(errors.contains(key), "{key} missing from:\n{errors}");
        }
    }

    #[cfg(not(feature = "nats"))]
    #[test]
    fn a_transport_without_its_feature_names_the_feature() {
        let errors = MicroOptions::from(Transport::nats("nats://x"))
            .check()
            .unwrap_err();
        assert_eq!(
            errors,
            ["micro.transport = \"nats\" needs the `micro-nats` feature of navree"]
        );
    }

    #[test]
    fn debug_never_shows_credentials() {
        let options = parse(json!({
            "transport": "nats",
            "url": "amqps://deals:url-secret@rabbitmq:5671/crm",
            "nats": { "token": "nats-token-secret" },
            "kafka": {
                "sasl": { "mechanism": "PLAIN", "username": "deals", "password": "kafka-sasl-secret" },
            },
            "mqtt": { "username": "deals", "password": "mqtt-password-secret" },
            "clients": {
                "billing": {
                    "transport": "mqtt",
                    "url": "mqtts://billing:client-url-secret@mqtt.internal:8883",
                    "mqtt": { "username": "billing", "password": "client-mqtt-secret" },
                },
            },
        }));
        let printed = format!("{options:?}");
        for secret in [
            "url-secret",
            "nats-token-secret",
            "kafka-sasl-secret",
            "mqtt-password-secret",
            "client-url-secret",
            "client-mqtt-secret",
        ] {
            assert!(!printed.contains(secret), "{secret} in {printed}");
        }
        assert!(
            printed.contains("amqps://deals:***@rabbitmq:5671/crm"),
            "{printed}"
        );
        assert!(printed.contains("[redacted]"), "{printed}");

        // The transports still read the values.
        let token = options
            .nats
            .token
            .as_ref()
            .map(|token| token.expose().as_str());
        assert_eq!(token, Some("nats-token-secret"));
        let sasl = options.kafka.sasl.as_ref().unwrap();
        assert_eq!(sasl.password.expose(), "kafka-sasl-secret");
        let password = options.mqtt.password.as_ref().map(|p| p.expose().as_str());
        assert_eq!(password, Some("mqtt-password-secret"));
    }

    #[test]
    fn urls_are_redacted_for_logs() {
        assert_eq!(
            redact_url("amqps://deals:secret@rabbitmq:5671/crm"),
            "amqps://deals:***@rabbitmq:5671/crm"
        );
        assert_eq!(redact_url("nats://127.0.0.1:4222"), "nats://127.0.0.1:4222");
        assert_eq!(redact_url("redis://token@host"), "redis://token@host");
    }

    #[test]
    fn the_boot_check_joins_the_problems() {
        let error = MicroOptions::default().validate().unwrap_err();
        assert!(error.starts_with("micro.transport is required"), "{error}");
        assert!(MicroOptions::from(Transport::memory()).validate().is_ok());
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = MicroOptions::schema().expect("schema");
        assert_eq!(schema.key, "micro");
        assert_eq!(
            schema.docs.as_deref(),
            Some("/docs/microservices/introduction/")
        );
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        // The NATS token, the Kafka SASL password and the MQTT password are
        // the credentials, wherever they are nested.
        let mut secrets = schema.secret_paths();
        secrets.sort();
        assert_eq!(
            secrets,
            [
                "micro.kafka.sasl.password",
                "micro.mqtt.password",
                "micro.nats.token",
            ]
        );
        let table = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("no field {name}"))
        };
        assert_eq!(
            table("transport").ty,
            FieldType::choice(&[
                "tcp", "nats", "kafka", "rabbitmq", "redis", "mqtt", "memory"
            ])
        );
        assert_eq!(table("codec").ty, FieldType::choice(&["json", "msgpack"]));
        for name in [
            "events", "tcp", "nats", "kafka", "rabbitmq", "redis", "mqtt",
        ] {
            assert_eq!(table(name).ty, FieldType::Table, "{name}");
        }
        assert_eq!(
            table("clients").ty,
            FieldType::map(FieldType::other("ClientOptions"))
        );
        let sasl = table("kafka")
            .fields
            .iter()
            .find(|field| field.name == "sasl")
            .expect("kafka.sasl");
        assert_eq!(sasl.ty, FieldType::Table);
        assert!(
            sasl.fields
                .iter()
                .any(|field| field.name == "password" && field.secret)
        );
        // A path to a credentials file is not a credential.
        let credentials = table("nats")
            .fields
            .iter()
            .find(|field| field.name == "credentials")
            .expect("nats.credentials");
        assert!(credentials.public && !credentials.secret);
        let defaults = MicroOptions::defaults().expect("defaults");
        assert_eq!(defaults["codec"], "json");
        assert_eq!(defaults["concurrency"], 64);
        assert_eq!(defaults["tcp"]["addr"], "127.0.0.1:4000");
        assert_eq!(defaults["nats"]["jetstream"], true);
        assert!(defaults["nats"]["token"].is_null());
        assert!(defaults["kafka"]["sasl"].is_null());
        assert!(defaults["events"]["dead_letter"].is_null());
    }
}
