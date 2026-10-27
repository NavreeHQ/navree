//! Telemetry: every emit and publish is an `EventEmitted` event for the
//! sinks that want events, with its payload and listeners.

use std::sync::Arc;

use navree_contracts::{Dispatcher, EventSink};
use navree_events::{EventBus, EventsModule, ModuleBuilderEventsExt};
use navree_kernel::{Module, ModuleBuilder, Navree};
use navree_telemetry::{EventKind, Telemetry, TelemetryContext, TelemetryRecorder};
use serde_json::json;

use crate::support::{CaptureDispatcher, Journal, UserRegistered, listener};

listener!(Greeter, "greeter", [UserRegistered => |e| e.email]);
listener!(Mailer, "mailer", queued, [UserRegistered => |e| e.email]);

struct SignupModule;

impl Module for SignupModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, Greeter>()
            .listen::<UserRegistered, Mailer>();
    }
}

#[tokio::test]
async fn emits_are_reported_with_payload_and_listeners() {
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(recorder.module())
        .module(EventsModule::for_root())
        .module(SignupModule)
        .override_value(Arc::new(Journal::default()))
        .override_value(Arc::new(CaptureDispatcher::default()) as Arc<dyn Dispatcher>)
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();

    let context =
        TelemetryContext::new(Telemetry::from_app(app.ctx()).await).with_request_id("req-1");
    context
        .scope(bus.emit(UserRegistered::new("ada@example.com")))
        .await
        .unwrap();

    let records = recorder.of_kind(EventKind::EventEmitted);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].context.request_id.as_deref(), Some("req-1"));
    let emitted = recorder.emitted_events();
    assert_eq!(emitted[0].name, "user.registered");
    assert_eq!(emitted[0].payload, json!({ "email": "ada@example.com" }));
    assert_eq!(emitted[0].listeners.len(), 1);
    assert!(emitted[0].listeners[0].ends_with("Greeter"), "{emitted:?}");
    assert_eq!(emitted[0].queued.len(), 1);
    assert!(emitted[0].queued[0].ends_with("Mailer"), "{emitted:?}");
    assert!(emitted[0].failed.is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn published_json_events_are_reported_too() {
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(recorder.module())
        .module(EventsModule::for_root())
        .boot()
        .await
        .unwrap();
    let sink = app.ctx().get::<dyn EventSink>().await.unwrap();
    sink.publish("deal.won", json!({ "id": 7 })).await.unwrap();

    let emitted = recorder.emitted_events();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].name, "deal.won");
    assert_eq!(emitted[0].payload, json!({ "id": 7 }));
    assert!(emitted[0].listeners.is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn nothing_is_reported_without_a_sink_that_wants_events() {
    let recorder = TelemetryRecorder::only([EventKind::Log]);
    let app = Navree::app()
        .module(recorder.module())
        .module(EventsModule::for_root())
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("ada")).await.unwrap();
    assert!(recorder.is_empty());
    app.shutdown().await.unwrap();
}
