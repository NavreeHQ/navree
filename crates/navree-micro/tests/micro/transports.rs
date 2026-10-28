//! The conformance suite on the memory and TCP transports, TCP's TLS and
//! mutual TLS, and the size limit.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use navree_kernel::BoxFuture;
use navree_micro::conformance::{self, Capabilities, Harness, Listening};
use navree_micro::{
    Envelope, Group, MemoryTransport, MicroError, MicroTransport, PatternKind, Subscription,
    TcpOptions, TcpTransport, TlsOptions, headers,
};

/// Every instance on one in-memory broker.
struct MemoryHarness {
    broker: MemoryTransport,
}

impl Harness for MemoryHarness {
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
        _service: &'a str,
        _patterns: &'a [&'a str],
    ) -> BoxFuture<'a, Arc<dyn MicroTransport>> {
        Box::pin(async move { Arc::new(self.broker.clone()) as Arc<dyn MicroTransport> })
    }

    fn client(&self) -> BoxFuture<'_, Arc<dyn MicroTransport>> {
        Box::pin(async move { Arc::new(self.broker.clone()) as Arc<dyn MicroTransport> })
    }
}

#[tokio::test]
async fn the_memory_transport_conforms() {
    conformance::run(&MemoryHarness {
        broker: MemoryTransport::new(),
    })
    .await;
}

/// One TCP listener per instance; the client routes each instance's
/// patterns to it.
#[derive(Default)]
struct TcpHarness {
    instances: Mutex<Vec<(TcpTransport, Vec<String>)>>,
    tls: Option<TlsOptions>,
    client_tls: Option<TlsOptions>,
}

impl TcpHarness {
    fn options(&self, tls: Option<&TlsOptions>) -> TcpOptions {
        TcpOptions {
            addr: "127.0.0.1:0".to_owned(),
            tls: tls.cloned().unwrap_or_default(),
            ..TcpOptions::default()
        }
    }
}

impl Harness for TcpHarness {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            consumer_groups: false,
            durable: false,
            no_responders: true,
            wildcards: true,
        }
    }

    fn instance<'a>(
        &'a self,
        _service: &'a str,
        patterns: &'a [&'a str],
    ) -> BoxFuture<'a, Arc<dyn MicroTransport>> {
        Box::pin(async move {
            let transport = TcpTransport::new(
                &self.options(self.tls.as_ref()),
                None,
                1 << 20,
                Duration::from_secs(2),
            )
            .expect("tcp transport");
            self.instances.lock().unwrap().push((
                transport.clone(),
                patterns.iter().map(|p| (*p).to_owned()).collect(),
            ));
            Arc::new(transport) as Arc<dyn MicroTransport>
        })
    }

    fn client(&self) -> BoxFuture<'_, Arc<dyn MicroTransport>> {
        Box::pin(async move {
            let instances = self.instances.lock().unwrap().clone();
            let mut routes = BTreeMap::new();
            for (transport, patterns) in instances {
                let Some(addr) = transport.local_addr() else {
                    continue;
                };
                for pattern in patterns {
                    // Two instances with one pattern: the second gets the
                    // wildcard of its prefix, so both receive its events.
                    let prefix = pattern
                        .rsplit_once('.')
                        .map_or(pattern.as_str(), |(p, _)| p);
                    let key = [
                        pattern.clone(),
                        format!("{prefix}.*"),
                        format!("{prefix}.>"),
                    ]
                    .into_iter()
                    .find(|key| !routes.contains_key(key))
                    .expect("a free route key");
                    routes.insert(key, format!("127.0.0.1:{}", addr.port()));
                }
            }
            let options = TcpOptions {
                routes,
                ..self.options(self.client_tls.as_ref().or(self.tls.as_ref()))
            };
            Arc::new(
                TcpTransport::new(&options, None, 1 << 20, Duration::from_secs(2))
                    .expect("tcp client"),
            ) as Arc<dyn MicroTransport>
        })
    }
}

#[tokio::test]
async fn the_tcp_transport_conforms() {
    conformance::run(&TcpHarness::default()).await;
}

/// A CA, a server certificate for `127.0.0.1` and a client certificate,
/// written as PEM files.
struct Certificates {
    _dir: tempfile::TempDir,
    server: TlsOptions,
    client: TlsOptions,
    stranger: TlsOptions,
}

fn certificates() -> Certificates {
    use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};

    let dir = tempfile::tempdir().expect("temp dir");
    let write = |name: &str, contents: String| {
        let path = dir.path().join(name);
        std::fs::write(&path, contents).expect("write pem");
        path
    };
    let ca_key = KeyPair::generate().expect("ca key");
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("ca params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = ca_params.self_signed(&ca_key).expect("ca");
    let issuer = Issuer::from_params(&ca_params, &ca_key);
    let leaf = |name: &str| {
        let key = KeyPair::generate().expect("key");
        let cert = CertificateParams::new(vec![name.to_owned()])
            .expect("params")
            .signed_by(&key, &issuer)
            .expect("signed");
        (cert.pem(), key.serialize_pem())
    };
    let (server_cert, server_key) = leaf("127.0.0.1");
    let (client_cert, client_key) = leaf("client.internal");
    // A certificate from another CA.
    let other_key = KeyPair::generate().expect("other key");
    let other = CertificateParams::new(vec!["127.0.0.1".to_owned()])
        .expect("params")
        .self_signed(&other_key)
        .expect("other");
    let ca_path = write("ca.pem", ca.pem());
    let options = |cert: &str, key: &str, prefix: &str| TlsOptions {
        ca: Some(ca_path.clone()),
        cert: Some(write(&format!("{prefix}.pem"), cert.to_owned())),
        key: Some(write(&format!("{prefix}.key"), key.to_owned())),
    };
    let server = options(&server_cert, &server_key, "server");
    let client = options(&client_cert, &client_key, "client");
    let stranger = TlsOptions {
        ca: Some(ca_path.clone()),
        cert: Some(write("stranger.pem", other.pem())),
        key: Some(write("stranger.key", other_key.serialize_pem())),
    };
    Certificates {
        _dir: dir,
        server,
        client,
        stranger,
    }
}

#[tokio::test]
async fn tcp_with_mutual_tls_conforms() {
    let certificates = certificates();
    conformance::request_reply(&TcpHarness {
        tls: Some(certificates.server.clone()),
        client_tls: Some(certificates.client.clone()),
        ..TcpHarness::default()
    })
    .await;
    conformance::events_reach_every_service(&TcpHarness {
        tls: Some(certificates.server.clone()),
        client_tls: Some(certificates.client.clone()),
        ..TcpHarness::default()
    })
    .await;
}

#[tokio::test]
async fn mutual_tls_refuses_a_peer_without_a_trusted_certificate() {
    let certificates = certificates();
    let harness = TcpHarness {
        tls: Some(certificates.server.clone()),
        client_tls: Some(certificates.stranger.clone()),
        ..TcpHarness::default()
    };
    let server = harness.instance("conf-tls", &["conf-tls.ping"]).await;
    let listening = Listening::start(
        server,
        vec![Subscription {
            pattern: "conf-tls.ping".into(),
            kind: PatternKind::Message,
            group: Group::Service("conf-tls".into()),
            ordered: false,
            ack_wait: Duration::from_secs(1),
        }],
    )
    .await;
    let client = harness.client().await;
    let mut envelope = Envelope::new("conf-tls.ping", "{}");
    envelope.set_deadline(SystemTime::now() + Duration::from_secs(2));
    let outcome = client.request(envelope).await;
    assert!(
        matches!(
            outcome,
            Err(MicroError::Transport(_) | MicroError::Timeout { .. })
        ),
        "a stranger's certificate is refused: {outcome:?}"
    );
    listening.stop().await;
}

#[tokio::test]
async fn tcp_tls_files_that_do_not_parse_fail_at_construction() {
    let dir = tempfile::tempdir().unwrap();
    let garbage = dir.path().join("garbage.pem");
    std::fs::write(&garbage, "not a certificate").unwrap();
    let options = TcpOptions {
        tls: TlsOptions {
            ca: None,
            cert: Some(garbage.clone()),
            key: Some(garbage),
        },
        ..TcpOptions::default()
    };
    let error = TcpTransport::new(&options, None, 1024, Duration::from_secs(1)).unwrap_err();
    assert!(error.to_string().contains("garbage.pem"), "{error}");
}

#[tokio::test]
async fn tcp_frames_over_the_limit_drop_the_connection() {
    let small = TcpOptions {
        addr: "127.0.0.1:0".into(),
        ..TcpOptions::default()
    };
    let server = TcpTransport::new(&small, None, 64, Duration::from_secs(1)).unwrap();
    let listening = Listening::start(
        Arc::new(server.clone()),
        vec![Subscription {
            pattern: "conf-big.ping".into(),
            kind: PatternKind::Message,
            group: Group::Service("conf-big".into()),
            ordered: false,
            ack_wait: Duration::from_secs(1),
        }],
    )
    .await;
    let addr = server.wait_bound().await;
    let client = TcpTransport::new(
        &TcpOptions {
            routes: BTreeMap::from([(
                "conf-big.*".to_owned(),
                format!("127.0.0.1:{}", addr.port()),
            )]),
            ..TcpOptions::default()
        },
        None,
        1 << 20,
        Duration::from_secs(1),
    )
    .unwrap();
    let mut envelope = Envelope::new("conf-big.ping", vec![b'x'; 200 * 1024]);
    envelope.set_deadline(SystemTime::now() + Duration::from_secs(2));
    let outcome = client.request(envelope).await;
    assert!(
        matches!(
            outcome,
            Err(MicroError::Transport(_) | MicroError::Timeout { .. })
        ),
        "{outcome:?}"
    );
    listening.stop().await;
}

#[tokio::test]
async fn tcp_unrouted_patterns_go_to_the_default_peer() {
    let server = TcpTransport::new(
        &TcpOptions {
            addr: "127.0.0.1:0".into(),
            ..TcpOptions::default()
        },
        None,
        1 << 20,
        Duration::from_secs(1),
    )
    .unwrap();
    let mut listening = Listening::start(
        Arc::new(server.clone()),
        vec![Subscription {
            pattern: "billing.charge".into(),
            kind: PatternKind::Message,
            group: Group::Service("billing".into()),
            ordered: false,
            ack_wait: Duration::from_secs(1),
        }],
    )
    .await;
    let addr = server.wait_bound().await;
    let client = TcpTransport::new(
        &TcpOptions::default(),
        Some(format!("127.0.0.1:{}", addr.port())),
        1 << 20,
        Duration::from_secs(1),
    )
    .unwrap();
    let responder = tokio::spawn(async move {
        let delivery = listening
            .next(Duration::from_secs(5))
            .await
            .expect("delivery");
        let reply = delivery
            .envelope()
            .reply(200, "application/json", "\"charged\"");
        delivery.reply(reply).await.unwrap();
        listening
    });
    let mut envelope = Envelope::new("billing.charge", "{}").with_header(headers::MESSAGE_ID, "m1");
    envelope.set_deadline(SystemTime::now() + Duration::from_secs(5));
    let reply = client.request(envelope).await.expect("reply");
    assert_eq!(reply.payload().as_ref(), b"\"charged\"");
    responder.await.unwrap().stop().await;
}

#[tokio::test]
async fn tcp_keeps_up_to_pool_size_connections_and_multiplexes_requests() {
    let server = TcpTransport::new(
        &TcpOptions {
            addr: "127.0.0.1:0".into(),
            ..TcpOptions::default()
        },
        None,
        1 << 20,
        Duration::from_secs(1),
    )
    .unwrap();
    let mut listening = Listening::start(
        Arc::new(server.clone()),
        vec![Subscription {
            pattern: "pool.echo".into(),
            kind: PatternKind::Message,
            group: Group::Service("pool".into()),
            ordered: false,
            ack_wait: Duration::from_secs(1),
        }],
    )
    .await;
    let addr = server.wait_bound().await;
    let client = Arc::new(
        TcpTransport::new(
            &TcpOptions {
                routes: BTreeMap::from([(
                    "pool.*".to_owned(),
                    format!("127.0.0.1:{}", addr.port()),
                )]),
                pool_size: 2,
                ..TcpOptions::default()
            },
            None,
            1 << 20,
            Duration::from_secs(1),
        )
        .unwrap(),
    );
    let responder = tokio::spawn(async move {
        for _ in 0..20 {
            let delivery = listening
                .next(Duration::from_secs(5))
                .await
                .expect("delivery");
            let reply = delivery.envelope().reply(
                200,
                "application/json",
                delivery.envelope().payload().clone(),
            );
            delivery.reply(reply).await.unwrap();
        }
        listening
    });
    let mut calls = Vec::new();
    for n in 0..20 {
        let client = client.clone();
        calls.push(tokio::spawn(async move {
            let mut envelope = Envelope::new("pool.echo", n.to_string());
            envelope.set_deadline(SystemTime::now() + Duration::from_secs(5));
            let reply = client.request(envelope).await.expect("reply");
            String::from_utf8(reply.payload().to_vec()).unwrap()
        }));
    }
    let mut answers = HashSet::new();
    for call in calls {
        answers.insert(call.await.unwrap());
    }
    assert_eq!(answers.len(), 20, "every reply reached its own request");
    responder.await.unwrap().stop().await;
}
