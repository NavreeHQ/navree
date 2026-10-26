//! The JSON format: one object per line,
//! `{"timestamp","level","target","fields",["span","spans"]}`, with redaction
//! applied to event and span fields.

use std::error::Error;
use std::fmt;

use serde_json::{Map, Number, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_log::NormalizeEvent;
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::{FormatTime, SystemTime};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::registry::LookupSpan;

use crate::redact::{REDACTED, Redactor};

/// Formats span fields as a JSON object (kept in the span's extensions).
#[derive(Debug)]
pub(crate) struct JsonFields {
    redactor: Redactor,
}

impl JsonFields {
    pub(crate) fn new(redactor: Redactor) -> Self {
        Self { redactor }
    }
}

impl<'writer> FormatFields<'writer> for JsonFields {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut map = Map::new();
        fields.record(&mut JsonVisitor::new(&mut map, &self.redactor));
        write!(writer, "{}", Value::Object(map))
    }

    /// Merges fields recorded later (`span.record(...)`) into the stored object.
    fn add_fields(
        &self,
        current: &'writer mut FormattedFields<Self>,
        fields: &tracing::span::Record<'_>,
    ) -> fmt::Result {
        let mut map = parse_object(&current.fields);
        fields.record(&mut JsonVisitor::new(&mut map, &self.redactor));
        current.fields = Value::Object(map).to_string();
        Ok(())
    }
}

/// Formats events as JSON lines.
#[derive(Debug)]
pub(crate) struct JsonEvents {
    redactor: Redactor,
}

impl JsonEvents {
    pub(crate) fn new(redactor: Redactor) -> Self {
        Self { redactor }
    }
}

impl<S> FormatEvent<S, JsonFields> for JsonEvents
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, JsonFields>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        // Events bridged from the `log` crate carry their real metadata in fields.
        let normalized = event.normalized_metadata();
        let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());

        let mut timestamp = String::new();
        SystemTime.format_time(&mut Writer::new(&mut timestamp))?;
        let mut fields = Map::new();
        event.record(&mut JsonVisitor::new(&mut fields, &self.redactor));

        write!(
            writer,
            "{{\"timestamp\":{},\"level\":{},\"target\":{},\"fields\":{}",
            Value::String(timestamp),
            Value::from(meta.level().as_str()),
            Value::from(meta.target()),
            Value::Object(fields),
        )?;
        // Inside a request or job with a trace: its trace and span ids.
        if let Some((trace_id, span_id)) = crate::telemetry::current_ids() {
            write!(
                writer,
                ",\"trace_id\":{},\"span_id\":{}",
                Value::String(trace_id),
                Value::String(span_id)
            )?;
        }
        if let Some(scope) = ctx.event_scope() {
            let spans: Vec<Value> = scope
                .from_root()
                .map(|span| {
                    let extensions = span.extensions();
                    let mut object = extensions
                        .get::<FormattedFields<JsonFields>>()
                        .map(|stored| parse_object(&stored.fields))
                        .unwrap_or_default();
                    object.insert("name".to_owned(), Value::from(span.name()));
                    Value::Object(object)
                })
                .collect();
            if let Some(current) = spans.last() {
                write!(writer, ",\"span\":{current}")?;
                write!(writer, ",\"spans\":{}", Value::Array(spans))?;
            }
        }
        writeln!(writer, "}}")
    }
}

/// A JSON object stored by [`JsonFields`]; empty when absent or malformed.
fn parse_object(stored: &str) -> Map<String, Value> {
    match serde_json::from_str(stored) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Records fields into a JSON map, redacting matching names.
pub(crate) struct JsonVisitor<'a> {
    map: &'a mut Map<String, Value>,
    redactor: &'a Redactor,
}

impl<'a> JsonVisitor<'a> {
    pub(crate) fn new(map: &'a mut Map<String, Value>, redactor: &'a Redactor) -> Self {
        Self { map, redactor }
    }

    fn put(&mut self, field: &Field, value: impl FnOnce() -> Value) {
        let name = field.name();
        // Metadata of events bridged from the `log` crate, already normalized.
        if name.starts_with("log.") {
            return;
        }
        let value = if self.redactor.is_redacted(name) {
            Value::from(REDACTED)
        } else {
            value()
        };
        self.map.insert(name.to_owned(), value);
    }
}

impl Visit for JsonVisitor<'_> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, || {
            Number::from_f64(value).map_or_else(|| Value::String(value.to_string()), Value::Number)
        });
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, || Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, || Value::from(value));
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.put(field, || {
            i64::try_from(value).map_or_else(|_| Value::String(value.to_string()), Value::from)
        });
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.put(field, || {
            u64::try_from(value).map_or_else(|_| Value::String(value.to_string()), Value::from)
        });
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, || Value::Bool(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, || Value::from(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        self.put(field, || Value::String(error_chain(value)));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.put(field, || Value::String(format!("{value:?}")));
    }
}

/// `error: source: source of source...`.
fn error_chain(error: &(dyn Error + 'static)) -> String {
    let mut chain = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        chain.push_str(": ");
        chain.push_str(&cause.to_string());
        source = cause.source();
    }
    chain
}
