//! Telemetry: log lines as `LogRecord` events, and trace ids in JSON lines.

use std::sync::Arc;

use navree_log::LogOptions;
use navree_telemetry::{
    EventKind, LogLevel, Telemetry, TelemetryContext, TelemetryRecorder, TraceContext,
};
use serde_json::{Value, json};

use crate::support::{capture, json as json_options, pretty};

fn recording() -> (TelemetryContext, TelemetryRecorder) {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    (TelemetryContext::new(telemetry), recorder)
}

#[test]
fn log_lines_in_a_context_become_log_records() {
    let (context, recorder) = recording();
    capture(&pretty(), |_| {
        context.sync_scope(|| {
            tracing::warn!(target: "crm::deals", deal_id = 7, password = "hunter2", "slow export");
        });
    });
    let logs = recorder.logs();
    assert_eq!(logs.len(), 1, "{logs:?}");
    let log = &logs[0];
    assert_eq!(log.level, LogLevel::Warn);
    assert_eq!(log.target, "crm::deals");
    assert_eq!(log.message, "slow export");
    assert_eq!(log.fields.get("deal_id"), Some(&json!(7)));
    assert_eq!(
        log.fields.get("password"),
        Some(&json!("[redacted]")),
        "redacted like the output"
    );
    assert!(!log.fields.contains_key("message"));
}

#[test]
fn filtered_out_lines_are_not_recorded() {
    let (context, recorder) = recording();
    let options = LogOptions {
        level: "warn".to_owned(),
        ..pretty()
    };
    capture(&options, |_| {
        context.sync_scope(|| {
            tracing::info!("too quiet");
            tracing::error!("loud");
        });
    });
    let logs = recorder.logs();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].message, "loud");
    assert_eq!(logs[0].level, LogLevel::Error);
}

#[test]
fn lines_outside_a_context_go_nowhere_without_a_background_registry() {
    let (_context, recorder) = recording();
    capture(&pretty(), |_| tracing::info!("no context"));
    assert!(recorder.is_empty());
}

#[test]
fn a_sink_that_does_not_want_logs_gets_none() {
    let recorder = TelemetryRecorder::only([EventKind::RequestHandled]);
    let context = TelemetryContext::new(Telemetry::with_sinks(vec![Arc::new(recorder.clone())]));
    capture(&pretty(), |_| {
        context.sync_scope(|| tracing::info!("ignored"))
    });
    assert!(recorder.is_empty());
}

#[test]
fn json_lines_carry_the_trace_and_span_ids() {
    let trace = TraceContext::root(true);
    let context = TelemetryContext::new(Telemetry::disabled()).with_trace(trace.clone());
    let out = capture(&json_options(), |_| {
        context.sync_scope(|| tracing::info!("traced"));
        tracing::info!("untraced");
    });
    let lines: Vec<Value> = out
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON line"))
        .collect();
    assert_eq!(lines[0]["trace_id"], trace.trace_id.to_string(), "{out}");
    assert_eq!(lines[0]["span_id"], trace.span_id.to_string(), "{out}");
    assert!(lines[1].get("trace_id").is_none(), "{out}");
    assert!(lines[1].get("span_id").is_none(), "{out}");
}
