//! The typed events: [`TelemetryEvent`] and one struct per kind, the
//! [`EventContext`] they happened in, and the [`TelemetryRecord`] sinks
//! receive.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::TraceContext;

/// What kind of event a [`TelemetryEvent`] is; sinks state their
/// [`Interest`](crate::Interest) in kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EventKind {
    /// [`RequestHandled`].
    RequestHandled,
    /// [`QueryExecuted`].
    QueryExecuted,
    /// [`JobProcessed`].
    JobProcessed,
    /// [`JobFailed`].
    JobFailed,
    /// [`ScheduledTaskRan`].
    ScheduledTaskRan,
    /// [`MailSent`].
    MailSent,
    /// [`NotificationSent`].
    NotificationSent,
    /// [`CacheEvent`].
    Cache,
    /// [`HttpClientRequest`].
    HttpClientRequest,
    /// [`ExceptionReported`].
    ExceptionReported,
    /// [`LogRecord`].
    Log,
    /// [`EventEmitted`].
    EventEmitted,
}

impl EventKind {
    /// Every kind, in declaration order.
    pub const ALL: [EventKind; 12] = [
        EventKind::RequestHandled,
        EventKind::QueryExecuted,
        EventKind::JobProcessed,
        EventKind::JobFailed,
        EventKind::ScheduledTaskRan,
        EventKind::MailSent,
        EventKind::NotificationSent,
        EventKind::Cache,
        EventKind::HttpClientRequest,
        EventKind::ExceptionReported,
        EventKind::Log,
        EventKind::EventEmitted,
    ];

    /// The snake_case name, as in serialized records (`query_executed`).
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::RequestHandled => "request_handled",
            EventKind::QueryExecuted => "query_executed",
            EventKind::JobProcessed => "job_processed",
            EventKind::JobFailed => "job_failed",
            EventKind::ScheduledTaskRan => "scheduled_task_ran",
            EventKind::MailSent => "mail_sent",
            EventKind::NotificationSent => "notification_sent",
            EventKind::Cache => "cache",
            EventKind::HttpClientRequest => "http_client_request",
            EventKind::ExceptionReported => "exception_reported",
            EventKind::Log => "log",
            EventKind::EventEmitted => "event_emitted",
        }
    }

    /// The kind's bit in an [`Interest`](crate::Interest).
    pub(crate) fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Something that happened in the app. Serialized with a `type` tag
/// (`{"type": "query_executed", "sql": ..}`); durations are in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TelemetryEvent {
    /// An HTTP request was answered (navree-http).
    RequestHandled(RequestHandled),
    /// A SQL statement ran (navree-db).
    QueryExecuted(QueryExecuted),
    /// A queued job attempt succeeded (navree-queue).
    JobProcessed(JobProcessed),
    /// A queued job attempt failed (navree-queue).
    JobFailed(JobFailed),
    /// A scheduled task ran (navree-schedule).
    ScheduledTaskRan(ScheduledTaskRan),
    /// A mail was handed to the transport (navree-mail).
    MailSent(MailSent),
    /// A notification was delivered over one channel (navree-notify).
    NotificationSent(NotificationSent),
    /// A cache hit, miss, write or forget (navree-cache).
    Cache(CacheEvent),
    /// An outgoing HTTP request finished (navree-http-client).
    HttpClientRequest(HttpClientRequest),
    /// An error or panic was reported (navree-http for 5xx errors and
    /// panics, or the app through
    /// [`Telemetry::report_exception`](crate::Telemetry::report_exception)).
    ExceptionReported(ExceptionReported),
    /// A log line was written (navree-log).
    Log(LogRecord),
    /// An app event was emitted or published on the event bus
    /// (navree-events).
    EventEmitted(EventEmitted),
}

impl TelemetryEvent {
    /// The event's kind.
    pub fn kind(&self) -> EventKind {
        match self {
            TelemetryEvent::RequestHandled(_) => EventKind::RequestHandled,
            TelemetryEvent::QueryExecuted(_) => EventKind::QueryExecuted,
            TelemetryEvent::JobProcessed(_) => EventKind::JobProcessed,
            TelemetryEvent::JobFailed(_) => EventKind::JobFailed,
            TelemetryEvent::ScheduledTaskRan(_) => EventKind::ScheduledTaskRan,
            TelemetryEvent::MailSent(_) => EventKind::MailSent,
            TelemetryEvent::NotificationSent(_) => EventKind::NotificationSent,
            TelemetryEvent::Cache(_) => EventKind::Cache,
            TelemetryEvent::HttpClientRequest(_) => EventKind::HttpClientRequest,
            TelemetryEvent::ExceptionReported(_) => EventKind::ExceptionReported,
            TelemetryEvent::Log(_) => EventKind::Log,
            TelemetryEvent::EventEmitted(_) => EventKind::EventEmitted,
        }
    }
}

/// An HTTP request was answered: emitted by navree-http's outermost layer
/// once the response is ready.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestHandled {
    /// The method (`GET`): one HTTP defines, or `OTHER` for a made-up one
    /// (navree-http's pipeline).
    pub method: String,
    /// The matched route template (`/api/v1/deals/{id}`); `None` when no
    /// route matched.
    pub route: Option<String>,
    /// The request path, without the query string.
    pub path: String,
    /// The response status.
    pub status: u16,
    /// The time from the request's arrival to the response.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// The authenticated user, if any.
    pub user_id: Option<String>,
    /// The workspace the request worked in, if one was resolved.
    pub workspace_id: Option<String>,
    /// The request id (`X-Request-Id`).
    pub request_id: String,
    /// The client address, as resolved behind trusted proxies.
    pub client_ip: Option<String>,
}

/// A SQL statement ran: emitted by navree-db for every statement its pools
/// and transactions run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryExecuted {
    /// The statement with its placeholders, never the bound values.
    pub sql: String,
    /// How long it took.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// The rows returned or affected, when known.
    pub rows: Option<u64>,
    /// The connection: the database URL without its password.
    pub connection: String,
    /// Whether the statement failed.
    pub failed: bool,
}

/// A queued job attempt succeeded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobProcessed {
    /// The queue it ran from.
    pub queue: String,
    /// The job type (`Job::NAME`, `mail.send`).
    pub job_type: String,
    /// The job id.
    pub job_id: String,
    /// The attempt (1 for the first).
    pub attempt: u32,
    /// How long the attempt ran.
    #[serde(with = "millis")]
    pub duration: Duration,
}

/// A queued job attempt failed: released for another attempt
/// (`will_retry`), or moved to the failed jobs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFailed {
    /// The queue it ran from.
    pub queue: String,
    /// The job type.
    pub job_type: String,
    /// The job id.
    pub job_id: String,
    /// The attempt that failed.
    pub attempt: u32,
    /// How long the attempt ran.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// The error chain (`error: source: ...`), a timeout or a panic message.
    pub error: String,
    /// Whether the job goes back on the queue for another attempt.
    pub will_retry: bool,
}

/// A scheduled task ran (a skipped tick is not reported).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTaskRan {
    /// The task name.
    pub task: String,
    /// The minute it was due at.
    pub tick: DateTime<Utc>,
    /// How long it ran.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// The error chain when it failed or panicked.
    pub error: Option<String>,
}

/// A mail was handed to the transport successfully.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailSent {
    /// The recipients.
    pub to: Vec<String>,
    /// Carbon-copy recipients.
    pub cc: Vec<String>,
    /// Blind carbon-copy recipients.
    pub bcc: Vec<String>,
    /// The sender.
    pub from: Option<String>,
    /// The subject.
    pub subject: String,
    /// The plain-text body.
    pub text: Option<String>,
    /// The HTML body.
    pub html: Option<String>,
    /// The transport's name (`smtp`, `log`, `array`, `failover`).
    pub transport: String,
    /// How long the transport took.
    #[serde(with = "millis")]
    pub duration: Duration,
}

/// A notification was delivered over one channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationSent {
    /// The notification id (shared by its channels).
    pub notification_id: String,
    /// The notification type (`Notification::TYPE`).
    pub notification_type: String,
    /// The notifiable's type (`user`).
    pub notifiable_type: String,
    /// The notifiable's id.
    pub notifiable_id: String,
    /// The channel: `database`, `mail` or `broadcast`.
    pub channel: String,
}

/// What a cache call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheOperation {
    /// A read found a value.
    Hit,
    /// A read found nothing.
    Miss,
    /// A value was stored.
    Write,
    /// A key was removed.
    Forget,
}

impl CacheOperation {
    /// The snake_case name (`hit`).
    pub fn as_str(self) -> &'static str {
        match self {
            CacheOperation::Hit => "hit",
            CacheOperation::Miss => "miss",
            CacheOperation::Write => "write",
            CacheOperation::Forget => "forget",
        }
    }
}

/// A cache hit, miss, write or forget.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEvent {
    /// What happened.
    pub operation: CacheOperation,
    /// The key as the caller wrote it (without the cache prefix).
    pub key: String,
    /// The store (`memory`, or the store's name).
    pub store: String,
}

/// An outgoing HTTP request finished (after its retries).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpClientRequest {
    /// The method.
    pub method: String,
    /// The URL without its query string, fragment or credentials.
    pub url: String,
    /// The final response status; `None` when no response came.
    pub status: Option<u16>,
    /// The time from the first attempt to the final outcome.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// Attempts made (1 without retries).
    pub attempts: u32,
    /// The error, when no response came.
    pub error: Option<String>,
    /// The span id sent in the request's `traceparent`, when one was sent.
    pub span_id: Option<crate::SpanId>,
}

/// The request an exception happened in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestInfo {
    /// The method.
    pub method: String,
    /// The path, without the query string.
    pub path: String,
    /// The matched route template, if any.
    pub route: Option<String>,
    /// The response status.
    pub status: u16,
}

/// An error or panic was reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionReported {
    /// The error's message.
    pub message: String,
    /// The message, then each source's message, outermost first.
    pub chain: Vec<String>,
    /// Where it happened (`file:line:column`), when known: the panic's
    /// location for a panicking handler, the caller's for
    /// [`Telemetry::report_exception`](crate::Telemetry::report_exception);
    /// `None` for an error response.
    pub location: Option<String>,
    /// Whether it was a panic.
    pub panic: bool,
    /// The request, when it happened while handling one.
    pub request: Option<RequestInfo>,
}

impl ExceptionReported {
    /// An exception from `error` and its sources.
    pub fn from_error(error: &(dyn std::error::Error + 'static)) -> Self {
        let chain = error_chain(error);
        Self {
            message: error.to_string(),
            chain,
            location: None,
            panic: false,
            request: None,
        }
    }
}

/// The message of `error`, then of each source, skipping a source whose
/// text the previous message already ends with (errors that print their
/// source).
pub(crate) fn error_chain(error: &(dyn std::error::Error + 'static)) -> Vec<String> {
    let mut chain = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if chain.last().is_none_or(|last| !last.ends_with(&text)) {
            chain.push(text);
        }
        source = cause.source();
    }
    chain
}

/// A log level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevel {
    /// `TRACE`.
    Trace,
    /// `DEBUG`.
    Debug,
    /// `INFO`.
    Info,
    /// `WARN`.
    Warn,
    /// `ERROR`.
    Error,
}

impl LogLevel {
    /// The upper-case name (`WARN`).
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// A log line, with its fields already redacted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    /// The level.
    pub level: LogLevel,
    /// The target (`navree::request`, the module path).
    pub target: String,
    /// The message.
    pub message: String,
    /// The other fields of the line (redacted, as in the log output).
    pub fields: BTreeMap<String, Value>,
}

/// An app event was emitted (`EventBus::emit`) or published
/// (`EventSink::publish`) on the event bus: emitted by navree-events once
/// every listener ran or was queued.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventEmitted {
    /// The event name (`Event::NAME`, such as `deal.won`).
    pub name: String,
    /// The payload as JSON (`null` when it does not serialize).
    pub payload: Value,
    /// The listeners that handled it, by type name, in delivery order:
    /// sync listeners, and queued ones that ran inline (no dispatcher).
    pub listeners: Vec<String>,
    /// The queued listeners whose job was dispatched.
    pub queued: Vec<String>,
    /// The listeners that failed (their errors are logged).
    pub failed: Vec<String>,
    /// How long delivery took.
    #[serde(with = "millis")]
    pub duration: Duration,
}

/// The unit of work an event happened in, captured from the current
/// [`TelemetryContext`](crate::TelemetryContext) when it was emitted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventContext {
    /// The HTTP request id, inside a request.
    pub request_id: Option<String>,
    /// The job id, inside a job run.
    pub job_id: Option<String>,
    /// The background task's name, inside a scheduled task or a
    /// [`with_task`](crate::TelemetryContext::with_task) of the app's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The trace and the unit of work's span.
    pub trace: Option<TraceContext>,
    /// The authenticated user, once known.
    pub user_id: Option<String>,
    /// The workspace, once resolved.
    pub workspace_id: Option<String>,
}

/// What a sink receives: an event, when it was emitted and the context it
/// happened in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryRecord {
    /// When the event was emitted (at the end of the operation it reports).
    pub at: DateTime<Utc>,
    /// The request or job it happened in.
    pub context: EventContext,
    /// The event.
    pub event: TelemetryEvent,
}

/// Durations as (fractional) milliseconds.
mod millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(value: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(value.as_secs_f64() * 1000.0)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = f64::deserialize(d)?;
        Duration::try_from_secs_f64(ms / 1000.0).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_own_bit() {
        let mut seen = 0u32;
        for kind in EventKind::ALL {
            assert_eq!(seen & kind.bit(), 0, "{kind}");
            seen |= kind.bit();
        }
    }

    #[test]
    fn durations_round_trip_as_milliseconds() {
        let event = JobProcessed {
            queue: "default".into(),
            job_type: "mail.send".into(),
            job_id: "1".into(),
            attempt: 1,
            duration: Duration::from_micros(1500),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["duration"], 1.5);
        let back: JobProcessed = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn error_chains_skip_repeated_sources() {
        #[derive(Debug)]
        struct Wrapper(std::io::Error);
        impl fmt::Display for Wrapper {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "read failed: {}", self.0)
            }
        }
        impl std::error::Error for Wrapper {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let error = Wrapper(std::io::Error::other("disk full"));
        assert_eq!(error_chain(&error), ["read failed: disk full"]);
    }
}
