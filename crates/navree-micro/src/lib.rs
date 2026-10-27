//! # navree-micro
//!
//! `MicroModule`: messages and events between services, written the way
//! HTTP routes are.
//!
//! - **Request-response.** A `#[message_pattern("deals.get")]` handler
//!   (see `#[message_handlers]` in the `navree` facade, or a hand-written
//!   [`MessageHandlers`] impl) answers [`ServiceClient::send`]. The
//!   deadline travels with the message ([`headers::DEADLINE`]); errors come
//!   back in Navree's JSON error shape as [`MicroError::Remote`].
//! - **Events.** `#[event_pattern("deal.won")]` handlers receive what
//!   [`ServiceClient::emit`] publishes: one copy per subscribing service,
//!   one instance of each handles it, with retries ([`Backoff`](navree_contracts::Backoff)),
//!   dead letters ([`DeadLetters`]), per-key order, a transactional outbox
//!   and an inbox that skips repeated message ids.
//! - **One pipeline.** A message reaches its handler as request parts, so
//!   authentication (`dyn Authenticator`), guards, access declarations
//!   (`dyn RouteAuthorizer`, with `can(.., from = "id")` reading the
//!   payload), [`Payload`], `Valid<Payload<T>>`, `Uow` and every other
//!   extractor work as on routes, and the unit of work commits when the
//!   handler returns `Ok`.
//! - **Transports.** [`Transport::tcp`] and [`Transport::memory`] are built
//!   in; NATS, Kafka, RabbitMQ, Redis Streams and MQTT are behind the
//!   features `nats`, `kafka`, `rabbitmq`, `redis` and `mqtt` (facade:
//!   `micro-nats`, ...). Write your own with [`MicroTransport`].
//! - **Context.** The request id, the caller's `Authorization`, the
//!   workspace (`x-workspace`) and the W3C trace context follow every
//!   message: a handler runs in a navree-telemetry `TelemetryContext` that
//!   continues the sender's trace, and what it sends continues it again.
//! - **Tests.** [`MicroFake`] stubs other services ([`Reply`]), records
//!   what the app sends and emits, and sends into the app's handlers.
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_kernel::Navree;
//! use navree_micro::{MicroModule, Transport};
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(MicroModule::for_root(Transport::tcp("127.0.0.1:4000")))
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

mod client;
mod codec;
mod context;
mod envelope;
mod error;
mod fake;
mod handlers;
mod listener;
mod module;
mod options;
mod pattern;
mod payload;
mod pipeline;
mod runtime;
mod store;
mod transport;

pub mod conformance;

pub use client::{EmitBuilder, SendBuilder, ServiceClient};
pub use codec::Codec;
pub use context::MessageContext;
pub use envelope::{Envelope, headers};
pub use error::{MicroError, RemoteError};
pub use fake::{FakeClient, FakeEmit, MicroFake, Reply};
pub use handlers::{
    MessageHandlers, MessageReply, ModuleBuilderMicroExt, PatternDef, PatternMeta, ReplyCodec,
};
pub use module::{ConfiguredMicroModule, DeadLetters, MicroModule};
pub use options::{
    ClientOptions, DeadLetterMode, EventsOptions, KafkaOptions, MicroOptions, MqttOptions,
    NatsOptions, RabbitmqOptions, RedisOptions, SaslOptions, TcpOptions, TlsOptions,
};
pub use pattern::{check_pattern, has_wildcard, pattern_matches};
pub use payload::Payload;
pub use store::{DEAD_LETTERS_TABLE, DeadLetter, INBOX_TABLE, OUTBOX_TABLE};
pub use transport::{
    Delivery, DeliveryActions, Group, MemoryTransport, MicroTransport, PatternKind, Subscription,
    TcpTransport, Transport, TransportKind,
};

#[cfg(feature = "kafka")]
pub use transport::KafkaTransport;
#[cfg(feature = "mqtt")]
pub use transport::MqttTransport;
#[cfg(feature = "nats")]
pub use transport::NatsTransport;
#[cfg(feature = "rabbitmq")]
pub use transport::RabbitmqTransport;
#[cfg(feature = "redis")]
pub use transport::RedisTransport;
