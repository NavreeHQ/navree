//! The JSON side of the bus: `EventSink::publish` and `subscribe_json`.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::EventSink;
use navree_events::{EventBus, EventsError, EventsModule, ModuleBuilderEventsExt};
use navree_kernel::{Module, ModuleBuilder, ModuleId, Navree};
use serde_json::{Value, json};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::TryRecvError;

use crate::support::{Journal, UserRegistered, listener};

/// A module with nothing in it: resolving from it shows what is visible to
/// any module that does not import `EventsModule`.
struct PlainModule;

impl Module for PlainModule {
    fn register(&self, _m: &mut ModuleBuilder) {}
}

async fn next(rx: &mut Receiver<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a JSON event within 5 s")
        .expect("an open channel")
}

#[tokio::test]
async fn event_sink_publish_reaches_json_subscribers() {
    let app = Navree::app()
        .module(EventsModule::for_root())
        .module(PlainModule)
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();
    let mut registered = bus.subscribe_json("user.registered");
    let mut deleted = bus.subscribe_json("user.deleted");

    // Global export: any module can inject `dyn EventSink` without importing.
    let sink: Arc<dyn EventSink> = app
        .ctx()
        .resolver_for(ModuleId::of::<PlainModule>())
        .get::<dyn EventSink>()
        .await
        .unwrap();
    sink.publish("user.registered", json!({ "email": "ada" }))
        .await
        .unwrap();

    assert_eq!(next(&mut registered).await, json!({ "email": "ada" }));
    assert!(matches!(deleted.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn emit_reaches_json_subscribers() {
    let app = Navree::app()
        .module(EventsModule::for_root())
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();
    let mut first = bus.subscribe_json("user.registered");
    let mut second = bus.subscribe_json("user.registered");

    bus.emit(UserRegistered::new("ada")).await.unwrap();

    assert_eq!(next(&mut first).await, json!({ "email": "ada" }));
    assert_eq!(next(&mut second).await, json!({ "email": "ada" }));
}

listener!(Greeter, "greeter", [UserRegistered => |e| e.email]);

struct GreetingModule;

impl Module for GreetingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, Greeter>();
    }
}

async fn greeting_app(journal: &Arc<Journal>) -> navree_kernel::App {
    Navree::app()
        .module(EventsModule::for_root())
        .module(GreetingModule)
        .override_value(journal.clone())
        .boot()
        .await
        .unwrap()
}

#[tokio::test]
async fn event_sink_publish_runs_typed_listeners() {
    let journal = Arc::new(Journal::default());
    let app = greeting_app(&journal).await;
    let sink = app.ctx().get::<dyn EventSink>().await.unwrap();

    sink.publish("user.registered", json!({ "email": "ada" }))
        .await
        .unwrap();

    assert_eq!(journal.entries(), ["greeter:ada"]);
}

#[tokio::test]
async fn event_sink_publish_with_invalid_payload_fails_naming_event() {
    let journal = Arc::new(Journal::default());
    let app = greeting_app(&journal).await;
    let bus = app.ctx().get::<EventBus>().await.unwrap();
    let mut subscriber = bus.subscribe_json("user.registered");
    let sink = app.ctx().get::<dyn EventSink>().await.unwrap();

    let error = sink
        .publish("user.registered", json!({ "mail": 42 }))
        .await
        .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("user.registered"), "{message}");
    assert!(matches!(
        error.downcast_ref::<EventsError>(),
        Some(EventsError::InvalidPayload { event, .. }) if event == "user.registered"
    ));
    // Nothing was delivered, typed or JSON.
    assert!(journal.entries().is_empty());
    assert!(matches!(subscriber.try_recv(), Err(TryRecvError::Empty)));
}
