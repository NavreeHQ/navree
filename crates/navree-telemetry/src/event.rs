//! The typed events: [`TelemetryEvent`] and one struct per kind, the
//! [`EventContext`] they happened in, and the [`TelemetryRecord`] sinks
//! receive.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AiCall, BroadcastSent, SearchQueried, SignInEvent, StorageOp, TraceContext, UiOperation,
    WebhookDelivery,
};

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
    /// [`AiCall`].
    AiCall,
    /// [`BroadcastSent`].
    Broadcast,
    /// [`StorageOp`].
    StorageOp,
    /// [`WebhookDelivery`].
    WebhookDelivery,
    /// [`SearchQueried`].
    Search,
    /// [`SignInEvent`].
    SignIn,
    /// [`UiOperation`].
    UiOperation,
    /// [`CustomEvent`]: an event a module names itself.
    Custom,
}

impl EventKind {
    /// The kinds of the first releases: every kind but `Custom`. `EVERY`
    /// lists them all.
    pub const ALL: [EventKind; 19] = [
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
        EventKind::AiCall,
        EventKind::Broadcast,
        EventKind::StorageOp,
        EventKind::WebhookDelivery,
        EventKind::Search,
        EventKind::SignIn,
        EventKind::UiOperation,
    ];

    /// Every kind, in declaration order, `Custom` included.
    pub const EVERY: &'static [EventKind] = &[
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
        EventKind::AiCall,
        EventKind::Broadcast,
        EventKind::StorageOp,
        EventKind::WebhookDelivery,
        EventKind::Search,
        EventKind::SignIn,
        EventKind::UiOperation,
        EventKind::Custom,
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
            EventKind::AiCall => "ai_call",
            EventKind::Broadcast => "broadcast",
            EventKind::StorageOp => "storage_op",
            EventKind::WebhookDelivery => "webhook_delivery",
            EventKind::Search => "search",
            EventKind::SignIn => "sign_in",
            EventKind::UiOperation => "ui_operation",
            EventKind::Custom => "custom",
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
    /// A call to a model provider finished (navree-ai).
    AiCall(AiCall),
    /// An event was pushed to real-time channels (navree-realtime).
    Broadcast(BroadcastSent),
    /// A file operation finished (navree-storage).
    StorageOp(StorageOp),
    /// One attempt to deliver a webhook finished (navree-webhooks).
    WebhookDelivery(WebhookDelivery),
    /// A search finished (navree-search).
    Search(SearchQueried),
    /// A sign-in attempt ended (navree-auth, and the sign-in modules that
    /// build on it).
    SignIn(SignInEvent),
    /// A Navree UI operation was answered (navree-ui).
    UiOperation(UiOperation),
    /// An event a module names itself (`announcements.published`).
    Custom(CustomEvent),
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
            TelemetryEvent::AiCall(_) => EventKind::AiCall,
            TelemetryEvent::Broadcast(_) => EventKind::Broadcast,
            TelemetryEvent::StorageOp(_) => EventKind::StorageOp,
            TelemetryEvent::WebhookDelivery(_) => EventKind::WebhookDelivery,
            TelemetryEvent::Search(_) => EventKind::Search,
            TelemetryEvent::SignIn(_) => EventKind::SignIn,
            TelemetryEvent::UiOperation(_) => EventKind::UiOperation,
            TelemetryEvent::Custom(_) => EventKind::Custom,
        }
    }
}

/// An event a module names itself: a name, lower-case words joined by `.`
/// (`announcements.published`), and fields of its own. Emit it with
/// [`Telemetry::emit`](crate::Telemetry::emit) as
/// [`TelemetryEvent::Custom`]; a sink receives it when its interest
/// includes [`EventKind::Custom`], as an interest in every kind does.
///
/// An event whose name is not two or more lower-case words (each a letter,
/// then up to 63 letters, digits or `_`) joined by `.` is not delivered,
/// and the registry logs a warning once for that name.
///
/// ```
/// use navree_telemetry::{CustomEvent, Telemetry, TelemetryEvent};
///
/// let event = CustomEvent::new("announcements.published")
///     .field("by", "Harpreet Sandhu")
///     .field("amount", "₹1,00,000");
/// assert_eq!(event.fields["by"], "Harpreet Sandhu");
/// // Inside a request, job or task: the app's registry.
/// Telemetry::current().emit(TelemetryEvent::Custom(event));
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CustomEvent {
    /// The event's name: `announcements.published`.
    pub name: String,
    /// The event's fields.
    #[serde(default)]
    pub fields: serde_json::Map<String, Value>,
}

impl CustomEvent {
    /// An event named `name` (`announcements.published`), without fields.
    pub fn new(name: impl Into<String>) -> CustomEvent {
        CustomEvent {
            name: name.into(),
            fields: serde_json::Map::new(),
        }
    }

    /// The event with the field `key` set to `value`.
    #[must_use]
    pub fn field(mut self, key: impl Into<String>, value: impl Into<Value>) -> CustomEvent {
        self.fields.insert(key.into(), value.into());
        self
    }
}

/// Whether `name` is two or more lower-case words joined by `.`, each a
/// lower-case letter followed by up to 63 lower-case letters, digits or
/// `_`: the name of a [`CustomEvent`] that is delivered.
pub(crate) fn is_event_name(name: &str) -> bool {
    let mut words = 0usize;
    for word in name.split('.') {
        if !navree_kernel::__private::is_namespace(word) {
            return false;
        }
        words += 1;
    }
    words >= 2
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
    /// How long it ran, once it had a connection.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// How long it waited for a connection from the pool before it ran: a
    /// saturated pool shows here, not as slow statements. A statement that
    /// never got one has a `duration` of zero and is `failed`.
    #[serde(default, with = "millis")]
    pub pool_wait: Duration,
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
#[non_exhaustive]
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
pub(crate) mod millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(value: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(value.as_secs_f64() * 1000.0)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = f64::deserialize(d)?;
        Duration::try_from_secs_f64(ms / 1000.0).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{Interest, Telemetry, TelemetryRecorder};

    #[test]
    fn custom_events_reach_sinks_and_all_keeps_nineteen_kinds() {
        assert_eq!(EventKind::ALL.len(), 19);
        assert_eq!(EventKind::EVERY.len(), 20);
        assert_eq!(EventKind::EVERY[..19], EventKind::ALL);
        assert_eq!(EventKind::EVERY[19], EventKind::Custom);
        assert_eq!(EventKind::Custom.as_str(), "custom");
        assert!(
            EventKind::ALL
                .iter()
                .all(|kind| kind.bit() & EventKind::Custom.bit() == 0)
        );
        assert!(Interest::ALL.contains(EventKind::Custom));

        let event = CustomEvent::new("announcements.published")
            .field("by", "Harpreet Sandhu")
            .field("amount", "₹1,00,000");
        let json = serde_json::to_value(TelemetryEvent::Custom(event.clone())).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "custom",
                "name": "announcements.published",
                "fields": { "by": "Harpreet Sandhu", "amount": "₹1,00,000" }
            })
        );
        let back: TelemetryEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, TelemetryEvent::Custom(event.clone()));
        assert_eq!(back.kind(), EventKind::Custom);

        let every = TelemetryRecorder::new();
        let logs = TelemetryRecorder::only([EventKind::Log]);
        let telemetry =
            Telemetry::with_sinks(vec![Arc::new(every.clone()), Arc::new(logs.clone())]);
        telemetry.emit(TelemetryEvent::Custom(event.clone()));
        assert_eq!(every.events(), [TelemetryEvent::Custom(event)]);
        assert_eq!(every.of_kind(EventKind::Custom).len(), 1);
        assert!(logs.is_empty(), "a sink without the kind gets nothing");
    }

    #[test]
    fn custom_event_names_are_dotted_lower_case_words() {
        for good in ["announcements.published", "crm.deal.won", "notice_board.v2"] {
            assert!(is_event_name(good), "{good}");
        }
        for bad in [
            "",
            "published",
            "Announcements.Published",
            "announcements..published",
            ".announcements",
            "announcements.",
            "2fa.sent",
            "announcements.pub-lished",
        ] {
            assert!(!is_event_name(bad), "{bad}");
        }
    }

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
