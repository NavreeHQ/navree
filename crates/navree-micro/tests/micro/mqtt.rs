//! The MQTT transport (`mqtt` feature): the conformance suite, two apps,
//! reconnecting and the settings, against the small broker in
//! `mqtt_server.rs`; with `NAVREE_TEST_MQTT_URL` set (an MQTT 5 broker),
//! the suite again on it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_kernel::BoxFuture;
use navree_micro::conformance::{self, Capabilities, Harness, Listening};
use navree_micro::{
    Envelope, Group, MicroError, MicroOptions, MicroTransport, MqttOptions, MqttTransport,
    PatternKind, Subscription, Transport, headers,
};

use crate::mqtt_server::MiniMqtt;
use crate::services::deals_answer_the_api_over;

/// Every instance on one broker.
struct MqttHarness {
    url: String,
    options: MqttOptions,
    next: AtomicUsize,
}

impl MqttHarness {
    fn new(url: String, options: MqttOptions) -> Self {
        Self {
            url,
            options,
            next: AtomicUsize::new(0),
        }
    }

    fn transport(&self, service: &str) -> Arc<dyn MicroTransport> {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        Arc::new(
            MqttTransport::new(
                self.url.clone(),
                &self.options,
                service,
                &format!(
                    "{service}-{n}-{}",
                    &uuid::Uuid::now_v7().simple().to_string()[24..]
                ),
                Duration::from_secs(2),
            )
            .expect("an MQTT transport"),
        )
    }
}

impl Harness for MqttHarness {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            consumer_groups: true,
            durable: true,
            no_responders: true,
            wildcards: true,
        }
    }

    fn instance<'a>(
        &'a self,
        service: &'a str,
        _patterns: &'a [&'a str],
    ) -> BoxFuture<'a, Arc<dyn MicroTransport>> {
        Box::pin(async move { self.transport(service) })
    }

    fn client(&self) -> BoxFuture<'_, Arc<dyn MicroTransport>> {
        Box::pin(async move { self.transport("conf-client") })
    }
}

fn event(pattern: &str, service: &str) -> Subscription {
    Subscription {
        pattern: pattern.to_owned(),
        kind: PatternKind::Event,
        group: Group::Service(service.to_owned()),
        ordered: false,
        ack_wait: Duration::from_secs(30),
    }
}

fn published(pattern: &str, payload: &str) -> Envelope {
    Envelope::new(pattern, payload.to_owned())
        .with_header(headers::MESSAGE_ID, uuid::Uuid::now_v7().to_string())
}

#[tokio::test]
async fn mqtt_conforms() {
    let broker = MiniMqtt::start().await;
    conformance::run(&MqttHarness::new(broker.url(), MqttOptions::default())).await;
}

#[tokio::test]
async fn two_apps_talk_over_mqtt() {
    let broker = MiniMqtt::start().await;
    let options = || MicroOptions {
        mqtt: MqttOptions {
            prefix: "crm".into(),
            ..MqttOptions::default()
        },
        ..MicroOptions::from(Transport::mqtt(broker.url()))
    };
    deals_answer_the_api_over(options(), options()).await;
    let topics = broker.published();
    assert!(
        topics.iter().any(|topic| topic == "crm/deals/get"),
        "{topics:?}"
    );
    assert!(
        topics.iter().any(|topic| topic.starts_with("crm/replies/")),
        "{topics:?}"
    );
}

/// A listener that has said it is subscribed gets what is sent next: an event is not kept for a
/// topic nobody has subscribed to. A slow broker makes the time between saying so and being
/// subscribed long enough to see it.
#[tokio::test]
async fn an_event_sent_once_a_listener_is_subscribed_reaches_it() {
    let broker = MiniMqtt::start().await;
    broker.slow_subscriptions(Duration::from_millis(400));
    let harness = MqttHarness::new(broker.url(), MqttOptions::default());
    let client = harness.transport("shop");
    let mut listening = Listening::start(
        harness.transport("billing"),
        vec![event("invoice.paid", "billing")],
    )
    .await;
    client
        .publish(published("invoice.paid", "now"))
        .await
        .unwrap();
    // The event is there once it is sent; this only ends a hang.
    let delivery = listening
        .next(Duration::from_secs(20))
        .await
        .expect("the event sent once the listener was subscribed");
    assert_eq!(delivery.envelope().payload().as_ref(), b"now");
    delivery.ack().await.unwrap();
    listening.stop().await;
}

#[tokio::test]
async fn a_listener_gets_its_subscriptions_back_after_the_broker_restarts() {
    // The broker goes away and comes back on the same port.
    let first = MiniMqtt::start().await;
    let url = first.url();
    let addr = url.trim_start_matches("mqtt://").to_owned();
    let harness = MqttHarness::new(url, MqttOptions::default());
    let mut listening = Listening::start(
        harness.transport("billing"),
        vec![event("invoice.paid", "billing")],
    )
    .await;
    let client = harness.transport("shop");
    client
        .publish(published("invoice.paid", "1"))
        .await
        .unwrap();
    listening
        .next(Duration::from_secs(5))
        .await
        .expect("before")
        .ack()
        .await
        .unwrap();

    // What the new broker has to see again before the test goes on.
    let (clients, subscriptions) = (first.clients().len(), first.subscriptions().len());
    assert!(
        clients >= 2 && subscriptions >= 1,
        "{clients} {subscriptions}"
    );
    drop(first);
    let second = MiniMqtt::start_on(&addr).await;
    // The clients reconnect on their own, after the pauses they have between attempts, which
    // grow while the broker is away: the test waits for the broker to see them and the
    // listener's subscriptions again, however long that takes (the timeout only ends a hang).
    tokio::time::timeout(
        HANG,
        second.until(|broker| {
            broker.clients().len() >= clients && broker.subscriptions().len() >= subscriptions
        }),
    )
    .await
    .expect("the clients connect again and the listener subscribes again");
    // The broker has the connection; the client has it once it has read the answer.
    tokio::time::timeout(HANG, async {
        while !client.is_connected() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the client knows it is connected");
    // Nothing is lost now: the subscription is there when the event is sent.
    client
        .publish(published("invoice.paid", "2"))
        .await
        .expect("the client sends again");
    let delivery = listening
        .next(HANG)
        .await
        .expect("the listener subscribed again");
    assert_eq!(delivery.envelope().payload().as_ref(), b"2");
    delivery.ack().await.unwrap();
    listening.stop().await;
    drop(second);
}

/// Long enough that nothing but a hang reaches it, however busy the machine is.
const HANG: Duration = Duration::from_secs(120);

#[tokio::test]
async fn the_unreachable_broker_fails_calls_at_once() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let transport = MqttTransport::new(
        format!("mqtt://{unused}"),
        &MqttOptions::default(),
        "deals",
        "deals-1",
        Duration::from_secs(3),
    )
    .unwrap();
    let error = transport
        .publish(published("deal.won", "{}"))
        .await
        .unwrap_err();
    assert!(matches!(error, MicroError::Transport(_)), "{error:?}");
    assert!(!transport.is_connected());
    // "At once" is a future that is ready the first time it is polled: it waits for nothing,
    // not for the broker, not for the connect timeout, and no clock is read.
    let again =
        futures_util::FutureExt::now_or_never(transport.publish(published("deal.won", "{}")))
            .expect("at once while down, not after the connect timeout")
            .unwrap_err();
    assert!(matches!(again, MicroError::Transport(_)), "{again:?}");
}

#[tokio::test]
async fn setup_has_nothing_to_declare_and_dead_letters_go_to_a_topic() {
    let broker = MiniMqtt::start().await;
    let transport = MqttTransport::new(
        broker.url(),
        &MqttOptions::default(),
        "stats",
        "stats-1",
        Duration::from_secs(1),
    )
    .unwrap();
    let lines = transport
        .setup(&[event("deal.won", "stats")])
        .await
        .unwrap();
    assert_eq!(lines, ["topics need no declaration: nothing to set up"]);
    let letter = navree_micro::DeadLetter {
        id: "d1".into(),
        service: "stats".into(),
        pattern: "deal.won".into(),
        handler_pattern: "deal.won".into(),
        message_id: "m1".into(),
        headers: Default::default(),
        payload: bytes::Bytes::from_static(b"{}"),
        attempts: 5,
        error: "boom".into(),
        failed_at: chrono::Utc::now(),
    };
    transport.dead_letter(letter).await.unwrap();
    assert_eq!(broker.published(), ["navree/dead/stats/deal/won"]);
    assert!(transport.is_connected());
    assert!(broker.clients().contains(&"stats-1".to_owned()));
    transport.close().await;
}

/// An MQTT 5 broker: `NAVREE_TEST_MQTT_URL=mqtt://127.0.0.1:1883`.
#[tokio::test]
async fn a_real_broker_conforms() {
    let Ok(url) = std::env::var("NAVREE_TEST_MQTT_URL") else {
        eprintln!(
            "SKIPPED: set NAVREE_TEST_MQTT_URL to run the MQTT transport against an MQTT 5 broker"
        );
        return;
    };
    let prefix = format!("conf{}", &uuid::Uuid::now_v7().simple().to_string()[20..]);
    conformance::run(&MqttHarness::new(
        url,
        MqttOptions {
            prefix,
            ..MqttOptions::default()
        },
    ))
    .await;
}

/// A transport on `broker` that gives the broker `subscribe_timeout` to accept subscriptions.
fn transport_on(broker: &MiniMqtt, subscribe_timeout: Duration) -> Arc<dyn MicroTransport> {
    Arc::new(
        MqttTransport::new(
            broker.url(),
            &MqttOptions::default(),
            "billing",
            "billing-1",
            Duration::from_secs(5),
        )
        .expect("an MQTT transport")
        .with_subscribe_timeout(subscribe_timeout),
    )
}

fn three_subscriptions() -> Vec<Subscription> {
    vec![
        event("invoice.paid", "billing"),
        event("invoice.refunded", "billing"),
        event("invoice.voided", "billing"),
    ]
}

/// Starts `transport` listening, with nothing said to anyone.
fn listen_in_background(
    transport: Arc<dyn MicroTransport>,
    subscriptions: Vec<Subscription>,
) -> tokio::task::JoinHandle<Result<(), MicroError>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        // Held, so the listen ends only because of what the broker does.
        let _receiver = receiver;
        transport
            .listen_subscribed(subscriptions, sender, navree_micro::Subscribed::detached())
            .await
    })
}

/// A broker that answers nothing must not be waited for one subscription at a time: every
/// SUBSCRIBE goes out before any answer is waited for, and the listen then fails at one
/// deadline, naming the subscriptions that were not answered.
#[tokio::test]
async fn every_subscribe_goes_out_before_any_answer_is_waited_for() {
    let broker = MiniMqtt::start().await;
    broker.never_answer_subscriptions();
    let task = listen_in_background(
        transport_on(&broker, Duration::from_secs(3)),
        three_subscriptions(),
    );
    // The guard only ends a hang: a transport that sends one and waits for its answer never
    // gets here, and a transport that sends them all gets here at once.
    tokio::time::timeout(
        Duration::from_secs(20),
        broker.until(|broker| broker.subscribe_requests() >= 3),
    )
    .await
    .expect("all three SUBSCRIBEs were sent without waiting for an answer to the first");
    let error = tokio::time::timeout(Duration::from_secs(60), task)
        .await
        .expect("the listen ends at its deadline")
        .unwrap()
        .expect_err("a broker that never answers fails the listen");
    let text = error.to_string();
    assert!(text.contains("did not acknowledge"), "{text}");
    for topic in ["invoice/paid", "invoice/refunded", "invoice/voided"] {
        assert!(text.contains(topic), "{topic}: {text}");
    }
}

/// Stopping the transport ends a listen that is waiting for the broker, however long the
/// broker was given.
#[tokio::test]
async fn a_listen_waiting_for_the_broker_ends_when_the_transport_closes() {
    let broker = MiniMqtt::start().await;
    broker.never_answer_subscriptions();
    let transport = transport_on(&broker, Duration::from_secs(3_600));
    let task = listen_in_background(transport.clone(), three_subscriptions());
    tokio::time::timeout(
        Duration::from_secs(20),
        broker.until(|broker| broker.subscribe_requests() >= 3),
    )
    .await
    .expect("the SUBSCRIBEs were sent");
    transport.close().await;
    // The old wait was 30 seconds, for each subscription, whatever was said: this is well
    // inside it.
    let outcome = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("the listen ends when the transport closes")
        .unwrap();
    assert!(outcome.is_err(), "{outcome:?}");
}

/// A connection that is lost with the SUBACK on its way only delays the subscription: it is
/// asked for again when the connection comes back, and the listener is subscribed then.
#[tokio::test]
async fn a_connection_lost_before_the_suback_only_delays_the_subscription() {
    let broker = MiniMqtt::start().await;
    broker.drop_connection_on_subscribe(1);
    let harness = MqttHarness::new(broker.url(), MqttOptions::default());
    let listener = harness.transport("billing");
    let mut listening =
        Listening::start_subscribed(listener, vec![event("invoice.paid", "billing")]).await;
    // The broker read the first SUBSCRIBE and dropped the connection; it read another once the
    // client was back, and holds the subscription now.
    tokio::time::timeout(
        HANG,
        broker.until(|broker| !broker.subscriptions().is_empty()),
    )
    .await
    .expect("the client came back and subscribed");
    assert!(
        broker.subscribe_requests() >= 2,
        "{}",
        broker.subscribe_requests()
    );
    harness
        .transport("shop")
        .publish(published("invoice.paid", "late"))
        .await
        .unwrap();
    let delivery = listening
        .next(HANG)
        .await
        .expect("the listener subscribed once the connection was back");
    assert_eq!(delivery.envelope().payload().as_ref(), b"late");
    delivery.ack().await.unwrap();
    listening.stop().await;
}
