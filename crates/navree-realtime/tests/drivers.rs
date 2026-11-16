//! Backplanes a module registers under a name: `realtime.backplane` selects
//! one, the hub publishes its broadcasts through it, and a name that resolves
//! to nothing, or to two things, fails the boot.

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use navree_config::DriverName;
use navree_contracts::Secret;
use navree_http::{HttpModule, HttpOptions};
use navree_kernel::{AppBuilder, BoxError, BoxFuture, Module, ModuleBuilder, Navree};
use navree_realtime::{
    Backplane, BackplaneHandler, BackplaneKind, ModuleBuilderBackplaneExt, Realtime,
    RealtimeModule, RealtimeOptions,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// The test server's options.
fn options() -> RealtimeOptions {
    RealtimeOptions {
        app_id: "test".to_owned(),
        key: "test-key".to_owned(),
        secret: Secret::new("test-secret".to_owned()),
        allowed_origins: vec!["http://localhost:3000".to_owned()],
        activity_timeout_secs: 120,
        ..RealtimeOptions::default()
    }
}

/// The test app: HTTP and realtime with `options`.
fn app(options: RealtimeOptions) -> AppBuilder {
    Navree::app()
        .module(HttpModule::for_root(HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        }))
        .module(RealtimeModule::for_root(options))
}

/// The options of the test server, selecting the backplane `name`.
fn selecting(name: &'static str) -> RealtimeOptions {
    RealtimeOptions {
        backplane: BackplaneKind::Registered(DriverName::from_static(name)),
        ..options()
    }
}

/// A backplane that keeps what it is asked to carry: the topics it is
/// subscribed to, and each message published, with its topic.
#[derive(Clone, Default)]
struct RecordingBackplane {
    published: Arc<Mutex<Vec<(String, String)>>>,
    subscribed: Arc<Mutex<Vec<String>>>,
}

impl RecordingBackplane {
    fn published(&self) -> Vec<(String, String)> {
        self.published.lock().unwrap().clone()
    }

    fn subscribed(&self) -> Vec<String> {
        self.subscribed.lock().unwrap().clone()
    }
}

impl Backplane for RecordingBackplane {
    fn publish(&self, topic: &str, message: Bytes) {
        self.published.lock().unwrap().push((
            topic.to_owned(),
            String::from_utf8_lossy(&message).into_owned(),
        ));
    }

    fn subscribe(
        &self,
        topic: &str,
        _handler: BackplaneHandler,
        _shutdown: CancellationToken,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        self.subscribed.lock().unwrap().push(topic.to_owned());
        Box::pin(async { Ok(()) })
    }
}

/// Registers the backplane `kabootar` over a recording backplane the test
/// keeps, and records the name and the settings key its factory was given.
#[derive(Default)]
struct KabootarModule {
    backplane: RecordingBackplane,
    seen: Arc<Mutex<Option<(String, String)>>>,
}

impl Module for KabootarModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let (backplane, seen) = (self.backplane.clone(), self.seen.clone());
        m.backplane("kabootar", move |ctx| {
            let (backplane, seen) = (backplane.clone(), seen.clone());
            async move {
                *seen.lock().unwrap() = Some((
                    ctx.name().as_str().to_owned(),
                    ctx.settings_key().to_owned(),
                ));
                Ok(backplane)
            }
        });
    }
}

/// Registers `kabootar` a second time.
struct SecondKabootarModule;

impl Module for SecondKabootarModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.backplane("kabootar", |_| async { Ok(RecordingBackplane::default()) });
    }
}

/// Registers a backplane under the built-in name `none`.
struct NoneNamedModule;

impl Module for NoneNamedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.backplane("none", |_| async { Ok(RecordingBackplane::default()) });
    }
}

/// Registers a backplane under the name of a key of the `realtime` section.
struct StreamsNamedModule;

impl Module for StreamsNamedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.backplane("streams", |_| async { Ok(RecordingBackplane::default()) });
    }
}

/// Registers a backplane whose factory fails.
struct BrokenModule;

impl Module for BrokenModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.backplane("broken", |_| async {
            Err::<RecordingBackplane, BoxError>("the Amritsar relay is down".into())
        });
    }
}

#[tokio::test]
async fn a_registered_backplane_carries_broadcasts() {
    let module = KabootarModule::default();
    let (recording, seen) = (module.backplane.clone(), module.seen.clone());
    let app = app(selecting("kabootar"))
        .module(module)
        .boot()
        .await
        .expect("the app boots");
    let realtime = app.ctx().get::<Realtime>().await.expect("Realtime");

    assert!(realtime.has_backplane());
    assert_eq!(realtime.options().backplane.as_str(), "kabootar");
    assert_eq!(recording.subscribed(), ["realtime:test"]);
    assert_eq!(
        *seen.lock().unwrap(),
        Some(("kabootar".to_owned(), "realtime.kabootar".to_owned()))
    );

    let delivered = realtime
        .broadcast(
            ["private-crm.deals"],
            "deal.updated",
            json!({"city": "Amritsar"}),
        )
        .unwrap();
    assert_eq!(delivered, 0, "no socket is connected in this process");
    let published = recording.published();
    let broadcasts: Vec<&(String, String)> = published
        .iter()
        .filter(|(_, text)| text.contains("\"type\":\"broadcast\""))
        .collect();
    assert_eq!(broadcasts.len(), 1, "{published:?}");
    let (topic, text) = broadcasts[0];
    assert_eq!(topic, "realtime:test");
    assert!(
        text.contains("deal.updated") && text.contains("Amritsar"),
        "{text}"
    );
}

#[tokio::test]
async fn a_registered_backplane_needs_a_shared_secret() {
    let error = app(RealtimeOptions {
        secret: Secret::default(),
        ..selecting("kabootar")
    })
    .module(KabootarModule::default())
    .boot()
    .await
    .expect_err("no secret");
    let message = error.to_string();
    assert!(
        message.contains("realtime.backplane = \"kabootar\" needs realtime.secret: every process must sign channel subscriptions with the same secret"),
        "{message}"
    );
}

#[tokio::test]
async fn an_unregistered_backplane_fails_the_boot_naming_m_backplane() {
    let error = app(selecting("chithi"))
        .module(KabootarModule::default())
        .boot()
        .await
        .expect_err("no backplane named chithi");
    let message = error.to_string();
    assert!(
        message.contains("realtime.backplane does not name a built-in backplane or one a module registers with m.backplane (known: none, redis, kabootar)"),
        "{message}"
    );
}

#[tokio::test]
async fn a_backplane_registration_that_cannot_work_fails_the_boot_naming_m_backplane() {
    let twice = "backplane `kabootar` is registered by both `KabootarModule` and `SecondKabootarModule`; register it once";
    let message = app(options())
        .module(KabootarModule::default())
        .module(SecondKabootarModule)
        .boot()
        .await
        .expect_err("two backplanes named kabootar")
        .to_string();
    assert!(
        message.contains(twice),
        "checked although no backplane is selected: {message}"
    );

    let built_in = "m.backplane(\"none\") in `NoneNamedModule`: `none` is a built-in backplane; register yours under another name";
    let message = app(options())
        .module(NoneNamedModule)
        .boot()
        .await
        .expect_err("a built-in name")
        .to_string();
    assert!(message.contains(built_in), "{message}");

    let setting = "m.backplane(\"streams\") in `StreamsNamedModule`: `realtime.streams` is a setting of the realtime section; register the backplane under another name";
    let message = app(options())
        .module(StreamsNamedModule)
        .boot()
        .await
        .expect_err("the name of a setting")
        .to_string();
    assert!(message.contains(setting), "{message}");

    let message = app(selecting("broken"))
        .module(BrokenModule)
        .boot()
        .await
        .expect_err("a factory that fails")
        .to_string();
    assert!(
        message.contains("backplane `broken` (registered with m.backplane in `BrokenModule`) failed to start: the Amritsar relay is down"),
        "{message}"
    );
}
