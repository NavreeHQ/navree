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

    drop(first);
    let second = MiniMqtt::start_on(&addr).await;
    // Calls fail at once while disconnected, then work again.
    let mut sent = false;
    for _ in 0..100 {
        if client.publish(published("invoice.paid", "2")).await.is_ok() {
            sent = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(sent, "the client reconnects");
    let mut got = None;
    for _ in 0..50 {
        if let Some(delivery) = listening.next(Duration::from_millis(200)).await {
            got = Some(delivery);
            break;
        }
        client
            .publish(published("invoice.paid", "2"))
            .await
            .unwrap();
    }
    let delivery = got.expect("the listener subscribed again");
    assert_eq!(delivery.envelope().payload().as_ref(), b"2");
    delivery.ack().await.unwrap();
    listening.stop().await;
    drop(second);
}

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
        Duration::from_millis(300),
    )
    .unwrap();
    let error = transport
        .publish(published("deal.won", "{}"))
        .await
        .unwrap_err();
    assert!(matches!(error, MicroError::Transport(_)), "{error:?}");
    assert!(!transport.is_connected());
    let started = std::time::Instant::now();
    let again = transport
        .publish(published("deal.won", "{}"))
        .await
        .unwrap_err();
    assert!(matches!(again, MicroError::Transport(_)), "{again:?}");
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "at once while down"
    );
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
