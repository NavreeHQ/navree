//! [`RecordedSpan`]: the telemetry events of a traced unit of work as
//! spans, named and labeled with OpenTelemetry's semantic conventions.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_telemetry::{
    CacheOperation, EventKind, Interest, SpanId, SpanLink, TelemetryEvent, TelemetryRecord,
    TraceContext, TraceId,
};
use serde::Serialize;
use serde_json::{Value, json};

/// What a span stands for, as OpenTelemetry defines it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Handling a request (an HTTP request).
    Server,
    /// A call to another service (outgoing HTTP, a query, sending mail).
    Client,
    /// Handing work to a queue.
    Producer,
    /// Running queued work (a job).
    Consumer,
    /// Work inside the process (a cache call, a scheduled task).
    Internal,
}

/// How a span ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum SpanStatus {
    /// Nothing to report (success, or a client's mistake such as a 4xx).
    Unset,
    /// Failed: a 5xx response, a failed query, job attempt or task.
    Error {
        /// What went wrong.
        message: String,
    },
}

/// A finished span.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecordedSpan {
    /// The span name (`GET /api/v1/deals/{id}`, `SELECT deals`).
    pub name: String,
    /// The span kind.
    pub kind: SpanKind,
    /// The trace.
    pub trace_id: TraceId,
    /// The span.
    pub span_id: SpanId,
    /// The parent span; `None` for the root of a trace.
    pub parent_span_id: Option<SpanId>,
    /// When it started.
    pub start: DateTime<Utc>,
    /// When it ended.
    pub end: DateTime<Utc>,
    /// How it ended.
    pub status: SpanStatus,
    /// Its attributes (`http.route`, `db.query.text`, ...).
    pub attributes: BTreeMap<String, Value>,
    /// Spans of other traces it refers to (an untrusted caller's span).
    pub links: Vec<SpanLink>,
}

/// The event kinds that become spans.
pub(crate) fn span_interest() -> Interest {
    Interest::only([
        EventKind::RequestHandled,
        EventKind::HttpClientRequest,
        EventKind::QueryExecuted,
        EventKind::JobProcessed,
        EventKind::JobFailed,
        EventKind::ScheduledTaskRan,
        EventKind::MailSent,
        EventKind::Cache,
    ])
}

impl RecordedSpan {
    /// The span a telemetry record stands for: `None` when the record has no
    /// sampled trace, or its kind is not a span (logs, exceptions,
    /// notifications).
    pub fn from_record(record: &TelemetryRecord) -> Option<RecordedSpan> {
        let trace = record.context.trace.as_ref().filter(|trace| trace.sampled)?;
        let mut attributes = BTreeMap::new();
        let span = match &record.event {
            TelemetryEvent::RequestHandled(request) => {
                attributes.insert("http.request.method".into(), json!(request.method));
                if let Some(route) = &request.route {
                    attributes.insert("http.route".into(), json!(route));
                }
                attributes.insert("http.response.status_code".into(), json!(request.status));
                attributes.insert("url.path".into(), json!(request.path));
                attributes.insert("navree.request_id".into(), json!(request.request_id));
                if let Some(ip) = &request.client_ip {
                    attributes.insert("client.address".into(), json!(ip));
                }
                if let Some(user) = &request.user_id {
                    attributes.insert("enduser.id".into(), json!(user));
                }
                if let Some(workspace) = &request.workspace_id {
                    attributes.insert("navree.workspace_id".into(), json!(workspace));
                }
                let name = match &request.route {
                    Some(route) => format!("{} {route}", request.method),
                    None => request.method.clone(),
                };
                let status = if request.status >= 500 {
                    SpanStatus::Error {
                        message: format!("HTTP {}", request.status),
                    }
                } else {
                    SpanStatus::Unset
                };
                own_span(trace, name, SpanKind::Server, request.duration, status)
            }
            TelemetryEvent::HttpClientRequest(call) => {
                attributes.insert("http.request.method".into(), json!(call.method));
                attributes.insert("url.full".into(), json!(call.url));
                if let Some(host) = host_of(&call.url) {
                    attributes.insert("server.address".into(), json!(host));
                }
                if let Some(status) = call.status {
                    attributes.insert("http.response.status_code".into(), json!(status));
                }
                if call.attempts > 1 {
                    attributes.insert("http.request.resend_count".into(), json!(call.attempts - 1));
                }
                let status = match (call.status, &call.error) {
                    (Some(status), _) if status >= 500 => SpanStatus::Error {
                        message: format!("HTTP {status}"),
                    },
                    (None, Some(error)) => SpanStatus::Error {
                        message: error.clone(),
                    },
                    _ => SpanStatus::Unset,
                };
                let mut span = child_span(trace, call.method.clone(), SpanKind::Client, call.duration, status);
                if let Some(id) = call.span_id {
                    span.span_id = id;
                }
                span
            }
            TelemetryEvent::QueryExecuted(query) => {
                let (operation, table) = describe_sql(&query.sql);
                attributes.insert("db.system.name".into(), json!(db_system(&query.connection)));
                attributes.insert("db.operation.name".into(), json!(operation));
                if let Some(table) = &table {
                    attributes.insert("db.collection.name".into(), json!(table));
                }
                attributes.insert("db.query.text".into(), json!(query.sql));
                let name = match table {
                    Some(table) => format!("{operation} {table}"),
                    None => operation,
                };
                let status = if query.failed {
                    SpanStatus::Error {
                        message: "the statement failed".to_owned(),
                    }
                } else {
                    SpanStatus::Unset
                };
                child_span(trace, name, SpanKind::Client, query.duration, status)
            }
            TelemetryEvent::JobProcessed(job) => {
                job_attributes(&mut attributes, &job.queue, &job.job_id, job.attempt);
                own_span(
                    trace,
                    format!("job {}", job.job_type),
                    SpanKind::Consumer,
                    job.duration,
                    SpanStatus::Unset,
                )
            }
            TelemetryEvent::JobFailed(job) => {
                job_attributes(&mut attributes, &job.queue, &job.job_id, job.attempt);
                own_span(
                    trace,
                    format!("job {}", job.job_type),
                    SpanKind::Consumer,
                    job.duration,
                    SpanStatus::Error {
                        message: job.error.clone(),
                    },
                )
            }
            TelemetryEvent::ScheduledTaskRan(task) => {
                attributes.insert("navree.task".into(), json!(task.task));
                let status = match &task.error {
                    Some(error) => SpanStatus::Error {
                        message: error.clone(),
                    },
                    None => SpanStatus::Unset,
                };
                own_span(
                    trace,
                    format!("task {}", task.task),
                    SpanKind::Internal,
                    task.duration,
                    status,
                )
            }
            TelemetryEvent::MailSent(mail) => {
                attributes.insert("navree.mail.transport".into(), json!(mail.transport));
                child_span(
                    trace,
                    "mail send".to_owned(),
                    SpanKind::Client,
                    mail.duration,
                    SpanStatus::Unset,
                )
            }
            TelemetryEvent::Cache(cache) => {
                attributes.insert("navree.cache.store".into(), json!(cache.store));
                let verb = match cache.operation {
                    CacheOperation::Hit => {
                        attributes.insert("navree.cache.hit".into(), json!(true));
                        "get"
                    }
                    CacheOperation::Miss => {
                        attributes.insert("navree.cache.hit".into(), json!(false));
                        "get"
                    }
                    CacheOperation::Write => "put",
                    CacheOperation::Forget => "forget",
                };
                child_span(
                    trace,
                    format!("cache {verb}"),
                    SpanKind::Internal,
                    Duration::ZERO,
                    SpanStatus::Unset,
                )
            }
            _ => return None,
        };
        let end = record.at;
        let start = end
            - chrono::Duration::from_std(span.duration).unwrap_or_else(|_| chrono::Duration::zero());
        Some(RecordedSpan {
            name: span.name,
            kind: span.kind,
            trace_id: trace.trace_id,
            span_id: span.span_id,
            parent_span_id: span.parent_span_id,
            start,
            end,
            status: span.status,
            attributes,
            links: span.links,
        })
    }
}

/// The parts of a span decided per event.
struct Shape {
    name: String,
    kind: SpanKind,
    span_id: SpanId,
    parent_span_id: Option<SpanId>,
    duration: Duration,
    status: SpanStatus,
    links: Vec<SpanLink>,
}

/// The unit of work's own span (a request, a job run, a task run).
fn own_span(
    trace: &TraceContext,
    name: String,
    kind: SpanKind,
    duration: Duration,
    status: SpanStatus,
) -> Shape {
    Shape {
        name,
        kind,
        span_id: trace.span_id,
        parent_span_id: trace.parent_span_id,
        duration,
        status,
        links: trace.link.into_iter().collect(),
    }
}

/// A new span under the unit of work's span (a query, a call, a mail).
fn child_span(
    trace: &TraceContext,
    name: String,
    kind: SpanKind,
    duration: Duration,
    status: SpanStatus,
) -> Shape {
    Shape {
        name,
        kind,
        span_id: SpanId::random(),
        parent_span_id: Some(trace.span_id),
        duration,
        status,
        links: Vec::new(),
    }
}

fn job_attributes(attributes: &mut BTreeMap<String, Value>, queue: &str, id: &str, attempt: u32) {
    attributes.insert("navree.queue".into(), json!(queue));
    attributes.insert("navree.job_id".into(), json!(id));
    attributes.insert("navree.attempt".into(), json!(attempt));
}

/// The host of `url` (`https://host:port/path` → `host`).
fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next()?,
        None => host.split(':').next()?,
    };
    (!host.is_empty()).then_some(host)
}

/// The database system of a connection URL, as OpenTelemetry names it.
fn db_system(connection: &str) -> &'static str {
    let scheme = connection.split(':').next().unwrap_or_default();
    match scheme {
        "postgres" | "postgresql" => "postgresql",
        "sqlite" => "sqlite",
        "mysql" | "mariadb" => "mysql",
        _ => "other_sql",
    }
}

/// The operation (first keyword, upper case) and the table of a statement.
fn describe_sql(sql: &str) -> (String, Option<String>) {
    let words: Vec<&str> = sql.split_whitespace().collect();
    let operation = words
        .first()
        .map(|word| word.to_ascii_uppercase())
        .unwrap_or_default();
    let after = |keyword: &str| {
        words
            .iter()
            .position(|word| word.eq_ignore_ascii_case(keyword))
            .and_then(|index| words.get(index + 1))
            .map(|name| table_name(name))
    };
    let table = match operation.as_str() {
        "SELECT" | "DELETE" => after("FROM"),
        "INSERT" => after("INTO"),
        "UPDATE" => words.get(1).map(|name| table_name(name)),
        _ => None,
    };
    (operation, table.filter(|name| !name.is_empty()))
}

/// A table name without quotes, schema or trailing punctuation.
fn table_name(word: &str) -> String {
    let word = word.split(['(', ',', ';']).next().unwrap_or(word);
    let last = word.rsplit('.').next().unwrap_or(word);
    last.trim_matches(['"', '`', '[', ']']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_read_from_urls() {
        assert_eq!(host_of("https://api.example.com/v1"), Some("api.example.com"));
        assert_eq!(host_of("http://127.0.0.1:8080/x"), Some("127.0.0.1"));
        assert_eq!(host_of("http://[::1]:80/"), Some("::1"));
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn statements_are_described() {
        assert_eq!(
            describe_sql(r#"SELECT "a" FROM "public"."deals" WHERE 1"#),
            ("SELECT".to_owned(), Some("deals".to_owned()))
        );
        assert_eq!(
            describe_sql("insert into notes(id) values (?)"),
            ("INSERT".to_owned(), Some("notes".to_owned()))
        );
        assert_eq!(describe_sql("BEGIN"), ("BEGIN".to_owned(), None));
        assert_eq!(db_system("sqlite://x.db"), "sqlite");
    }
}
