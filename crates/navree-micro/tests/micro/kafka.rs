//! The Kafka transport (`kafka` feature): the conformance suite, two apps,
//! durable events, topic setup, dead letters and SASL, against the small
//! broker in `kafka_server.rs`; with `NAVREE_TEST_KAFKA_URL` set (Kafka 4.0
//! or later), the suite again on it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_kernel::BoxFuture;
use navree_micro::conformance::{self, Capabilities, Harness, Listening};
use navree_micro::{
    DeadLetter, Envelope, Group, KafkaOptions, KafkaTransport, MicroError, MicroOptions,
    MicroTransport, PatternKind, SaslOptions, Subscription, TlsOptions, Transport, headers,
};

use crate::kafka_server::{MiniKafka, TopicSettings};
use crate::services::deals_answer_the_api_over;

/// Every instance on one cluster.
struct KafkaHarness {
    url: String,
    options: KafkaOptions,
    /// The authority to trust over TLS, when not a public one.
    tls: Option<TlsOptions>,
    next: AtomicUsize,
}

impl KafkaHarness {
    fn new(url: String, options: KafkaOptions) -> Self {
        Self {
            url,
            options,
            tls: None,
            next: AtomicUsize::new(0),
        }
    }

    /// The same harness, connecting over TLS with `tls`.
    fn with_tls(mut self, tls: TlsOptions) -> Self {
        self.tls = Some(tls);
        self
    }

    fn transport(&self, service: &str) -> Arc<dyn MicroTransport> {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let mut transport = KafkaTransport::new(
            self.url.clone(),
            &self.options,
            service,
            &format!("{service}-{n}"),
            Duration::from_secs(5),
        )
        .expect("a Kafka transport");
        if let Some(tls) = &self.tls {
            transport = transport.with_tls_files(tls).expect("the authority loads");
        }
        Arc::new(transport)
    }
}

impl Harness for KafkaHarness {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            consumer_groups: true,
            // A group that never committed starts at the end of its topics:
            // `events_published_while_a_service_is_away_wait_for_it` checks
            // what Kafka keeps for a group that did.
            durable: false,
            no_responders: true,
            wildcards: false,
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

fn options() -> KafkaOptions {
    KafkaOptions {
        partitions: 3,
        ..KafkaOptions::default()
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

fn message(pattern: &str, service: &str) -> Subscription {
    Subscription {
        kind: PatternKind::Message,
        ..event(pattern, service)
    }
}

fn published(pattern: &str, payload: &str) -> Envelope {
    Envelope::new(pattern, payload.to_owned())
        .with_header(headers::MESSAGE_ID, uuid::Uuid::now_v7().to_string())
}

fn transport(url: String, options: &KafkaOptions, service: &str) -> KafkaTransport {
    KafkaTransport::new(
        url,
        options,
        service,
        &format!("{service}-1"),
        Duration::from_secs(2),
    )
    .expect("a Kafka transport")
}

#[tokio::test]
async fn kafka_conforms() {
    let broker = MiniKafka::start().await;
    conformance::run(&KafkaHarness::new(broker.url(), options())).await;
    assert!(broker.problems().is_empty(), "{:?}", broker.problems());
}

/// A topic just created has no leader for a moment, and its leader then
/// refuses to list offsets until it has loaded the partitions (Kafka 4.0
/// does both): the listener and the asking side's reply reader wait for it
/// instead of failing.
#[tokio::test]
async fn new_topics_are_used_once_their_leaders_are_ready() {
    let broker = MiniKafka::start().await;
    broker.settle_new_topics(3);
    conformance::request_reply(&KafkaHarness::new(broker.url(), options())).await;
    assert!(broker.problems().is_empty(), "{:?}", broker.problems());
}

#[tokio::test]
async fn two_apps_talk_over_kafka() {
    let broker = MiniKafka::start().await;
    let micro = || MicroOptions {
        kafka: options(),
        ..MicroOptions::from(Transport::kafka([broker.url()]))
    };
    deals_answer_the_api_over(micro(), micro()).await;
    let topics = broker.topics();
    assert!(topics.contains(&"deals.get".to_owned()), "{topics:?}");
    assert!(topics.contains(&"crm-api.replies".to_owned()), "{topics:?}");
    let requests = broker.records("deals.get");
    assert!(!requests.is_empty());
    let (headers, _) = &requests[0];
    assert_eq!(
        headers.get("navree-pattern").map(String::as_str),
        Some("deals.get")
    );
    assert_eq!(
        headers.get("navree-service").map(String::as_str),
        Some("crm-api")
    );
    assert!(headers.contains_key("traceparent"), "{headers:?}");
    assert!(broker.problems().is_empty(), "{:?}", broker.problems());
}

#[tokio::test]
async fn events_published_while_a_service_is_away_wait_for_it() {
    let broker = MiniKafka::start().await;
    let shop = transport(broker.url(), &options(), "shop");
    let first = transport(broker.url(), &options(), "billing");
    let mut listening =
        Listening::start(Arc::new(first), vec![event("invoice.paid", "billing")]).await;
    shop.publish(published("invoice.paid", "before"))
        .await
        .unwrap();
    let delivery = listening
        .next(Duration::from_secs(5))
        .await
        .expect("the first event");
    assert_eq!(delivery.envelope().payload().as_ref(), b"before");
    delivery.ack().await.unwrap();
    // Leaving commits what was handled.
    listening.stop().await;
    assert_eq!(broker.members("billing"), 0, "the instance left its group");
    assert_eq!(
        broker
            .committed("billing", "invoice.paid")
            .values()
            .sum::<i64>(),
        1
    );

    shop.publish(published("invoice.paid", "while away"))
        .await
        .unwrap();
    let again = KafkaTransport::new(
        broker.url(),
        &options(),
        "billing",
        "billing-2",
        Duration::from_secs(2),
    )
    .unwrap();
    let mut back = Listening::start(Arc::new(again), vec![event("invoice.paid", "billing")]).await;
    let delivery = back
        .next(Duration::from_secs(5))
        .await
        .expect("the event waited");
    assert_eq!(delivery.envelope().payload().as_ref(), b"while away");
    delivery.ack().await.unwrap();
    assert!(
        back.next(Duration::from_millis(300)).await.is_none(),
        "handled events stay handled"
    );
    back.stop().await;
}

/// A new group starts at the end of the topics as they were when the
/// listener started: an event published once the listener says it is
/// subscribed is delivered, however slowly the broker answers.
#[tokio::test]
async fn an_event_published_once_the_listener_is_subscribed_is_delivered() {
    let broker = MiniKafka::start().await;
    broker.delay_list_offsets(Duration::from_millis(500));
    let shop = transport(broker.url(), &options(), "shop");
    let mut listening = Listening::start(
        Arc::new(transport(broker.url(), &options(), "ledger")),
        vec![event("refund.issued", "ledger")],
    )
    .await;
    shop.publish(published("refund.issued", "first"))
        .await
        .unwrap();
    let delivery = listening
        .next(Duration::from_secs(5))
        .await
        .expect("the event published after the listener subscribed");
    assert_eq!(delivery.envelope().payload().as_ref(), b"first");
    delivery.ack().await.unwrap();
    listening.stop().await;
}

#[tokio::test]
async fn earliest_reads_a_new_group_from_the_start() {
    let broker = MiniKafka::start().await;
    let shop = transport(broker.url(), &options(), "shop");
    shop.publish(published("order.placed", "old"))
        .await
        .unwrap();
    let earliest = KafkaOptions {
        start_from: "earliest".into(),
        ..options()
    };
    let mut listening = Listening::start(
        Arc::new(transport(broker.url(), &earliest, "audit")),
        vec![event("order.placed", "audit")],
    )
    .await;
    let delivery = listening
        .next(Duration::from_secs(5))
        .await
        .expect("the topic's history");
    assert_eq!(delivery.envelope().payload().as_ref(), b"old");
    delivery.ack().await.unwrap();
    listening.stop().await;
}

#[tokio::test]
async fn setup_creates_the_topics_with_the_settings_once() {
    let broker = MiniKafka::start().await;
    let settings = KafkaOptions {
        partitions: 4,
        replication_factor: 1,
        retention_hours: 2,
        ..KafkaOptions::default()
    };
    let deals = transport(broker.url(), &settings, "deals");
    let subscriptions = [
        event("company.renamed", "deals"),
        message("deals.get", "deals"),
    ];
    let lines = deals.setup(&subscriptions).await.unwrap();
    assert_eq!(
        lines,
        [
            "created topic company.renamed",
            "created topic deals.dead",
            "created topic deals.get",
            "created topic deals.replies",
            "created topic deals.retry",
        ]
    );
    assert_eq!(
        broker.topic("deals.get"),
        Some(TopicSettings {
            partitions: 4,
            replication_factor: 1,
            retention_ms: Some("7200000".into()),
        })
    );
    let again = deals.setup(&subscriptions).await.unwrap();
    assert!(
        again
            .iter()
            .all(|line| line.starts_with("topic ") && line.ends_with(" exists")),
        "{again:?}"
    );
}

#[tokio::test]
async fn publishing_creates_a_missing_topic_and_requests_to_one_have_no_responders() {
    let broker = MiniKafka::start().await;
    let shop = transport(broker.url(), &options(), "shop");
    shop.publish(published("cart.abandoned", "{}"))
        .await
        .unwrap();
    assert_eq!(
        broker.topic("cart.abandoned").map(|topic| topic.partitions),
        Some(3)
    );
    let mut request = published("pricing.quote", "{}");
    request.set_deadline(std::time::SystemTime::now() + Duration::from_secs(2));
    let error = shop.request(request).await.unwrap_err();
    assert!(
        matches!(&error, MicroError::NoResponders { pattern } if pattern == "pricing.quote"),
        "{error:?}"
    );
}

#[tokio::test]
async fn wildcards_need_full_pattern_names() {
    let broker = MiniKafka::start().await;
    let stats = transport(broker.url(), &options(), "stats");
    assert!(!stats.supports_wildcards());
    let (deliveries, _received) = tokio::sync::mpsc::channel(1);
    let error = stats
        .listen(vec![event("deal.*", "stats")], deliveries)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("full pattern names"), "{error}");
}

#[tokio::test]
async fn dead_letters_go_to_the_service_dead_topic() {
    let broker = MiniKafka::start().await;
    let stats = transport(broker.url(), &options(), "stats");
    let letter = DeadLetter {
        id: "d1".into(),
        service: "stats".into(),
        pattern: "deal.won".into(),
        handler_pattern: "deal.won".into(),
        message_id: "m1".into(),
        headers: Default::default(),
        payload: bytes::Bytes::from_static(b"{\"deal\":1}"),
        attempts: 5,
        error: "smtp refused\nthe message".into(),
        failed_at: chrono::Utc::now(),
    };
    stats.dead_letter(letter).await.unwrap();
    let records = broker.records("stats.dead");
    assert_eq!(records.len(), 1);
    let (headers, value) = &records[0];
    assert_eq!(value, "{\"deal\":1}");
    assert_eq!(
        headers.get("navree-error").map(String::as_str),
        Some("smtp refused the message")
    );
    assert_eq!(
        headers.get("navree-attempts").map(String::as_str),
        Some("5")
    );
}

#[tokio::test]
async fn sasl_plain_authenticates_and_wrong_credentials_fail() {
    let broker = MiniKafka::with_plain("deals", "s3cret").await;
    let with = |password: &str| KafkaOptions {
        sasl: Some(SaslOptions {
            mechanism: "PLAIN".into(),
            username: "deals".into(),
            password: password.to_owned().into(),
        }),
        ..options()
    };
    let good = transport(broker.url(), &with("s3cret"), "deals");
    good.publish(published("deal.won", "{}")).await.unwrap();
    assert_eq!(broker.records("deal.won").len(), 1);

    let bad = transport(broker.url(), &with("wrong"), "deals");
    let error = bad.publish(published("deal.won", "{}")).await.unwrap_err();
    assert!(matches!(error, MicroError::Transport(_)), "{error:?}");
    assert!(error.to_string().contains("SASL"), "{error}");

    let scram = KafkaOptions {
        sasl: Some(SaslOptions {
            mechanism: "SCRAM-SHA-256".into(),
            username: "deals".into(),
            password: "s3cret".to_owned().into(),
        }),
        ..options()
    };
    let unsupported = transport(broker.url(), &scram, "deals");
    let error = unsupported
        .publish(published("deal.won", "{}"))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("refused SASL SCRAM-SHA-256"),
        "{error}"
    );
    assert!(
        error.to_string().contains("PLAIN"),
        "names what the broker offers: {error}"
    );

    let none = transport(broker.url(), &options(), "deals");
    assert!(
        none.publish(published("deal.won", "{}")).await.is_err(),
        "a broker that needs SASL refuses"
    );
}

#[tokio::test]
async fn the_unreachable_broker_fails_calls() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let shop = KafkaTransport::new(
        unused.to_string(),
        &options(),
        "shop",
        "shop-1",
        Duration::from_millis(300),
    )
    .unwrap();
    let error = shop
        .publish(published("order.placed", "{}"))
        .await
        .unwrap_err();
    assert!(matches!(error, MicroError::Transport(_)), "{error:?}");
    // Refused at once, or (Windows retries refused connections) the connect
    // timeout.
    assert!(error.to_string().contains("Kafka at"), "{error}");
}

/// A listener whose broker can't be reached doesn't hold the app back: it
/// says it is subscribed at once and keeps trying.
#[tokio::test]
async fn a_listener_goes_on_while_the_broker_is_unreachable() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let billing = KafkaTransport::new(
        unused.to_string(),
        &options(),
        "billing",
        "billing-1",
        Duration::from_millis(300),
    )
    .unwrap();
    let started = std::time::Instant::now();
    let listening =
        Listening::start(Arc::new(billing), vec![event("invoice.paid", "billing")]).await;
    // Listening::start waits up to 5 s for the signal.
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    listening.stop().await;
}

#[tokio::test]
async fn the_settings_are_checked() {
    let check = |url: &str, options: KafkaOptions| {
        KafkaTransport::new(url, &options, "deals", "deals-1", Duration::from_secs(1))
            .err()
            .map(|error| error.to_string())
    };
    assert!(
        check("", options())
            .unwrap()
            .contains("names no Kafka broker")
    );
    assert!(
        check(
            "127.0.0.1:9092",
            KafkaOptions {
                start_from: "middle".into(),
                ..options()
            }
        )
        .unwrap()
        .contains("micro.kafka.start_from")
    );
    assert!(
        check(
            "127.0.0.1:9092",
            KafkaOptions {
                partitions: 0,
                ..options()
            }
        )
        .unwrap()
        .contains("micro.kafka.partitions")
    );
    assert!(check("kafka://127.0.0.1:9092, 127.0.0.1:9093", options()).is_none());
}

/// Kafka 4.0 or later: `NAVREE_TEST_KAFKA_URL=127.0.0.1:9092`.
#[tokio::test]
async fn a_real_broker_conforms() {
    let Ok(url) = std::env::var("NAVREE_TEST_KAFKA_URL") else {
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
            "NAVREE_TEST_KAFKA_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
        );
        eprintln!(
            "SKIPPED: set NAVREE_TEST_KAFKA_URL to run the Kafka transport against Kafka 4.0 or later"
        );
        return;
    };
    conformance::run(&KafkaHarness::new(url, options())).await;
}

/// A Kafka 4.0 broker that asks for a SASL PLAIN login, in plain text
/// (`NAVREE_TEST_KAFKA_SASL_URL`) and over TLS with a certificate a private
/// authority signed (`NAVREE_TEST_KAFKA_TLS_URL`, `NAVREE_TEST_BROKER_CA`):
/// requests and replies go through with the login, a wrong password is
/// refused, and so is the certificate when the authority is not trusted.
#[tokio::test]
async fn a_real_broker_takes_a_sasl_login_in_plain_text_and_over_tls() {
    let Some([plain, secure, ca, user, password]) = crate::support::services(
        "a_real_broker_takes_a_sasl_login_in_plain_text_and_over_tls",
        [
            "NAVREE_TEST_KAFKA_SASL_URL",
            "NAVREE_TEST_KAFKA_TLS_URL",
            "NAVREE_TEST_BROKER_CA",
            "NAVREE_TEST_BROKER_USER",
            "NAVREE_TEST_BROKER_PASSWORD",
        ],
    ) else {
        return;
    };
    let login = |password: &str| KafkaOptions {
        sasl: Some(SaslOptions {
            mechanism: "PLAIN".into(),
            username: user.clone(),
            password: password.to_owned().into(),
        }),
        ..options()
    };
    let authority = TlsOptions {
        ca: Some(ca.into()),
        ..TlsOptions::default()
    };

    conformance::request_reply(&KafkaHarness::new(plain.clone(), login(&password))).await;
    conformance::request_reply(
        &KafkaHarness::new(secure.clone(), login(&password)).with_tls(authority),
    )
    .await;

    let wrong = transport(plain, &login("wrong-password"), "deals");
    let error = wrong
        .publish(published("deal.won", "{}"))
        .await
        .unwrap_err();
    assert!(matches!(error, MicroError::Transport(_)), "{error:?}");
    assert!(error.to_string().contains("SASL"), "{error}");

    // Over TLS with the public authorities only, the broker's certificate
    // is refused.
    let untrusted = transport(
        secure,
        &KafkaOptions {
            tls: true,
            ..login(&password)
        },
        "deals",
    );
    assert!(
        untrusted
            .publish(published("deal.won", "{}"))
            .await
            .is_err(),
        "a certificate no trusted authority signed"
    );
}
