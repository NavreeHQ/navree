//! The NATS transport (`nats` feature): the conformance suite and two apps
//! over core NATS, against the small server in `nats_server.rs`; with
//! `NAVREE_TEST_NATS_URL` set (a server with JetStream), the suite again
//! with durable events.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_kernel::BoxFuture;
use navree_micro::conformance::{self, Capabilities, Harness};
use navree_micro::{
    Envelope, MicroError, MicroOptions, MicroTransport, NatsOptions, NatsTransport, TlsOptions,
    Transport,
};

use crate::nats_server::MiniNats;
use crate::services::deals_answer_the_api_over;

/// Every instance on one NATS server.
struct NatsHarness {
    url: String,
    options: NatsOptions,
    next: AtomicUsize,
}

impl NatsHarness {
    fn new(url: String, options: NatsOptions) -> Self {
        Self {
            url,
            options,
            next: AtomicUsize::new(0),
        }
    }

    fn transport(&self, service: &str) -> Arc<dyn MicroTransport> {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        Arc::new(
            NatsTransport::new(
                self.url.clone(),
                &self.options,
                service,
                &format!("{service}-{n}"),
                Duration::from_secs(2),
            )
            .expect("a NATS transport"),
        )
    }
}

impl Harness for NatsHarness {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            consumer_groups: true,
            durable: self.options.jetstream,
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

fn core_nats() -> NatsOptions {
    NatsOptions {
        jetstream: false,
        ..NatsOptions::default()
    }
}

#[tokio::test]
async fn core_nats_conforms() {
    let server = MiniNats::start().await;
    conformance::run(&NatsHarness::new(server.url(), core_nats())).await;
}

#[tokio::test]
async fn two_apps_talk_over_core_nats() {
    let server = MiniNats::start().await;
    let options = || MicroOptions {
        nats: core_nats(),
        ..MicroOptions::from(Transport::nats(server.url()))
    };
    deals_answer_the_api_over(options(), options()).await;
}

#[tokio::test]
async fn a_server_that_is_not_there_fails_the_call_as_a_transport_error() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let transport = NatsTransport::new(
        format!("nats://{unused}"),
        &core_nats(),
        "deals",
        "deals-1",
        Duration::from_millis(300),
    )
    .unwrap();
    let error = transport
        .publish(navree_micro::Envelope::new("deal.won", "{}"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, navree_micro::MicroError::Transport(_)),
        "{error:?}"
    );
}

/// JetStream on a real server: `NAVREE_TEST_NATS_URL=nats://127.0.0.1:4222`.
#[tokio::test]
async fn jetstream_conforms() {
    let Ok(url) = std::env::var("NAVREE_TEST_NATS_URL") else {
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
            "NAVREE_TEST_NATS_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
        );
        eprintln!("SKIPPED: set NAVREE_TEST_NATS_URL to run the NATS transport against JetStream");
        return;
    };
    let stream = format!("CONF{}", &uuid::Uuid::now_v7().simple().to_string()[20..]);
    conformance::run(&NatsHarness::new(
        url,
        NatsOptions {
            stream,
            ..NatsOptions::default()
        },
    ))
    .await;
}

/// A NATS server that asks for a token and speaks TLS only, with a
/// certificate a private authority signed (`NAVREE_TEST_NATS_TLS_URL`,
/// `NAVREE_TEST_NATS_TOKEN`, `NAVREE_TEST_BROKER_CA`): requests and replies
/// go through with the token, a wrong token is refused, and so is the
/// certificate when the authority is not trusted.
#[tokio::test]
async fn a_real_server_takes_a_token_over_tls() {
    let Some([url, token, ca]) = crate::support::services(
        "a_real_server_takes_a_token_over_tls",
        [
            "NAVREE_TEST_NATS_TLS_URL",
            "NAVREE_TEST_NATS_TOKEN",
            "NAVREE_TEST_BROKER_CA",
        ],
    ) else {
        return;
    };
    let options = |token: &str, ca: Option<&str>| NatsOptions {
        stream: format!("AUTH{}", &uuid::Uuid::now_v7().simple().to_string()[20..]),
        token: Some(token.to_owned().into()),
        tls: TlsOptions {
            ca: ca.map(Into::into),
            ..TlsOptions::default()
        },
        ..NatsOptions::default()
    };
    conformance::request_reply(&NatsHarness::new(url.clone(), options(&token, Some(&ca)))).await;

    for (refused, options) in [
        ("a wrong token", options("wrong-token", Some(&ca))),
        ("an untrusted certificate", options(&token, None)),
    ] {
        let transport = NatsTransport::new(
            url.clone(),
            &options,
            "deals",
            "deals-1",
            Duration::from_secs(5),
        )
        .unwrap();
        let error = transport
            .publish(Envelope::new("deal.won", "{}"))
            .await
            .unwrap_err();
        assert!(
            matches!(error, MicroError::Transport(_)),
            "{refused}: {error:?}"
        );
    }
}
