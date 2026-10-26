//! `Telemetry`: the registry emitters hold, and how events reach sinks.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_telemetry::{
    CacheEvent, CacheOperation, EventKind, Interest, QueryExecuted, Telemetry, TelemetryContext,
    TelemetryEvent, TelemetryRecord, TelemetryRecorder, TelemetrySink,
};

fn query(sql: &str) -> TelemetryEvent {
    TelemetryEvent::QueryExecuted(QueryExecuted {
        sql: sql.to_owned(),
        duration: Duration::from_millis(3),
        rows: None,
        connection: "sqlite".to_owned(),
        failed: false,
    })
}

fn cache_hit(key: &str) -> TelemetryEvent {
    TelemetryEvent::Cache(CacheEvent {
        operation: CacheOperation::Hit,
        key: key.to_owned(),
        store: "memory".to_owned(),
    })
}

/// A sink that only wants queries and counts what it receives.
#[derive(Default)]
struct QueriesOnly(AtomicUsize);

impl TelemetrySink for QueriesOnly {
    fn interest(&self) -> Interest {
        Interest::only([EventKind::QueryExecuted])
    }

    fn record(&self, _record: &TelemetryRecord) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_disabled_registry_wants_nothing() {
    let telemetry = Telemetry::disabled();
    assert!(!telemetry.is_active());
    for kind in EventKind::ALL {
        assert!(!telemetry.enabled(kind), "{kind:?}");
    }
    telemetry.emit(query("SELECT 1")); // goes nowhere, does not panic
    assert!(!Telemetry::default().is_active());
}

#[test]
fn emit_with_does_not_build_events_nobody_wants() {
    let telemetry = Telemetry::disabled();
    telemetry.emit_with(EventKind::QueryExecuted, || {
        panic!("the event must not be built for a disabled registry")
    });

    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(QueriesOnly::default())]);
    telemetry.emit_with(EventKind::Cache, || {
        panic!("no sink wants cache events")
    });
    let built = AtomicUsize::new(0);
    telemetry.emit_with(EventKind::QueryExecuted, || {
        built.fetch_add(1, Ordering::SeqCst);
        query("SELECT 1")
    });
    assert_eq!(built.load(Ordering::SeqCst), 1);
    assert!(recorder.is_empty(), "the recorder is not registered here");
}

#[test]
fn events_reach_every_interested_sink() {
    let recorder = TelemetryRecorder::new();
    let queries = Arc::new(QueriesOnly::default());
    let telemetry = Telemetry::with_sinks(vec![
        Arc::new(recorder.clone()) as Arc<dyn TelemetrySink>,
        queries.clone(),
    ]);
    assert!(telemetry.is_active());
    assert!(telemetry.enabled(EventKind::QueryExecuted));
    assert!(telemetry.enabled(EventKind::Cache), "the recorder wants everything");

    telemetry.emit(query("SELECT 1"));
    telemetry.emit(cache_hit("stats"));

    assert_eq!(recorder.len(), 2);
    assert_eq!(queries.0.load(Ordering::SeqCst), 1, "only the query");
    assert_eq!(recorder.queries()[0].sql, "SELECT 1");
    assert_eq!(recorder.cache_events()[0].key, "stats");
}

#[test]
fn interest_is_the_union_of_the_sinks() {
    let telemetry = Telemetry::with_sinks(vec![Arc::new(QueriesOnly::default())]);
    assert!(telemetry.enabled(EventKind::QueryExecuted));
    assert!(!telemetry.enabled(EventKind::RequestHandled));
    assert!(!telemetry.enabled(EventKind::Log));
    let none = Telemetry::with_sinks(Vec::new());
    assert!(!none.is_active());
}

#[test]
fn interest_sets_combine() {
    let jobs = Interest::only([EventKind::JobProcessed, EventKind::JobFailed]);
    assert!(jobs.contains(EventKind::JobFailed));
    assert!(!jobs.contains(EventKind::MailSent));
    let more = jobs.with(EventKind::MailSent);
    assert!(more.contains(EventKind::MailSent));
    assert!(Interest::ALL.contains(EventKind::Log));
    assert!(!Interest::NONE.contains(EventKind::Log));
    assert!(Interest::ALL.without(EventKind::Log).contains(EventKind::Cache));
    assert!(!Interest::ALL.without(EventKind::Log).contains(EventKind::Log));
    assert_eq!(jobs.union(Interest::only([EventKind::MailSent])), more);
    assert!(Interest::NONE.is_empty());
}

#[test]
fn every_event_names_its_kind() {
    assert_eq!(query("SELECT 1").kind(), EventKind::QueryExecuted);
    assert_eq!(cache_hit("k").kind(), EventKind::Cache);
    assert_eq!(EventKind::QueryExecuted.as_str(), "query_executed");
    assert_eq!(EventKind::ALL.len(), 11);
}

#[tokio::test]
async fn records_carry_the_current_context() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let trace = navree_telemetry::TraceContext::root(true);
    let context = TelemetryContext::new(telemetry.clone())
        .with_request_id("req-1")
        .with_trace(trace.clone());
    context
        .clone()
        .scope(async {
            TelemetryContext::current()
                .expect("in scope")
                .set_user_id("user-7");
            telemetry.emit(query("SELECT 1"));
        })
        .await;
    telemetry.emit(query("SELECT 2"));

    let records = recorder.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].context.request_id.as_deref(), Some("req-1"));
    assert_eq!(records[0].context.user_id.as_deref(), Some("user-7"));
    assert_eq!(
        records[0].context.trace.as_ref().map(|t| t.trace_id),
        Some(trace.trace_id)
    );
    assert_eq!(records[1].context.request_id, None, "outside the scope");
    assert!(records[0].at <= records[1].at);
}

#[test]
fn exceptions_are_reported_with_their_chain() {
    #[derive(Debug)]
    struct Outer(std::io::Error);
    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("cannot load the deal")
        }
    }
    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let error = Outer(std::io::Error::other("disk full"));
    telemetry.report_exception(&error);

    let exceptions = recorder.exceptions();
    assert_eq!(exceptions.len(), 1);
    assert_eq!(exceptions[0].message, "cannot load the deal");
    assert_eq!(exceptions[0].chain, ["cannot load the deal", "disk full"]);
    assert!(!exceptions[0].panic);
    let location = exceptions[0]
        .location
        .as_deref()
        .expect("the caller's location");
    assert!(location.contains("registry.rs"), "{location}");
}

#[tokio::test]
async fn panic_locations_are_kept_in_the_context() {
    navree_telemetry::capture_panic_locations();
    navree_telemetry::capture_panic_locations(); // idempotent
    let context = TelemetryContext::new(Telemetry::disabled());
    let inner = context.clone();
    let joined = tokio::spawn(context.scope(async {
        panic!("boom");
    }))
    .await;
    assert!(joined.is_err());
    let location = inner.panic_location().expect("recorded by the hook");
    assert!(location.contains("registry.rs"), "{location}");
}

#[test]
fn the_current_registry_comes_from_the_context() {
    assert!(!Telemetry::current().is_active(), "no context: disabled");
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    TelemetryContext::new(telemetry).sync_scope(|| {
        assert!(Telemetry::current().is_active());
        Telemetry::current().emit(query("SELECT 1"));
    });
    assert_eq!(recorder.queries().len(), 1);
}

#[test]
fn records_serialize_to_json() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    telemetry.emit(query("SELECT 1"));
    let json = serde_json::to_value(&recorder.records()[0]).expect("serializable");
    assert_eq!(json["event"]["type"], "query_executed");
    assert_eq!(json["event"]["sql"], "SELECT 1");
    assert!(json["at"].is_string());
}
