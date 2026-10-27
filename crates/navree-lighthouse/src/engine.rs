//! The engine: one background task that turns telemetry records and
//! reports into issue hits, occurrences, samples (traces) and aggregates,
//! and writes them in batches.
//!
//! The sink and [`Lighthouse::capture`](crate::Lighthouse::capture) only
//! copy a record into a bounded channel. The task groups the events of one
//! request or job (a *unit*: its queries, outgoing calls, cache calls,
//! exceptions, tags and captured headers) until the unit ends, then
//! decides what to keep. Counts are always exact; details (samples first,
//! occurrences last) are dropped when more than `lighthouse.buffer` wait to
//! be written.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use chrono::{DateTime, TimeDelta, Utc};
use http::HeaderMap;
use navree_telemetry::{
    EventContext, ExceptionReported, HttpClientRequest, JobFailed, JobProcessed, QueryExecuted,
    RequestHandled, ScheduledTaskRan, TelemetryEvent, TelemetryRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::aggregate::{Aggregates, Observation, SlowCall, kind};
use crate::model::{
    IssueKind, Level, custom_fingerprint, exception_fingerprint, failed_job_fingerprint,
    outgoing_fingerprint, panic_fingerprint, scheduled_task_fingerprint, slow_query_fingerprint,
    slow_request_fingerprint,
};
use crate::normalize::{
    first_line, normalize_sql, outgoing_error_kind, path_matches, truncate, url_host,
};
use crate::options::LighthouseOptions;
use crate::redact::Redactor;
use crate::report::Report;

/// The job type of alert deliveries, never recorded.
pub(crate) const ALERT_JOB: &str = "lighthouse.alert";

/// The job id Lighthouse's own background work runs under (its queries
/// and calls are not recorded).
pub(crate) const INTERNAL_UNIT: &str = "lighthouse:internal";

/// At most this many spans per trace.
const MAX_SPANS: usize = 200;
/// At most this many requests and jobs followed at once.
const MAX_UNITS: usize = 10_000;
/// A unit not finished after this long is dropped.
const STALE_AFTER: Duration = Duration::from_secs(600);
/// Occurrences of one issue stored per hour before sampling applies.
const OCCURRENCES_PER_HOUR: u32 = 100;
/// Flush as soon as this many details wait.
const FLUSH_AT: usize = 500;

/// What the engine receives.
pub(crate) enum Input {
    /// A telemetry record.
    Record(Box<TelemetryRecord>),
    /// A report from [`Lighthouse::capture`](crate::Lighthouse::capture).
    Report(Box<(Report, EventContext, DateTime<Utc>)>),
    /// A tag on the current request or job.
    Tag(UnitKey, String, String),
    /// The headers (and body) of a request, from the HTTP layer.
    Capture(String, Box<RequestCapture>),
    /// Write everything now, then answer.
    Flush(oneshot::Sender<()>),
}

/// What the HTTP layer captured of a request.
#[derive(Clone, Debug, Default)]
pub(crate) struct RequestCapture {
    pub(crate) headers: HeaderMap,
    pub(crate) body: Option<Bytes>,
}

/// A request or a job.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum UnitKey {
    Request(String),
    Job(String),
}

impl UnitKey {
    pub(crate) fn of(context: &EventContext) -> Option<UnitKey> {
        match (&context.request_id, &context.job_id) {
            (Some(id), _) => Some(UnitKey::Request(id.clone())),
            (None, Some(id)) => Some(UnitKey::Job(id.clone())),
            (None, None) => None,
        }
    }
}

/// One span of a trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// `db`, `http`, `cache` or `mail`.
    pub kind: String,
    /// The statement, the call, the cache key or the subject.
    pub label: String,
    /// When it started, in milliseconds from the start of the request or
    /// job.
    pub start_ms: f64,
    /// How long it took, in milliseconds.
    pub duration_ms: f64,
    /// Whether it failed.
    pub failed: bool,
}

/// A span before the unit's start is known.
#[derive(Clone, Debug)]
struct RawSpan {
    kind: &'static str,
    label: String,
    end: DateTime<Utc>,
    duration_ms: f64,
    failed: bool,
}

/// An exception waiting for its unit to end.
#[derive(Clone, Debug)]
struct PendingException {
    at: DateTime<Utc>,
    context: EventContext,
    message: String,
    chain: Vec<String>,
    location: Option<String>,
    panic: bool,
    error_type: Option<String>,
    level: Level,
    tags: BTreeMap<String, String>,
    values: BTreeMap<String, Value>,
    fingerprint: Option<Vec<String>>,
    user: Option<String>,
    backtrace: Option<String>,
}

/// A query waiting for its unit to end.
#[derive(Clone, Debug)]
struct PendingQuery {
    at: DateTime<Utc>,
    context: EventContext,
    statement: String,
    duration_ms: f64,
    failed: bool,
}

/// An outgoing call waiting for its unit to end.
#[derive(Clone, Debug)]
struct PendingCall {
    at: DateTime<Utc>,
    context: EventContext,
    call: HttpClientRequest,
}

/// The events of one request or job so far.
#[derive(Default)]
struct Unit {
    created: Option<Instant>,
    spans: Vec<RawSpan>,
    spans_dropped: u32,
    queries: Vec<PendingQuery>,
    calls: Vec<PendingCall>,
    exceptions: Vec<PendingException>,
    tags: BTreeMap<String, String>,
    capture: Option<RequestCapture>,
}

impl Unit {
    fn push_span(&mut self, span: RawSpan) {
        if self.spans.len() < MAX_SPANS {
            self.spans.push(span);
        } else {
            self.spans_dropped += 1;
        }
    }

    fn spans_from(&self, start: DateTime<Utc>) -> Vec<Span> {
        let mut spans: Vec<Span> = self
            .spans
            .iter()
            .map(|span| {
                let begin = span.end - duration_delta(span.duration_ms);
                Span {
                    kind: span.kind.to_owned(),
                    label: span.label.clone(),
                    start_ms: ((begin - start).num_microseconds().unwrap_or(0) as f64 / 1000.0)
                        .max(0.0),
                    duration_ms: span.duration_ms,
                    failed: span.failed,
                }
            })
            .collect();
        spans.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
        spans
    }
}

/// The labels every record carries.
#[derive(Clone, Debug)]
pub(crate) struct Labels {
    pub(crate) environment: String,
    pub(crate) release: String,
    pub(crate) server: String,
    pub(crate) process: String,
    /// This process's aggregate rows: `server/process/random`.
    pub(crate) process_key: String,
}

/// An occurrence to store.
#[derive(Clone, Debug)]
pub(crate) struct NewOccurrence {
    pub(crate) id: Uuid,
    pub(crate) at: DateTime<Utc>,
    pub(crate) level: Level,
    pub(crate) message: String,
    pub(crate) chain: Vec<String>,
    pub(crate) backtrace: Option<String>,
    pub(crate) location: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) job_id: Option<String>,
    pub(crate) trace_id: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) workspace_id: Option<String>,
    pub(crate) method: Option<String>,
    pub(crate) route: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) status: Option<u16>,
    pub(crate) sample_id: Option<Uuid>,
    pub(crate) context: Value,
}

/// One occurrence of an issue (always counted; its details only when
/// `occurrence` is set).
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub(crate) fingerprint: String,
    pub(crate) kind: IssueKind,
    pub(crate) level: Level,
    pub(crate) title: String,
    pub(crate) culprit: String,
    pub(crate) message: String,
    pub(crate) chain: Vec<String>,
    pub(crate) location: Option<String>,
    pub(crate) error_type: Option<String>,
    pub(crate) at: DateTime<Utc>,
    pub(crate) failed_job: bool,
    pub(crate) occurrence: Option<NewOccurrence>,
}

/// A request or job sample to store.
#[derive(Clone, Debug)]
pub(crate) struct NewSample {
    pub(crate) id: Uuid,
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    pub(crate) at: DateTime<Utc>,
    pub(crate) duration_ms: f64,
    pub(crate) status: Option<u16>,
    pub(crate) outcome: &'static str,
    pub(crate) slow: bool,
    pub(crate) context: EventContext,
    pub(crate) spans: Vec<Span>,
    pub(crate) details: Value,
    pub(crate) error: Option<String>,
}

/// What a flush writes.
#[derive(Default)]
pub(crate) struct Batch {
    pub(crate) hits: Vec<Hit>,
    pub(crate) samples: Vec<NewSample>,
    pub(crate) rows: Vec<crate::aggregate::Row>,
}

/// Writes batches (the database writer, or a test double).
pub(crate) trait BatchWriter: Send + Sync + 'static {
    /// Writes `batch`; on error the counts in `batch.rows` are written
    /// again with the next batch.
    fn write<'a>(
        &'a self,
        batch: Batch,
        labels: &'a Labels,
    ) -> navree_kernel::BoxFuture<'a, Result<(), navree_kernel::BoxError>>;
}

/// The handle the sink, the service and the HTTP layer send through.
#[derive(Clone)]
pub(crate) struct EngineHandle {
    tx: mpsc::Sender<Input>,
    dropped: Arc<AtomicU64>,
}

impl EngineHandle {
    /// A handle and the receiver the engine task reads.
    pub(crate) fn new(capacity: usize) -> (EngineHandle, mpsc::Receiver<Input>) {
        let (tx, rx) = mpsc::channel(capacity.max(1024));
        (
            EngineHandle {
                tx,
                dropped: Arc::new(AtomicU64::new(0)),
            },
            rx,
        )
    }

    /// Queues `input`; counts it as dropped when the channel is full.
    pub(crate) fn send(&self, input: Input) {
        if self.tx.try_send(input).is_err() {
            self.count_dropped(1);
        }
    }

    /// Writes everything received so far and waits for it.
    pub(crate) async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Input::Flush(tx)).await.is_ok() {
            let _ = rx.await;
        }
    }

    /// Details dropped by this process so far.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn count_dropped(&self, n: u64) {
        self.dropped.fetch_add(n, Ordering::Relaxed);
        metrics::counter!("lighthouse_dropped_total").increment(n);
    }
}

/// The engine's state, owned by its task.
pub(crate) struct Processor {
    options: Arc<LighthouseOptions>,
    labels: Labels,
    redactor: Redactor,
    handle: EngineHandle,
    units: HashMap<UnitKey, Unit>,
    aggregates: Aggregates,
    hits: Vec<Hit>,
    samples: Vec<NewSample>,
    stored_occurrences: usize,
    budgets: HashMap<String, (i64, u32)>,
    ignored_paths: Vec<String>,
}

impl Processor {
    pub(crate) fn new(
        options: Arc<LighthouseOptions>,
        labels: Labels,
        redactor: Redactor,
        handle: EngineHandle,
    ) -> Processor {
        let mut ignored_paths = options.ignore.routes.clone();
        ignored_paths.push(options.mount_path().to_owned());
        ignored_paths.push(format!("{}/*", options.mount_path()));
        Processor {
            options,
            labels,
            redactor,
            handle,
            units: HashMap::new(),
            aggregates: Aggregates::default(),
            hits: Vec::new(),
            samples: Vec::new(),
            stored_occurrences: 0,
            budgets: HashMap::new(),
            ignored_paths,
        }
    }

    /// Runs until the receiver closes or `stop` resolves, then writes what
    /// it holds.
    pub(crate) async fn run(
        mut self,
        mut rx: mpsc::Receiver<Input>,
        writer: Arc<dyn BatchWriter>,
        stop: impl std::future::Future<Output = ()>,
    ) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tokio::pin!(stop);
        loop {
            tokio::select! {
                input = rx.recv() => match input {
                    Some(Input::Flush(done)) => {
                        self.flush(writer.as_ref()).await;
                        let _ = done.send(());
                    }
                    Some(input) => {
                        self.handle(input, Utc::now());
                        if self.details() >= FLUSH_AT {
                            self.flush(writer.as_ref()).await;
                        }
                    }
                    None => break,
                },
                _ = tick.tick() => self.flush(writer.as_ref()).await,
                () = &mut stop => {
                    while let Ok(input) = rx.try_recv() {
                        match input {
                            Input::Flush(done) => { let _ = done.send(()); }
                            input => self.handle(input, Utc::now()),
                        }
                    }
                    break;
                }
            }
        }
        self.flush(writer.as_ref()).await;
    }

    /// Details waiting to be written.
    pub(crate) fn details(&self) -> usize {
        self.samples.len() + self.stored_occurrences
    }

    /// Writes what is pending.
    pub(crate) async fn flush(&mut self, writer: &dyn BatchWriter) {
        let now = Utc::now();
        self.sweep(now);
        let batch = Batch {
            hits: std::mem::take(&mut self.hits),
            samples: std::mem::take(&mut self.samples),
            rows: self.aggregates.take_dirty(),
        };
        self.stored_occurrences = 0;
        if batch.hits.is_empty() && batch.samples.is_empty() && batch.rows.is_empty() {
            return;
        }
        let rows = batch.rows.clone();
        let details = batch.samples.len()
            + batch
                .hits
                .iter()
                .filter(|hit| hit.occurrence.is_some())
                .count();
        if let Err(error) = writer.write(batch, &self.labels).await {
            tracing::warn!(target: "navree::lighthouse", %error, "could not write monitoring data");
            self.aggregates.mark_dirty(&rows);
            self.handle.count_dropped(details as u64);
        }
        self.aggregates.evict(now);
    }

    /// Drops units that never ended and old occurrence budgets.
    fn sweep(&mut self, now: DateTime<Utc>) {
        self.units.retain(|_, unit| {
            unit.created
                .is_none_or(|created| created.elapsed() < STALE_AFTER)
        });
        let hour = now.timestamp() / 3600;
        self.budgets.retain(|_, (budget_hour, _)| *budget_hour >= hour - 1);
    }

    /// Handles one input.
    pub(crate) fn handle(&mut self, input: Input, now: DateTime<Utc>) {
        match input {
            Input::Record(record) => self.record(*record, now),
            Input::Report(report) => {
                let (report, context, at) = *report;
                self.report(report, context, at, now);
            }
            Input::Tag(key, name, value) => {
                if let Some(unit) = self.unit(&key) {
                    unit.tags.insert(name, value);
                }
            }
            Input::Capture(request_id, capture) => {
                if let Some(unit) = self.unit(&UnitKey::Request(request_id)) {
                    unit.capture = Some(*capture);
                }
            }
            Input::Flush(done) => {
                let _ = done.send(());
            }
        }
    }

    /// The unit for `key`, created on first use (none past the limit).
    fn unit(&mut self, key: &UnitKey) -> Option<&mut Unit> {
        if !self.units.contains_key(key) {
            if self.units.len() >= MAX_UNITS {
                return None;
            }
            self.units.insert(
                key.clone(),
                Unit {
                    created: Some(Instant::now()),
                    ..Unit::default()
                },
            );
        }
        self.units.get_mut(key)
    }

    fn record(&mut self, record: TelemetryRecord, now: DateTime<Utc>) {
        let TelemetryRecord { at, context, event } = record;
        if context.job_id.as_deref() == Some(INTERNAL_UNIT) {
            return;
        }
        match event {
            TelemetryEvent::RequestHandled(request) => {
                self.finish_request(at, &context, &request, now)
            }
            TelemetryEvent::JobProcessed(job) => {
                self.finish_job(at, &context, JobEnd::processed(&job), now)
            }
            TelemetryEvent::JobFailed(job) => {
                self.finish_job(at, &context, JobEnd::failed(&job), now)
            }
            TelemetryEvent::QueryExecuted(query) => self.query(at, context, &query, now),
            TelemetryEvent::HttpClientRequest(call) => self.call(at, context, call, now),
            TelemetryEvent::Cache(cache) => {
                if let Some(key) = UnitKey::of(&context)
                    && let Some(unit) = self.unit(&key)
                {
                    unit.push_span(RawSpan {
                        kind: "cache",
                        label: format!("{} {}", cache.operation.as_str(), cache.key),
                        end: at,
                        duration_ms: 0.0,
                        failed: false,
                    });
                }
            }
            TelemetryEvent::MailSent(mail) => {
                if let Some(key) = UnitKey::of(&context)
                    && let Some(unit) = self.unit(&key)
                {
                    unit.push_span(RawSpan {
                        kind: "mail",
                        label: truncate(&mail.subject, 120),
                        end: at,
                        duration_ms: millis(mail.duration),
                        failed: false,
                    });
                }
            }
            TelemetryEvent::ScheduledTaskRan(task) => self.scheduled(at, &context, &task, now),
            TelemetryEvent::ExceptionReported(exception) => {
                self.exception(at, context, exception, now)
            }
            _ => {}
        }
    }

    fn report(&mut self, report: Report, context: EventContext, at: DateTime<Utc>, now: DateTime<Utc>) {
        let exception = PendingException {
            at,
            context,
            message: report.message,
            chain: report.chain,
            location: report.location,
            panic: false,
            error_type: report.error_type.map(str::to_owned),
            level: report.level,
            tags: report.tags,
            values: report.context,
            fingerprint: report.fingerprint,
            user: report.user,
            backtrace: report.backtrace,
        };
        self.pend_exception(exception, now);
    }

    fn exception(
        &mut self,
        at: DateTime<Utc>,
        context: EventContext,
        exception: ExceptionReported,
        now: DateTime<Utc>,
    ) {
        let pending = PendingException {
            at,
            context,
            message: exception.message,
            chain: exception.chain,
            location: exception.location,
            panic: exception.panic,
            error_type: None,
            level: Level::Error,
            tags: BTreeMap::new(),
            values: BTreeMap::new(),
            fingerprint: None,
            user: None,
            backtrace: None,
        };
        self.pend_exception(pending, now);
    }

    /// Keeps an exception for its unit, or records it now outside one.
    fn pend_exception(&mut self, exception: PendingException, now: DateTime<Utc>) {
        if let Some(key) = UnitKey::of(&exception.context)
            && let Some(unit) = self.unit(&key)
        {
            unit.exceptions.push(exception);
            return;
        }
        let culprit = self.labels.process.clone();
        self.exception_hit(exception, &culprit, &Origin::default(), None, now);
    }

    fn query(&mut self, at: DateTime<Utc>, context: EventContext, query: &QueryExecuted, now: DateTime<Utc>) {
        let duration_ms = millis(query.duration);
        let pending = PendingQuery {
            at,
            statement: normalize_sql(&query.sql),
            duration_ms,
            failed: query.failed,
            context: context.clone(),
        };
        if let Some(key) = UnitKey::of(&context)
            && let Some(unit) = self.unit(&key)
        {
            unit.push_span(RawSpan {
                kind: "db",
                label: truncate(query.sql.trim(), 500),
                end: at,
                duration_ms,
                failed: query.failed,
            });
            unit.queries.push(pending);
            return;
        }
        let origin = self.labels.process.clone();
        self.query_done(pending, &origin, &Origin::default(), now);
    }

    fn call(&mut self, at: DateTime<Utc>, context: EventContext, call: HttpClientRequest, now: DateTime<Utc>) {
        let pending = PendingCall {
            at,
            context: context.clone(),
            call,
        };
        if let Some(key) = UnitKey::of(&context)
            && let Some(unit) = self.unit(&key)
        {
            let call = &pending.call;
            unit.push_span(RawSpan {
                kind: "http",
                label: format!("{} {}", call.method, call.url),
                end: at,
                duration_ms: millis(call.duration),
                failed: call.error.is_some() || call.status.is_some_and(|s| s >= 500),
            });
            unit.calls.push(pending);
            return;
        }
        let culprit = self.labels.process.clone();
        self.call_done(pending, &culprit, &Origin::default(), now);
    }

    fn scheduled(
        &mut self,
        at: DateTime<Utc>,
        context: &EventContext,
        task: &ScheduledTaskRan,
        now: DateTime<Utc>,
    ) {
        let Some(error) = &task.error else {
            return;
        };
        let fingerprint = scheduled_task_fingerprint(&task.task, error);
        let occurrence = self.occurrence(
            &fingerprint,
            at,
            context,
            Level::Error,
            error.clone(),
            vec![error.clone()],
            &Origin::default(),
            json!({ "task": { "name": task.task, "tick": task.tick, "duration_ms": millis(task.duration) } }),
        );
        self.push_hit(
            Hit {
                fingerprint,
                kind: IssueKind::ScheduledTask,
                level: Level::Error,
                title: first_line(&format!("{} failed: {error}", task.task), 200),
                culprit: task.task.clone(),
                message: error.clone(),
                chain: vec![error.clone()],
                location: None,
                error_type: None,
                at,
                failed_job: false,
                occurrence,
            },
            now,
        );
    }

    fn finish_request(
        &mut self,
        at: DateTime<Utc>,
        context: &EventContext,
        request: &RequestHandled,
        now: DateTime<Utc>,
    ) {
        let unit = self
            .units
            .remove(&UnitKey::Request(request.request_id.clone()))
            .unwrap_or_default();
        if self.ignored_path(&request.path) {
            return;
        }
        let endpoint = format!(
            "{} {}",
            request.method,
            request.route.as_deref().unwrap_or("unmatched")
        );
        let duration_ms = millis(request.duration);
        self.aggregates.observe(
            now,
            kind::REQUEST,
            &endpoint,
            &Observation {
                duration_ms: Some(duration_ms),
                error: request.status >= 400,
                extra: request.status >= 500,
                ..Observation::default()
            },
        );
        let slow = duration_ms >= self.options.thresholds.slow_request_ms as f64;
        let keep = slow
            || request.status >= 500
            || !unit.exceptions.is_empty()
            || sampled(&request.request_id, self.options.sample.requests);
        let sample_id = keep.then(Uuid::now_v7);
        let mut context = context.clone();
        context.request_id = Some(request.request_id.clone());
        if context.user_id.is_none() {
            context.user_id.clone_from(&request.user_id);
        }
        if context.workspace_id.is_none() {
            context.workspace_id.clone_from(&request.workspace_id);
        }
        let request_details = self.request_details(request, unit.capture.as_ref());
        let origin = Origin {
            method: Some(request.method.clone()),
            route: Some(request.route.clone().unwrap_or_else(|| "unmatched".to_owned())),
            path: Some(request.path.clone()),
            status: Some(request.status),
            sample_id,
            tags: unit.tags.clone(),
            details: request_details.clone(),
            user_id: context.user_id.clone(),
            workspace_id: context.workspace_id.clone(),
        };
        self.finish_children(&unit, &endpoint, &origin, now);
        if slow {
            let fingerprint = slow_request_fingerprint(&endpoint);
            let message = format!("{endpoint} took {} ms", duration_ms.round());
            let occurrence = self.occurrence(
                &fingerprint,
                at,
                &context,
                Level::Warning,
                message.clone(),
                vec![message.clone()],
                &origin,
                Value::Null,
            );
            self.push_hit(
                Hit {
                    fingerprint,
                    kind: IssueKind::SlowRequest,
                    level: Level::Warning,
                    title: format!("Slow request: {endpoint}"),
                    culprit: endpoint.clone(),
                    message: message.clone(),
                    chain: vec![message],
                    location: None,
                    error_type: None,
                    at,
                    failed_job: false,
                    occurrence,
                },
                now,
            );
        }
        if let Some(id) = sample_id {
            let start = at - duration_delta(duration_ms);
            let mut details = request_details;
            if !unit.tags.is_empty() {
                details["tags"] = json!(unit.tags);
            }
            if unit.spans_dropped > 0 {
                details["spans_dropped"] = json!(unit.spans_dropped);
            }
            self.push_sample(NewSample {
                id,
                kind: "request",
                name: endpoint,
                at,
                duration_ms,
                status: Some(request.status),
                outcome: if request.status >= 400 { "error" } else { "ok" },
                slow,
                context,
                spans: unit.spans_from(start),
                details,
                error: None,
            });
        }
    }

    /// The request part of an occurrence's or sample's context.
    fn request_details(&self, request: &RequestHandled, capture: Option<&RequestCapture>) -> Value {
        let mut details = Map::new();
        let mut req = Map::new();
        req.insert("method".into(), json!(request.method));
        req.insert("path".into(), json!(request.path));
        req.insert("route".into(), json!(request.route));
        req.insert("status".into(), json!(request.status));
        if self.options.capture.ip {
            req.insert("ip".into(), json!(request.client_ip));
        }
        if let Some(capture) = capture {
            if self.options.capture.request_headers {
                req.insert(
                    "headers".into(),
                    Value::Object(self.redactor.headers(&capture.headers)),
                );
            }
            if self.options.capture.request_body
                && let Some(body) = &capture.body
                && let Ok(value) = serde_json::from_slice::<Value>(body)
            {
                req.insert("body".into(), self.redactor.value(value));
            }
        }
        details.insert("request".into(), Value::Object(req));
        Value::Object(details)
    }

    fn finish_job(&mut self, at: DateTime<Utc>, context: &EventContext, job: JobEnd, now: DateTime<Utc>) {
        let unit = self
            .units
            .remove(&UnitKey::Job(job.job_id.clone()))
            .unwrap_or_default();
        if job.job_type == ALERT_JOB || self.options.ignore.jobs.contains(&job.job_type) {
            return;
        }
        let failed = job.error.is_some();
        let moved_to_failed = failed && !job.will_retry;
        self.aggregates.observe(
            now,
            kind::JOB,
            &job.job_type,
            &Observation {
                duration_ms: Some(job.duration_ms),
                error: failed,
                extra: moved_to_failed,
                ..Observation::default()
            },
        );
        let slow = job.duration_ms >= self.options.thresholds.slow_job_ms as f64;
        let keep = slow
            || failed
            || !unit.exceptions.is_empty()
            || sampled(&format!("{}#{}", job.job_id, job.attempt), self.options.sample.jobs);
        let sample_id = keep.then(Uuid::now_v7);
        let mut context = context.clone();
        context.job_id = Some(job.job_id.clone());
        let details = json!({ "job": {
            "id": job.job_id,
            "type": job.job_type,
            "queue": job.queue,
            "attempt": job.attempt,
        }});
        let origin = Origin {
            sample_id,
            tags: unit.tags.clone(),
            details: details.clone(),
            user_id: context.user_id.clone(),
            workspace_id: context.workspace_id.clone(),
            ..Origin::default()
        };
        self.finish_children(&unit, &job.job_type, &origin, now);
        if moved_to_failed {
            let error = job.error.clone().unwrap_or_default();
            let fingerprint = failed_job_fingerprint(&job.job_type, &error);
            let occurrence = self.occurrence(
                &fingerprint,
                at,
                &context,
                Level::Error,
                error.clone(),
                error_lines(&error),
                &origin,
                Value::Null,
            );
            self.push_hit(
                Hit {
                    fingerprint,
                    kind: IssueKind::FailedJob,
                    level: Level::Error,
                    title: first_line(&format!("{} failed: {error}", job.job_type), 200),
                    culprit: job.job_type.clone(),
                    message: error.clone(),
                    chain: error_lines(&error),
                    location: None,
                    error_type: None,
                    at,
                    failed_job: true,
                    occurrence,
                },
                now,
            );
        }
        if let Some(id) = sample_id {
            let start = at - duration_delta(job.duration_ms);
            let mut details = details;
            if !unit.tags.is_empty() {
                details["tags"] = json!(unit.tags);
            }
            self.push_sample(NewSample {
                id,
                kind: "job",
                name: job.job_type.clone(),
                at,
                duration_ms: job.duration_ms,
                status: None,
                outcome: match (failed, job.will_retry) {
                    (false, _) => "ok",
                    (true, true) => "failed_attempt",
                    (true, false) => "failed",
                },
                slow,
                context,
                spans: unit.spans_from(start),
                details,
                error: job.error,
            });
        }
    }

    /// The queries, calls and exceptions of a finished unit.
    fn finish_children(&mut self, unit: &Unit, culprit: &str, origin: &Origin, now: DateTime<Utc>) {
        for query in &unit.queries {
            self.query_done(query.clone(), culprit, origin, now);
        }
        for call in &unit.calls {
            self.call_done(call.clone(), culprit, origin, now);
        }
        for exception in &unit.exceptions {
            let mut origin = origin.clone();
            for (key, value) in &unit.tags {
                origin.tags.entry(key.clone()).or_insert_with(|| value.clone());
            }
            self.exception_hit(exception.clone(), culprit, &origin, Some(unit), now);
        }
    }

    fn query_done(&mut self, query: PendingQuery, origin_name: &str, origin: &Origin, now: DateTime<Utc>) {
        self.aggregates.observe(
            now,
            kind::QUERY,
            &query.statement,
            &Observation {
                duration_ms: Some(query.duration_ms),
                error: query.failed,
                origin: Some(origin_name.to_owned()),
                ..Observation::default()
            },
        );
        if query.duration_ms < self.options.thresholds.slow_query_ms as f64 {
            return;
        }
        let fingerprint = slow_query_fingerprint(&query.statement);
        let message = format!("{} took {} ms", query.statement, query.duration_ms.round());
        let occurrence = self.occurrence(
            &fingerprint,
            query.at,
            &query.context,
            Level::Warning,
            message.clone(),
            vec![message.clone()],
            origin,
            json!({ "query": { "statement": query.statement, "duration_ms": query.duration_ms, "ran_by": origin_name } }),
        );
        self.push_hit(
            Hit {
                fingerprint,
                kind: IssueKind::SlowQuery,
                level: Level::Warning,
                title: format!("Slow query: {}", truncate(&query.statement, 160)),
                culprit: origin_name.to_owned(),
                message,
                chain: vec![query.statement.clone()],
                location: None,
                error_type: None,
                at: query.at,
                failed_job: false,
                occurrence,
            },
            now,
        );
    }

    fn call_done(&mut self, pending: PendingCall, origin_name: &str, origin: &Origin, now: DateTime<Utc>) {
        let call = &pending.call;
        let host = url_host(&call.url);
        let duration_ms = millis(call.duration);
        let failed = call.error.is_some() || call.status.is_some_and(|status| status >= 500);
        let slow = duration_ms >= self.options.thresholds.slow_outgoing_ms as f64;
        self.aggregates.observe(
            now,
            kind::OUTGOING,
            &host,
            &Observation {
                duration_ms: Some(duration_ms),
                error: failed || call.status.is_some_and(|status| status >= 400),
                slow_call: Some(SlowCall {
                    method: call.method.clone(),
                    url: call.url.clone(),
                    status: call.status,
                    duration_ms,
                }),
                origin: Some(origin_name.to_owned()),
                ..Observation::default()
            },
        );
        let failure = match (&call.error, call.status) {
            (Some(error), _) => outgoing_error_kind(error),
            (None, Some(status)) if status >= 500 => "5xx",
            _ if slow => "slow",
            _ => return,
        };
        let fingerprint = outgoing_fingerprint(&host, failure);
        let message = match (&call.error, call.status) {
            (Some(error), _) => format!(
                "{} {} failed after {} ms: {error}",
                call.method,
                call.url,
                duration_ms.round()
            ),
            (None, Some(status)) => format!(
                "{} {} answered {status} after {} ms",
                call.method,
                call.url,
                duration_ms.round()
            ),
            (None, None) => format!("{} {} took {} ms", call.method, call.url, duration_ms.round()),
        };
        let occurrence = self.occurrence(
            &fingerprint,
            pending.at,
            &pending.context,
            Level::Warning,
            message.clone(),
            vec![message.clone()],
            origin,
            json!({ "outgoing": {
                "method": call.method,
                "url": call.url,
                "status": call.status,
                "attempts": call.attempts,
                "duration_ms": duration_ms,
                "error": call.error,
                "called_by": origin_name,
            }}),
        );
        self.push_hit(
            Hit {
                fingerprint,
                kind: IssueKind::OutgoingRequest,
                level: Level::Warning,
                title: format!("{host}: {failure}"),
                culprit: host,
                message: message.clone(),
                chain: vec![message],
                location: None,
                error_type: None,
                at: pending.at,
                failed_job: false,
                occurrence,
            },
            now,
        );
    }

    fn exception_hit(
        &mut self,
        exception: PendingException,
        culprit: &str,
        origin: &Origin,
        _unit: Option<&Unit>,
        now: DateTime<Utc>,
    ) {
        let fingerprint = match (&exception.fingerprint, exception.panic, &exception.location) {
            (Some(parts), _, _) => custom_fingerprint(parts),
            (None, true, Some(location)) => panic_fingerprint(location, &exception.message),
            _ => exception_fingerprint(exception.error_type.as_deref(), culprit, &exception.chain),
        };
        let mut context = exception.context.clone();
        if let Some(user) = &exception.user {
            context.user_id = Some(user.clone());
        }
        if context.user_id.is_none() {
            context.user_id.clone_from(&origin.user_id);
        }
        if context.workspace_id.is_none() {
            context.workspace_id.clone_from(&origin.workspace_id);
        }
        let mut extra = Map::new();
        if !exception.values.is_empty() {
            extra.insert(
                "context".into(),
                self.redactor
                    .value(Value::Object(exception.values.clone().into_iter().collect())),
            );
        }
        let mut origin = origin.clone();
        for (key, value) in &exception.tags {
            origin.tags.insert(key.clone(), value.clone());
        }
        if exception.panic {
            extra.insert("panic".into(), Value::Bool(true));
        }
        if let Some(error_type) = &exception.error_type {
            extra.insert("error_type".into(), json!(error_type));
        }
        let mut occurrence = self.occurrence(
            &fingerprint,
            exception.at,
            &context,
            exception.level,
            exception.message.clone(),
            exception.chain.clone(),
            &origin,
            Value::Object(extra),
        );
        if let Some(occurrence) = &mut occurrence {
            occurrence.backtrace.clone_from(&exception.backtrace);
            occurrence.location.clone_from(&exception.location);
        }
        self.aggregates.observe(
            now,
            kind::EXCEPTION,
            "all",
            &Observation::default(),
        );
        self.push_hit(
            Hit {
                fingerprint,
                kind: IssueKind::Exception,
                level: exception.level,
                title: first_line(&exception.message, 200),
                culprit: culprit.to_owned(),
                message: exception.message,
                chain: exception.chain,
                location: exception.location,
                error_type: exception.error_type,
                at: exception.at,
                failed_job: false,
                occurrence,
            },
            now,
        );
    }

    /// The occurrence to store for a hit of `fingerprint`, or `None` when
    /// sampling or the buffer leaves it out.
    #[allow(clippy::too_many_arguments)]
    fn occurrence(
        &mut self,
        fingerprint: &str,
        at: DateTime<Utc>,
        context: &EventContext,
        level: Level,
        message: String,
        chain: Vec<String>,
        origin: &Origin,
        extra: Value,
    ) -> Option<NewOccurrence> {
        let id = Uuid::now_v7();
        let hour = at.timestamp() / 3600;
        let budget = self
            .budgets
            .entry(fingerprint.to_owned())
            .or_insert((hour, 0));
        if budget.0 != hour {
            *budget = (hour, 0);
        }
        let keep = if budget.1 < OCCURRENCES_PER_HOUR {
            true
        } else {
            sampled(&id.to_string(), self.options.sample.exceptions)
        };
        if !keep {
            return None;
        }
        budget.1 += 1;
        if self.details() >= self.options.buffer {
            if self.samples.pop().is_some() {
                self.handle.count_dropped(1);
            } else {
                self.handle.count_dropped(1);
                return None;
            }
        }
        self.stored_occurrences += 1;
        let mut details = match &origin.details {
            Value::Object(map) => map.clone(),
            _ => Map::new(),
        };
        if let Value::Object(extra) = extra {
            details.extend(extra);
        }
        if !origin.tags.is_empty() {
            details.insert("tags".into(), json!(origin.tags));
        }
        Some(NewOccurrence {
            id,
            at,
            level,
            message,
            chain,
            backtrace: None,
            location: None,
            request_id: context.request_id.clone(),
            job_id: context.job_id.clone(),
            trace_id: context.trace.as_ref().map(|trace| trace.trace_id.to_string()),
            user_id: context.user_id.clone(),
            workspace_id: context.workspace_id.clone(),
            method: origin.method.clone(),
            route: origin.route.clone(),
            path: origin.path.clone(),
            status: origin.status,
            sample_id: origin.sample_id,
            context: Value::Object(details),
        })
    }

    fn push_hit(&mut self, hit: Hit, now: DateTime<Utc>) {
        self.aggregates.observe(
            now,
            kind::ISSUE,
            &hit.fingerprint,
            &Observation::default(),
        );
        self.hits.push(hit);
    }

    fn push_sample(&mut self, sample: NewSample) {
        if self.details() >= self.options.buffer {
            self.handle.count_dropped(1);
            return;
        }
        self.samples.push(sample);
    }

    fn ignored_path(&self, path: &str) -> bool {
        self.ignored_paths
            .iter()
            .any(|pattern| path_matches(pattern, path))
    }

    /// The hits waiting (for tests).
    #[cfg(test)]
    pub(crate) fn pending_hits(&self) -> &[Hit] {
        &self.hits
    }

    /// The samples waiting (for tests).
    #[cfg(test)]
    pub(crate) fn pending_samples(&self) -> &[NewSample] {
        &self.samples
    }
}

/// Where a unit's children happened.
#[derive(Clone, Debug, Default)]
struct Origin {
    method: Option<String>,
    route: Option<String>,
    path: Option<String>,
    status: Option<u16>,
    sample_id: Option<Uuid>,
    tags: BTreeMap<String, String>,
    details: Value,
    user_id: Option<String>,
    workspace_id: Option<String>,
}

/// The end of a job attempt.
struct JobEnd {
    queue: String,
    job_type: String,
    job_id: String,
    attempt: u32,
    duration_ms: f64,
    error: Option<String>,
    will_retry: bool,
}

impl JobEnd {
    fn processed(job: &JobProcessed) -> JobEnd {
        JobEnd {
            queue: job.queue.clone(),
            job_type: job.job_type.clone(),
            job_id: job.job_id.clone(),
            attempt: job.attempt,
            duration_ms: millis(job.duration),
            error: None,
            will_retry: false,
        }
    }

    fn failed(job: &JobFailed) -> JobEnd {
        JobEnd {
            queue: job.queue.clone(),
            job_type: job.job_type.clone(),
            job_id: job.job_id.clone(),
            attempt: job.attempt,
            duration_ms: millis(job.duration),
            error: Some(job.error.clone()),
            will_retry: job.will_retry,
        }
    }
}

/// An error chain written as `error: source: source` split into lines.
fn error_lines(error: &str) -> Vec<String> {
    vec![error.to_owned()]
}

/// A duration in fractional milliseconds.
pub(crate) fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn duration_delta(ms: f64) -> TimeDelta {
    TimeDelta::microseconds((ms * 1000.0) as i64)
}

/// Whether `key` falls in the sampled fraction `rate` (deterministic: the
/// same key is always in or out).
pub(crate) fn sampled(key: &str, rate: f64) -> bool {
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (hash as f64 / u64::MAX as f64) < rate
}

#[cfg(test)]
mod tests {
    use navree_telemetry::{CacheEvent, CacheOperation};

    use super::*;

    fn processor(options: LighthouseOptions) -> Processor {
        let (handle, _rx) = EngineHandle::new(10);
        Processor::new(
            Arc::new(options),
            Labels {
                environment: "test".into(),
                release: "r1".into(),
                server: "host".into(),
                process: "serve".into(),
                process_key: "host/serve/x".into(),
            },
            Redactor::new(&[]),
            handle,
        )
    }

    fn record(context: EventContext, event: TelemetryEvent) -> Input {
        Input::Record(Box::new(TelemetryRecord {
            at: Utc::now(),
            context,
            event,
        }))
    }

    fn in_request(id: &str) -> EventContext {
        EventContext {
            request_id: Some(id.into()),
            ..EventContext::default()
        }
    }

    fn request(id: &str, route: &str, status: u16, ms: u64) -> TelemetryEvent {
        TelemetryEvent::RequestHandled(RequestHandled {
            method: "GET".into(),
            route: Some(route.into()),
            path: route.into(),
            status,
            duration: Duration::from_millis(ms),
            user_id: Some("u1".into()),
            workspace_id: None,
            request_id: id.into(),
            client_ip: Some("10.0.0.1".into()),
        })
    }

    fn query(sql: &str, ms: u64) -> TelemetryEvent {
        TelemetryEvent::QueryExecuted(QueryExecuted {
            sql: sql.into(),
            duration: Duration::from_millis(ms),
            rows: None,
            connection: "sqlite".into(),
            failed: false,
        })
    }

    fn exception(message: &str) -> TelemetryEvent {
        TelemetryEvent::ExceptionReported(ExceptionReported {
            message: message.into(),
            chain: vec![message.into()],
            location: None,
            panic: false,
            request: None,
        })
    }

    #[test]
    fn a_slow_request_keeps_its_trace_and_its_slow_queries() {
        let mut p = processor(LighthouseOptions::default());
        let now = Utc::now();
        for _ in 0..3 {
            p.handle(record(in_request("r1"), query("SELECT * FROM contacts WHERE id = $1", 300)), now);
        }
        p.handle(
            record(
                in_request("r1"),
                TelemetryEvent::Cache(CacheEvent {
                    operation: CacheOperation::Hit,
                    key: "dashboard:wins".into(),
                    store: "memory".into(),
                }),
            ),
            now,
        );
        p.handle(record(in_request("r1"), request("r1", "/api/v1/deals/board", 200, 1412)), now);
        let kinds: Vec<IssueKind> = p.pending_hits().iter().map(|hit| hit.kind).collect();
        assert_eq!(
            kinds,
            [IssueKind::SlowQuery, IssueKind::SlowQuery, IssueKind::SlowQuery, IssueKind::SlowRequest]
        );
        let slow_query = &p.pending_hits()[0];
        assert_eq!(slow_query.culprit, "GET /api/v1/deals/board");
        assert_eq!(slow_query.fingerprint, p.pending_hits()[1].fingerprint);
        let sample = &p.pending_samples()[0];
        assert!(sample.slow);
        assert_eq!(sample.name, "GET /api/v1/deals/board");
        assert_eq!(sample.spans.len(), 4);
        assert_eq!(sample.spans.iter().filter(|s| s.kind == "db").count(), 3);
        assert_eq!(
            p.pending_hits()[3].occurrence.as_ref().unwrap().sample_id,
            Some(sample.id)
        );
        assert!(p.units.is_empty());
    }

    #[test]
    fn exceptions_take_the_route_of_their_request() {
        let mut p = processor(LighthouseOptions::default());
        let now = Utc::now();
        p.handle(record(in_request("r2"), exception("deal 42 not found")), now);
        p.handle(record(in_request("r2"), request("r2", "/api/v1/deals/{id}", 500, 5)), now);
        let hit = &p.pending_hits()[0];
        assert_eq!(hit.kind, IssueKind::Exception);
        assert_eq!(hit.culprit, "GET /api/v1/deals/{id}");
        let occurrence = hit.occurrence.as_ref().unwrap();
        assert_eq!(occurrence.route.as_deref(), Some("/api/v1/deals/{id}"));
        assert_eq!(occurrence.user_id.as_deref(), Some("u1"));
        assert_eq!(occurrence.context["request"]["ip"], "10.0.0.1");
        assert_eq!(p.pending_samples().len(), 1, "a 500 keeps its trace");
    }

    #[test]
    fn ignored_routes_and_the_dashboard_are_left_out() {
        let mut p = processor(LighthouseOptions::default());
        let now = Utc::now();
        p.handle(record(in_request("a"), exception("boom")), now);
        p.handle(record(in_request("a"), request("a", "/health/live", 500, 5)), now);
        p.handle(record(in_request("b"), request("b", "/_lighthouse/issues", 200, 5000)), now);
        assert!(p.pending_hits().is_empty());
        assert!(p.pending_samples().is_empty());
        assert!(p.aggregates.take_dirty().is_empty());
    }

    #[test]
    fn sampling_keeps_counts_exact() {
        let mut options = LighthouseOptions::default();
        options.sample.requests = 0.0;
        let mut p = processor(options);
        let now = Utc::now();
        for i in 0..50 {
            let id = format!("q{i}");
            p.handle(record(in_request(&id), request(&id, "/x", 200, 3)), now);
        }
        assert!(p.pending_samples().is_empty());
        let rows = p.aggregates.take_dirty();
        let minute = rows
            .iter()
            .find(|row| row.key.kind == kind::REQUEST && row.key.resolution == 60)
            .unwrap();
        assert_eq!(minute.values.count, 50);
    }

    #[test]
    fn the_first_hundred_occurrences_per_hour_are_kept() {
        let mut options = LighthouseOptions::default();
        options.sample.exceptions = 0.0;
        let mut p = processor(options);
        let now = Utc::now();
        for _ in 0..150 {
            p.handle(record(EventContext::default(), exception("disk full")), now);
        }
        assert_eq!(p.pending_hits().len(), 150, "every occurrence is counted");
        let stored = p
            .pending_hits()
            .iter()
            .filter(|hit| hit.occurrence.is_some())
            .count();
        assert_eq!(stored, 100);
        assert_eq!(p.pending_hits()[0].culprit, "serve");
    }

    #[test]
    fn a_full_buffer_drops_samples_before_occurrences() {
        let mut options = LighthouseOptions::default();
        options.buffer = 100;
        let mut p = processor(options);
        let now = Utc::now();
        for i in 0..100 {
            let id = format!("s{i}");
            p.handle(record(in_request(&id), request(&id, "/x", 200, 3)), now);
        }
        assert_eq!(p.pending_samples().len(), 100);
        for _ in 0..10 {
            p.handle(record(EventContext::default(), exception("boom")), now);
        }
        assert_eq!(p.pending_samples().len(), 90, "samples made room");
        assert_eq!(p.stored_occurrences, 10);
        assert_eq!(p.handle.dropped(), 10);
    }

    #[test]
    fn jobs_record_failures_and_failed_jobs() {
        let mut p = processor(LighthouseOptions::default());
        let now = Utc::now();
        let in_job = EventContext {
            job_id: Some("j1".into()),
            ..EventContext::default()
        };
        let failed = |will_retry| {
            TelemetryEvent::JobFailed(JobFailed {
                queue: "default".into(),
                job_type: "invoices.sync".into(),
                job_id: "j1".into(),
                attempt: 1,
                duration: Duration::from_millis(20),
                error: "provider timed out after 30 s".into(),
                will_retry,
            })
        };
        p.handle(record(in_job.clone(), query("SELECT 1", 2)), now);
        p.handle(record(in_job.clone(), failed(true)), now);
        assert!(p.pending_hits().is_empty(), "an attempt to retry is no issue");
        assert_eq!(p.pending_samples()[0].outcome, "failed_attempt");
        p.handle(record(in_job.clone(), failed(false)), now);
        let hit = &p.pending_hits()[0];
        assert_eq!(hit.kind, IssueKind::FailedJob);
        assert!(hit.failed_job);
        assert_eq!(hit.culprit, "invoices.sync");
        p.handle(
            record(
                EventContext {
                    job_id: Some("j2".into()),
                    ..EventContext::default()
                },
                TelemetryEvent::JobFailed(JobFailed {
                    job_type: ALERT_JOB.into(),
                    job_id: "j2".into(),
                    will_retry: false,
                    queue: "default".into(),
                    attempt: 5,
                    duration: Duration::ZERO,
                    error: "slack down".into(),
                }),
            ),
            now,
        );
        assert_eq!(p.pending_hits().len(), 1, "alert deliveries are never recorded");
    }

    #[test]
    fn internal_work_is_not_recorded() {
        let mut p = processor(LighthouseOptions::default());
        let internal = EventContext {
            job_id: Some(INTERNAL_UNIT.into()),
            ..EventContext::default()
        };
        p.handle(record(internal, query("INSERT INTO lighthouse_issues", 900)), Utc::now());
        assert!(p.pending_hits().is_empty());
        assert!(p.aggregates.take_dirty().is_empty());
    }

    #[test]
    fn sampling_is_deterministic_and_proportional() {
        assert!(sampled("x", 1.0));
        assert!(!sampled("x", 0.0));
        assert_eq!(sampled("abc", 0.5), sampled("abc", 0.5));
        let kept = (0..10_000).filter(|i| sampled(&format!("req-{i}"), 0.1)).count();
        assert!((800..1200).contains(&kept), "{kept}");
    }
}
