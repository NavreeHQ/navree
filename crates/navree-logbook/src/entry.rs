//! What Logbook records: [`Entry`], its [`EntryKind`] and [`Content`].

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The kind of an [`Entry`]; requests, jobs and scheduled tasks are
/// **batches** (the entries recorded while they ran belong to them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EntryKind {
    /// An HTTP request (a batch).
    Request,
    /// A SQL statement.
    Query,
    /// A job attempt (a batch).
    Job,
    /// A mail handed to the transport.
    Mail,
    /// An app event on the event bus.
    Event,
    /// A cache hit, miss, write or forget.
    Cache,
    /// A log line.
    Log,
    /// An error or panic.
    Exception,
    /// An outgoing HTTP call.
    HttpClient,
    /// A notification delivered over one channel.
    Notification,
    /// A data change a model made in a unit of work.
    Model,
    /// A scheduled task run (a batch).
    Schedule,
    /// A value recorded with [`Logbook::dump`](crate::Logbook::dump).
    Dump,
    /// A call to a model provider.
    Ai,
    /// An event pushed to real-time channels.
    Broadcast,
    /// A file operation on a storage disk.
    Storage,
    /// One attempt to deliver a webhook.
    Webhook,
    /// A search.
    Search,
    /// A sign-in attempt.
    SignIn,
    /// A Navree UI operation.
    Ui,
}

impl EntryKind {
    /// Every kind, in the order of the UI's filter buttons.
    pub const ALL: [EntryKind; 20] = [
        EntryKind::Request,
        EntryKind::Query,
        EntryKind::Job,
        EntryKind::Mail,
        EntryKind::Event,
        EntryKind::Cache,
        EntryKind::Log,
        EntryKind::Exception,
        EntryKind::HttpClient,
        EntryKind::Notification,
        EntryKind::Model,
        EntryKind::Schedule,
        EntryKind::Dump,
        EntryKind::Ai,
        EntryKind::Broadcast,
        EntryKind::Storage,
        EntryKind::Webhook,
        EntryKind::Search,
        EntryKind::SignIn,
        EntryKind::Ui,
    ];

    /// The name used in filters and JSON (`query`, `http_client`).
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Request => "request",
            EntryKind::Query => "query",
            EntryKind::Job => "job",
            EntryKind::Mail => "mail",
            EntryKind::Event => "event",
            EntryKind::Cache => "cache",
            EntryKind::Log => "log",
            EntryKind::Exception => "exception",
            EntryKind::HttpClient => "http_client",
            EntryKind::Notification => "notification",
            EntryKind::Model => "model",
            EntryKind::Schedule => "schedule",
            EntryKind::Dump => "dump",
            EntryKind::Ai => "ai",
            EntryKind::Broadcast => "broadcast",
            EntryKind::Storage => "storage",
            EntryKind::Webhook => "webhook",
            EntryKind::Search => "search",
            EntryKind::SignIn => "sign_in",
            EntryKind::Ui => "ui",
        }
    }

    /// The label of the UI's filter button (`Outgoing HTTP`).
    pub fn label(self) -> &'static str {
        match self {
            EntryKind::Request => "Requests",
            EntryKind::Query => "Queries",
            EntryKind::Job => "Jobs",
            EntryKind::Mail => "Mail",
            EntryKind::Event => "Events",
            EntryKind::Cache => "Cache",
            EntryKind::Log => "Logs",
            EntryKind::Exception => "Exceptions",
            EntryKind::HttpClient => "Outgoing HTTP",
            EntryKind::Notification => "Notifications",
            EntryKind::Model => "Data changes",
            EntryKind::Schedule => "Scheduled tasks",
            EntryKind::Dump => "Dumps",
            EntryKind::Ai => "AI calls",
            EntryKind::Broadcast => "Broadcasts",
            EntryKind::Storage => "Storage",
            EntryKind::Webhook => "Webhooks",
            EntryKind::Search => "Searches",
            EntryKind::SignIn => "Sign-ins",
            EntryKind::Ui => "UI operations",
        }
    }

    /// A kind from its name, its plural or its watcher's name
    /// (`query`, `queries`, `http_client`, `models`, `sign_ins`),
    /// case-insensitively.
    pub fn parse(name: &str) -> Option<EntryKind> {
        let name = name.trim().to_ascii_lowercase().replace('-', "_");
        let kind = match name.as_str() {
            "request" | "requests" => EntryKind::Request,
            "query" | "queries" => EntryKind::Query,
            "job" | "jobs" => EntryKind::Job,
            "mail" | "mails" => EntryKind::Mail,
            "event" | "events" => EntryKind::Event,
            "cache" => EntryKind::Cache,
            "log" | "logs" => EntryKind::Log,
            "exception" | "exceptions" => EntryKind::Exception,
            "http_client" | "http" => EntryKind::HttpClient,
            "notification" | "notifications" => EntryKind::Notification,
            "model" | "models" => EntryKind::Model,
            "schedule" | "scheduled" => EntryKind::Schedule,
            "dump" | "dumps" => EntryKind::Dump,
            "ai" | "ai_call" | "ai_calls" => EntryKind::Ai,
            "broadcast" | "broadcasts" => EntryKind::Broadcast,
            "storage" | "storage_op" | "storage_ops" => EntryKind::Storage,
            "webhook" | "webhooks" => EntryKind::Webhook,
            "search" | "searches" => EntryKind::Search,
            "sign_in" | "sign_ins" | "signin" | "signins" => EntryKind::SignIn,
            "ui" | "ui_operation" | "ui_operations" => EntryKind::Ui,
            _ => return None,
        };
        Some(kind)
    }

    /// Whether entries of this kind are batches (requests, jobs and
    /// scheduled tasks).
    pub fn is_batch(self) -> bool {
        matches!(
            self,
            EntryKind::Request | EntryKind::Job | EntryKind::Schedule
        )
    }
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One recorded thing: a request, a query, a log line... Entries recorded
/// while a request, job or scheduled task ran carry its `batch_id` (the
/// request id, the job id, or the scheduled run's trace id); the batch's own
/// entry carries it too.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// A unique id (UUID v7).
    pub id: String,
    /// The store's order: higher is newer (0 until stored).
    pub seq: u64,
    /// The batch it belongs to, if any.
    pub batch_id: Option<String>,
    /// When it started.
    pub at: DateTime<Utc>,
    /// How long it took, when it has a duration.
    pub duration_ms: Option<f64>,
    /// The signed-in user, once known.
    pub user_id: Option<String>,
    /// The workspace, once resolved.
    pub workspace_id: Option<String>,
    /// The trace it happened in.
    pub trace_id: Option<String>,
    /// The tags of its batch (or of the entry itself, outside a batch).
    pub tags: Vec<String>,
    /// A failed request (5xx), job, query or task.
    pub failed: bool,
    /// A slow query, or a batch that ran one.
    pub slow: bool,
    /// What it records.
    pub content: Content,
}

impl Entry {
    /// An entry of `content` at `at`, with a fresh id and nothing else set.
    pub fn new(at: DateTime<Utc>, content: Content) -> Entry {
        Entry {
            id: uuid::Uuid::now_v7().to_string(),
            seq: 0,
            batch_id: None,
            at,
            duration_ms: None,
            user_id: None,
            workspace_id: None,
            trace_id: None,
            tags: Vec::new(),
            failed: false,
            slow: false,
            content,
        }
    }

    /// Its kind.
    pub fn kind(&self) -> EntryKind {
        self.content.kind()
    }

    /// The key its tags are kept under: the batch id, else its own id.
    pub fn batch_key(&self) -> &str {
        self.batch_id.as_deref().unwrap_or(&self.id)
    }

    /// Whether it shows in the unfiltered list: a batch, or an entry
    /// outside any batch.
    pub fn is_top_level(&self) -> bool {
        self.kind().is_batch() || self.batch_id.is_none()
    }

    /// The response status of a request.
    pub fn status(&self) -> Option<u16> {
        match &self.content {
            Content::Request(request) => Some(request.status),
            _ => None,
        }
    }

    /// The route template of a request.
    pub fn route(&self) -> Option<&str> {
        match &self.content {
            Content::Request(request) => request.route.as_deref(),
            _ => None,
        }
    }

    /// The batch summary of a request, job or scheduled task.
    pub fn summary(&self) -> Option<&BatchSummary> {
        match &self.content {
            Content::Request(x) => Some(&x.summary),
            Content::Job(x) => Some(&x.summary),
            Content::Schedule(x) => Some(&x.summary),
            _ => None,
        }
    }

    /// The batch summary, to fill in.
    pub(crate) fn summary_mut(&mut self) -> Option<&mut BatchSummary> {
        match &mut self.content {
            Content::Request(x) => Some(&mut x.summary),
            Content::Job(x) => Some(&mut x.summary),
            Content::Schedule(x) => Some(&mut x.summary),
            _ => None,
        }
    }

    /// One line describing it, as in the list (`GET /api/v1/deals`,
    /// the SQL, the job type, the mail subject...).
    pub fn title(&self) -> String {
        match &self.content {
            Content::Request(x) => format!("{} {}", x.method, x.path),
            Content::Query(x) => x.sql.clone(),
            Content::Job(x) => x.job_type.clone(),
            Content::Mail(x) => x.subject.clone(),
            Content::Event(x) => x.name.clone(),
            Content::Cache(x) => format!("{} {}", x.operation, x.key),
            Content::Log(x) => x.message.clone(),
            Content::Exception(x) => x.message.clone(),
            Content::HttpClient(x) => format!("{} {}", x.method, x.url),
            Content::Notification(x) => {
                format!("{} via {}", x.notification_type, x.channel)
            }
            Content::Model(x) => format!("{} {} {}", x.entity, x.op, x.id),
            Content::Schedule(x) => x.task.clone(),
            Content::Dump(x) => x.label.clone(),
            Content::Ai(x) => format!("{} {}", x.model, x.operation),
            Content::Broadcast(x) => format!("{} to {}", x.event, x.channels.join(", ")),
            Content::Storage(x) => format!("{} {}", x.operation, x.path),
            Content::Webhook(x) => format!("{} to {}", x.event, x.endpoint),
            Content::Search(x) => format!("{} search of {}", x.mode, x.index),
            Content::SignIn(x) => format!("{} sign-in {}", x.method, x.outcome),
            Content::Ui(x) => format!("{} {}", x.kind, x.name),
        }
    }

    /// The text free-text search looks in: paths, SQL, job types, event
    /// names, mail subjects and recipients, log messages and the rest.
    pub fn search_text(&self) -> String {
        let mut text = self.title();
        match &self.content {
            Content::Request(x) => {
                if let Some(route) = &x.route {
                    text.push(' ');
                    text.push_str(route);
                }
            }
            Content::Mail(x) => {
                for to in &x.to {
                    text.push(' ');
                    text.push_str(to);
                }
            }
            Content::Log(x) => {
                text.push(' ');
                text.push_str(&x.target);
            }
            Content::Exception(x) => {
                for line in &x.chain {
                    text.push(' ');
                    text.push_str(line);
                }
            }
            Content::Job(x) => {
                if let Some(error) = &x.error {
                    text.push(' ');
                    text.push_str(error);
                }
            }
            Content::Ai(x) => {
                text.push(' ');
                text.push_str(&x.provider);
            }
            Content::Storage(x) => {
                text.push(' ');
                text.push_str(&x.disk);
            }
            Content::Search(x) => {
                text.push(' ');
                text.push_str(&x.engine);
            }
            Content::SignIn(x) => {
                for part in [x.provider.as_deref(), x.reason.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    text.push(' ');
                    text.push_str(part);
                }
            }
            _ => {}
        }
        text
    }
}

/// What an [`Entry`] records, one variant per kind. Serialized with a
/// `type` tag (`{"type": "query", "sql": ..}`).
// A request carries its captured exchange inline: entries are built once,
// moved to the store's writer and serialized, never copied around.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Content {
    /// An HTTP request.
    Request(RequestEntry),
    /// A SQL statement.
    Query(QueryEntry),
    /// A job attempt.
    Job(JobEntry),
    /// A mail.
    Mail(MailEntry),
    /// An app event.
    Event(EventEntry),
    /// A cache call.
    Cache(CacheEntry),
    /// A log line.
    Log(LogEntry),
    /// An error or panic.
    Exception(ExceptionEntry),
    /// An outgoing HTTP call.
    HttpClient(HttpClientEntry),
    /// A notification.
    Notification(NotificationEntry),
    /// A data change.
    Model(ModelEntry),
    /// A scheduled task run.
    Schedule(ScheduleEntry),
    /// A dumped value.
    Dump(DumpEntry),
    /// A call to a model provider.
    Ai(AiEntry),
    /// A broadcast.
    Broadcast(BroadcastEntry),
    /// A file operation.
    Storage(StorageEntry),
    /// A webhook delivery attempt.
    Webhook(WebhookEntry),
    /// A search.
    Search(SearchEntry),
    /// A sign-in attempt.
    SignIn(SignInEntry),
    /// A Navree UI operation.
    Ui(UiEntry),
}

impl Content {
    /// The kind of entry it is.
    pub fn kind(&self) -> EntryKind {
        match self {
            Content::Request(_) => EntryKind::Request,
            Content::Query(_) => EntryKind::Query,
            Content::Job(_) => EntryKind::Job,
            Content::Mail(_) => EntryKind::Mail,
            Content::Event(_) => EntryKind::Event,
            Content::Cache(_) => EntryKind::Cache,
            Content::Log(_) => EntryKind::Log,
            Content::Exception(_) => EntryKind::Exception,
            Content::HttpClient(_) => EntryKind::HttpClient,
            Content::Notification(_) => EntryKind::Notification,
            Content::Model(_) => EntryKind::Model,
            Content::Schedule(_) => EntryKind::Schedule,
            Content::Dump(_) => EntryKind::Dump,
            Content::Ai(_) => EntryKind::Ai,
            Content::Broadcast(_) => EntryKind::Broadcast,
            Content::Storage(_) => EntryKind::Storage,
            Content::Webhook(_) => EntryKind::Webhook,
            Content::Search(_) => EntryKind::Search,
            Content::SignIn(_) => EntryKind::SignIn,
            Content::Ui(_) => EntryKind::Ui,
        }
    }
}

/// What a batch did, counted when it ended.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchSummary {
    /// Queries run.
    pub queries: u32,
    /// Queries slower than `logbook.slow_query_ms`.
    pub slow_queries: u32,
    /// Statements repeated `logbook.duplicate_queries` times or more.
    pub duplicate_queries: u32,
    /// Events emitted.
    pub events: u32,
    /// Jobs queued (job attempts in the same trace).
    pub jobs: u32,
    /// Mails sent.
    pub mail: u32,
    /// Cache calls.
    pub cache: u32,
    /// Log lines.
    pub logs: u32,
    /// Exceptions.
    pub exceptions: u32,
    /// Outgoing HTTP calls.
    pub http_client: u32,
    /// Notifications.
    pub notifications: u32,
    /// Data changes.
    pub models: u32,
    /// Dumps.
    pub dumps: u32,
    /// Calls to a model provider.
    pub ai_calls: u32,
    /// Broadcasts.
    pub broadcasts: u32,
    /// File operations.
    pub storage_ops: u32,
    /// Webhook delivery attempts.
    pub webhooks: u32,
    /// Searches.
    pub searches: u32,
    /// Sign-in attempts.
    pub sign_ins: u32,
    /// Navree UI operations.
    pub ui_ops: u32,
}

impl BatchSummary {
    /// Counts one entry of `kind` in.
    pub(crate) fn count(&mut self, kind: EntryKind) {
        let counter = match kind {
            EntryKind::Query => &mut self.queries,
            EntryKind::Event => &mut self.events,
            EntryKind::Mail => &mut self.mail,
            EntryKind::Cache => &mut self.cache,
            EntryKind::Log => &mut self.logs,
            EntryKind::Exception => &mut self.exceptions,
            EntryKind::HttpClient => &mut self.http_client,
            EntryKind::Notification => &mut self.notifications,
            EntryKind::Model => &mut self.models,
            EntryKind::Dump => &mut self.dumps,
            EntryKind::Ai => &mut self.ai_calls,
            EntryKind::Broadcast => &mut self.broadcasts,
            EntryKind::Storage => &mut self.storage_ops,
            EntryKind::Webhook => &mut self.webhooks,
            EntryKind::Search => &mut self.searches,
            EntryKind::SignIn => &mut self.sign_ins,
            EntryKind::Ui => &mut self.ui_ops,
            EntryKind::Request | EntryKind::Job | EntryKind::Schedule => return,
        };
        *counter += 1;
    }
}

/// An HTTP request and its response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestEntry {
    /// The method (`GET`).
    pub method: String,
    /// The path, without the query string.
    pub path: String,
    /// The matched route template (`/api/v1/deals/{id}`).
    pub route: Option<String>,
    /// The response status.
    pub status: u16,
    /// The request id (`X-Request-Id`), which is also the batch id.
    pub request_id: String,
    /// The client address.
    pub client_ip: Option<String>,
    /// How long it took.
    pub duration_ms: f64,
    /// The request: headers, query string and body (when captured).
    pub request: Option<HttpMessage>,
    /// The response: headers and body (when captured).
    pub response: Option<HttpMessage>,
    /// The request this one replayed.
    pub replay_of: Option<String>,
    /// What the request did.
    pub summary: BatchSummary,
}

/// The captured side of an HTTP exchange.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HttpMessage {
    /// Headers, in order; `Authorization`, `Cookie` and `Set-Cookie` are
    /// never stored and secret-looking values are `[redacted]`.
    pub headers: Vec<(String, String)>,
    /// The query string (requests), with secret-looking values redacted.
    pub query: Option<String>,
    /// The body, when there is one.
    pub body: Option<CapturedBody>,
}

impl HttpMessage {
    /// The first header named `name` (case-insensitively).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A request or response body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CapturedBody {
    /// The `Content-Type`.
    pub content_type: Option<String>,
    /// The full size in bytes, when known.
    pub size: Option<u64>,
    /// The text, cut after `logbook.capture.max_body_kb`; `None` for
    /// binary bodies, which are stored as their size and type only.
    pub text: Option<String>,
    /// Whether `text` was cut.
    pub truncated: bool,
}

/// A SQL statement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryEntry {
    /// The statement with its placeholders (navree-db reports no bound
    /// values).
    pub sql: String,
    /// The connection (the database URL without its password).
    pub connection: String,
    /// The rows returned or affected, when known.
    pub rows: Option<u64>,
    /// How long it took.
    pub duration_ms: f64,
    /// Whether it failed.
    pub failed: bool,
    /// Whether it was slower than `logbook.slow_query_ms`.
    pub slow: bool,
}

/// How a job attempt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum JobOutcome {
    /// It succeeded.
    Processed,
    /// It failed and goes back on the queue.
    Released,
    /// It failed for good (moved to the failed jobs).
    Failed,
}

impl JobOutcome {
    /// The name (`processed`).
    pub fn as_str(self) -> &'static str {
        match self {
            JobOutcome::Processed => "processed",
            JobOutcome::Released => "released",
            JobOutcome::Failed => "failed",
        }
    }
}

/// A job attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobEntry {
    /// The queue.
    pub queue: String,
    /// The job type (`mail.send`).
    pub job_type: String,
    /// The job id, which is also the batch id.
    pub job_id: String,
    /// The attempt (1 for the first).
    pub attempt: u32,
    /// How it ended.
    pub outcome: JobOutcome,
    /// The error, when it failed.
    pub error: Option<String>,
    /// How long it ran.
    pub duration_ms: f64,
    /// What the attempt did.
    pub summary: BatchSummary,
}

/// A mail handed to the transport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MailEntry {
    /// The sender.
    pub from: Option<String>,
    /// The recipients.
    pub to: Vec<String>,
    /// Carbon-copy recipients.
    pub cc: Vec<String>,
    /// Blind carbon-copy recipients.
    pub bcc: Vec<String>,
    /// The subject.
    pub subject: String,
    /// The plain-text body.
    pub text: Option<String>,
    /// The HTML body.
    pub html: Option<String>,
    /// The transport (`smtp`, `log`, `array`).
    pub transport: String,
}

/// An app event on the event bus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventEntry {
    /// The event name (`deal.won`).
    pub name: String,
    /// The payload.
    pub payload: Value,
    /// The listeners that ran.
    pub listeners: Vec<String>,
    /// The queued listeners whose job was dispatched.
    pub queued: Vec<String>,
    /// The listeners that failed.
    pub failed: Vec<String>,
}

/// A cache call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CacheEntry {
    /// `hit`, `miss`, `write` or `forget`.
    pub operation: String,
    /// The key.
    pub key: String,
    /// The store.
    pub store: String,
}

/// A log line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    /// The level (`DEBUG`).
    pub level: String,
    /// The target (`navree::request`, a module path).
    pub target: String,
    /// The message.
    pub message: String,
    /// The other fields (redacted as in the log output).
    pub fields: BTreeMap<String, Value>,
}

/// An error or panic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExceptionEntry {
    /// The message.
    pub message: String,
    /// The message, then each source's message.
    pub chain: Vec<String>,
    /// Where it happened (`file:line:column`), when known.
    pub location: Option<String>,
    /// Whether it was a panic.
    pub panic: bool,
}

/// An outgoing HTTP call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HttpClientEntry {
    /// The method.
    pub method: String,
    /// The URL without its query string or credentials.
    pub url: String,
    /// The final status, if a response came.
    pub status: Option<u16>,
    /// Attempts made.
    pub attempts: u32,
    /// The error, when no response came.
    pub error: Option<String>,
}

/// A notification delivered over one channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NotificationEntry {
    /// The notification id (shared by its channels).
    pub notification_id: String,
    /// The notification type.
    pub notification_type: String,
    /// The notifiable's type (`user`).
    pub notifiable_type: String,
    /// The notifiable's id.
    pub notifiable_id: String,
    /// The channel: `database`, `mail` or `broadcast`.
    pub channel: String,
}

/// A data change a model made in a unit of work.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelEntry {
    /// The table (`deals`).
    pub entity: String,
    /// The primary key.
    pub id: String,
    /// `created`, `updated` or `deleted`.
    pub op: String,
    /// Before: the whole record for deletes, the changed columns for updates.
    pub old: Option<Value>,
    /// After: the whole record for creates, the changed columns for updates.
    pub new: Option<Value>,
}

/// A scheduled task run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScheduleEntry {
    /// The task name.
    pub task: String,
    /// The minute it was due at.
    pub tick: DateTime<Utc>,
    /// The error, when it failed.
    pub error: Option<String>,
    /// How long it ran.
    pub duration_ms: f64,
    /// What the run did.
    pub summary: BatchSummary,
}

/// A value recorded with [`Logbook::dump`](crate::Logbook::dump).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DumpEntry {
    /// The label.
    pub label: String,
    /// The value, as JSON.
    pub value: Value,
}

/// A call to a model provider. Never the prompt or the answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiEntry {
    /// The provider (`anthropic`, `openai`).
    pub provider: String,
    /// The model.
    pub model: String,
    /// `generate`, `stream` or `embed`.
    pub operation: String,
    /// The input tokens, when the provider said.
    pub input_tokens: Option<u64>,
    /// The output tokens, when the provider said.
    pub output_tokens: Option<u64>,
    /// The part of the input read from the provider's prompt cache.
    pub cached_input_tokens: Option<u64>,
    /// The cost in millionths of a US dollar, when the app prices its models.
    pub cost_micros: Option<u64>,
    /// The failure, when the call failed.
    pub error: Option<String>,
}

/// An event pushed to real-time channels. Never its data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BroadcastEntry {
    /// The channels.
    pub channels: Vec<String>,
    /// The event name (`deal.updated`).
    pub event: String,
    /// The deliveries queued to subscribers.
    pub delivered: u64,
    /// The failure, when the broadcast failed.
    pub error: Option<String>,
}

/// A file operation on a storage disk. Never the contents.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageEntry {
    /// The disk.
    pub disk: String,
    /// The disk's driver (`local`, `s3`).
    pub driver: String,
    /// `read`, `write`, `delete`, `list` or `copy`.
    pub operation: String,
    /// The path, or the directory listed.
    pub path: String,
    /// The bytes read or written, when known.
    pub bytes: Option<u64>,
    /// The failure, when the operation failed.
    pub error: Option<String>,
}

/// One attempt to deliver a webhook. Never the payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookEntry {
    /// The endpoint.
    pub endpoint: String,
    /// The event delivered (`invoice.paid`).
    pub event: String,
    /// The attempt (1 for the first).
    pub attempt: u32,
    /// The status the endpoint answered; `None` when no answer came.
    pub status: Option<u16>,
    /// The failure, when the endpoint did not take it.
    pub error: Option<String>,
}

/// A search. Never the term, only its length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEntry {
    /// The engine.
    pub engine: String,
    /// The index searched.
    pub index: String,
    /// `text`, `hybrid`, `similar` or `across`.
    pub mode: String,
    /// The length of the term in characters.
    pub term_length: u32,
    /// The hits, when the engine said.
    pub hits: Option<u64>,
    /// The failure, when the search failed.
    pub error: Option<String>,
}

/// A sign-in attempt. Never the email address or a secret; the entry's
/// `user_id` is set only when the sign-in worked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInEntry {
    /// `password`, `two_factor`, `magic_link`, `code`, `passkey`, `social`,
    /// `sso` or `token_exchange`.
    pub method: String,
    /// The social provider, single sign-on connection or token issuer.
    pub provider: Option<String>,
    /// `success` or `failure`.
    pub outcome: String,
    /// Why it failed, as a code (`invalid_credentials`).
    pub reason: Option<String>,
}

/// A Navree UI operation. Never the form's values or the markup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiEntry {
    /// The route template, or the operation's own name.
    pub name: String,
    /// `form_post`, `action`, `table_query`, `live_region`, `fragment` or
    /// `upload`.
    pub kind: String,
    /// How many operations the answer held.
    pub ops: u32,
    /// The response status.
    pub status: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_through_their_names() {
        for kind in EntryKind::ALL {
            assert_eq!(EntryKind::parse(kind.as_str()), Some(kind));
            assert!(!kind.label().is_empty());
        }
        assert_eq!(EntryKind::parse("Queries"), Some(EntryKind::Query));
        assert_eq!(EntryKind::parse("models"), Some(EntryKind::Model));
        assert_eq!(EntryKind::parse("http-client"), Some(EntryKind::HttpClient));
        assert_eq!(EntryKind::parse("nope"), None);
    }

    #[test]
    fn the_newer_kinds_answer_to_their_plurals() {
        for (name, kind) in [
            ("ai_calls", EntryKind::Ai),
            ("Broadcasts", EntryKind::Broadcast),
            ("storage-op", EntryKind::Storage),
            ("webhooks", EntryKind::Webhook),
            ("searches", EntryKind::Search),
            ("sign-ins", EntryKind::SignIn),
            ("signin", EntryKind::SignIn),
            ("ui_operation", EntryKind::Ui),
        ] {
            assert_eq!(EntryKind::parse(name), Some(kind), "{name}");
        }
        assert_eq!(EntryKind::ALL.len(), 20);
    }

    #[test]
    fn the_summary_counts_the_newer_kinds() {
        let mut summary = BatchSummary::default();
        for kind in [
            EntryKind::Ai,
            EntryKind::Ai,
            EntryKind::Broadcast,
            EntryKind::Storage,
            EntryKind::Webhook,
            EntryKind::Search,
            EntryKind::SignIn,
            EntryKind::Ui,
        ] {
            summary.count(kind);
        }
        assert_eq!(summary.ai_calls, 2);
        assert_eq!(
            [
                summary.broadcasts,
                summary.storage_ops,
                summary.webhooks,
                summary.searches,
                summary.sign_ins,
                summary.ui_ops
            ],
            [1; 6]
        );
    }

    #[test]
    fn batches_are_requests_jobs_and_scheduled_tasks() {
        let batches: Vec<_> = EntryKind::ALL
            .into_iter()
            .filter(|k| k.is_batch())
            .collect();
        assert_eq!(
            batches,
            [EntryKind::Request, EntryKind::Job, EntryKind::Schedule]
        );
    }

    #[test]
    fn content_serializes_with_a_type_tag() {
        let entry = Entry::new(
            Utc::now(),
            Content::Dump(DumpEntry {
                label: "rows".into(),
                value: serde_json::json!([1, 2]),
            }),
        );
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["content"]["type"], "dump");
        let back: Entry = serde_json::from_value(json).unwrap();
        assert_eq!(back, entry);
        assert!(entry.is_top_level());
        assert_eq!(entry.batch_key(), entry.id);
    }
}
