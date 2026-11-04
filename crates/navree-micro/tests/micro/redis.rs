//! The Redis Streams transport (`redis` feature): the conformance suite,
//! two apps, claims after a crash and the settings, against the small
//! server in `redis_server.rs`; with `NAVREE_TEST_REDIS_URL` set, the suite
//! again on a real Redis (6.2 or later).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_kernel::BoxFuture;
use navree_micro::conformance::{self, Capabilities, Harness, Listening};
use navree_micro::{
    Envelope, Group, MicroError, MicroOptions, MicroTransport, PatternKind, RedisOptions,
    RedisTransport, Subscription, Transport, headers,
};

use crate::redis_server::MiniRedis;
use crate::services::deals_answer_the_api_over;

/// Every instance on one Redis.
struct RedisHarness {
    url: String,
    options: RedisOptions,
    next: AtomicUsize,
}

impl RedisHarness {
    fn new(url: String, options: RedisOptions) -> Self {
        Self {
            url,
            options,
            next: AtomicUsize::new(0),
        }
    }

    fn transport(&self, service: &str) -> Arc<dyn MicroTransport> {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        Arc::new(
            RedisTransport::new(
                self.url.clone(),
                &self.options,
                service,
                &format!("{service}-{n}"),
                Duration::from_secs(2),
            )
            .expect("a Redis transport"),
        )
    }
}

impl Harness for RedisHarness {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            consumer_groups: true,
            durable: true,
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

/// Short claim times and reads, so retries come back quickly in tests. An entry left
/// unacknowledged is claimed after the claim time, so it is long enough that a test the
/// machine pauses for a moment, between taking an entry and answering it, has not had it
/// claimed from under it.
fn quick(prefix: &str) -> RedisOptions {
    RedisOptions {
        prefix: prefix.to_owned(),
        claim_after_ms: 1_000,
        block_ms: 200,
        ..RedisOptions::default()
    }
}

/// Options for an instance that does not claim what another holds, within any test's time:
/// a crash is made by dropping its delivery, and until the instance is stopped it must not
/// claim that entry back for itself, however long a busy machine keeps the test from
/// stopping it.
fn slow_to_claim(prefix: &str) -> RedisOptions {
    RedisOptions {
        claim_after_ms: 600_000,
        ..quick(prefix)
    }
}

/// Long enough that nothing but a hang reaches it, however busy the machine is.
const HANG: Duration = Duration::from_secs(120);

fn event(pattern: &str, service: &str) -> Subscription {
    Subscription {
        pattern: pattern.to_owned(),
        kind: PatternKind::Event,
        group: Group::Service(service.to_owned()),
        ordered: false,
        ack_wait: Duration::ZERO,
    }
}

fn published(pattern: &str, payload: &str) -> Envelope {
    Envelope::new(pattern, payload.to_owned())
        .with_header(headers::MESSAGE_ID, uuid::Uuid::now_v7().to_string())
}

#[tokio::test]
async fn redis_streams_conform() {
    let server = MiniRedis::start().await;
    conformance::run(&RedisHarness::new(server.url(), quick("navree:"))).await;
}

#[tokio::test]
async fn two_apps_talk_over_redis_streams() {
    let server = MiniRedis::start().await;
    let options = || MicroOptions {
        redis: quick("crm:"),
        ..MicroOptions::from(Transport::redis(server.url()))
    };
    deals_answer_the_api_over(options(), options()).await;
    assert!(
        server.has_stream("crm:deals.get"),
        "requests go to the pattern's stream"
    );
}

/// A listener that has said it is subscribed gets what is sent next. A consumer group is made
/// at the end of its stream and a broadcast reader starts after the last entry, so a listener
/// that said so before either was in place would miss an event sent at once; a slow server makes
/// the time between the two long enough to see it.
#[tokio::test]
async fn an_event_sent_once_a_listener_is_subscribed_reaches_it() {
    let server = MiniRedis::start().await;
    server.slow("XGROUP", Duration::from_millis(400));
    server.slow("XREVRANGE", Duration::from_millis(400));
    let harness = RedisHarness::new(server.url(), quick("navree:"));
    let client = harness.transport("shop");
    let broadcast = Subscription {
        pattern: "invoice.cast".to_owned(),
        kind: PatternKind::Event,
        group: Group::Instance("cast-1".to_owned()),
        ordered: false,
        ack_wait: Duration::ZERO,
    };
    for (service, subscription) in [
        ("billing", event("invoice.paid", "billing")),
        ("cast", broadcast),
    ] {
        let pattern = subscription.pattern.clone();
        let mut listening = Listening::start(harness.transport(service), vec![subscription]).await;
        client.publish(published(&pattern, "now")).await.unwrap();
        // The event is there once it is sent; this only ends a hang.
        let delivery = listening
            .next(Duration::from_secs(20))
            .await
            .unwrap_or_else(|| {
                panic!("{pattern}: the event sent once the listener was subscribed")
            });
        assert_eq!(delivery.envelope().payload().as_ref(), b"now");
        delivery.ack().await.unwrap();
        listening.stop().await;
    }
}

#[tokio::test]
async fn an_entry_a_crashed_instance_took_is_claimed_by_another() {
    let server = MiniRedis::start().await;
    let harness = RedisHarness::new(server.url(), quick("navree:"));
    let first = RedisHarness::new(server.url(), slow_to_claim("navree:")).transport("billing");
    let mut crashed = Listening::start(first, vec![event("invoice.paid", "billing")]).await;
    let client = harness.transport("shop");
    client
        .publish(published("invoice.paid", "42"))
        .await
        .unwrap();
    let taken = crashed
        .next(HANG)
        .await
        .expect("the first instance takes it");
    assert_eq!(taken.attempt(), 1);
    // It dies without acknowledging.
    drop(taken);
    crashed.stop().await;

    let mut survivor = Listening::start(
        harness.transport("billing"),
        vec![event("invoice.paid", "billing")],
    )
    .await;
    let claimed = survivor
        .next(HANG)
        .await
        .expect("claimed after claim_after_ms");
    assert_eq!(claimed.attempt(), 2, "the claim is the next attempt");
    assert_eq!(claimed.envelope().payload().as_ref(), b"42");
    claimed.ack().await.unwrap();
    assert!(
        survivor.next(Duration::from_millis(600)).await.is_none(),
        "acknowledged"
    );
    survivor.stop().await;
}

#[tokio::test]
async fn a_retry_longer_than_the_claim_time_waits_its_backoff() {
    // While the backoff runs, the instance keeps the entry its own by setting it up again every
    // half claim time, so a pause of a few hundred milliseconds in the instance does not hand it
    // to a claim early; the claim time is long here so that a busy machine does not make such
    // a pause. (A process that stops for the whole claim time may have its entries claimed
    // early: at-least-once delivery allows it.)
    let server = MiniRedis::start().await;
    let harness = RedisHarness::new(
        server.url(),
        RedisOptions {
            claim_after_ms: 2_000,
            ..quick("navree:")
        },
    );
    let mut listening = Listening::start(
        harness.transport("mailer"),
        vec![event("mail.queued", "mailer")],
    )
    .await;
    harness
        .transport("app")
        .publish(published("mail.queued", "m"))
        .await
        .unwrap();
    let first = listening.next(HANG).await.expect("first attempt");
    let started = std::time::Instant::now();
    first.retry_in(Duration::from_millis(3_000)).await.unwrap();
    let second = listening.next(HANG).await.expect("second attempt");
    assert_eq!(second.attempt(), 2);
    assert!(
        started.elapsed() >= Duration::from_millis(2_500),
        "came back after {:?}",
        started.elapsed()
    );
    second.ack().await.unwrap();
    listening.stop().await;
}

#[tokio::test]
async fn wildcards_dead_letters_and_names() {
    let server = MiniRedis::start().await;
    let transport = RedisTransport::new(
        server.url(),
        &quick("crm:"),
        "stats",
        "stats-1",
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(!transport.supports_wildcards());
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);
    let error = transport
        .listen(vec![event("deal.*", "stats")], sender)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("full pattern names"), "{error}");

    let lines = transport
        .setup(&[event("deal.won", "stats")])
        .await
        .unwrap();
    assert_eq!(lines, ["created consumer group stats on crm:deal.won"]);
    let again = transport
        .setup(&[event("deal.won", "stats")])
        .await
        .unwrap();
    assert_eq!(again, ["consumer group stats on crm:deal.won exists"]);

    let letter = navree_micro::DeadLetter {
        id: "d1".into(),
        service: "stats".into(),
        pattern: "deal.won".into(),
        handler_pattern: "deal.won".into(),
        message_id: "m1".into(),
        headers: Default::default(),
        payload: bytes::Bytes::from_static(b"{\"deal\":1}"),
        attempts: 5,
        error: "boom".into(),
        failed_at: chrono::Utc::now(),
    };
    transport.dead_letter(letter).await.unwrap();
    assert_eq!(server.payloads("crm:dead"), ["{\"deal\":1}"]);
    assert!(transport.is_connected());
}

#[tokio::test]
async fn a_server_that_is_not_there_is_a_transport_error() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let transport = RedisTransport::new(
        format!("redis://{unused}"),
        &RedisOptions::default(),
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
}

/// A real Redis: `NAVREE_TEST_REDIS_URL=redis://127.0.0.1:6379`.
#[tokio::test]
async fn real_redis_streams_conform() {
    let Ok(url) = std::env::var("NAVREE_TEST_REDIS_URL") else {
        eprintln!("SKIPPED: set NAVREE_TEST_REDIS_URL to run the Redis transport against Redis");
        return;
    };
    let prefix = format!("conf{}:", &uuid::Uuid::now_v7().simple().to_string()[20..]);
    conformance::run(&RedisHarness::new(url, quick(&prefix))).await;
}

/// A broadcast reader starts after the last entry of its stream. When the server cannot say
/// which that is, it must not start from the beginning, which would replay every entry the
/// stream still holds: the listen fails, and whoever runs it starts it again.
#[tokio::test]
async fn a_broadcast_reader_never_starts_from_the_beginning_when_the_server_cannot_say_where() {
    let server = MiniRedis::start().await;
    let harness = RedisHarness::new(server.url(), quick("navree:"));
    // An old broadcast is in the stream before anyone listens.
    harness
        .transport("shop")
        .publish(published("cast.changed", "old"))
        .await
        .unwrap();
    server.fail("XREVRANGE");
    let transport = harness.transport("cast");
    let (sender, mut received) = tokio::sync::mpsc::channel(8);
    let broadcast = Subscription {
        pattern: "cast.changed".to_owned(),
        kind: PatternKind::Event,
        group: Group::Instance("cast-1".to_owned()),
        ordered: false,
        ack_wait: Duration::ZERO,
    };
    let task = tokio::spawn(async move {
        transport
            .listen_subscribed(
                vec![broadcast],
                sender,
                navree_micro::Subscribed::detached(),
            )
            .await
    });
    let outcome = tokio::time::timeout(HANG, task)
        .await
        .expect("the listen ends: it was not allowed to carry on from the beginning")
        .unwrap();
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(
        received.try_recv().is_err(),
        "the old broadcast was replayed"
    );
}
