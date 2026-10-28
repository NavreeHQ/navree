//! The transport conformance suite: the behavior every [`MicroTransport`]
//! must have, run against the memory and TCP transports in this crate's
//! tests, and usable for your own.
//!
//! Implement [`Harness`] for your transport and call [`run`] (or the
//! checks one by one) from a test:
//!
//! ```ignore
//! #[tokio::test]
//! async fn my_transport_conforms() {
//!     navree_micro::conformance::run(&MyHarness::new()).await;
//! }
//! ```
//!
//! Each check uses its own patterns, prefixed with the service it talks
//! to (`conf-echo.ping` goes to service `conf-echo`), so a transport that
//! routes by prefix (TCP) can be set up from the instances alone.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use navree_kernel::BoxFuture;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::envelope::headers;
use crate::{
    Delivery, Envelope, Group, MicroError, MicroTransport, PatternKind, Subscribed, Subscription,
};

/// What a transport can do; checks that need something it lacks are
/// skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// Two instances of a service share its messages and events.
    pub consumer_groups: bool,
    /// Events published while a service is away wait for it.
    pub durable: bool,
    /// Nobody listening is reported at once (rather than as a timeout).
    pub no_responders: bool,
    /// Event patterns may hold `*` and `>`.
    pub wildcards: bool,
}

/// Builds the transports one check uses.
pub trait Harness: Send + Sync {
    /// What the transport can do.
    fn capabilities(&self) -> Capabilities;

    /// A transport for a new instance of `service`, which will listen to
    /// `patterns`.
    fn instance<'a>(
        &'a self,
        service: &'a str,
        patterns: &'a [&'a str],
    ) -> BoxFuture<'a, Arc<dyn MicroTransport>>;

    /// A transport that sends to the instances created so far (once they
    /// listen).
    fn client(&self) -> BoxFuture<'_, Arc<dyn MicroTransport>>;
}

/// Runs every check.
pub async fn run(harness: &dyn Harness) {
    request_reply(harness).await;
    no_responders(harness).await;
    timeout(harness).await;
    events_reach_every_service(harness).await;
    consumer_groups_share_messages(harness).await;
    broadcast_reaches_every_instance(harness).await;
    retries_redeliver_events(harness).await;
    per_key_order(harness).await;
    durable_events_wait(harness).await;
    wildcards(harness).await;
}

/// A listening instance: its deliveries and its listen task.
pub struct Listening {
    deliveries: mpsc::Receiver<Delivery>,
    task: JoinHandle<Result<(), MicroError>>,
}

impl Listening {
    /// Starts `transport` listening to `subscriptions`.
    pub async fn start(
        transport: Arc<dyn MicroTransport>,
        subscriptions: Vec<Subscription>,
    ) -> Listening {
        let (sender, deliveries) = mpsc::channel(64);
        let (subscribed, is_subscribed) = Subscribed::channel();
        let task = tokio::spawn(async move {
            transport
                .listen_subscribed(subscriptions, sender, subscribed)
                .await
        });
        // The transport says when the broker has the subscriptions; the
        // pause covers transports that start consuming a moment later.
        let _ = tokio::time::timeout(WAIT, is_subscribed).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        Listening { deliveries, task }
    }

    /// The next delivery, within `wait`.
    pub async fn next(&mut self, wait: Duration) -> Option<Delivery> {
        tokio::time::timeout(wait, self.deliveries.recv())
            .await
            .ok()
            .flatten()
    }

    /// Stops listening.
    pub async fn stop(self) {
        drop(self.deliveries);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}

const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(300);

fn subscription(pattern: &str, kind: PatternKind, group: Group) -> Subscription {
    Subscription {
        pattern: pattern.to_owned(),
        kind,
        group,
        ordered: false,
        ack_wait: Duration::from_secs(30),
    }
}

fn message(pattern: &str, service: &str) -> Subscription {
    subscription(
        pattern,
        PatternKind::Message,
        Group::Service(service.to_owned()),
    )
}

fn event(pattern: &str, service: &str) -> Subscription {
    subscription(
        pattern,
        PatternKind::Event,
        Group::Service(service.to_owned()),
    )
}

fn request(pattern: &str, payload: &'static str, wait: Duration) -> Envelope {
    let mut envelope = Envelope::new(pattern, Bytes::from_static(payload.as_bytes()))
        .with_header(headers::MESSAGE_ID, uuid::Uuid::now_v7().to_string())
        .with_header(headers::CONTENT_TYPE, "application/json")
        .with_header("x-custom", "kept");
    envelope.set_deadline(SystemTime::now() + wait);
    envelope
}

fn published(pattern: &str, payload: String) -> Envelope {
    Envelope::new(pattern, payload)
        .with_header(headers::MESSAGE_ID, uuid::Uuid::now_v7().to_string())
        .with_header(headers::CONTENT_TYPE, "application/json")
}

/// A request reaches the one instance that answers its pattern, with its
/// headers, and the reply comes back with its status.
pub async fn request_reply(harness: &dyn Harness) {
    let service = "conf-echo";
    let transport = harness.instance(service, &["conf-echo.ping"]).await;
    let mut listening = Listening::start(transport, vec![message("conf-echo.ping", service)]).await;
    let client = harness.client().await;
    let responder = tokio::spawn(async move {
        let delivery = listening.next(WAIT).await.expect("the request arrives");
        assert_eq!(delivery.subscription(), 0);
        assert_eq!(delivery.envelope().pattern(), "conf-echo.ping");
        assert_eq!(
            delivery.envelope().header("x-custom"),
            Some("kept"),
            "headers travel"
        );
        let reply = delivery.envelope().reply(
            201,
            "application/json",
            delivery.envelope().payload().clone(),
        );
        delivery.reply(reply).await.expect("the reply is sent");
        listening
    });
    let sent = request("conf-echo.ping", r#"{"n":1}"#, WAIT);
    let reply = client.request(sent.clone()).await.expect("a reply");
    assert_eq!(reply.status(), Some(201));
    assert_eq!(reply.payload().as_ref(), br#"{"n":1}"#);
    assert_eq!(
        reply.message_id(),
        sent.message_id(),
        "the reply keeps the message id"
    );
    responder.await.expect("responder").stop().await;
}

/// A request nobody answers fails at once with `NoResponders` (or, on
/// transports that cannot know, with `Timeout`).
pub async fn no_responders(harness: &dyn Harness) {
    let client = harness.client().await;
    let started = std::time::Instant::now();
    let outcome = client
        .request(request("conf-nobody.ping", "{}", Duration::from_secs(2)))
        .await;
    match outcome {
        Err(MicroError::NoResponders { pattern }) => {
            assert_eq!(pattern, "conf-nobody.ping");
            assert!(started.elapsed() < Duration::from_secs(2), "at once");
        }
        Err(MicroError::Timeout { .. }) if !harness.capabilities().no_responders => {}
        other => panic!("expected no responders, got {other:?}"),
    }
}

/// A request without a reply ends at its deadline.
pub async fn timeout(harness: &dyn Harness) {
    let service = "conf-slow";
    let transport = harness.instance(service, &["conf-slow.ping"]).await;
    let mut listening = Listening::start(transport, vec![message("conf-slow.ping", service)]).await;
    let client = harness.client().await;
    let started = std::time::Instant::now();
    let outcome = client
        .request(request("conf-slow.ping", "{}", Duration::from_millis(300)))
        .await;
    assert!(
        matches!(outcome, Err(MicroError::Timeout { .. })),
        "expected a timeout, got {outcome:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    let _held = listening.next(WAIT).await;
    listening.stop().await;
}

/// An event reaches every subscribing service once.
pub async fn events_reach_every_service(harness: &dyn Harness) {
    let mut instances = Vec::new();
    for service in ["conf-fan-a", "conf-fan-b"] {
        let transport = harness.instance(service, &["conf-fan.happened"]).await;
        instances
            .push(Listening::start(transport, vec![event("conf-fan.happened", service)]).await);
    }
    let client = harness.client().await;
    client
        .publish(published("conf-fan.happened", "1".into()))
        .await
        .expect("published");
    for listening in &mut instances {
        let delivery = listening
            .next(WAIT)
            .await
            .expect("every service gets the event");
        assert_eq!(delivery.envelope().payload().as_ref(), b"1");
        delivery.ack().await.expect("ack");
        assert!(listening.next(QUIET).await.is_none(), "once per service");
    }
    for listening in instances {
        listening.stop().await;
    }
}

/// Two instances of one service share its events: each event is handled
/// by one of them.
pub async fn consumer_groups_share_messages(harness: &dyn Harness) {
    if !harness.capabilities().consumer_groups {
        return;
    }
    let service = "conf-group";
    let mut instances = Vec::new();
    for _ in 0..2 {
        let transport = harness.instance(service, &["conf-group.happened"]).await;
        instances
            .push(Listening::start(transport, vec![event("conf-group.happened", service)]).await);
    }
    let client = harness.client().await;
    for n in 0..10 {
        client
            .publish(published("conf-group.happened", n.to_string()))
            .await
            .expect("published");
    }
    let mut seen = Vec::new();
    loop {
        let mut got = false;
        for listening in &mut instances {
            if let Some(delivery) = listening.next(Duration::from_millis(200)).await {
                seen.push(String::from_utf8_lossy(delivery.envelope().payload()).into_owned());
                delivery.ack().await.expect("ack");
                got = true;
            }
        }
        if !got {
            break;
        }
    }
    seen.sort_by_key(|n| n.parse::<u32>().unwrap_or_default());
    let expected: Vec<String> = (0..10).map(|n: u32| n.to_string()).collect();
    assert_eq!(seen, expected, "each event once, over the group");
    for listening in instances {
        listening.stop().await;
    }
}

/// A broadcast subscription gets every event on every instance.
pub async fn broadcast_reaches_every_instance(harness: &dyn Harness) {
    let mut instances = Vec::new();
    for instance in ["conf-cast-1", "conf-cast-2"] {
        let transport = harness.instance("conf-cast", &["conf-cast.changed"]).await;
        instances.push(
            Listening::start(
                transport,
                vec![subscription(
                    "conf-cast.changed",
                    PatternKind::Event,
                    Group::Instance(instance.to_owned()),
                )],
            )
            .await,
        );
    }
    let client = harness.client().await;
    client
        .publish(published("conf-cast.changed", "x".into()))
        .await
        .expect("published");
    for listening in &mut instances {
        let delivery = listening.next(WAIT).await.expect("every instance gets it");
        delivery.ack().await.expect("ack");
    }
    for listening in instances {
        listening.stop().await;
    }
}

/// `retry_in` delivers the event again, as the next attempt.
pub async fn retries_redeliver_events(harness: &dyn Harness) {
    let service = "conf-retry";
    let transport = harness.instance(service, &["conf-retry.happened"]).await;
    let mut listening =
        Listening::start(transport, vec![event("conf-retry.happened", service)]).await;
    let client = harness.client().await;
    let sent = published("conf-retry.happened", "r".into());
    client.publish(sent.clone()).await.expect("published");
    let first = listening.next(WAIT).await.expect("first attempt");
    assert_eq!(first.attempt(), 1);
    first
        .retry_in(Duration::from_millis(20))
        .await
        .expect("retry");
    let second = listening.next(WAIT).await.expect("second attempt");
    assert_eq!(second.attempt(), 2);
    assert_eq!(
        second.envelope().message_id(),
        sent.message_id(),
        "same message id"
    );
    second.ack().await.expect("ack");
    assert!(
        listening.next(QUIET).await.is_none(),
        "acknowledged events stay handled"
    );
    listening.stop().await;
}

/// Events with the same key arrive in the order they were published.
pub async fn per_key_order(harness: &dyn Harness) {
    let service = "conf-order";
    let transport = harness.instance(service, &["conf-order.updated"]).await;
    let mut listening =
        Listening::start(transport, vec![event("conf-order.updated", service)]).await;
    let client = harness.client().await;
    for n in 0..20 {
        let envelope =
            published("conf-order.updated", n.to_string()).with_header(headers::KEY, "deal-1");
        client.publish(envelope).await.expect("published");
    }
    let mut seen = Vec::new();
    while let Some(delivery) = listening.next(Duration::from_secs(2)).await {
        assert_eq!(delivery.envelope().key(), Some("deal-1"), "the key travels");
        seen.push(
            String::from_utf8_lossy(delivery.envelope().payload())
                .parse::<u32>()
                .unwrap_or(999),
        );
        delivery.ack().await.expect("ack");
        if seen.len() == 20 {
            break;
        }
    }
    assert_eq!(seen, (0..20).collect::<Vec<u32>>());
    listening.stop().await;
}

/// Events published while a service is away wait for it (durable
/// transports).
pub async fn durable_events_wait(harness: &dyn Harness) {
    if !harness.capabilities().durable {
        return;
    }
    let service = "conf-away";
    let transport = harness.instance(service, &["conf-away.happened"]).await;
    let listening = Listening::start(
        transport.clone(),
        vec![event("conf-away.happened", service)],
    )
    .await;
    listening.stop().await;
    let client = harness.client().await;
    client
        .publish(published("conf-away.happened", "waited".into()))
        .await
        .expect("published");
    let mut back = Listening::start(transport, vec![event("conf-away.happened", service)]).await;
    let delivery = back.next(WAIT).await.expect("the event waited");
    assert_eq!(delivery.envelope().payload().as_ref(), b"waited");
    delivery.ack().await.expect("ack");
    back.stop().await;
}

/// `*` matches one segment and `>` the rest (transports with wildcards).
pub async fn wildcards(harness: &dyn Harness) {
    if !harness.capabilities().wildcards {
        return;
    }
    let service = "conf-wild";
    let transport = harness
        .instance(service, &["conf-wild.*", "conf-wild.deep.>"])
        .await;
    let mut listening = Listening::start(
        transport,
        vec![
            event("conf-wild.*", service),
            event("conf-wild.deep.>", service),
        ],
    )
    .await;
    let client = harness.client().await;
    let mut expected = HashMap::new();
    for (pattern, subscription) in [("conf-wild.won", 0), ("conf-wild.deep.a.b", 1)] {
        client
            .publish(published(pattern, "w".into()))
            .await
            .expect("published");
        expected.insert(pattern.to_owned(), subscription);
    }
    for _ in 0..2 {
        let delivery = listening.next(WAIT).await.expect("a wildcard delivery");
        let pattern = delivery.envelope().pattern().to_owned();
        assert_eq!(
            expected.remove(&pattern),
            Some(delivery.subscription()),
            "{pattern} matched its subscription"
        );
        delivery.ack().await.expect("ack");
    }
    listening.stop().await;
}
