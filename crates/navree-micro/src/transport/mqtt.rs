//! [`MqttTransport`] (`mqtt` feature): MQTT 5, on rumqttc's MQTT 5 packet
//! codec with a session of Navree's own (packet ids, acknowledgments,
//! resends and subscriptions across reconnects).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use navree_kernel::BoxFuture;
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::mqttbytes::v5::{
    Codec, ConnAck, Connect, ConnectProperties, ConnectReturnCode, Disconnect,
    DisconnectReasonCode, Filter, Login, Packet, PingReq, PubAck, PubAckReason, Publish,
    PublishProperties, Subscribe, SubscribeProperties, SubscribeReasonCode, Unsubscribe,
};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{Notify, OnceCell, mpsc, oneshot, watch};
use tokio_rustls::TlsConnector;
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::envelope::headers;
use crate::options::MqttOptions;
use crate::transport::LocalRetry;
use crate::transport::broker::{BrokerUrl, client_config, reconnect_delay};
use crate::{
    DeadLetter, Delivery, DeliveryActions, Envelope, MicroError, MicroTransport, PatternKind,
    Subscribed, Subscription,
};

/// The URL schemes: `mqtt://` (1883) and `mqtts://` (TLS, 8883).
const SCHEMES: &[(&str, bool, u16)] = &[
    ("mqtt", false, 1883),
    ("mqtts", true, 8883),
    ("ssl", true, 8883),
    ("tcp", false, 1883),
];

/// Unacknowledged messages the broker may send this client at once.
const RECEIVE_MAXIMUM: u16 = 256;

/// The largest packet accepted.
const MAX_PACKET: u32 = 64 * 1024 * 1024;

/// How long a publish or a subscription waits for the broker's
/// acknowledgment.
const ACK_TIMEOUT: Duration = Duration::from_secs(30);

/// Messages kept for a listener that is not running (unacknowledged, so a
/// resumed session gets the rest from the broker).
const PARKED_MAX: usize = 10_000;

/// MQTT 5: a pattern is a topic under `micro.mqtt.prefix` with dots turned
/// into slashes (`deal.won` is `navree/deal/won`; `*` and `>` become `+`
/// and `#`).
///
/// - **Events** are published with QoS 1, and `emit` returns once the broker
///   acknowledged them. Each service subscribes with a shared subscription,
///   `$share/<service>/navree/deal/won`, so one instance of each service
///   gets each event; broadcast handlers subscribe without one. A handled
///   event is acknowledged (PUBACK); a failed one stays unacknowledged
///   while this instance retries it with its backoff (MQTT has no negative
///   acknowledgment), so the broker hands it out again if the instance
///   goes away first.
/// - **Requests** carry MQTT 5's response topic and correlation data; the
///   reply comes back on `<prefix>/replies/<client id>`, a topic private
///   to the instance. A request no service subscribes to fails at once with
///   [`MicroError::NoResponders`] when the broker reports "no matching
///   subscribers" in its PUBACK.
/// - **Sessions** use `micro.mqtt.session_expiry_secs`: with a stable
///   `micro.mqtt.client_id` per instance, a restarted instance resumes its
///   session and gets what was queued for it. On reconnect, unacknowledged
///   publishes are sent again and lost subscriptions are restored.
/// - **Dead letters** (`micro.events.dead_letter = "transport"`) go to
///   `<prefix>/dead/<service>/<topic>`.
pub struct MqttTransport {
    settings: Arc<Settings>,
    service: String,
    session: OnceCell<Arc<Session>>,
    closed: CancellationToken,
    /// How long the broker has to accept a listener's subscriptions.
    subscribe_timeout: Duration,
}

/// What a connection needs.
struct Settings {
    url: BrokerUrl,
    prefix: String,
    client_id: String,
    keep_alive: Duration,
    session_expiry: u32,
    username: Option<String>,
    password: Option<String>,
    tls: Option<TlsConnector>,
    connect_timeout: Duration,
}

impl MqttTransport {
    /// A transport for `url` (`mqtt://` or `mqtts://`) that listens as
    /// `service` (its shared subscriptions' group) and connects as
    /// `micro.mqtt.client_id`, else `instance`. Connects on first use,
    /// giving up after `connect_timeout`. `MicroModule` builds it from
    /// `micro.mqtt`; build one yourself for
    /// [`Transport::custom`](crate::Transport::custom) or the
    /// [conformance suite](crate::conformance).
    pub fn new(
        url: impl Into<String>,
        options: &MqttOptions,
        service: &str,
        instance: &str,
        connect_timeout: Duration,
    ) -> Result<Self, MicroError> {
        let url = BrokerUrl::parse(&url.into(), SCHEMES)?;
        let client_id = options
            .client_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| instance.to_owned());
        if client_id.contains(['/', '+', '#']) {
            return Err(MicroError::transport(format!(
                "micro.mqtt.client_id `{client_id}` may not hold `/`, `+` or `#`"
            )));
        }
        if options.prefix.contains(['+', '#']) {
            return Err(MicroError::transport(format!(
                "micro.mqtt.prefix `{}` may not hold `+` or `#`",
                options.prefix
            )));
        }
        let wants_tls = url.tls || options.tls.ca.is_some() || options.tls.cert.is_some();
        let tls = if wants_tls {
            Some(TlsConnector::from(client_config(
                &options.tls,
                "micro.mqtt",
            )?))
        } else {
            None
        };
        let settings = Settings {
            username: options.username.clone().or_else(|| url.username.clone()),
            password: options.password.clone().or_else(|| url.password.clone()),
            url,
            prefix: options.prefix.trim_end_matches('/').to_owned(),
            client_id,
            keep_alive: Duration::from_secs(options.keep_alive_secs.clamp(1, u64::from(u16::MAX))),
            session_expiry: options.session_expiry_secs,
            tls,
            connect_timeout,
        };
        Ok(Self {
            settings: Arc::new(settings),
            service: service.to_owned(),
            session: OnceCell::new(),
            closed: CancellationToken::new(),
            subscribe_timeout: ACK_TIMEOUT,
        })
    }

    /// Sets how long the broker has to accept all of a listener's subscriptions (30 seconds
    /// by default). A listen that is not answered in that time fails, and the listener that
    /// runs it starts it again.
    #[must_use]
    pub fn with_subscribe_timeout(mut self, timeout: Duration) -> Self {
        self.subscribe_timeout = timeout;
        self
    }

    /// The topic of `pattern`.
    fn topic(&self, pattern: &str) -> String {
        topic(&self.settings.prefix, pattern)
    }

    /// This instance's reply topic.
    fn reply_topic(&self) -> String {
        join(
            &self.settings.prefix,
            &format!("replies/{}", self.settings.client_id),
        )
    }

    /// The session, connected (or failing at once while the broker is
    /// unreachable).
    async fn session(&self) -> Result<Arc<Session>, MicroError> {
        let session = self.started().await;
        session.connected(self.settings.connect_timeout).await?;
        Ok(session)
    }

    /// The session, connected or not; its task keeps reconnecting.
    async fn started(&self) -> Arc<Session> {
        self.session
            .get_or_init(|| async { Session::start(self.settings.clone(), self.closed.clone()) })
            .await
            .clone()
    }
}

impl Drop for MqttTransport {
    fn drop(&mut self) {
        self.closed.cancel();
    }
}

impl fmt::Debug for MqttTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MqttTransport")
            .field("url", &self.settings.url)
            .field("prefix", &self.settings.prefix)
            .field("client_id", &self.settings.client_id)
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl MicroTransport for MqttTransport {
    fn name(&self) -> &'static str {
        "mqtt"
    }

    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        Box::pin(async move {
            let pattern = envelope.pattern().to_owned();
            let session = self.session().await?;
            let reply_topic = self.reply_topic();
            session
                .subscribe_replies(&reply_topic, ACK_TIMEOUT, &self.closed)
                .await?;
            let correlation = Bytes::copy_from_slice(Uuid::now_v7().as_bytes());
            let (sender, reply) = oneshot::channel();
            session.lock().replies.insert(correlation.clone(), sender);
            let forget = || {
                session.lock().replies.remove(&correlation);
            };
            let wait = envelope.remaining();
            let expiry = wait.map(|left| u32::try_from(left.as_secs().max(1)).unwrap_or(u32::MAX));
            let publish = to_publish(
                &self.topic(&pattern),
                &envelope,
                Some(reply_topic),
                Some(correlation.clone()),
                expiry,
            );
            let started = tokio::time::Instant::now();
            let limit = wait.unwrap_or(ACK_TIMEOUT).min(ACK_TIMEOUT);
            match session.publish(publish, limit).await {
                Ok(PubAckReason::Success) => {}
                Ok(PubAckReason::NoMatchingSubscribers) => {
                    forget();
                    return Err(MicroError::NoResponders { pattern });
                }
                Ok(reason) => {
                    forget();
                    return Err(MicroError::transport(format!(
                        "the MQTT broker refused the request: {reason:?}"
                    )));
                }
                Err(Unacknowledged) if wait.is_some() => {
                    forget();
                    return Err(MicroError::Timeout {
                        pattern,
                        after: wait.unwrap_or_default(),
                    });
                }
                Err(Unacknowledged) => {
                    forget();
                    return Err(MicroError::transport(
                        "the MQTT broker did not acknowledge the request",
                    ));
                }
            }
            let outcome = match wait {
                Some(wait) => tokio::time::timeout(wait.saturating_sub(started.elapsed()), reply)
                    .await
                    .ok(),
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
            let session = self.session().await?;
            let publish = to_publish(&self.topic(envelope.pattern()), &envelope, None, None, None);
            match session.publish(publish, ACK_TIMEOUT).await {
                Ok(PubAckReason::Success | PubAckReason::NoMatchingSubscribers) => Ok(()),
                Ok(reason) => Err(MicroError::transport(format!(
                    "the MQTT broker refused the event: {reason:?}"
                ))),
                Err(Unacknowledged) => Err(MicroError::transport(
                    "the MQTT broker did not acknowledge the event",
                )),
            }
        })
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        self.listen_subscribed(subscriptions, deliveries, Subscribed::detached())
    }

    /// An event is not kept for a topic nobody has subscribed to, so the listener says it is
    /// subscribed once the broker has accepted every subscription, and not before. All the
    /// SUBSCRIBEs go out together and the broker has the subscribe timeout (30 seconds unless
    /// [`with_subscribe_timeout`](Self::with_subscribe_timeout) says otherwise) to answer all
    /// of them.
    /// A connection that closes first only delays that: the subscriptions are asked for again
    /// when it comes back. If the broker refuses one, does not answer in time or the transport
    /// closes, the listen fails, and the listener that runs it starts it again.
    fn listen_subscribed(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
        subscribed: Subscribed,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            // Subscriptions made while the broker is unreachable are sent
            // once it is back.
            let session = self.started().await;
            let _ = session.connected(self.settings.connect_timeout).await;
            let entries: Vec<RouteEntry> = subscriptions
                .iter()
                .enumerate()
                .map(|(index, subscription)| {
                    let topic = self.topic(&subscription.pattern);
                    let path = if subscription.is_broadcast() {
                        topic.clone()
                    } else {
                        format!("$share/{}/{topic}", self.service)
                    };
                    RouteEntry {
                        index,
                        topic,
                        path,
                        kind: subscription.kind,
                        broadcast: subscription.is_broadcast(),
                    }
                })
                .collect();
            let (incoming, mut received) = mpsc::unbounded_channel();
            session.set_route(Route { incoming });
            for entry in &entries {
                session.subscribe(&entry.path, Some(entry.index + 1));
            }
            let paths: Vec<String> = entries.iter().map(|entry| entry.path.clone()).collect();
            if let Err(error) = session
                .subscribed(&paths, self.subscribe_timeout, &self.closed)
                .await
            {
                session.clear_route(&mut received);
                return Err(error);
            }
            subscribed.signal();
            loop {
                let publish = tokio::select! {
                    publish = received.recv() => publish,
                    () = deliveries.closed() => None,
                    () = self.closed.cancelled() => None,
                };
                let Some(publish) = publish else {
                    break;
                };
                let Some(delivery) = deliver(
                    &session,
                    &entries,
                    &self.settings.prefix,
                    publish,
                    &deliveries,
                ) else {
                    continue;
                };
                if deliveries.send(delivery).await.is_err() {
                    break;
                }
            }
            session.clear_route(&mut received);
            for entry in entries.iter().filter(|entry| entry.broadcast) {
                session.unsubscribe(&entry.path);
            }
            Ok(())
        })
    }

    fn setup(
        &self,
        _subscriptions: &[Subscription],
    ) -> BoxFuture<'_, Result<Vec<String>, MicroError>> {
        Box::pin(async {
            Ok(vec![
                "topics need no declaration: nothing to set up".to_owned(),
            ])
        })
    }

    fn is_connected(&self) -> bool {
        self.session.get().is_none_or(|session| session.is_up())
    }

    fn dead_letter(&self, letter: DeadLetter) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let session = self.session().await?;
            let mut envelope = letter.envelope();
            envelope.set_header("navree-error", single_line(&letter.error));
            envelope.set_header("navree-attempts", letter.attempts.to_string());
            let topic = join(
                &self.settings.prefix,
                &format!("dead/{}/{}", letter.service, topic("", &letter.pattern)),
            );
            match session
                .publish(to_publish(&topic, &envelope, None, None, None), ACK_TIMEOUT)
                .await
            {
                Ok(PubAckReason::Success | PubAckReason::NoMatchingSubscribers) => Ok(()),
                Ok(reason) => Err(MicroError::transport(format!(
                    "the MQTT broker refused the dead letter: {reason:?}"
                ))),
                Err(Unacknowledged) => Err(MicroError::transport(
                    "the MQTT broker did not acknowledge the dead letter",
                )),
            }
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.closed.cancel();
        })
    }
}

/// The broker did not acknowledge in time.
#[derive(Debug)]
struct Unacknowledged;

/// One subscription of a listener.
#[derive(Clone, Debug)]
struct RouteEntry {
    index: usize,
    /// The topic filter, without `$share/<group>/`.
    topic: String,
    /// What was subscribed (with `$share/<group>/` for groups).
    path: String,
    kind: PatternKind,
    broadcast: bool,
}

/// Where a listener wants its messages.
struct Route {
    incoming: mpsc::UnboundedSender<Publish>,
}

/// The connection state shared by the transport's calls and its
/// connection task.
struct Session {
    state: Mutex<State>,
    status: watch::Sender<Status>,
    /// Woken when a SUBACK says what became of a subscription.
    subscription_answered: Notify,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Connecting,
    Up,
    Down(String),
}

#[derive(Default)]
struct State {
    writer: Option<mpsc::UnboundedSender<Packet>>,
    last_pkid: u16,
    seq: u64,
    /// Outgoing QoS 1 publishes the broker has not acknowledged.
    inflight: HashMap<u16, Inflight>,
    /// SUBSCRIBEs sent on this connection and not yet answered: packet id to path.
    sub_pending: HashMap<u16, String>,
    /// The paths the broker has accepted, on this connection or in the session it kept.
    confirmed: HashSet<String>,
    /// What `confirmed` held when the connection was lost: the broker still has those if it
    /// kept the session, and does not if it did not.
    confirmed_before: HashSet<String>,
    /// The paths the broker refused, and what it said.
    refused: HashMap<String, String>,
    /// Every subscription, restored on a reconnect without a session.
    subscriptions: Vec<(String, Option<usize>)>,
    replies: HashMap<Bytes, oneshot::Sender<Envelope>>,
    reply_topic: Option<String>,
    route: Option<Route>,
    parked: VecDeque<Publish>,
}

struct Inflight {
    seq: u64,
    publish: Publish,
    waiter: Option<oneshot::Sender<PubAckReason>>,
}

impl State {
    /// A packet id that is free (1..=65535).
    fn next_pkid(&mut self) -> u16 {
        loop {
            self.last_pkid = self.last_pkid.wrapping_add(1).max(1);
            if !self.inflight.contains_key(&self.last_pkid)
                && !self.sub_pending.contains_key(&self.last_pkid)
            {
                return self.last_pkid;
            }
        }
    }

    fn write(&mut self, packet: Packet) {
        if let Some(writer) = &self.writer
            && writer.send(packet).is_err()
        {
            self.writer = None;
        }
    }
}

static SESSIONS: AtomicU64 = AtomicU64::new(0);

impl Session {
    /// A session whose connection task runs until `closed`.
    fn start(settings: Arc<Settings>, closed: CancellationToken) -> Arc<Session> {
        let (status, _) = watch::channel(Status::Connecting);
        let session = Arc::new(Session {
            state: Mutex::new(State::default()),
            status,
            subscription_answered: Notify::new(),
        });
        let id = SESSIONS.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(run(session.clone(), settings, closed, id));
        session
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_up(&self) -> bool {
        *self.status.borrow() == Status::Up
    }

    /// Ok once connected; waits for the first attempt (up to `wait`), and
    /// fails at once while the broker is unreachable after that.
    async fn connected(&self, wait: Duration) -> Result<(), MicroError> {
        let mut status = self.status.subscribe();
        let _ = tokio::time::timeout(wait, async {
            let _ = status
                .wait_for(|status| *status != Status::Connecting)
                .await;
        })
        .await;
        let current = status.borrow().clone();
        match current {
            Status::Up => Ok(()),
            Status::Down(error) => Err(MicroError::transport(error)),
            Status::Connecting => Err(MicroError::transport(format!(
                "connecting to the MQTT broker took longer than {wait:?}"
            ))),
        }
    }

    /// Publishes with QoS 1 and waits for the broker's PUBACK, up to
    /// `limit`. Resent on reconnect until acknowledged or given up.
    async fn publish(
        &self,
        mut publish: Publish,
        limit: Duration,
    ) -> Result<PubAckReason, Unacknowledged> {
        let (sender, ack) = oneshot::channel();
        let pkid = {
            let mut state = self.lock();
            let pkid = state.next_pkid();
            publish.pkid = pkid;
            publish.qos = QoS::AtLeastOnce;
            state.seq += 1;
            let seq = state.seq;
            state.inflight.insert(
                pkid,
                Inflight {
                    seq,
                    publish: publish.clone(),
                    waiter: Some(sender),
                },
            );
            state.write(Packet::Publish(publish));
            pkid
        };
        match tokio::time::timeout(limit, ack).await {
            Ok(Ok(reason)) => Ok(reason),
            _ => {
                self.lock().inflight.remove(&pkid);
                Err(Unacknowledged)
            }
        }
    }

    /// Asks for `path` with QoS 1 (and a subscription identifier) and keeps it across
    /// reconnects, without waiting: the SUBSCRIBE goes out now when connected, and when the
    /// connection comes back otherwise. [`subscribed`](Self::subscribed) waits for the answer.
    fn subscribe(&self, path: &str, id: Option<usize>) {
        let mut state = self.lock();
        if !state.subscriptions.iter().any(|(known, _)| known == path) {
            state.subscriptions.push((path.to_owned(), id));
        }
        state.refused.remove(path);
        if state.writer.is_some() {
            let pkid = state.next_pkid();
            state.sub_pending.insert(pkid, path.to_owned());
            state.write(subscribe_packet(pkid, path, id));
        }
    }

    /// Waits until the broker has accepted every one of `paths`, all of them against one
    /// `deadline`: they were all asked for together, so they are answered together. A
    /// connection that closes first only delays it, because a subscription that was asked for
    /// is asked again when the connection comes back, and its answer counts then. Fails when
    /// the broker refuses one, when the deadline passes, or when `closed` is cancelled.
    async fn subscribed(
        &self,
        paths: &[String],
        deadline: Duration,
        closed: &CancellationToken,
    ) -> Result<(), MicroError> {
        let until = tokio::time::Instant::now() + deadline;
        loop {
            let answered = self.subscription_answered.notified();
            tokio::pin!(answered);
            // Listening before looking, so an answer between the two is not missed.
            answered.as_mut().enable();
            let waiting: Vec<&String> = {
                let state = self.lock();
                if let Some((path, why)) = paths
                    .iter()
                    .find_map(|path| state.refused.get(path).map(|why| (path, why)))
                {
                    return Err(MicroError::transport(format!(
                        "the MQTT broker refused the subscription to {path}: {why}"
                    )));
                }
                paths
                    .iter()
                    .filter(|path| !state.confirmed.contains(*path))
                    .collect()
            };
            if waiting.is_empty() {
                return Ok(());
            }
            tokio::select! {
                () = &mut answered => {}
                () = tokio::time::sleep_until(until) => {
                    return Err(MicroError::transport(format!(
                        "the MQTT broker did not acknowledge the subscription to {}",
                        waiting.iter().map(|path| path.as_str()).collect::<Vec<_>>().join(", ")
                    )));
                }
                () = closed.cancelled() => {
                    return Err(MicroError::transport(
                        "the transport closed before the broker acknowledged the subscriptions",
                    ));
                }
            }
        }
    }

    /// Subscribes this instance's reply topic, once.
    async fn subscribe_replies(
        &self,
        topic: &str,
        deadline: Duration,
        closed: &CancellationToken,
    ) -> Result<(), MicroError> {
        if self.lock().reply_topic.as_deref() == Some(topic) {
            return Ok(());
        }
        self.subscribe(topic, None);
        self.subscribed(&[topic.to_owned()], deadline, closed)
            .await?;
        self.lock().reply_topic = Some(topic.to_owned());
        Ok(())
    }

    fn unsubscribe(&self, path: &str) {
        let mut state = self.lock();
        state.subscriptions.retain(|(known, _)| known != path);
        let mut unsubscribe = Unsubscribe::new(path, None);
        unsubscribe.pkid = state.next_pkid();
        state.write(Packet::Unsubscribe(unsubscribe));
    }

    /// Acknowledges an incoming QoS 1 publish.
    fn ack(&self, pkid: u16) {
        if pkid != 0 {
            self.lock().write(Packet::PubAck(PubAck::new(pkid, None)));
        }
    }

    /// Routes incoming messages to a listener, starting with the ones that
    /// arrived while none listened.
    fn set_route(&self, route: Route) {
        let mut state = self.lock();
        while let Some(publish) = state.parked.pop_front() {
            let _ = route.incoming.send(publish);
        }
        state.route = Some(route);
    }

    /// Stops routing to the listener; what it had not taken waits for the
    /// next one.
    fn clear_route(&self, received: &mut mpsc::UnboundedReceiver<Publish>) {
        let mut state = self.lock();
        state.route = None;
        while let Ok(publish) = received.try_recv() {
            if state.parked.len() < PARKED_MAX {
                state.parked.push_back(publish);
            }
        }
    }

    /// A connection is up: send what is waiting and restore the
    /// subscriptions the broker forgot.
    fn on_connected(&self, writer: mpsc::UnboundedSender<Packet>, session_present: bool) {
        let mut state = self.lock();
        state.writer = Some(writer);
        // A broker that kept the session still has what it had accepted; one that did not
        // forgot it all. Whatever it does not have is asked for, whether it was asked for before
        // or only while the connection was down, and counted once the broker says yes.
        let before = std::mem::take(&mut state.confirmed_before);
        if session_present {
            state.confirmed.extend(before);
        } else {
            state.confirmed.clear();
        }
        let subscriptions = state.subscriptions.clone();
        for (path, id) in subscriptions {
            if !state.confirmed.contains(&path) {
                let pkid = state.next_pkid();
                state.sub_pending.insert(pkid, path.clone());
                state.write(subscribe_packet(pkid, &path, id));
            }
        }
        let mut resend: Vec<(u64, Publish)> = state
            .inflight
            .values()
            .map(|inflight| {
                let mut publish = inflight.publish.clone();
                publish.dup = true;
                (inflight.seq, publish)
            })
            .collect();
        resend.sort_by_key(|(seq, _)| *seq);
        for (_, publish) in resend {
            state.write(Packet::Publish(publish));
        }
        drop(state);
        self.status.send_replace(Status::Up);
    }

    fn on_disconnected(&self, error: String) {
        let mut state = self.lock();
        state.writer = None;
        // Subscriptions waiting for a SUBACK are asked for again on reconnect, and none counts as
        // accepted until the broker says whether it kept them.
        state.sub_pending.clear();
        let confirmed = std::mem::take(&mut state.confirmed);
        state.confirmed_before.extend(confirmed);
        drop(state);
        self.status.send_replace(Status::Down(error));
    }

    /// Handles a packet from the broker.
    fn incoming(&self, packet: Packet) {
        match packet {
            Packet::PubAck(ack) => {
                if let Some(mut inflight) = self.lock().inflight.remove(&ack.pkid)
                    && let Some(waiter) = inflight.waiter.take()
                {
                    let _ = waiter.send(ack.reason);
                }
            }
            Packet::SubAck(ack) => {
                let mut state = self.lock();
                if let Some(path) = state.sub_pending.remove(&ack.pkid) {
                    match ack.return_codes.first() {
                        Some(SubscribeReasonCode::Success(_)) => {
                            state.confirmed.insert(path);
                        }
                        other => {
                            state.refused.insert(path, format!("{other:?}"));
                        }
                    }
                    drop(state);
                    self.subscription_answered.notify_waiters();
                }
            }
            Packet::Publish(publish) => self.on_publish(publish),
            _ => {}
        }
    }

    fn on_publish(&self, publish: Publish) {
        let mut state = self.lock();
        let is_reply = state
            .reply_topic
            .as_deref()
            .is_some_and(|topic| publish.topic.as_ref() == topic.as_bytes());
        if is_reply {
            let correlation = publish
                .properties
                .as_ref()
                .and_then(|properties| properties.correlation_data.clone());
            let waiter = correlation.and_then(|correlation| state.replies.remove(&correlation));
            if publish.qos != QoS::AtMostOnce {
                state.write(Packet::PubAck(PubAck::new(publish.pkid, None)));
            }
            drop(state);
            if let Some(waiter) = waiter {
                let _ = waiter.send(from_publish("", &publish));
            }
            return;
        }
        match &state.route {
            Some(route) => {
                let _ = route.incoming.send(publish);
            }
            None if state.parked.len() < PARKED_MAX => state.parked.push_back(publish),
            None => {}
        }
    }
}

trait Io: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

type Connection = Framed<Box<dyn Io>, Codec>;

/// Connects, reconnecting with backoff, until `closed`.
async fn run(session: Arc<Session>, settings: Arc<Settings>, closed: CancellationToken, id: u64) {
    let mut attempt = 0u32;
    loop {
        let connecting = tokio::select! {
            connecting = connect(&settings) => connecting,
            () = closed.cancelled() => return,
        };
        match connecting {
            Ok((connection, ack)) => {
                attempt = 0;
                let (writer, outgoing) = mpsc::unbounded_channel();
                session.on_connected(writer, ack.session_present);
                tracing::debug!(target: "navree::micro", session = id, client_id = %settings.client_id, "connected to the MQTT broker");
                let reason = serve(&session, connection, outgoing, &settings, &closed).await;
                session.on_disconnected(format!(
                    "the connection to the MQTT broker was lost: {reason}"
                ));
                if closed.is_cancelled() {
                    return;
                }
                tracing::warn!(target: "navree::micro", %reason, "the MQTT connection was lost; reconnecting");
            }
            Err(error) => {
                session.on_disconnected(error.to_string());
                tracing::warn!(target: "navree::micro", %error, "connecting to the MQTT broker failed");
            }
        }
        attempt = attempt.saturating_add(1);
        tokio::select! {
            () = tokio::time::sleep(reconnect_delay(attempt)) => {}
            () = closed.cancelled() => return,
        }
    }
}

/// Opens a connection: TCP, TLS when asked, CONNECT and CONNACK.
async fn connect(settings: &Settings) -> Result<(Connection, ConnAck), MicroError> {
    let address = settings.url.address();
    let connecting = async {
        let tcp = TcpStream::connect(&address).await.map_err(|error| {
            MicroError::transport(format!(
                "cannot connect to the MQTT broker at {address}: {error}"
            ))
        })?;
        let _ = tcp.set_nodelay(true);
        let io: Box<dyn Io> = match &settings.tls {
            Some(connector) => {
                let name = ServerName::try_from(settings.url.host.clone()).map_err(|error| {
                    MicroError::transport(format!("bad TLS server name: {error}"))
                })?;
                Box::new(connector.connect(name, tcp).await.map_err(|error| {
                    MicroError::transport(format!(
                        "the TLS handshake with {address} failed: {error}"
                    ))
                })?)
            }
            None => Box::new(tcp),
        };
        let mut connection = Framed::new(
            io,
            Codec {
                max_incoming_size: Some(MAX_PACKET),
                max_outgoing_size: None,
            },
        );
        let connect = Connect {
            keep_alive: u16::try_from(settings.keep_alive.as_secs()).unwrap_or(u16::MAX),
            client_id: settings.client_id.clone(),
            clean_start: false,
            properties: Some(ConnectProperties {
                session_expiry_interval: Some(settings.session_expiry),
                receive_maximum: Some(RECEIVE_MAXIMUM),
                ..ConnectProperties::new()
            }),
        };
        let login = settings.username.as_ref().map(|username| {
            Login::new(
                username.clone(),
                settings.password.clone().unwrap_or_default(),
            )
        });
        connection
            .send(Packet::Connect(connect, None, login))
            .await
            .map_err(|error| {
                MicroError::transport(format!("cannot send CONNECT to {address}: {error}"))
            })?;
        match connection.next().await {
            Some(Ok(Packet::ConnAck(ack))) if ack.code == ConnectReturnCode::Success => {
                Ok((connection, ack))
            }
            Some(Ok(Packet::ConnAck(ack))) => Err(MicroError::transport(format!(
                "the MQTT broker at {address} refused the connection: {:?}",
                ack.code
            ))),
            Some(Ok(other)) => Err(MicroError::transport(format!(
                "the MQTT broker at {address} answered CONNECT with {other:?}"
            ))),
            Some(Err(error)) => Err(MicroError::transport(format!(
                "reading CONNACK from {address} failed: {error}"
            ))),
            None => Err(MicroError::transport(format!(
                "the MQTT broker at {address} closed the connection"
            ))),
        }
    };
    match tokio::time::timeout(settings.connect_timeout, connecting).await {
        Ok(outcome) => outcome,
        Err(_) => Err(MicroError::transport(format!(
            "connecting to the MQTT broker at {address} took longer than {:?}",
            settings.connect_timeout
        ))),
    }
}

/// Reads and writes packets until the connection ends; returns why.
async fn serve(
    session: &Session,
    mut connection: Connection,
    mut outgoing: mpsc::UnboundedReceiver<Packet>,
    settings: &Settings,
    closed: &CancellationToken,
) -> String {
    let mut ping = tokio::time::interval(settings.keep_alive);
    ping.tick().await;
    loop {
        tokio::select! {
            incoming = connection.next() => match incoming {
                Some(Ok(packet)) => session.incoming(packet),
                Some(Err(error)) => return error.to_string(),
                None => return "the broker closed the connection".to_owned(),
            },
            packet = outgoing.recv() => match packet {
                Some(packet) => {
                    if let Err(error) = connection.send(packet).await {
                        return error.to_string();
                    }
                }
                None => return "the session ended".to_owned(),
            },
            _ = ping.tick() => {
                if let Err(error) = connection.send(Packet::PingReq(PingReq)).await {
                    return error.to_string();
                }
            }
            () = closed.cancelled() => {
                // Send what is queued (acknowledgments), then say goodbye.
                while let Ok(packet) = outgoing.try_recv() {
                    let _ = connection.send(packet).await;
                }
                let _ = connection
                    .send(Packet::Disconnect(Disconnect::new(DisconnectReasonCode::NormalDisconnection)))
                    .await;
                return "closed".to_owned();
            }
        }
    }
}

/// A SUBSCRIBE of one filter with QoS 1.
fn subscribe_packet(pkid: u16, path: &str, id: Option<usize>) -> Packet {
    let properties = id.map(|id| SubscribeProperties {
        id: Some(id),
        user_properties: Vec::new(),
    });
    let mut subscribe = Subscribe::new(Filter::new(path, QoS::AtLeastOnce), properties);
    subscribe.pkid = pkid;
    Packet::Subscribe(subscribe)
}

/// The delivery of an incoming publish, or `None` when no subscription of
/// the listener matches it.
fn deliver(
    session: &Arc<Session>,
    entries: &[RouteEntry],
    prefix: &str,
    publish: Publish,
    deliveries: &mpsc::Sender<Delivery>,
) -> Option<Delivery> {
    let topic = String::from_utf8_lossy(&publish.topic).into_owned();
    let identifiers = publish
        .properties
        .as_ref()
        .map(|properties| properties.subscription_identifiers.clone())
        .unwrap_or_default();
    let entry = identifiers
        .iter()
        .find_map(|id| entries.iter().find(|entry| entry.index + 1 == *id))
        .or_else(|| {
            entries
                .iter()
                .find(|entry| filter_matches(&entry.topic, &topic))
        });
    let Some(entry) = entry else {
        // Not ours any more: acknowledge so the broker stops resending it.
        session.ack(publish.pkid);
        return None;
    };
    let envelope = from_publish(prefix, &publish);
    let attempt = if publish.dup { 2 } else { 1 };
    let pkid = if publish.qos == QoS::AtMostOnce {
        0
    } else {
        publish.pkid
    };
    Some(match (entry.kind, entry.broadcast) {
        (PatternKind::Event, true) => {
            session.ack(pkid);
            LocalRetry::delivery(deliveries.clone(), envelope, entry.index)
        }
        (PatternKind::Event, false) => {
            let actions = EventActions {
                session: session.clone(),
                pkid,
                deliveries: deliveries.clone(),
                envelope: envelope.clone(),
                index: entry.index,
                attempt,
            };
            Delivery::new(envelope, entry.index, attempt, actions)
        }
        (PatternKind::Message, _) => {
            let properties = publish.properties.unwrap_or_default();
            let actions = RequestActions {
                session: session.clone(),
                pkid,
                response_topic: properties.response_topic,
                correlation: properties.correlation_data,
            };
            Delivery::new(envelope, entry.index, 1, actions)
        }
    })
}

/// An event: acknowledged once handled; retried in this instance (it stays
/// unacknowledged meanwhile).
struct EventActions {
    session: Arc<Session>,
    pkid: u16,
    deliveries: mpsc::Sender<Delivery>,
    envelope: Envelope,
    index: usize,
    attempt: u32,
}

impl DeliveryActions for EventActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        self.session.ack(self.pkid);
        Box::pin(async { Ok(()) })
    }

    fn retry_in(self: Box<Self>, delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        let this = *self;
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let deliveries = this.deliveries.clone();
            let attempt = this.attempt + 1;
            let envelope = this.envelope.clone();
            let index = this.index;
            let actions = EventActions { attempt, ..this };
            // A stopped listener drops it: the broker sends it again to
            // the resumed session or another instance.
            let _ = deliveries
                .send(Delivery::new(envelope, index, attempt, actions))
                .await;
        });
        Box::pin(async { Ok(()) })
    }

    fn reply(self: Box<Self>, _reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        self.ack()
    }
}

/// A request: the reply goes to its response topic with its correlation
/// data, then the request is acknowledged.
struct RequestActions {
    session: Arc<Session>,
    pkid: u16,
    response_topic: Option<String>,
    correlation: Option<Bytes>,
}

impl DeliveryActions for RequestActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        self.session.ack(self.pkid);
        Box::pin(async { Ok(()) })
    }

    fn retry_in(self: Box<Self>, _delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        self.ack()
    }

    fn reply(self: Box<Self>, reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            let outcome = match &self.response_topic {
                Some(topic) => {
                    let publish = to_publish(topic, &reply, None, self.correlation.clone(), None);
                    match self.session.publish(publish, ACK_TIMEOUT).await {
                        Ok(_) => Ok(()),
                        Err(Unacknowledged) => Err(MicroError::transport(
                            "the MQTT broker did not acknowledge the reply",
                        )),
                    }
                }
                None => Ok(()),
            };
            self.session.ack(self.pkid);
            outcome
        })
    }
}

/// `<prefix>/<rest>`, or `rest` without a prefix.
fn join(prefix: &str, rest: &str) -> String {
    if prefix.is_empty() {
        rest.to_owned()
    } else {
        format!("{prefix}/{rest}")
    }
}

/// The topic of `pattern` under `prefix`: dots become slashes, `*` and `>`
/// become `+` and `#`.
fn topic(prefix: &str, pattern: &str) -> String {
    let path = pattern
        .split('.')
        .map(|segment| match segment {
            "*" => "+",
            ">" => "#",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("/");
    join(prefix, &path)
}

/// The pattern of a topic under `prefix`.
fn pattern_of(prefix: &str, topic: &str) -> String {
    let rest = if prefix.is_empty() {
        topic
    } else {
        topic
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(topic)
    };
    rest.replace('/', ".")
}

/// Whether topic filter `filter` (`+`, `#`) matches `topic`.
fn filter_matches(filter: &str, topic: &str) -> bool {
    let mut levels = topic.split('/');
    for part in filter.split('/') {
        match (part, levels.next()) {
            ("#", _) => return true,
            ("+", Some(_)) => {}
            (part, Some(level)) if part == level => {}
            _ => return false,
        }
    }
    levels.next().is_none()
}

/// A QoS 1 publish of `envelope` to `topic`: the headers as user
/// properties, the content type, and the request's response topic,
/// correlation data and expiry.
fn to_publish(
    topic: &str,
    envelope: &Envelope,
    response_topic: Option<String>,
    correlation: Option<Bytes>,
    expiry: Option<u32>,
) -> Publish {
    let properties = PublishProperties {
        content_type: Some(envelope.content_type().to_owned()),
        user_properties: envelope
            .headers()
            .iter()
            .map(|(name, value)| (name.clone(), single_line(value)))
            .collect(),
        response_topic,
        correlation_data: correlation,
        message_expiry_interval: expiry,
        ..PublishProperties::default()
    };
    Publish::new(
        topic,
        QoS::AtLeastOnce,
        envelope.payload().clone(),
        Some(properties),
    )
}

/// The envelope of a publish: user properties are headers; the pattern is
/// `navree-pattern`, else the topic's.
fn from_publish(prefix: &str, publish: &Publish) -> Envelope {
    let topic = String::from_utf8_lossy(&publish.topic);
    let properties = publish.properties.clone().unwrap_or_default();
    let pattern = properties
        .user_properties
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(headers::PATTERN))
        .map_or_else(|| pattern_of(prefix, &topic), |(_, value)| value.clone());
    let mut envelope = Envelope::new(pattern, publish.payload.clone());
    if let Some(content_type) = properties.content_type {
        envelope.set_header(headers::CONTENT_TYPE, content_type);
    }
    for (name, value) in properties.user_properties {
        envelope.set_header(&name, value);
    }
    envelope
}

/// Header values are one line.
fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_topics_under_the_prefix() {
        assert_eq!(topic("navree", "deal.won"), "navree/deal/won");
        assert_eq!(topic("navree", "deal.*"), "navree/deal/+");
        assert_eq!(topic("crm", "audit.>"), "crm/audit/#");
        assert_eq!(topic("", "deal.won"), "deal/won");
        assert_eq!(pattern_of("navree", "navree/deal/won"), "deal.won");
        assert_eq!(pattern_of("", "deal/won"), "deal.won");
    }

    #[test]
    fn filters_match_with_plus_and_hash() {
        assert!(filter_matches("navree/deal/+", "navree/deal/won"));
        assert!(!filter_matches("navree/deal/+", "navree/deal/won/late"));
        assert!(filter_matches("navree/audit/#", "navree/audit/a/b"));
        assert!(filter_matches("navree/deal/won", "navree/deal/won"));
        assert!(!filter_matches("navree/deal/won", "navree/deal/lost"));
    }

    #[test]
    fn envelopes_travel_as_user_properties() {
        let envelope = Envelope::new("deal.won", "{}")
            .with_header(headers::MESSAGE_ID, "m1")
            .with_header(headers::CONTENT_TYPE, "application/msgpack");
        let publish = to_publish(
            "navree/deal/won",
            &envelope,
            Some("navree/replies/x".into()),
            Some(Bytes::from_static(b"c")),
            Some(5),
        );
        assert_eq!(publish.qos, QoS::AtLeastOnce);
        let properties = publish.properties.clone().unwrap();
        assert_eq!(
            properties.content_type.as_deref(),
            Some("application/msgpack")
        );
        assert_eq!(
            properties.response_topic.as_deref(),
            Some("navree/replies/x")
        );
        assert_eq!(properties.message_expiry_interval, Some(5));
        let back = from_publish("navree", &publish);
        assert_eq!(back.pattern(), "deal.won");
        assert_eq!(back.message_id(), Some("m1"));
        assert_eq!(back.content_type(), "application/msgpack");
        let bare = from_publish(
            "navree",
            &Publish::new("navree/deal/lost", QoS::AtLeastOnce, "x", None),
        );
        assert_eq!(bare.pattern(), "deal.lost");
    }

    #[test]
    fn settings_are_checked() {
        let options = MqttOptions::default();
        let transport = MqttTransport::new(
            "mqtt://u:secret@127.0.0.1",
            &options,
            "deals",
            "deals-1",
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(transport.settings.url.address(), "127.0.0.1:1883");
        assert_eq!(transport.settings.client_id, "deals-1");
        assert_eq!(transport.settings.username.as_deref(), Some("u"));
        assert_eq!(transport.reply_topic(), "navree/replies/deals-1");
        assert!(!format!("{transport:?}").contains("secret"));
        let bad = MqttOptions {
            client_id: Some("a/b".into()),
            ..MqttOptions::default()
        };
        let error =
            MqttTransport::new("mqtt://x", &bad, "s", "s-1", Duration::from_secs(1)).unwrap_err();
        assert!(
            error.to_string().contains("micro.mqtt.client_id"),
            "{error}"
        );
        let error = MqttTransport::new("amqp://x", &options, "s", "s-1", Duration::from_secs(1))
            .unwrap_err();
        assert!(error.to_string().contains("mqtt://"), "{error}");
    }

    #[test]
    fn packet_ids_skip_zero_and_the_ones_in_use() {
        let mut state = State {
            last_pkid: u16::MAX - 1,
            ..State::default()
        };
        assert_eq!(state.next_pkid(), u16::MAX);
        assert_eq!(state.next_pkid(), 1, "wraps past zero");
        state.sub_pending.insert(2, "a/b".to_owned());
        assert_eq!(state.next_pkid(), 3, "2 is in use");
    }
}
