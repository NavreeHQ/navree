//! [`RabbitmqTransport`] (`rabbitmq` feature): RabbitMQ (AMQP 0-9-1)
//! through lapin.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::SelectAll;
use lapin::message::Delivery as AmqpDelivery;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions, BasicQosOptions,
    ConfirmSelectOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
};
use lapin::tcp::OwnedTLSConfig;
use lapin::types::{AMQPValue, FieldTable, LongString, ShortString};
use lapin::{
    BasicProperties, Channel, Confirmation, Connection, ConnectionProperties, Consumer,
    DefaultConnectionBuilder, ExchangeKind,
};
use navree_kernel::BoxFuture;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::envelope::headers;
use crate::options::RabbitmqOptions;
use crate::transport::LocalRetry;
use crate::transport::broker::reconnect_delay;
use crate::{
    DeadLetter, Delivery, DeliveryActions, Envelope, MicroError, MicroTransport, PatternKind,
    Subscription,
};

/// RabbitMQ's direct reply-to pseudo-queue.
const DIRECT_REPLY_TO: &str = "amq.rabbitmq.reply-to";

/// RabbitMQ: one topic exchange, `micro.rabbitmq.exchange`, carries every
/// message with its pattern as the routing key (`*` and `>` become `*` and
/// `#` in bindings).
///
/// - **Events.** Each subscribing service gets a queue per pattern,
///   `<service>.<pattern>` (`micro.rabbitmq.queue_type`, `quorum` by
///   default), bound to the exchange: every service gets its own copy and
///   the instances of a service compete for the messages in its queue.
///   `ordered` handlers' queues have a single active consumer. `emit`
///   waits for the broker's publisher confirm.
/// - **Retries** wait in `<service>.<pattern>.retry`, a delay queue whose
///   messages expire after their backoff (per-message TTL) and dead-letter
///   back to the main queue, carrying `navree-attempt`.
/// - **Requests.** One queue per pattern, `<service>.<pattern>`, shared by
///   the instances of the answering service; replies come back through
///   direct reply-to. A request is published as mandatory, so one that no
///   queue is bound for comes back at once as
///   [`MicroError::NoResponders`].
/// - **Broadcast** handlers each get an exclusive, auto-deleted queue of
///   their own.
/// - **Dead letters** (`micro.events.dead_letter = "transport"`) go to the
///   `<exchange>.dead` topic exchange, routed by pattern.
pub struct RabbitmqTransport {
    url: String,
    options: RabbitmqOptions,
    service: String,
    instance: String,
    connect_timeout: Duration,
    tls: OwnedTLSConfig,
    link: tokio::sync::Mutex<Option<Arc<Link>>>,
    replies: Arc<Replies>,
    closed: CancellationToken,
}

/// One connection: a channel for events, retries and replies (with
/// publisher confirms), and one for requests with its direct reply-to
/// consumer.
struct Link {
    connection: Connection,
    publish: Channel,
    request: Channel,
}

/// Requests waiting for their replies, by correlation id.
#[derive(Default)]
struct Replies(Mutex<HashMap<String, oneshot::Sender<Envelope>>>);

impl Replies {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, oneshot::Sender<Envelope>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl RabbitmqTransport {
    /// A transport for `url` (`amqp://` or `amqps://`, with the virtual
    /// host as its path) that listens as `service` (its queues' prefix).
    /// Connects on first use, giving up after `connect_timeout`.
    /// `MicroModule` builds it from `micro.rabbitmq`; build one yourself for
    /// [`Transport::custom`](crate::Transport::custom) or the
    /// [conformance suite](crate::conformance).
    pub fn new(
        url: impl Into<String>,
        options: &RabbitmqOptions,
        service: &str,
        instance: &str,
        connect_timeout: Duration,
    ) -> Result<Self, MicroError> {
        let url = url.into();
        let scheme = url.split("://").next().unwrap_or_default().to_ascii_lowercase();
        if !matches!(scheme.as_str(), "amqp" | "amqps") || !url.contains("://") {
            return Err(MicroError::transport(format!(
                "`{}` is not a RabbitMQ URL: expected amqp:// or amqps://",
                crate::options::redact_url(&url)
            )));
        }
        if !matches!(options.queue_type.as_str(), "quorum" | "classic") {
            return Err(MicroError::transport(format!(
                "micro.rabbitmq.queue_type must be `quorum` or `classic`, not `{}`",
                options.queue_type
            )));
        }
        if options.exchange.is_empty() {
            return Err(MicroError::transport("micro.rabbitmq.exchange may not be empty"));
        }
        let cert_chain = match &options.tls.ca {
            Some(path) => Some(std::fs::read_to_string(path).map_err(|error| {
                MicroError::transport(format!("cannot read {}: {error}", path.display()))
            })?),
            None => None,
        };
        Ok(Self {
            url,
            options: options.clone(),
            service: service.to_owned(),
            instance: instance.to_owned(),
            connect_timeout,
            tls: OwnedTLSConfig {
                identity: None,
                cert_chain,
            },
            link: tokio::sync::Mutex::new(None),
            replies: Arc::new(Replies::default()),
            closed: CancellationToken::new(),
        })
    }

    /// The live connection, made (again) when there is none.
    async fn link(&self) -> Result<Arc<Link>, MicroError> {
        let mut current = self.link.lock().await;
        if let Some(link) = current.as_ref()
            && link.connection.status().connected()
            && link.publish.status().connected()
            && link.request.status().connected()
        {
            return Ok(link.clone());
        }
        let link = match tokio::time::timeout(self.connect_timeout, self.connect()).await {
            Ok(link) => link?,
            Err(_) => {
                return Err(MicroError::transport(format!(
                    "connecting to RabbitMQ took longer than {:?}",
                    self.connect_timeout
                )));
            }
        };
        let link = Arc::new(link);
        *current = Some(link.clone());
        Ok(link)
    }

    async fn connect(&self) -> Result<Link, MicroError> {
        let properties = ConnectionProperties::default().with_connection_name(self.instance.clone().into());
        let connection = DefaultConnectionBuilder::new()
            .map_err(amqp)?
            .with_uri_str(self.url.clone())
            .with_properties(properties)
            .with_tls_config(self.tls.clone())
            .connect()
            .await
            .map_err(|error| MicroError::transport(format!("cannot connect to RabbitMQ: {error}")))?;
        let publish = connection.create_channel().await.map_err(amqp)?;
        publish.confirm_select(ConfirmSelectOptions::default()).await.map_err(amqp)?;
        let request = connection.create_channel().await.map_err(amqp)?;
        request.confirm_select(ConfirmSelectOptions::default()).await.map_err(amqp)?;
        let replies = request
            .basic_consume(
                DIRECT_REPLY_TO.into(),
                format!("{}-replies", self.instance).into(),
                BasicConsumeOptions {
                    no_ack: true,
                    ..BasicConsumeOptions::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(amqp)?;
        tokio::spawn(read_replies(replies, self.replies.clone(), self.closed.clone()));
        Ok(Link {
            connection,
            publish,
            request,
        })
    }

    /// `<service>.<pattern>`.
    fn queue(&self, pattern: &str) -> String {
        format!("{}.{pattern}", self.service)
    }

    /// Declares the exchange, and the queues and bindings of
    /// `subscription` (not broadcast); returns what it did.
    async fn declare(&self, channel: &Channel, subscription: &Subscription) -> Result<Vec<String>, MicroError> {
        let mut lines = Vec::new();
        let queue = self.queue(&subscription.pattern);
        let mut arguments = queue_arguments(&self.options.queue_type);
        if subscription.ordered {
            arguments.insert("x-single-active-consumer".into(), AMQPValue::Boolean(true));
        }
        channel
            .queue_declare(queue.clone().into(), durable(), arguments)
            .await
            .map_err(|error| declare_error(&queue, &error))?;
        lines.push(format!("declared queue {queue} ({})", self.options.queue_type));
        let key = routing_key(&subscription.pattern);
        channel
            .queue_bind(
                queue.clone().into(),
                self.options.exchange.clone().into(),
                key.clone().into(),
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(amqp)?;
        lines.push(format!("bound queue {queue} to {} with {key}", self.options.exchange));
        if subscription.kind == PatternKind::Event {
            let retry = format!("{queue}.retry");
            let mut arguments = queue_arguments(&self.options.queue_type);
            arguments.insert("x-dead-letter-exchange".into(), AMQPValue::LongString("".into()));
            arguments.insert(
                "x-dead-letter-routing-key".into(),
                AMQPValue::LongString(queue.clone().into()),
            );
            channel
                .queue_declare(retry.clone().into(), durable(), arguments)
                .await
                .map_err(|error| declare_error(&retry, &error))?;
            lines.push(format!("declared retry queue {retry}"));
        }
        Ok(lines)
    }

    /// The exchange and the dead-letter exchange.
    async fn declare_exchanges(&self, channel: &Channel) -> Result<Vec<String>, MicroError> {
        let options = ExchangeDeclareOptions {
            durable: true,
            ..ExchangeDeclareOptions::default()
        };
        for name in [self.options.exchange.clone(), self.dead_exchange()] {
            channel
                .exchange_declare(name.clone().into(), ExchangeKind::Topic, options, FieldTable::default())
                .await
                .map_err(|error| declare_error(&name, &error))?;
        }
        Ok(vec![
            format!("declared topic exchange {}", self.options.exchange),
            format!("declared topic exchange {}", self.dead_exchange()),
        ])
    }

    fn dead_exchange(&self) -> String {
        format!("{}.dead", self.options.exchange)
    }

    /// Opens a consumer channel and starts consuming `subscriptions`.
    async fn consume(
        &self,
        subscriptions: &[Subscription],
    ) -> Result<(Channel, SelectAll<IndexedConsumer>, Vec<Option<String>>), MicroError> {
        let link = self.link().await?;
        let channel = link.connection.create_channel().await.map_err(amqp)?;
        channel
            .basic_qos(self.options.prefetch.max(1), BasicQosOptions::default())
            .await
            .map_err(amqp)?;
        self.declare_exchanges(&channel).await?;
        let mut consumers = SelectAll::new();
        let mut retry_queues = Vec::with_capacity(subscriptions.len());
        for (index, subscription) in subscriptions.iter().enumerate() {
            let tag = format!("{}-{index}", self.instance);
            if subscription.is_broadcast() {
                let queue = channel
                    .queue_declare(
                        "".into(),
                        QueueDeclareOptions {
                            exclusive: true,
                            auto_delete: true,
                            ..QueueDeclareOptions::default()
                        },
                        FieldTable::default(),
                    )
                    .await
                    .map_err(amqp)?;
                channel
                    .queue_bind(
                        queue.name().clone(),
                        self.options.exchange.clone().into(),
                        routing_key(&subscription.pattern).into(),
                        QueueBindOptions::default(),
                        FieldTable::default(),
                    )
                    .await
                    .map_err(amqp)?;
                let consumer = channel
                    .basic_consume(
                        queue.name().clone(),
                        tag.into(),
                        BasicConsumeOptions {
                            no_ack: true,
                            ..BasicConsumeOptions::default()
                        },
                        FieldTable::default(),
                    )
                    .await
                    .map_err(amqp)?;
                consumers.push(IndexedConsumer::new(index, consumer));
                retry_queues.push(None);
            } else {
                self.declare(&channel, subscription).await?;
                let queue = self.queue(&subscription.pattern);
                let consumer = channel
                    .basic_consume(queue.clone().into(), tag.into(), BasicConsumeOptions::default(), FieldTable::default())
                    .await
                    .map_err(amqp)?;
                consumers.push(IndexedConsumer::new(index, consumer));
                retry_queues.push((subscription.kind == PatternKind::Event).then(|| format!("{queue}.retry")));
            }
        }
        Ok((channel, consumers, retry_queues))
    }
}

impl Drop for RabbitmqTransport {
    fn drop(&mut self) {
        self.closed.cancel();
    }
}

impl fmt::Debug for RabbitmqTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RabbitmqTransport")
            .field("url", &crate::options::redact_url(&self.url))
            .field("exchange", &self.options.exchange)
            .field("queue_type", &self.options.queue_type)
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl MicroTransport for RabbitmqTransport {
    fn name(&self) -> &'static str {
        "rabbitmq"
    }

    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        Box::pin(async move {
            let pattern = envelope.pattern().to_owned();
            let link = self.link().await?;
            let correlation = Uuid::now_v7().to_string();
            let (sender, reply) = oneshot::channel();
            self.replies.lock().insert(correlation.clone(), sender);
            let forget = || {
                self.replies.lock().remove(&correlation);
            };
            let wait = envelope.remaining();
            let mut properties = properties(&envelope)
                .with_reply_to(DIRECT_REPLY_TO.into())
                .with_correlation_id(correlation.clone().into());
            if let Some(wait) = wait {
                properties = properties.with_expiration(wait.as_millis().max(1).to_string().into());
            }
            let confirm = link
                .request
                .basic_publish(
                    self.options.exchange.clone().into(),
                    pattern.clone().into(),
                    BasicPublishOptions {
                        mandatory: true,
                        ..BasicPublishOptions::default()
                    },
                    envelope.payload(),
                    properties,
                )
                .await;
            let confirmed = match confirm {
                Ok(confirm) => confirm.await,
                Err(error) => {
                    forget();
                    return Err(amqp(error));
                }
            };
            match confirmed {
                Ok(Confirmation::Ack(None) | Confirmation::NotRequested) => {}
                Ok(Confirmation::Ack(Some(_)) | Confirmation::Nack(Some(_))) => {
                    forget();
                    return Err(MicroError::NoResponders { pattern });
                }
                Ok(Confirmation::Nack(None)) => {
                    forget();
                    return Err(MicroError::transport("RabbitMQ refused the request"));
                }
                Err(error) => {
                    forget();
                    return Err(amqp(error));
                }
            }
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
            let link = self.link().await?;
            publish_confirmed(
                &link.publish,
                &self.options.exchange,
                envelope.pattern(),
                &envelope,
                properties(&envelope),
            )
            .await
        })
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let mut attempt = 0u32;
            loop {
                let (channel, mut consumers, retry_queues) = match self.consume(&subscriptions).await {
                    Ok(consuming) => {
                        attempt = 0;
                        consuming
                    }
                    Err(error) => {
                        tracing::warn!(target: "navree::micro", %error, "consuming from RabbitMQ failed; retrying");
                        attempt = attempt.saturating_add(1);
                        tokio::select! {
                            () = tokio::time::sleep(reconnect_delay(attempt)) => continue,
                            () = deliveries.closed() => return Ok(()),
                            () = self.closed.cancelled() => return Ok(()),
                        }
                    }
                };
                loop {
                    let next = tokio::select! {
                        next = consumers.next() => next,
                        () = deliveries.closed() => {
                            let _ = channel.close(200, "OK".into()).await;
                            return Ok(());
                        }
                        () = self.closed.cancelled() => return Ok(()),
                    };
                    let Some((index, received)) = next else {
                        break;
                    };
                    let message = match received {
                        Ok(message) => message,
                        Err(error) => {
                            tracing::warn!(target: "navree::micro", %error, "a RabbitMQ consumer failed");
                            break;
                        }
                    };
                    let Some(subscription) = subscriptions.get(index) else {
                        continue;
                    };
                    let delivery = match self.delivery(subscription, index, retry_queues[index].clone(), message, &deliveries).await {
                        Ok(delivery) => delivery,
                        Err(error) => {
                            tracing::warn!(target: "navree::micro", %error, "a RabbitMQ delivery could not be handed over");
                            continue;
                        }
                    };
                    if deliveries.send(delivery).await.is_err() {
                        let _ = channel.close(200, "OK".into()).await;
                        return Ok(());
                    }
                }
                // The connection or channel broke: consume again.
                attempt = attempt.saturating_add(1);
                tokio::select! {
                    () = tokio::time::sleep(reconnect_delay(attempt)) => {}
                    () = deliveries.closed() => return Ok(()),
                    () = self.closed.cancelled() => return Ok(()),
                }
            }
        })
    }

    fn setup(&self, subscriptions: &[Subscription]) -> BoxFuture<'_, Result<Vec<String>, MicroError>> {
        let durable: Vec<Subscription> = subscriptions
            .iter()
            .filter(|subscription| !subscription.is_broadcast())
            .cloned()
            .collect();
        Box::pin(async move {
            let link = self.link().await?;
            let channel = link.connection.create_channel().await.map_err(amqp)?;
            let mut lines = self.declare_exchanges(&channel).await?;
            for subscription in &durable {
                lines.extend(self.declare(&channel, subscription).await?);
            }
            let _ = channel.close(200, "OK".into()).await;
            Ok(lines)
        })
    }

    fn is_connected(&self) -> bool {
        match self.link.try_lock() {
            Ok(link) => link.as_ref().is_none_or(|link| link.connection.status().connected()),
            Err(_) => true,
        }
    }

    fn dead_letter(&self, letter: DeadLetter) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let link = self.link().await?;
            let mut envelope = letter.envelope();
            envelope.set_header("navree-error", single_line(&letter.error));
            envelope.set_header("navree-attempts", letter.attempts.to_string());
            envelope.set_header("navree-dead-service", letter.service.clone());
            let exchange = self.dead_exchange();
            publish_confirmed(&link.publish, &exchange, &letter.pattern, &envelope, properties(&envelope)).await
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.closed.cancel();
            if let Some(link) = self.link.lock().await.take() {
                let _ = link.connection.close(200, "OK".into()).await;
            }
        })
    }
}

impl RabbitmqTransport {
    /// The delivery of a received message.
    async fn delivery(
        &self,
        subscription: &Subscription,
        index: usize,
        retry_queue: Option<String>,
        message: AmqpDelivery,
        deliveries: &mpsc::Sender<Delivery>,
    ) -> Result<Delivery, MicroError> {
        let envelope = from_delivery(&message);
        if subscription.is_broadcast() {
            return Ok(LocalRetry::delivery(deliveries.clone(), envelope, index));
        }
        let attempt = envelope.attempt() + u32::from(message.redelivered);
        let link = self.link().await?;
        let actions = MessageActions {
            publish: link.publish.clone(),
            message,
            retry_queue,
            envelope: envelope.clone(),
            attempt,
        };
        Ok(Delivery::new(envelope, index, attempt, actions))
    }
}

/// A consumer whose deliveries carry its subscription's index.
struct IndexedConsumer {
    index: usize,
    consumer: Consumer,
}

impl IndexedConsumer {
    fn new(index: usize, consumer: Consumer) -> Self {
        Self { index, consumer }
    }
}

impl futures_util::Stream for IndexedConsumer {
    type Item = (usize, Result<AmqpDelivery, lapin::Error>);

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let index = self.index;
        self.consumer
            .poll_next_unpin(cx)
            .map(|item| item.map(|delivery| (index, delivery)))
    }
}

/// Acknowledges a message, sends it to its retry queue, or replies to the
/// request it holds.
struct MessageActions {
    publish: Channel,
    message: AmqpDelivery,
    retry_queue: Option<String>,
    envelope: Envelope,
    attempt: u32,
}

impl DeliveryActions for MessageActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            self.message.acker.ack(BasicAckOptions::default()).await.map_err(amqp)?;
            Ok(())
        })
    }

    fn retry_in(self: Box<Self>, delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            let Some(retry_queue) = &self.retry_queue else {
                // A request is answered once.
                self.message.acker.ack(BasicAckOptions::default()).await.map_err(amqp)?;
                return Ok(());
            };
            let mut envelope = self.envelope.clone();
            envelope.set_header(headers::ATTEMPT, (self.attempt + 1).to_string());
            let properties = properties(&envelope).with_expiration(delay.as_millis().max(1).to_string().into());
            match publish_confirmed(&self.publish, "", retry_queue, &envelope, properties).await {
                Ok(()) => {
                    self.message.acker.ack(BasicAckOptions::default()).await.map_err(amqp)?;
                    Ok(())
                }
                Err(error) => {
                    // Back on the queue now rather than lost.
                    let _ = self
                        .message
                        .acker
                        .nack(BasicNackOptions {
                            requeue: true,
                            ..BasicNackOptions::default()
                        })
                        .await;
                    Err(error)
                }
            }
        })
    }

    fn reply(self: Box<Self>, reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            let to = self.message.properties.reply_to().clone();
            let correlation = self.message.properties.correlation_id().clone();
            let outcome = match to {
                Some(to) => {
                    let mut properties = properties(&reply);
                    if let Some(correlation) = correlation {
                        properties = properties.with_correlation_id(correlation);
                    }
                    self.publish
                        .basic_publish("".into(), to, BasicPublishOptions::default(), reply.payload(), properties)
                        .await
                        .map(|_| ())
                        .map_err(amqp)
                }
                None => Ok(()),
            };
            self.message.acker.ack(BasicAckOptions::default()).await.map_err(amqp)?;
            outcome
        })
    }
}

/// Reads the direct reply-to consumer and hands each reply to its request.
async fn read_replies(mut replies: Consumer, waiting: Arc<Replies>, closed: CancellationToken) {
    loop {
        let next = tokio::select! {
            next = replies.next() => next,
            () = closed.cancelled() => return,
        };
        let Some(Ok(message)) = next else {
            return;
        };
        let Some(correlation) = message.properties.correlation_id().clone() else {
            continue;
        };
        if let Some(sender) = waiting.lock().remove(correlation.as_str()) {
            let _ = sender.send(from_delivery(&message));
        }
    }
}

/// Publishes and waits for the broker's confirm.
async fn publish_confirmed(
    channel: &Channel,
    exchange: &str,
    routing_key: &str,
    envelope: &Envelope,
    properties: BasicProperties,
) -> Result<(), MicroError> {
    let confirm = channel
        .basic_publish(
            exchange.into(),
            routing_key.into(),
            BasicPublishOptions::default(),
            envelope.payload(),
            properties,
        )
        .await
        .map_err(amqp)?;
    match confirm.await.map_err(amqp)? {
        Confirmation::Ack(_) | Confirmation::NotRequested => Ok(()),
        Confirmation::Nack(_) => Err(MicroError::transport("RabbitMQ did not store the message")),
    }
}

/// Durable, shared queues.
fn durable() -> QueueDeclareOptions {
    QueueDeclareOptions {
        durable: true,
        ..QueueDeclareOptions::default()
    }
}

/// `x-queue-type`.
fn queue_arguments(queue_type: &str) -> FieldTable {
    let mut arguments = FieldTable::default();
    arguments.insert("x-queue-type".into(), AMQPValue::LongString(queue_type.into()));
    arguments
}

/// A binding's routing key: `>` becomes `#`.
fn routing_key(pattern: &str) -> String {
    pattern
        .split('.')
        .map(|segment| if segment == ">" { "#" } else { segment })
        .collect::<Vec<_>>()
        .join(".")
}

/// The AMQP properties of an envelope: its headers, content type and
/// message id; persistent.
fn properties(envelope: &Envelope) -> BasicProperties {
    let mut table = FieldTable::default();
    for (name, value) in envelope.headers() {
        table.insert(ShortString::from(name.clone()), AMQPValue::LongString(LongString::from(value.clone())));
    }
    let mut properties = BasicProperties::default()
        .with_headers(table)
        .with_content_type(envelope.content_type().to_owned().into())
        .with_delivery_mode(2);
    if let Some(id) = envelope.message_id() {
        properties = properties.with_message_id(id.to_owned().into());
    }
    properties
}

/// The envelope of a received message: headers from its AMQP headers; the
/// pattern is `navree-pattern`, else the routing key.
fn from_delivery(message: &AmqpDelivery) -> Envelope {
    let mut pairs = BTreeMap::new();
    if let Some(table) = message.properties.headers() {
        for (name, value) in table {
            if let Some(text) = value_text(value) {
                pairs.insert(name.as_str().to_ascii_lowercase(), text);
            }
        }
    }
    let pattern = pairs
        .get(headers::PATTERN)
        .cloned()
        .unwrap_or_else(|| message.routing_key.as_str().to_owned());
    let mut envelope = Envelope::new(pattern, Bytes::from(message.data.clone()));
    if let Some(content_type) = message.properties.content_type() {
        envelope.set_header(headers::CONTENT_TYPE, content_type.as_str());
    }
    for (name, value) in pairs {
        envelope.set_header(&name, value);
    }
    envelope
}

fn value_text(value: &AMQPValue) -> Option<String> {
    Some(match value {
        AMQPValue::LongString(text) => String::from_utf8_lossy(text.as_bytes()).into_owned(),
        AMQPValue::ShortString(text) => text.as_str().to_owned(),
        AMQPValue::Boolean(value) => value.to_string(),
        AMQPValue::ShortShortInt(n) => n.to_string(),
        AMQPValue::ShortShortUInt(n) => n.to_string(),
        AMQPValue::ShortInt(n) => n.to_string(),
        AMQPValue::ShortUInt(n) => n.to_string(),
        AMQPValue::LongInt(n) => n.to_string(),
        AMQPValue::LongUInt(n) => n.to_string(),
        AMQPValue::LongLongInt(n) => n.to_string(),
        _ => return None,
    })
}

fn amqp(error: lapin::Error) -> MicroError {
    MicroError::transport(format!("RabbitMQ: {error}"))
}

fn declare_error(name: &str, error: &lapin::Error) -> MicroError {
    MicroError::transport(format!(
        "RabbitMQ refused to declare {name}: {error} (an existing queue or exchange with other settings, such as another micro.rabbitmq.queue_type, must be deleted first)"
    ))
}

/// Header values are one line.
fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_turn_the_multi_segment_wildcard_into_hash() {
        assert_eq!(routing_key("deal.won"), "deal.won");
        assert_eq!(routing_key("deal.*"), "deal.*");
        assert_eq!(routing_key("audit.>"), "audit.#");
    }

    #[test]
    fn envelopes_travel_as_amqp_properties() {
        let envelope = Envelope::new("deal.won", "{}")
            .with_header(headers::MESSAGE_ID, "m1")
            .with_header(headers::CONTENT_TYPE, "application/msgpack");
        let properties = properties(&envelope);
        assert_eq!(properties.delivery_mode(), &Some(2), "persistent");
        assert_eq!(properties.message_id().as_ref().map(ShortString::as_str), Some("m1"));
        assert_eq!(
            properties.content_type().as_ref().map(ShortString::as_str),
            Some("application/msgpack")
        );
        let table = properties.headers().clone().unwrap();
        assert!(table.inner().contains_key(&ShortString::from("navree-pattern")));
        assert_eq!(value_text(&AMQPValue::LongLongInt(3)), Some("3".into()));
    }

    #[test]
    fn queues_are_named_after_the_service_and_pattern() {
        let transport = RabbitmqTransport::new(
            "amqp://guest:secret@127.0.0.1:5672/%2f",
            &RabbitmqOptions::default(),
            "notifications",
            "notifications-1",
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(transport.queue("deal.won"), "notifications.deal.won");
        assert_eq!(transport.dead_exchange(), "navree.dead");
        assert!(!format!("{transport:?}").contains("secret"));
        let arguments = queue_arguments("quorum");
        assert_eq!(
            arguments.inner().get(&ShortString::from("x-queue-type")),
            Some(&AMQPValue::LongString("quorum".into()))
        );
    }

    #[test]
    fn bad_settings_are_refused() {
        let options = RabbitmqOptions::default();
        let error = RabbitmqTransport::new("nats://x", &options, "s", "s-1", Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("amqp://"), "{error}");
        let bad = RabbitmqOptions {
            queue_type: "stream".into(),
            ..RabbitmqOptions::default()
        };
        let error = RabbitmqTransport::new("amqp://x", &bad, "s", "s-1", Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("micro.rabbitmq.queue_type"), "{error}");
        let missing_ca = RabbitmqOptions {
            tls: crate::TlsOptions {
                ca: Some("missing-ca.pem".into()),
                ..crate::TlsOptions::default()
            },
            ..RabbitmqOptions::default()
        };
        let error = RabbitmqTransport::new("amqps://x", &missing_ca, "s", "s-1", Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("missing-ca.pem"), "{error}");
    }
}
