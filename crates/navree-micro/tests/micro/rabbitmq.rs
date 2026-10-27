//! The RabbitMQ transport (`rabbitmq` feature). Without a broker: the
//! settings and a refused connection. With `NAVREE_TEST_RABBITMQ_URL` set
//! (`amqp://guest:guest@127.0.0.1:5672/%2f`): the conformance suite, the
//! topology `micro:setup` declares and two apps.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_kernel::BoxFuture;
use navree_micro::conformance::{self, Capabilities, Harness};
use navree_micro::{
    Envelope, Group, MicroError, MicroOptions, MicroTransport, PatternKind, RabbitmqOptions,
    RabbitmqTransport, Subscription, Transport, headers,
};

use crate::services::deals_answer_the_api_over;

/// Every instance on one broker.
struct RabbitHarness {
    url: String,
    options: RabbitmqOptions,
    next: AtomicUsize,
}

impl RabbitHarness {
    fn new(url: String, options: RabbitmqOptions) -> Self {
        Self {
            url,
            options,
            next: AtomicUsize::new(0),
        }
    }

    fn transport(&self, service: &str) -> Arc<dyn MicroTransport> {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        Arc::new(
            RabbitmqTransport::new(
                self.url.clone(),
                &self.options,
                service,
                &format!("{service}-{n}"),
                Duration::from_secs(5),
            )
            .expect("a RabbitMQ transport"),
        )
    }
}

impl Harness for RabbitHarness {
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

/// A throwaway exchange per run, so runs don't see each other's queues'
/// bindings.
fn run_options() -> RabbitmqOptions {
    RabbitmqOptions {
        exchange: format!("conf{}", &uuid::Uuid::now_v7().simple().to_string()[20..]),
        queue_type: "classic".into(),
        ..RabbitmqOptions::default()
    }
}

fn broker() -> Option<String> {
    match std::env::var("NAVREE_TEST_RABBITMQ_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            eprintln!(
                "SKIPPED: set NAVREE_TEST_RABBITMQ_URL to run the RabbitMQ transport against RabbitMQ"
            );
            None
        }
    }
}

#[tokio::test]
async fn an_unreachable_broker_is_a_transport_error() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let transport = RabbitmqTransport::new(
        format!("amqp://guest:guest@{unused}/%2f"),
        &RabbitmqOptions::default(),
        "deals",
        "deals-1",
        Duration::from_millis(500),
    )
    .unwrap();
    let error = transport
        .publish(Envelope::new("deal.won", "{}").with_header(headers::MESSAGE_ID, "m1"))
        .await
        .unwrap_err();
    assert!(matches!(error, MicroError::Transport(_)), "{error:?}");
    assert_eq!(transport.name(), "rabbitmq");
    assert!(
        transport.supports_wildcards(),
        "topic exchanges route `*` and `#`"
    );
}

#[tokio::test]
async fn rabbitmq_conforms() {
    let Some(url) = broker() else {
        return;
    };
    conformance::run(&RabbitHarness::new(url, run_options())).await;
}

#[tokio::test]
async fn setup_declares_the_exchanges_queues_and_bindings() {
    let Some(url) = broker() else {
        return;
    };
    let options = run_options();
    let exchange = options.exchange.clone();
    let transport = RabbitmqTransport::new(
        url,
        &options,
        "notifications",
        "notifications-1",
        Duration::from_secs(5),
    )
    .unwrap();
    let subscription = Subscription {
        pattern: "deal.won".into(),
        kind: PatternKind::Event,
        group: Group::Service("notifications".into()),
        ordered: false,
        ack_wait: Duration::from_secs(30),
    };
    let lines = transport.setup(&[subscription]).await.unwrap();
    assert_eq!(
        lines,
        [
            format!("declared topic exchange {exchange}"),
            format!("declared topic exchange {exchange}.dead"),
            "declared queue notifications.deal.won (classic)".to_owned(),
            format!("bound queue notifications.deal.won to {exchange} with deal.won"),
            "declared retry queue notifications.deal.won.retry".to_owned(),
        ]
    );
    transport.close().await;
}

#[tokio::test]
async fn two_apps_talk_over_rabbitmq() {
    let Some(url) = broker() else {
        return;
    };
    let rabbitmq = run_options();
    let options = || MicroOptions {
        rabbitmq: rabbitmq.clone(),
        ..MicroOptions::from(Transport::rabbitmq(url.clone()))
    };
    deals_answer_the_api_over(options(), options()).await;
}
