//! [`NatsTransport`] (`nats` feature): core request-reply, and JetStream for
//! durable events.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, AckKind, consumer, stream};
use async_nats::{ConnectOptions, HeaderMap, RequestErrorKind};
use bytes::Bytes;
use futures_util::StreamExt;
use navree_kernel::BoxFuture;
use tokio::sync::{OnceCell, mpsc};
use tokio::task::JoinSet;

use crate::envelope::headers;
use crate::options::NatsOptions;
use crate::transport::LocalRetry;
use crate::{
    DeadLetter, Delivery, DeliveryActions, Envelope, MicroError, MicroTransport, PatternKind,
    Subscribed, Subscription,
};

/// NATS: requests go to the subject named after the pattern, answered by a
/// queue group named after the service, so each request reaches one
/// instance and a request nobody listens to fails at once with
/// [`MicroError::NoResponders`].
///
/// Events travel on `<stream>.events.<pattern>` (`NAVREE.events.deal.won`).
/// With JetStream (`micro.nats.jetstream`, the default) the stream
/// `micro.nats.stream` stores them, and each subscribing service reads them
/// through a durable pull consumer of its own (`<service>_<pattern>`), so
/// events wait for a service that is down; a failed event is negatively
/// acknowledged with its backoff as the delay. Without JetStream, events
/// use core publish-subscribe: at most once. Broadcast handlers always use
/// a plain core subscription. Dead letters go to
/// `<stream>.dead.<service>.<pattern>`, stored by the same stream.
pub struct NatsTransport {
    servers: Vec<String>,
    options: NatsOptions,
    service: String,
    instance: String,
    connect_timeout: Duration,
    client: OnceCell<async_nats::Client>,
}

impl NatsTransport {
    /// A transport for `url` (several servers comma-separated) that listens
    /// as `service` (the queue group and the durable consumers' prefix) and
    /// names its connection `instance`. Connects on first use, giving up
    /// after `connect_timeout`. `MicroModule` builds it from `micro.nats`;
    /// build one yourself for [`Transport::custom`](crate::Transport::custom)
    /// or the [conformance suite](crate::conformance).
    pub fn new(
        url: String,
        options: &NatsOptions,
        service: &str,
        instance: &str,
        connect_timeout: Duration,
    ) -> Result<Self, MicroError> {
        let servers = servers(&url);
        if servers.is_empty() {
            return Err(MicroError::transport("micro.url names no NATS server"));
        }
        if options.stream.is_empty()
            || !options
                .stream
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(MicroError::transport(format!(
                "micro.nats.stream `{}` may only hold letters, digits, `_` and `-`",
                options.stream
            )));
        }
        Ok(Self {
            servers,
            options: options.clone(),
            service: service.to_owned(),
            instance: instance.to_owned(),
            connect_timeout,
            client: OnceCell::new(),
        })
    }

    /// The connection, made on first use.
    async fn client(&self) -> Result<&async_nats::Client, MicroError> {
        self.client
            .get_or_try_init(|| async {
                let options =
                    connect_options(&self.options, &self.instance, self.connect_timeout).await?;
                let tls = self
                    .servers
                    .iter()
                    .any(|server| server.starts_with("tls://"));
                let options = options.require_tls(
                    tls || self.options.tls.ca.is_some() || self.options.tls.cert.is_some(),
                );
                let connecting = options.connect(self.servers.clone());
                match tokio::time::timeout(self.connect_timeout, connecting).await {
                    Ok(Ok(client)) => Ok(client),
                    Ok(Err(error)) => Err(MicroError::transport(format!(
                        "cannot connect to NATS: {error}"
                    ))),
                    Err(_) => Err(MicroError::transport(format!(
                        "connecting to NATS took longer than {:?}",
                        self.connect_timeout
                    ))),
                }
            })
            .await
    }

    fn jetstream(&self, client: &async_nats::Client) -> jetstream::Context {
        jetstream::new(client.clone())
    }

    /// `<stream>.events.<pattern>`.
    fn event_subject(&self, pattern: &str) -> String {
        event_subject(&self.options.stream, pattern)
    }

    /// The stream, created when missing; the line `micro:setup` prints.
    async fn ensure_stream(
        &self,
        js: &jetstream::Context,
    ) -> Result<(stream::Stream, String), MicroError> {
        let name = self.options.stream.clone();
        match js.get_stream(&name).await {
            Ok(stream) => Ok((stream, format!("stream {name} exists"))),
            Err(_) => {
                let stream = js
                    .create_stream(stream_config(&self.options))
                    .await
                    .map_err(|error| {
                        MicroError::transport(format!("cannot create stream {name}: {error}"))
                    })?;
                Ok((stream, format!("created stream {name}")))
            }
        }
    }

    /// The durable consumer of `subscription`, created when missing.
    async fn ensure_consumer(
        &self,
        stream: &stream::Stream,
        subscription: &Subscription,
    ) -> Result<(consumer::Consumer<consumer::pull::Config>, String), MicroError> {
        let name = consumer_name(&self.service, &subscription.pattern);
        let exists = stream.consumer_info(&name).await.is_ok();
        let config = consumer_config(&self.options, &self.service, subscription);
        let consumer = stream
            .get_or_create_consumer(&name, config)
            .await
            .map_err(|error| {
                MicroError::transport(format!("cannot create consumer {name}: {error}"))
            })?;
        let line = if exists {
            format!("consumer {name} exists ({})", subscription.pattern)
        } else {
            format!("created consumer {name} ({})", subscription.pattern)
        };
        Ok((consumer, line))
    }
}

impl fmt::Debug for NatsTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NatsTransport")
            .field(
                "servers",
                &self
                    .servers
                    .iter()
                    .map(|s| crate::options::redact_url(s))
                    .collect::<Vec<_>>(),
            )
            .field("jetstream", &self.options.jetstream)
            .field("stream", &self.options.stream)
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl MicroTransport for NatsTransport {
    fn name(&self) -> &'static str {
        "nats"
    }

    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        Box::pin(async move {
            let client = self.client().await?;
            let pattern = envelope.pattern().to_owned();
            let wait = envelope.remaining();
            let request = async_nats::Request::new()
                .headers(to_nats(&envelope))
                .payload(envelope.payload().clone())
                .timeout(wait);
            match client.send_request(pattern.clone(), request).await {
                Ok(reply) => Ok(from_nats(&pattern, reply.headers.as_ref(), reply.payload)),
                Err(error) => Err(match error.kind() {
                    RequestErrorKind::NoResponders => MicroError::NoResponders { pattern },
                    RequestErrorKind::TimedOut => MicroError::Timeout {
                        pattern,
                        after: wait.unwrap_or_default(),
                    },
                    _ => MicroError::transport(format!("the NATS request failed: {error}")),
                }),
            }
        })
    }

    fn publish(&self, envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let client = self.client().await?;
            let subject = self.event_subject(envelope.pattern());
            let headers = to_nats(&envelope);
            let payload = envelope.payload().clone();
            if self.options.jetstream {
                let js = self.jetstream(client);
                let ack = js
                    .publish_with_headers(subject, headers, payload)
                    .await
                    .map_err(|error| {
                        MicroError::transport(format!("the JetStream publish failed: {error}"))
                    })?;
                ack.await.map_err(|error| {
                    MicroError::transport(format!("JetStream did not store the event: {error}"))
                })?;
            } else {
                client
                    .publish_with_headers(subject, headers, payload)
                    .await
                    .map_err(|error| {
                        MicroError::transport(format!("the NATS publish failed: {error}"))
                    })?;
                client.flush().await.map_err(|error| {
                    MicroError::transport(format!("the NATS publish failed: {error}"))
                })?;
            }
            Ok(())
        })
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        self.listen_subscribed(subscriptions, deliveries, Subscribed::detached())
    }

    fn listen_subscribed(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
        subscribed: Subscribed,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let client = self.client().await?.clone();
            let mut tasks = JoinSet::new();
            let mut stream = None;
            for (index, subscription) in subscriptions.iter().enumerate() {
                match subscription.kind {
                    PatternKind::Message => {
                        let subscriber = client
                            .queue_subscribe(subscription.pattern.clone(), self.service.clone())
                            .await
                            .map_err(|error| {
                                MicroError::transport(format!(
                                    "cannot subscribe to {}: {error}",
                                    subscription.pattern
                                ))
                            })?;
                        tasks.spawn(requests(
                            client.clone(),
                            subscriber,
                            index,
                            deliveries.clone(),
                        ));
                    }
                    PatternKind::Event
                        if subscription.is_broadcast() || !self.options.jetstream =>
                    {
                        let subject = self.event_subject(&subscription.pattern);
                        let subscriber = if subscription.is_broadcast() {
                            client.subscribe(subject.clone()).await
                        } else {
                            client
                                .queue_subscribe(subject.clone(), self.service.clone())
                                .await
                        }
                        .map_err(|error| {
                            MicroError::transport(format!("cannot subscribe to {subject}: {error}"))
                        })?;
                        tasks.spawn(core_events(subscriber, index, deliveries.clone()));
                    }
                    PatternKind::Event => {
                        let js = self.jetstream(&client);
                        if stream.is_none() {
                            stream = Some(self.ensure_stream(&js).await?.0);
                        }
                        let Some(stream) = &stream else {
                            continue;
                        };
                        let (consumer, _) = self.ensure_consumer(stream, subscription).await?;
                        let messages = consumer.messages().await.map_err(|error| {
                            MicroError::transport(format!(
                                "cannot read consumer of {}: {error}",
                                subscription.pattern
                            ))
                        })?;
                        tasks.spawn(durable_events(messages, index, deliveries.clone()));
                    }
                }
            }
            // A subscription is only queued on the client until the server
            // reads it, and core NATS drops a message nobody is subscribed
            // to: wait until the server has taken them all in (the flush is
            // a PING answered after them) before saying the listener is up.
            match tokio::time::timeout(self.connect_timeout, client.flush()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(target: "navree::micro", %error, "the NATS subscriptions could not be confirmed");
                }
                Err(_) => {
                    tracing::warn!(target: "navree::micro", "the NATS server did not confirm the subscriptions in time; they are sent when it answers");
                }
            }
            subscribed.signal();
            deliveries.closed().await;
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            Ok(())
        })
    }

    fn setup(
        &self,
        subscriptions: &[Subscription],
    ) -> BoxFuture<'_, Result<Vec<String>, MicroError>> {
        let durable: Vec<Subscription> = subscriptions
            .iter()
            .filter(|s| s.kind == PatternKind::Event && !s.is_broadcast())
            .cloned()
            .collect();
        Box::pin(async move {
            if !self.options.jetstream {
                return Ok(vec![
                    "core publish-subscribe (micro.nats.jetstream = false): nothing to set up"
                        .to_owned(),
                ]);
            }
            let client = self.client().await?;
            let js = self.jetstream(client);
            let (stream, line) = self.ensure_stream(&js).await?;
            let mut lines = vec![line];
            for subscription in &durable {
                lines.push(self.ensure_consumer(&stream, subscription).await?.1);
            }
            Ok(lines)
        })
    }

    fn is_connected(&self) -> bool {
        self.client.get().is_none_or(|client| {
            client.connection_state() == async_nats::connection::State::Connected
        })
    }

    fn dead_letter(&self, letter: DeadLetter) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let client = self.client().await?;
            let subject = format!(
                "{}.dead.{}.{}",
                self.options.stream, letter.service, letter.pattern
            );
            let mut headers = to_nats(&letter.envelope());
            headers.insert("navree-error", single_line(&letter.error));
            headers.insert("navree-attempts", letter.attempts.to_string());
            if self.options.jetstream {
                let ack = self
                    .jetstream(client)
                    .publish_with_headers(subject, headers, letter.payload.clone())
                    .await
                    .map_err(|error| {
                        MicroError::transport(format!("the dead letter was not published: {error}"))
                    })?;
                ack.await.map_err(|error| {
                    MicroError::transport(format!(
                        "JetStream did not store the dead letter: {error}"
                    ))
                })?;
            } else {
                client
                    .publish_with_headers(subject, headers, letter.payload.clone())
                    .await
                    .map_err(|error| {
                        MicroError::transport(format!("the dead letter was not published: {error}"))
                    })?;
            }
            Ok(())
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let Some(client) = self.client.get() {
                let _ = client.flush().await;
            }
        })
    }
}

/// The connect options from `micro.nats`.
async fn connect_options(
    options: &NatsOptions,
    instance: &str,
    connect_timeout: Duration,
) -> Result<ConnectOptions, MicroError> {
    let mut connect = ConnectOptions::new()
        .name(instance)
        .connection_timeout(connect_timeout)
        .request_timeout(None)
        .reconnect_delay_callback(reconnect_delay);
    if let Some(path) = &options.credentials {
        // `.creds` files sign the server's nonce with Ed25519 through the
        // `nkeys` crate (ed25519-dalek), which the crypto policy keeps out.
        return Err(MicroError::transport(format!(
            "micro.nats.credentials ({}) is not supported: `.creds` files (JWT and NKey) \
             are not available; use micro.nats.token, a user and password in the URL, \
             or TLS client certificates",
            path.display()
        )));
    }
    if let Some(token) = &options.token {
        connect = connect.token(token.expose().clone());
    }
    if let Some(ca) = &options.tls.ca {
        connect = connect.add_root_certificates(ca.clone());
    }
    match (&options.tls.cert, &options.tls.key) {
        (Some(cert), Some(key)) => {
            connect = connect.add_client_certificate(cert.clone(), key.clone())
        }
        (None, None) => {}
        _ => {
            return Err(MicroError::transport(
                "micro.nats.tls.cert and micro.nats.tls.key go together",
            ));
        }
    }
    Ok(connect)
}

/// Reconnect pauses: 100 ms doubling to 30 seconds.
fn reconnect_delay(attempts: usize) -> Duration {
    let exponent = u32::try_from(attempts.saturating_sub(1))
        .unwrap_or(u32::MAX)
        .min(16);
    Duration::from_millis(100u64.saturating_mul(1u64 << exponent)).min(Duration::from_secs(30))
}

/// The servers of `micro.url`: comma-separated, blanks ignored.
fn servers(url: &str) -> Vec<String> {
    url.split(',')
        .map(str::trim)
        .filter(|server| !server.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `<stream>.events.<pattern>`.
fn event_subject(stream: &str, pattern: &str) -> String {
    format!("{stream}.events.{pattern}")
}

/// The durable consumer of `service` for `pattern`: `notifications_deal_won`
/// (`*` is `any`, `>` is `all`).
fn consumer_name(service: &str, pattern: &str) -> String {
    let pattern = pattern
        .split('.')
        .map(|segment| match segment {
            "*" => "any",
            ">" => "all",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("_");
    format!("{service}_{pattern}")
}

/// The stream: every subject under `<stream>.`, kept `max_age_hours`.
fn stream_config(options: &NatsOptions) -> stream::Config {
    stream::Config {
        name: options.stream.clone(),
        subjects: vec![format!("{}.>", options.stream)],
        max_age: Duration::from_secs(options.max_age_hours.saturating_mul(3_600)),
        num_replicas: options.replicas.max(1),
        storage: stream::StorageType::File,
        ..stream::Config::default()
    }
}

/// A durable pull consumer: explicit acks, redelivery after `ack_wait_secs`
/// (or the handler's time limit when longer), no delivery cap (Navree
/// counts attempts), one event in flight for `ordered` handlers.
fn consumer_config(
    options: &NatsOptions,
    service: &str,
    subscription: &Subscription,
) -> consumer::pull::Config {
    let name = consumer_name(service, &subscription.pattern);
    consumer::pull::Config {
        durable_name: Some(name.clone()),
        name: Some(name),
        filter_subject: event_subject(&options.stream, &subscription.pattern),
        ack_policy: consumer::AckPolicy::Explicit,
        ack_wait: Duration::from_secs(options.ack_wait_secs).max(subscription.ack_wait),
        max_deliver: -1,
        deliver_policy: consumer::DeliverPolicy::New,
        max_ack_pending: if subscription.ordered { 1 } else { 1_000 },
        ..consumer::pull::Config::default()
    }
}

/// The envelope's headers as NATS headers.
fn to_nats(envelope: &Envelope) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in envelope.headers() {
        map.insert(name.as_str(), single_line(value));
    }
    map
}

/// An envelope from NATS headers and a payload; the pattern comes from
/// `navree-pattern`, else `fallback`.
fn from_nats(fallback: &str, map: Option<&HeaderMap>, payload: Bytes) -> Envelope {
    let mut pairs: BTreeMap<String, String> = BTreeMap::new();
    if let Some(map) = map {
        for (name, values) in map.iter() {
            if let Some(value) = values.last() {
                let name: &str = name.as_ref();
                pairs.insert(name.to_ascii_lowercase(), value.as_str().to_owned());
            }
        }
    }
    let pattern = pairs
        .get(headers::PATTERN)
        .cloned()
        .unwrap_or_else(|| fallback.to_owned());
    let mut envelope = Envelope::new(pattern, payload);
    for (name, value) in pairs {
        envelope.set_header(&name, value);
    }
    envelope
}

/// Header values are one line.
fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

/// Hands the requests of a queue subscription to the listener.
async fn requests(
    client: async_nats::Client,
    mut subscriber: async_nats::Subscriber,
    index: usize,
    deliveries: mpsc::Sender<Delivery>,
) {
    while let Some(message) = subscriber.next().await {
        let pattern = message.subject.to_string();
        let envelope = from_nats(&pattern, message.headers.as_ref(), message.payload.clone());
        let Some(reply) = message.reply.clone() else {
            continue;
        };
        let actions = NatsReply {
            client: client.clone(),
            reply: reply.to_string(),
        };
        if deliveries
            .send(Delivery::new(envelope, index, 1, actions))
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = subscriber.unsubscribe().await;
}

/// Replies to a NATS request on its reply subject.
struct NatsReply {
    client: async_nats::Client,
    reply: String,
}

impl DeliveryActions for NatsReply {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async { Ok(()) })
    }

    fn retry_in(self: Box<Self>, _delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async { Ok(()) })
    }

    fn reply(self: Box<Self>, reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            self.client
                .publish_with_headers(self.reply, to_nats(&reply), reply.payload().clone())
                .await
                .map_err(|error| {
                    MicroError::transport(format!("the reply was not sent: {error}"))
                })?;
            self.client
                .flush()
                .await
                .map_err(|error| MicroError::transport(format!("the reply was not sent: {error}")))
        })
    }
}

/// Hands the events of a core subscription to the listener (at most once).
async fn core_events(
    mut subscriber: async_nats::Subscriber,
    index: usize,
    deliveries: mpsc::Sender<Delivery>,
) {
    while let Some(message) = subscriber.next().await {
        let envelope = from_nats(&message.subject, message.headers.as_ref(), message.payload);
        if deliveries
            .send(LocalRetry::delivery(deliveries.clone(), envelope, index))
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = subscriber.unsubscribe().await;
}

/// Hands the events of a durable consumer to the listener.
async fn durable_events(
    mut messages: consumer::pull::Stream,
    index: usize,
    deliveries: mpsc::Sender<Delivery>,
) {
    while let Some(message) = messages.next().await {
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!(target: "navree::micro", %error, "reading a JetStream consumer failed");
                continue;
            }
        };
        let attempt = message
            .info()
            .ok()
            .and_then(|info| u32::try_from(info.delivered).ok())
            .unwrap_or(1);
        let envelope = from_nats(
            &message.subject,
            message.headers.as_ref(),
            message.payload.clone(),
        );
        let actions = JetStreamActions {
            message: Arc::new(message),
        };
        if deliveries
            .send(Delivery::new(envelope, index, attempt, actions))
            .await
            .is_err()
        {
            break;
        }
    }
}

/// Acknowledges a JetStream message, or asks for it again after a delay.
struct JetStreamActions {
    message: Arc<jetstream::Message>,
}

impl DeliveryActions for JetStreamActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            self.message.ack().await.map_err(|error| {
                MicroError::transport(format!("the acknowledgment failed: {error}"))
            })
        })
    }

    fn retry_in(self: Box<Self>, delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            self.message
                .ack_with(AckKind::Nak(Some(delay)))
                .await
                .map_err(|error| {
                    MicroError::transport(format!("the negative acknowledgment failed: {error}"))
                })
        })
    }

    fn reply(self: Box<Self>, _reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        self.ack()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_and_consumers_are_named_after_the_pattern() {
        assert_eq!(event_subject("CRM", "deal.won"), "CRM.events.deal.won");
        assert_eq!(
            consumer_name("notifications", "deal.won"),
            "notifications_deal_won"
        );
        assert_eq!(consumer_name("stats", "deal.*"), "stats_deal_any");
        assert_eq!(consumer_name("audit", "audit.>"), "audit_audit_all");
    }

    #[test]
    fn several_servers_are_comma_separated() {
        assert_eq!(
            servers("nats://nats-1:4222, nats://nats-2:4222,"),
            ["nats://nats-1:4222", "nats://nats-2:4222"]
        );
        assert!(servers(" , ").is_empty());
    }

    #[test]
    fn reconnects_back_off_from_100_ms_to_30_seconds() {
        assert_eq!(reconnect_delay(1), Duration::from_millis(100));
        assert_eq!(reconnect_delay(2), Duration::from_millis(200));
        assert_eq!(reconnect_delay(5), Duration::from_millis(1_600));
        assert_eq!(reconnect_delay(40), Duration::from_secs(30));
    }

    #[test]
    fn the_stream_and_consumers_follow_the_options() {
        let options = NatsOptions {
            stream: "CRM".into(),
            replicas: 3,
            max_age_hours: 24,
            ack_wait_secs: 30,
            ..NatsOptions::default()
        };
        let stream = stream_config(&options);
        assert_eq!(stream.subjects, ["CRM.>"]);
        assert_eq!(stream.num_replicas, 3);
        assert_eq!(stream.max_age, Duration::from_secs(86_400));
        let subscription = Subscription {
            pattern: "deal.won".into(),
            kind: PatternKind::Event,
            group: crate::Group::Service("notifications".into()),
            ordered: true,
            ack_wait: Duration::from_secs(120),
        };
        let consumer = consumer_config(&options, "notifications", &subscription);
        assert_eq!(
            consumer.durable_name.as_deref(),
            Some("notifications_deal_won")
        );
        assert_eq!(consumer.filter_subject, "CRM.events.deal.won");
        assert_eq!(
            consumer.ack_wait,
            Duration::from_secs(120),
            "raised to the handler's limit"
        );
        assert_eq!(consumer.max_ack_pending, 1, "ordered");
        assert_eq!(consumer.max_deliver, -1);
    }

    #[test]
    fn headers_round_trip_through_nats_headers() {
        let envelope = Envelope::new("deals.get", "{}")
            .with_header(headers::MESSAGE_ID, "m1")
            .with_header("x-custom", "a\r\nb");
        let map = to_nats(&envelope);
        let back = from_nats("ignored", Some(&map), Bytes::from_static(b"{}"));
        assert_eq!(back.pattern(), "deals.get");
        assert_eq!(back.message_id(), Some("m1"));
        assert_eq!(back.header("x-custom"), Some("a  b"), "one line");
        let bare = from_nats("deal.won", None, Bytes::new());
        assert_eq!(bare.pattern(), "deal.won");
    }

    #[test]
    fn bad_settings_are_refused() {
        let options = NatsOptions::default();
        let error = NatsTransport::new(" ".into(), &options, "s", "s-1", Duration::from_secs(1))
            .unwrap_err();
        assert!(error.to_string().contains("no NATS server"), "{error}");
        let bad_stream = NatsOptions {
            stream: "a.b".into(),
            ..NatsOptions::default()
        };
        let error = NatsTransport::new(
            "nats://x:4222".into(),
            &bad_stream,
            "s",
            "s-1",
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(error.to_string().contains("micro.nats.stream"), "{error}");
    }

    #[tokio::test]
    async fn creds_files_are_refused() {
        let options = NatsOptions {
            credentials: Some("deals.creds".into()),
            ..NatsOptions::default()
        };
        let Err(error) = connect_options(&options, "s-1", Duration::from_secs(1)).await else {
            panic!("a `.creds` file must be refused");
        };
        assert!(
            error.to_string().contains("micro.nats.credentials"),
            "{error}"
        );
    }
}
