//! Helpers shared by the navree-logbook tests.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_logbook::{
    Content, DumpEntry, Entry, Logbook, LogbookOptions, QueryEntry, RequestEntry, StoreKind,
};
use navree_telemetry::{
    EventContext, QueryExecuted, RequestHandled, TelemetryEvent, TelemetryRecord, TelemetrySink,
    TraceContext,
};
use serde_json::json;

/// Options for tests: on, in memory.
pub fn options() -> LogbookOptions {
    LogbookOptions {
        enabled: Some(true),
        store: StoreKind::Memory,
        ..LogbookOptions::default()
    }
}

/// A Logbook recording in memory.
pub fn logbook() -> Arc<Logbook> {
    Arc::new(Logbook::in_memory(options()))
}

/// A Logbook recording in memory with `f` applied to its options.
pub fn logbook_with(f: impl FnOnce(&mut LogbookOptions)) -> Arc<Logbook> {
    let mut options = options();
    f(&mut options);
    Arc::new(Logbook::in_memory(options))
}

/// The context of a request.
pub fn in_request(request_id: &str) -> EventContext {
    EventContext {
        request_id: Some(request_id.to_owned()),
        trace: Some(TraceContext::root(true)),
        ..EventContext::default()
    }
}

/// The context of a job.
pub fn in_job(job_id: &str) -> EventContext {
    EventContext {
        job_id: Some(job_id.to_owned()),
        trace: Some(TraceContext::root(true)),
        ..EventContext::default()
    }
}

/// Feeds one event to the Logbook as its telemetry sink would get it.
pub fn feed(logbook: &Logbook, context: &EventContext, event: TelemetryEvent) {
    logbook.record(&TelemetryRecord {
        at: Utc::now(),
        context: context.clone(),
        event,
    });
}

pub fn query_event(sql: &str, ms: u64) -> TelemetryEvent {
    TelemetryEvent::QueryExecuted(QueryExecuted {
        sql: sql.to_owned(),
        duration: Duration::from_millis(ms),
        pool_wait: std::time::Duration::ZERO,
        rows: None,
        connection: "sqlite".to_owned(),
        failed: false,
    })
}

pub fn request_event(request_id: &str, method: &str, path: &str, status: u16) -> TelemetryEvent {
    TelemetryEvent::RequestHandled(RequestHandled {
        method: method.to_owned(),
        route: Some(path.to_owned()),
        path: path.to_owned(),
        status,
        duration: Duration::from_millis(12),
        user_id: None,
        workspace_id: None,
        request_id: request_id.to_owned(),
        client_ip: Some("127.0.0.1".to_owned()),
    })
}

/// A plain entry for store tests.
pub fn entry_at(at: DateTime<Utc>, batch: Option<&str>, content: Content) -> Entry {
    let mut entry = Entry::new(at, content);
    entry.batch_id = batch.map(str::to_owned);
    entry
}

pub fn dump(label: &str) -> Content {
    Content::Dump(DumpEntry {
        label: label.to_owned(),
        value: json!(label),
    })
}

pub fn query(sql: &str, ms: f64) -> Content {
    Content::Query(QueryEntry {
        sql: sql.to_owned(),
        connection: "sqlite".to_owned(),
        rows: None,
        duration_ms: ms,
        failed: false,
        slow: false,
    })
}

pub fn request(method: &str, path: &str, route: &str, status: u16) -> Content {
    Content::Request(RequestEntry {
        method: method.to_owned(),
        path: path.to_owned(),
        route: Some(route.to_owned()),
        status,
        request_id: "r".to_owned(),
        client_ip: None,
        duration_ms: 10.0,
        request: None,
        response: None,
        replay_of: None,
        summary: Default::default(),
    })
}
