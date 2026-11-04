//! A listener is subscribed when it says so, it starts again when its listen fails, and while
//! it is not subscribed the app's readiness check says so. A boot waits for it for a while,
//! and never past the app being stopped. The transport here is a memory broker that can be
//! told to fail, or to say nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use navree_contracts::{HealthCheck, HealthStatus};
use navree_kernel::{BoxFuture, Module, ModuleBuilder};
use navree_micro::{
    Delivery, Envelope, MemoryTransport, MessageHandlers, MicroError, MicroModule, MicroOptions,
    MicroTransport, ModuleBuilderMicroExt, PatternDef, Payload, ServiceClient, Subscribed,
    Subscription, Transport,
};
use navree_testing::TestApp;
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};

/// Long enough that nothing but a hang reaches it, however busy the machine is.
const HANG: Duration = Duration::from_secs(120);

/// What a scripted listen does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    /// Fails with an error, as a broker that refused the subscriptions would.
    Fail,
    /// Waits for the test to let it go, then works.
    WaitForTheGate,
    /// Says nothing and never ends until the listener is stopped, as a hung broker would.
    Silent,
}

/// A memory broker whose listen follows a script: the attempts in order, then it works.
#[derive(Clone)]
struct Scripted {
    broker: MemoryTransport,
    script: Arc<Vec<Script>>,
    attempts: Arc<AtomicUsize>,
    gate: Arc<Notify>,
    at_boot: Arc<AtomicBool>,
}

impl Scripted {
    fn new(script: Vec<Script>) -> Self {
        Self {
            broker: MemoryTransport::new(),
            script: Arc::new(script),
            attempts: Arc::default(),
            gate: Arc::new(Notify::new()),
            at_boot: Arc::new(AtomicBool::new(true)),
        }
    }

    fn not_at_boot(self) -> Self {
        self.at_boot.store(false, Ordering::SeqCst);
        self
    }

    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }
}

impl MicroTransport for Scripted {
    fn name(&self) -> &'static str {
        "scripted"
    }

    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        self.broker.request(envelope)
    }

    fn publish(&self, envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>> {
        self.broker.publish(envelope)
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        self.broker.listen(subscriptions, deliveries)
    }

    fn listen_subscribed(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
        subscribed: Subscribed,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        let step = self.script.get(attempt).copied();
        Box::pin(async move {
            match step {
                Some(Script::Fail) => Err(MicroError::transport("the broker refused the listener")),
                Some(Script::Silent) => {
                    // Holds the signal, and ends when nobody is listening any more.
                    let _held = subscribed;
                    deliveries.closed().await;
                    Ok(())
                }
                Some(Script::WaitForTheGate) | None => {
                    if step.is_some() {
                        self.gate.notified().await;
                    }
                    self.broker
                        .listen_subscribed(subscriptions, deliveries, subscribed)
                        .await
                }
            }
        })
    }

    fn listens_at_boot(&self) -> bool {
        self.at_boot.load(Ordering::SeqCst)
    }
}

/// One message handler, `hello.open`.
#[navree_macros::injectable]
pub struct Hello;

impl MessageHandlers for Hello {
    fn patterns(self: Arc<Self>) -> Vec<PatternDef> {
        vec![
            PatternDef::message("hello.open", |Payload(value): Payload<Value>| async move {
                navree_http::Json(value)
            })
            .handler_name("open")
            .public(),
        ]
    }
}

struct HelloModule;

impl Module for HelloModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.message_handlers::<Hello>();
    }
}

fn options(transport: Scripted, connect_timeout_ms: u64) -> MicroOptions {
    MicroOptions {
        service: "hello".into(),
        connect_timeout_ms,
        ..MicroOptions::from(Transport::custom(transport))
    }
}

async fn health(app: &TestApp) -> HealthStatus {
    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    let check = checks
        .iter()
        .find(|check| check.name() == "micro")
        .expect("a micro check");
    check.check().await
}

/// Waits until the readiness check is up; the timeout only ends a hang.
async fn until_up(app: &TestApp) {
    tokio::time::timeout(HANG, async {
        while !matches!(health(app).await, HealthStatus::Up) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the listener subscribed");
}

/// A listen that fails is not the end of the listener: it starts again, and until it has its
/// subscriptions the readiness check is down, so an app that hears nothing does not look well.
#[tokio::test]
async fn a_failed_listen_is_started_again_and_the_app_is_not_ready_until_it_is_subscribed() {
    let transport = Scripted::new(vec![Script::Fail, Script::WaitForTheGate]);
    let app = TestApp::builder()
        .module(MicroModule::for_root(options(transport.clone(), 5_000)))
        .module(HelloModule)
        .build()
        .await;
    // The first listen failed, and boot went on without waiting for a listener that is down.
    match health(&app).await {
        HealthStatus::Down(reason) => {
            assert_eq!(
                reason,
                "the scripted listener is not subscribed to its messages"
            );
        }
        other => panic!("{other:?}"),
    }
    // The second listen is held at the gate: the app is still not ready, and the listener is
    // not silently gone.
    tokio::time::timeout(HANG, async {
        while transport.attempts() < 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the listener started again");
    assert!(matches!(health(&app).await, HealthStatus::Down(_)));
    // The broker answers: the app is ready, and the handler is heard.
    transport.gate.notify_one();
    until_up(&app).await;
    let client = app.get::<ServiceClient>().await;
    let reply: Value = client
        .send("hello.open", &json!({ "hello": "again" }))
        .await
        .expect("the handler answers");
    assert_eq!(reply, json!({ "hello": "again" }));
    app.shutdown().await.unwrap();
}

/// A boot waits for the listener only so long: a broker that says nothing for six times the
/// connect timeout lets the app start, not ready, with the listener still trying.
#[tokio::test]
async fn a_boot_does_not_wait_for_ever_for_a_listener_that_says_nothing() {
    let transport = Scripted::new(vec![Script::Silent]);
    // The wait is 1.2 seconds; the guard only ends a hang.
    let app = tokio::time::timeout(
        Duration::from_secs(30),
        TestApp::builder()
            .module(MicroModule::for_root(options(transport.clone(), 200)))
            .module(HelloModule)
            .build(),
    )
    .await
    .expect("the boot went on");
    assert_eq!(transport.attempts(), 1);
    assert!(matches!(health(&app).await, HealthStatus::Down(_)));
    app.shutdown().await.unwrap();
}

/// A boot that is stopped does not wait for the listener at all, whatever it was given.
#[tokio::test]
async fn a_listener_that_says_nothing_does_not_hold_up_stopping_the_app() {
    let transport = Scripted::new(vec![Script::Silent]).not_at_boot();
    let app = TestApp::builder()
        .module(MicroModule::for_root(options(transport.clone(), 3_600_000)))
        .module(HelloModule)
        .build()
        .await;
    let ctx = app.ctx().clone();
    let starting = tokio::spawn(async move { MicroModule::start_listener(&ctx).await });
    tokio::time::timeout(HANG, async {
        while transport.attempts() < 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the listener started");
    // An hour of boot wait is given, and nothing has been said: stopping must end it.
    tokio::time::timeout(Duration::from_secs(30), app.shutdown())
        .await
        .expect("stopping the app does not wait for the listener")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), starting)
        .await
        .expect("starting the listener ended with the app")
        .unwrap()
        .unwrap();
}
