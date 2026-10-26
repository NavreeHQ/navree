//! `TelemetryModule` and `ModuleBuilderTelemetryExt`: sinks in the container.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, Navree, ProviderKey,
    Resolver,
};
use navree_telemetry::{
    EventKind, ModuleBuilderTelemetryExt, QueryExecuted, Telemetry, TelemetryEvent,
    TelemetryModule, TelemetryRecord, TelemetryRecorder, TelemetrySink,
};

fn query() -> TelemetryEvent {
    TelemetryEvent::QueryExecuted(QueryExecuted {
        sql: "SELECT 1".to_owned(),
        duration: Duration::from_millis(1),
        rows: Some(1),
        connection: "sqlite".to_owned(),
        failed: false,
    })
}

/// An emitter, like `Db`: resolves `Telemetry` when it is built.
struct Emitter {
    telemetry: Telemetry,
}

impl Injectable for Emitter {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Emitter {
                telemetry: Telemetry::from_app(&r.app()).await,
            })
        })
    }
}

struct EmitterModule;

impl Module for EmitterModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global().provider::<Emitter>().exports::<Emitter>();
    }
}

#[tokio::test]
async fn without_a_sink_the_registry_is_disabled_and_absent() {
    let app = Navree::app().module(EmitterModule).boot().await.unwrap();
    let emitter = app.ctx().get::<Emitter>().await.unwrap();
    assert!(!emitter.telemetry.is_active());
    assert!(app.ctx().try_get::<Telemetry>().await.is_none());
    assert!(
        !app.ctx().modules().iter().any(|m| m.name.contains("TelemetryModule")),
        "no sink, no module"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_recorder_module_receives_what_emitters_send() {
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(EmitterModule)
        .module(recorder.module())
        .boot()
        .await
        .unwrap();
    let emitter = app.ctx().get::<Emitter>().await.unwrap();
    assert!(emitter.telemetry.is_active());
    emitter.telemetry.emit(query());
    assert_eq!(recorder.queries().len(), 1);
    app.shutdown().await.unwrap();
}

/// A sink built by the container that depends on an emitter (as a
/// database-backed sink depends on `Db`, which emits queries).
struct StoringSink {
    _emitter: Arc<Emitter>,
    seen: Arc<AtomicUsize>,
}

impl Injectable for StoringSink {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Emitter>(), ProviderKey::of::<Arc<AtomicUsize>>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(StoringSink {
                _emitter: r.get::<Emitter>().await?,
                seen: (*r.get::<Arc<AtomicUsize>>().await?).clone(),
            })
        })
    }
}

impl TelemetrySink for StoringSink {
    fn record(&self, _record: &TelemetryRecord) {
        self.seen.fetch_add(1, Ordering::SeqCst);
    }
}

struct StoringModule(Arc<AtomicUsize>);

impl Module for StoringModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(self.0.clone()).telemetry_sink::<StoringSink>();
    }
}

#[tokio::test]
async fn injectable_sinks_may_depend_on_emitters() {
    let seen = Arc::new(AtomicUsize::new(0));
    let app = tokio::time::timeout(
        Duration::from_secs(10),
        Navree::app()
            .module(EmitterModule)
            .module(StoringModule(seen.clone()))
            .boot(),
    )
    .await
    .expect("no deadlock between the sink and the emitter")
    .expect("boots");
    let emitter = app.ctx().get::<Emitter>().await.unwrap();
    emitter.telemetry.emit(query());
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    app.shutdown().await.unwrap();
}

struct FactorySinkModule(TelemetryRecorder);

impl Module for FactorySinkModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let recorder = self.0.clone();
        m.telemetry_sink_factory(Factory::<dyn TelemetrySink>::new(move |_| {
            let recorder = recorder.clone();
            async move { Ok(Arc::new(recorder) as Arc<dyn TelemetrySink>) }
        }));
    }
}

struct ValueSinkModule(TelemetryRecorder);

impl Module for ValueSinkModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.telemetry_sink_value(self.0.clone());
    }
}

#[tokio::test]
async fn several_modules_share_one_registry() {
    let first = TelemetryRecorder::new();
    let second = TelemetryRecorder::new();
    let app = Navree::app()
        .module(EmitterModule)
        .module(FactorySinkModule(first.clone()))
        .module(ValueSinkModule(second.clone()))
        .boot()
        .await
        .unwrap();
    let telemetry = app.ctx().get::<Telemetry>().await.unwrap();
    assert!(telemetry.enabled(EventKind::QueryExecuted));
    app.ctx().get::<Emitter>().await.unwrap().telemetry.emit(query());
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    let modules = app.ctx().modules();
    assert_eq!(
        modules
            .iter()
            .filter(|m| m.name.contains("TelemetryModule"))
            .count(),
        1
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_plain_telemetry_module_without_sinks_stays_inactive() {
    let app = Navree::app()
        .module(TelemetryModule)
        .module(EmitterModule)
        .boot()
        .await
        .unwrap();
    let telemetry = app.ctx().get::<Telemetry>().await.unwrap();
    assert!(!telemetry.is_active());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn nothing_is_delivered_after_shutdown() {
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(EmitterModule)
        .module(recorder.module())
        .boot()
        .await
        .unwrap();
    let emitter = app.ctx().get::<Emitter>().await.unwrap();
    app.shutdown().await.unwrap();
    assert!(!emitter.telemetry.is_active());
    emitter.telemetry.emit(query());
    assert!(recorder.is_empty());
}
