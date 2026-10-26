//! Log lines as telemetry: [`TelemetryLayer`] turns every event that passes
//! the level filter into a [`LogRecord`] for the sinks that want logs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{PoisonError, RwLock};

use navree_telemetry::{
    EventKind, LogLevel, LogRecord, Telemetry, TelemetryContext, TelemetryEvent,
};
use serde_json::{Map, Value};
use tracing::{Event, Level, Subscriber};
use tracing_log::NormalizeEvent;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use crate::json::JsonVisitor;
use crate::redact::Redactor;

/// The registry of the app whose `LoggerModule` runs, for lines written
/// outside any request or job.
static BACKGROUND: RwLock<Option<Telemetry>> = RwLock::new(None);
/// Whether [`BACKGROUND`] is set (checked before taking the lock).
static HAS_BACKGROUND: AtomicBool = AtomicBool::new(false);

/// Makes `telemetry` receive the lines written outside a
/// [`TelemetryContext`] (the latest booted app wins).
pub(crate) fn set_background(telemetry: Telemetry) {
    *BACKGROUND.write().unwrap_or_else(PoisonError::into_inner) = Some(telemetry);
    HAS_BACKGROUND.store(true, Ordering::Release);
}

/// Stops sending background lines to `telemetry`, if it still gets them.
pub(crate) fn clear_background(telemetry: &Telemetry) {
    let mut slot = BACKGROUND.write().unwrap_or_else(PoisonError::into_inner);
    if slot
        .as_ref()
        .is_some_and(|current| current.same_registry(telemetry))
    {
        *slot = None;
        HAS_BACKGROUND.store(false, Ordering::Release);
    }
}

/// The registry a line goes to: the current context's when it wants logs,
/// else the background one when that wants logs.
fn target() -> Option<Telemetry> {
    let current = TelemetryContext::with_current(|context| {
        let telemetry = context.telemetry();
        telemetry
            .enabled(EventKind::Log)
            .then(|| telemetry.clone())
    })
    .flatten();
    current.or_else(|| {
        if !HAS_BACKGROUND.load(Ordering::Acquire) {
            return None;
        }
        BACKGROUND
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|telemetry| telemetry.enabled(EventKind::Log))
            .cloned()
    })
}

/// Emits a [`LogRecord`] for each event, with fields redacted as in the
/// output. Sits under the level filter, so filtered-out lines are skipped.
pub(crate) struct TelemetryLayer {
    redactor: Redactor,
}

impl TelemetryLayer {
    pub(crate) fn new(redactor: Redactor) -> Self {
        Self { redactor }
    }
}

impl<S: Subscriber> Layer<S> for TelemetryLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let Some(telemetry) = target() else {
            return;
        };
        let normalized = event.normalized_metadata();
        let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());
        let mut fields = Map::new();
        event.record(&mut JsonVisitor::new(&mut fields, &self.redactor));
        let message = match fields.remove("message") {
            Some(Value::String(message)) => message,
            Some(other) => other.to_string(),
            None => String::new(),
        };
        telemetry.emit(TelemetryEvent::Log(LogRecord {
            level: level(meta.level()),
            target: meta.target().to_owned(),
            message,
            fields: fields.into_iter().collect(),
        }));
    }
}

fn level(level: &Level) -> LogLevel {
    match *level {
        Level::TRACE => LogLevel::Trace,
        Level::DEBUG => LogLevel::Debug,
        Level::INFO => LogLevel::Info,
        Level::WARN => LogLevel::Warn,
        _ => LogLevel::Error,
    }
}

/// The current trace and span ids, for the JSON format.
pub(crate) fn current_ids() -> Option<(String, String)> {
    TelemetryContext::with_current(|context| {
        context
            .trace()
            .map(|trace| (trace.trace_id.to_string(), trace.span_id.to_string()))
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use navree_telemetry::TelemetryRecorder;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    #[test]
    fn background_lines_go_to_the_background_registry_until_cleared() {
        let recorder = TelemetryRecorder::new();
        let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
        let subscriber = tracing_subscriber::registry()
            .with(TelemetryLayer::new(Redactor::new(&["token"])));
        tracing::subscriber::with_default(subscriber, || {
            set_background(telemetry.clone());
            tracing::info!(token = "abc", "in the background");
            clear_background(&Telemetry::with_sinks(Vec::new()));
            tracing::info!("still the background registry");
            clear_background(&telemetry);
            tracing::info!("nobody");
        });
        // Other tests may log through the global subscriber meanwhile.
        let logs: Vec<LogRecord> = recorder
            .logs()
            .into_iter()
            .filter(|log| log.target == "navree_log::telemetry::tests")
            .collect();
        let messages: Vec<&str> = logs.iter().map(|log| log.message.as_str()).collect();
        assert_eq!(
            messages,
            ["in the background", "still the background registry"],
            "{logs:?}"
        );
        assert_eq!(logs[0].fields.get("token"), Some(&Value::from("[redacted]")));
    }
}
