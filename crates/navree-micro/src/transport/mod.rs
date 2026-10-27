//! [`MicroTransport`] (what a transport implements), [`Subscription`],
//! [`Delivery`] and [`Transport`] (which transport an app uses).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use navree_kernel::BoxFuture;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{DeadLetter, Envelope, MicroError};

pub(crate) mod broker;
pub(crate) mod memory;
pub(crate) mod tcp;

#[cfg(feature = "kafka")]
pub(crate) mod kafka;
#[cfg(feature = "mqtt")]
pub(crate) mod mqtt;
#[cfg(feature = "nats")]
pub(crate) mod nats;
#[cfg(feature = "rabbitmq")]
pub(crate) mod rabbitmq;
#[cfg(feature = "redis")]
pub(crate) mod redis;

pub use memory::MemoryTransport;
pub use tcp::TcpTransport;

#[cfg(feature = "kafka")]
pub use kafka::KafkaTransport;
#[cfg(feature = "mqtt")]
pub use mqtt::MqttTransport;
#[cfg(feature = "nats")]
pub use nats::NatsTransport;
#[cfg(feature = "rabbitmq")]
pub use rabbitmq::RabbitmqTransport;
#[cfg(feature = "redis")]
pub use redis::RedisTransport;

/// Moves envelopes between services. Navree hands a transport envelopes to
/// send and publish, and a channel for the deliveries it receives; the
/// pipeline, the codec, retries, dead letters and the inbox are Navree's.
///
/// ```
/// use navree_micro::{Delivery, Envelope, MicroError, MicroTransport, Subscription};
/// use navree_kernel::BoxFuture;
/// use tokio::sync::mpsc;
///
/// /// A transport that knows nobody.
/// struct Nowhere;
///
/// impl MicroTransport for Nowhere {
///     fn name(&self) -> &'static str {
///         "nowhere"
///     }
///
///     fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
///         let pattern = envelope.pattern().to_owned();
///         Box::pin(async move { Err(MicroError::NoResponders { pattern }) })
///     }
///
///     fn publish(&self, _envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>> {
///         Box::pin(async { Ok(()) })
///     }
///
///     fn listen(
///         &self,
///         _subscriptions: Vec<Subscription>,
///         deliveries: mpsc::Sender<Delivery>,
///     ) -> BoxFuture<'_, Result<(), MicroError>> {
///         // Nothing arrives; stop when Navree closes the channel.
///         Box::pin(async move {
///             deliveries.closed().await;
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait MicroTransport: Send + Sync + 'static {
    /// A short name for logs and `micro:patterns` (`tcp`, `nats`, ...).
    fn name(&self) -> &'static str;

    /// Sends a request and waits for its reply, until
    /// [`envelope.deadline()`](Envelope::deadline) (Navree also stops
    /// waiting then). Fails with [`MicroError::NoResponders`] when the
    /// transport knows nobody listens.
    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>>;

    /// Publishes an event; returns once the broker has stored it (or, for
    /// transports without storage, once it was sent).
    fn publish(&self, envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>>;

    /// Receives the subscribed patterns and hands each message over as a
    /// [`Delivery`] (whose [`subscription`](Delivery::subscription) is the
    /// index of its subscription in `subscriptions`), until the
    /// `deliveries` channel closes (`deliveries.closed()`), which is how
    /// Navree stops the listener. Messages taken but not acknowledged are
    /// left to the broker's redelivery.
    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>>;

    /// Declares the streams, topics, queues and consumers `subscriptions`
    /// need (`micro:setup`, and `micro.auto_setup` when the listener
    /// starts); only adds what is missing. Returns one line per step for
    /// the command's output. The default declares nothing.
    fn setup(
        &self,
        subscriptions: &[Subscription],
    ) -> BoxFuture<'_, Result<Vec<String>, MicroError>> {
        let _ = subscriptions;
        Box::pin(async { Ok(Vec::new()) })
    }

    /// Whether subscriptions may use the wildcards `*` and `>`. A wildcard
    /// event pattern on a transport without them fails the boot.
    fn supports_wildcards(&self) -> bool {
        true
    }

    /// Whether the transport is connected (the `micro` readiness check).
    fn is_connected(&self) -> bool {
        true
    }

    /// Whether the app starts its listener at boot rather than with
    /// `serve` or `micro:listen`. The in-memory transport does: it has
    /// nothing to connect to, so tests need no command.
    fn listens_at_boot(&self) -> bool {
        false
    }

    /// Whether failed events are retried at once rather than after their
    /// backoff (what `MicroFake` does, so tests never wait).
    fn instant_retries(&self) -> bool {
        false
    }

    /// Stores a dead letter in the broker's dead-letter destination
    /// (`micro.events.dead_letter = "transport"`). The default has none.
    fn dead_letter(&self, letter: DeadLetter) -> BoxFuture<'_, Result<(), MicroError>> {
        let name = self.name();
        drop(letter);
        Box::pin(async move {
            Err(MicroError::transport(format!(
                "the {name} transport has no dead-letter destination; use micro.events.dead_letter = \"database\" or \"log\""
            )))
        })
    }

    /// Told about every dead letter, whatever `micro.events.dead_letter`
    /// says (`MicroFake` records them). The default does nothing.
    fn observe_dead_letter(&self, letter: &DeadLetter) {
        let _ = letter;
    }

    /// Closes connections at shutdown. The default does nothing.
    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// Whether a pattern is a message (request-response) or an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatternKind {
    /// Request-response: one handler replies.
    Message,
    /// Fire-and-forget: every subscribing service gets a copy.
    Event,
}

impl PatternKind {
    /// `message` or `event`.
    pub fn as_str(self) -> &'static str {
        match self {
            PatternKind::Message => "message",
            PatternKind::Event => "event",
        }
    }
}

impl fmt::Display for PatternKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who shares a subscription.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// The instances of a service (`micro.service`): each message goes to
    /// one of them.
    Service(String),
    /// One instance only (broadcast): every instance gets every message.
    Instance(String),
}

impl Group {
    /// The service or instance name.
    pub fn name(&self) -> &str {
        match self {
            Group::Service(name) | Group::Instance(name) => name,
        }
    }
}

/// One pattern a listener receives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    /// The pattern; event patterns may end in a wildcard.
    pub pattern: String,
    /// Message or event.
    pub kind: PatternKind,
    /// Who shares it.
    pub group: Group,
    /// `ordered`: one instance at a time consumes the pattern.
    pub ordered: bool,
    /// The handler's time limit: transports with an acknowledgment
    /// timeout raise theirs to it.
    pub ack_wait: Duration,
}

impl Subscription {
    /// Whether every instance gets every message (a broadcast handler).
    pub fn is_broadcast(&self) -> bool {
        matches!(self.group, Group::Instance(_))
    }
}

/// What a transport does with a [`Delivery`] once Navree has handled it.
pub trait DeliveryActions: Send + 'static {
    /// The message was handled: remove it from the broker.
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>>;

    /// The handler failed: deliver the message again after `delay` (to
    /// any instance of the group), as attempt `attempt + 1`.
    fn retry_in(self: Box<Self>, delay: Duration) -> BoxFuture<'static, Result<(), MicroError>>;

    /// Sends `reply` to the requester of this message and acknowledges it.
    fn reply(self: Box<Self>, reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>>;
}

/// One received message: its envelope, the index of the subscription it
/// matched, the attempt number and the transport's callbacks.
pub struct Delivery {
    envelope: Envelope,
    subscription: usize,
    attempt: u32,
    actions: Box<dyn DeliveryActions>,
}

impl Delivery {
    /// A delivery of `envelope` for subscription number `subscription`
    /// (an index into the list given to [`MicroTransport::listen`]).
    pub fn new(
        envelope: Envelope,
        subscription: usize,
        attempt: u32,
        actions: impl DeliveryActions,
    ) -> Self {
        Self {
            envelope,
            subscription,
            attempt: attempt.max(1),
            actions: Box::new(actions),
        }
    }

    /// The envelope.
    pub fn envelope(&self) -> &Envelope {
        &self.envelope
    }

    /// The index of the subscription it matched.
    pub fn subscription(&self) -> usize {
        self.subscription
    }

    /// The attempt number, starting at 1.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Acknowledges the message.
    pub async fn ack(self) -> Result<(), MicroError> {
        self.actions.ack().await
    }

    /// Asks for redelivery after `delay`.
    pub async fn retry_in(self, delay: Duration) -> Result<(), MicroError> {
        self.actions.retry_in(delay).await
    }

    /// Replies (and acknowledges).
    pub async fn reply(self, reply: Envelope) -> Result<(), MicroError> {
        self.actions.reply(reply).await
    }

    /// The envelope and the callbacks, separated.
    pub(crate) fn into_parts(self) -> (Envelope, u32, Box<dyn DeliveryActions>) {
        (self.envelope, self.attempt, self.actions)
    }
}

impl fmt::Debug for Delivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Delivery")
            .field("envelope", &self.envelope)
            .field("subscription", &self.subscription)
            .field("attempt", &self.attempt)
            .finish_non_exhaustive()
    }
}

/// The callbacks of an event delivered at most once (TCP, core NATS,
/// broadcast subscriptions): acknowledging does nothing, and a retry hands
/// the same envelope back to this process's listener after the delay, as
/// the next attempt. A stopped listener drops it.
pub(crate) struct LocalRetry {
    deliveries: mpsc::Sender<Delivery>,
    envelope: Envelope,
    index: usize,
    attempt: u32,
}

impl LocalRetry {
    /// The first delivery of `envelope` for subscription `index`.
    pub(crate) fn delivery(
        deliveries: mpsc::Sender<Delivery>,
        envelope: Envelope,
        index: usize,
    ) -> Delivery {
        let actions = LocalRetry {
            deliveries,
            envelope: envelope.clone(),
            index,
            attempt: 1,
        };
        Delivery::new(envelope, index, 1, actions)
    }
}

impl DeliveryActions for LocalRetry {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async { Ok(()) })
    }

    fn retry_in(self: Box<Self>, delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        let this = *self;
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let attempt = this.attempt + 1;
            let actions = LocalRetry {
                deliveries: this.deliveries.clone(),
                envelope: this.envelope.clone(),
                index: this.index,
                attempt,
            };
            let delivery = Delivery::new(this.envelope, this.index, attempt, actions);
            let _ = this.deliveries.send(delivery).await;
        });
        Box::pin(async { Ok(()) })
    }

    fn reply(self: Box<Self>, _reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async { Ok(()) })
    }
}

/// The transports `micro.transport` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportKind {
    /// Direct TCP connections (`micro` feature).
    Tcp,
    /// NATS (`micro-nats`).
    Nats,
    /// Kafka (`micro-kafka`).
    Kafka,
    /// RabbitMQ (`micro-rabbitmq`).
    Rabbitmq,
    /// Redis Streams (`micro-redis`).
    Redis,
    /// MQTT 5 (`micro-mqtt`).
    Mqtt,
    /// In-process, for tests.
    Memory,
    /// [`Transport::custom`].
    Custom,
}

impl TransportKind {
    /// The configuration value: `tcp`, `nats`, `kafka`, `rabbitmq`,
    /// `redis`, `mqtt`, `memory` or `custom`.
    pub fn as_str(self) -> &'static str {
        match self {
            TransportKind::Tcp => "tcp",
            TransportKind::Nats => "nats",
            TransportKind::Kafka => "kafka",
            TransportKind::Rabbitmq => "rabbitmq",
            TransportKind::Redis => "redis",
            TransportKind::Mqtt => "mqtt",
            TransportKind::Memory => "memory",
            TransportKind::Custom => "custom",
        }
    }

    /// The kind named `name` in configuration.
    pub fn parse(name: &str) -> Option<TransportKind> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "tcp" => TransportKind::Tcp,
            "nats" => TransportKind::Nats,
            "kafka" => TransportKind::Kafka,
            "rabbitmq" | "amqp" => TransportKind::Rabbitmq,
            "redis" => TransportKind::Redis,
            "mqtt" => TransportKind::Mqtt,
            "memory" => TransportKind::Memory,
            _ => return None,
        })
    }

    /// The facade feature that compiles the driver in.
    pub fn feature(self) -> &'static str {
        match self {
            TransportKind::Nats => "micro-nats",
            TransportKind::Kafka => "micro-kafka",
            TransportKind::Rabbitmq => "micro-rabbitmq",
            TransportKind::Redis => "micro-redis",
            TransportKind::Mqtt => "micro-mqtt",
            TransportKind::Tcp | TransportKind::Memory | TransportKind::Custom => "micro",
        }
    }
}

impl fmt::Display for TransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which transport an app (or a named client) uses: the value of
/// `micro.transport` plus `micro.url`, or a constructor in code.
///
/// | Constructor | Transport |
/// |---|---|
/// | `Transport::tcp("127.0.0.1:4000")` | TCP, listening on the address |
/// | `Transport::nats("nats://127.0.0.1:4222")` | NATS |
/// | `Transport::kafka(["kafka-1:9092", "kafka-2:9092"])` | Kafka, with its bootstrap brokers |
/// | `Transport::rabbitmq("amqp://guest:guest@127.0.0.1:5672/%2f")` | RabbitMQ |
/// | `Transport::redis("redis://127.0.0.1:6379")` | Redis Streams |
/// | `Transport::mqtt("mqtt://127.0.0.1:1883")` | MQTT 5 |
/// | `Transport::memory()` | In-process, for tests |
/// | `Transport::custom(transport)` | Your own [`MicroTransport`] |
///
/// Clones of a `Transport::memory()` share one in-process broker, so two
/// apps given clones of it talk to each other.
#[derive(Clone, Default)]
pub struct Transport {
    kind: Option<TransportKind>,
    url: Option<String>,
    instance: Option<Arc<dyn MicroTransport>>,
}

impl Transport {
    fn of(kind: TransportKind, url: Option<String>) -> Self {
        Self {
            kind: Some(kind),
            url,
            instance: None,
        }
    }

    /// TCP: this service listens on `addr` (`micro.tcp.addr`).
    pub fn tcp(addr: impl Into<String>) -> Self {
        Self::of(TransportKind::Tcp, Some(addr.into()))
    }

    /// NATS at `url` (several servers comma-separated).
    pub fn nats(url: impl Into<String>) -> Self {
        Self::of(TransportKind::Nats, Some(url.into()))
    }

    /// Kafka with its bootstrap brokers.
    pub fn kafka<I, S>(brokers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let brokers: Vec<String> = brokers.into_iter().map(Into::into).collect();
        Self::of(TransportKind::Kafka, Some(brokers.join(",")))
    }

    /// RabbitMQ at an `amqp://` or `amqps://` URL.
    pub fn rabbitmq(url: impl Into<String>) -> Self {
        Self::of(TransportKind::Rabbitmq, Some(url.into()))
    }

    /// Redis Streams at a `redis://` or `rediss://` URL.
    pub fn redis(url: impl Into<String>) -> Self {
        Self::of(TransportKind::Redis, Some(url.into()))
    }

    /// MQTT 5 at an `mqtt://` or `mqtts://` URL.
    pub fn mqtt(url: impl Into<String>) -> Self {
        Self::of(TransportKind::Mqtt, Some(url.into()))
    }

    /// A new in-process broker: requests, events, consumer groups,
    /// broadcast and retries behave as on a real broker, without the
    /// network.
    pub fn memory() -> Self {
        Self {
            kind: Some(TransportKind::Memory),
            url: None,
            instance: Some(Arc::new(MemoryTransport::new())),
        }
    }

    /// Your own transport.
    pub fn custom(transport: impl MicroTransport) -> Self {
        Self::custom_arc(Arc::new(transport))
    }

    /// Your own transport, already shared.
    pub fn custom_arc(transport: Arc<dyn MicroTransport>) -> Self {
        Self {
            kind: Some(TransportKind::Custom),
            url: None,
            instance: Some(transport),
        }
    }

    /// The kind; `None` until one is chosen (`micro.transport` is
    /// required).
    pub fn kind(&self) -> Option<TransportKind> {
        self.kind
    }

    /// The URL given to the constructor (the TCP listen address for
    /// `tcp`), if any.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The transport instance of `memory` and `custom`.
    pub(crate) fn instance(&self) -> Option<&Arc<dyn MicroTransport>> {
        self.instance.as_ref()
    }
}

impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transport")
            .field("kind", &self.kind)
            .field("url", &self.url.as_deref().map(crate::options::redact_url))
            .finish_non_exhaustive()
    }
}

/// `micro.transport` is a name: `tcp`, `nats`, `kafka`, `rabbitmq`,
/// `redis`, `mqtt` or `memory`.
impl<'de> Deserialize<'de> for Transport {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        match TransportKind::parse(&name) {
            Some(TransportKind::Memory) => Ok(Transport::memory()),
            Some(kind) => Ok(Transport::of(kind, None)),
            None => Err(de::Error::custom(format!(
                "unknown transport `{name}`: expected tcp, nats, kafka, rabbitmq, redis, mqtt or memory"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_kind_and_url() {
        assert_eq!(
            Transport::tcp("127.0.0.1:4000").kind(),
            Some(TransportKind::Tcp)
        );
        assert_eq!(
            Transport::kafka(["kafka-1:9092", "kafka-2:9092"]).url(),
            Some("kafka-1:9092,kafka-2:9092")
        );
        assert_eq!(
            Transport::nats("nats://x:4222").url(),
            Some("nats://x:4222")
        );
        assert_eq!(
            Transport::rabbitmq("amqp://x").kind(),
            Some(TransportKind::Rabbitmq)
        );
        assert_eq!(
            Transport::redis("redis://x").kind(),
            Some(TransportKind::Redis)
        );
        assert_eq!(
            Transport::mqtt("mqtt://x").kind(),
            Some(TransportKind::Mqtt)
        );
        assert!(Transport::memory().instance().is_some());
        assert_eq!(Transport::default().kind(), None);
    }

    #[test]
    fn deserializes_from_a_name() {
        let transport: Transport = serde_json::from_value(serde_json::json!("NATS")).unwrap();
        assert_eq!(transport.kind(), Some(TransportKind::Nats));
        assert_eq!(transport.url(), None);
        let memory: Transport = serde_json::from_value(serde_json::json!("memory")).unwrap();
        assert!(memory.instance().is_some());
        let error = serde_json::from_value::<Transport>(serde_json::json!("sqs")).unwrap_err();
        assert!(
            error.to_string().contains("unknown transport `sqs`"),
            "{error}"
        );
    }

    #[test]
    fn kinds_name_their_features() {
        assert_eq!(TransportKind::Nats.feature(), "micro-nats");
        assert_eq!(TransportKind::Tcp.feature(), "micro");
        assert_eq!(TransportKind::parse("amqp"), Some(TransportKind::Rabbitmq));
        assert_eq!(TransportKind::Rabbitmq.to_string(), "rabbitmq");
    }
}
