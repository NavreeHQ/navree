//! The watchers: telemetry records and data changes turned into entries.

use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_db::{ChangeEvent, ChangeObserver, DbError, Uow};
use navree_kernel::BoxFuture;
use navree_telemetry::{EventContext, Telemetry, TelemetryEvent, TelemetryRecord};

use crate::logbook::Active;
use crate::{
    CacheEntry, Content, Entry, EventEntry, ExceptionEntry, HttpClientEntry, JobEntry, JobOutcome,
    LogEntry, Logbook, MailEntry, ModelEntry, NotificationEntry, QueryEntry, RequestEntry,
    ScheduleEntry,
};

/// Log lines of these targets are never recorded: Logbook's own lines.
const OWN_TARGET: &str = "navree::logbook";

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// When something that ended at `end` after `duration` started.
fn started(end: DateTime<Utc>, duration: Duration) -> DateTime<Utc> {
    end - chrono::Duration::from_std(duration).unwrap_or_default()
}

/// What one record becomes, before the context is applied.
struct Recorded {
    content: Content,
    at: DateTime<Utc>,
    duration_ms: Option<f64>,
    failed: bool,
    slow: bool,
    tags: Vec<String>,
    /// The batch this entry heads (requests, jobs).
    heads: Option<String>,
}

impl Recorded {
    fn new(content: Content, at: DateTime<Utc>) -> Recorded {
        Recorded {
            content,
            at,
            duration_ms: None,
            failed: false,
            slow: false,
            tags: Vec::new(),
            heads: None,
        }
    }

    fn lasting(mut self, end: DateTime<Utc>, duration: Duration) -> Recorded {
        self.at = started(end, duration);
        self.duration_ms = Some(ms(duration));
        self
    }
}

impl Active {
    /// The batch an entry recorded in `context` belongs to: the request,
    /// else the job, else (a scheduled run) the trace, each only when its
    /// watcher is on.
    pub(crate) fn batch_of(&self, context: &EventContext) -> Option<String> {
        let watchers = &self.options.watchers;
        if let Some(id) = &context.request_id {
            return watchers.requests.then(|| id.clone());
        }
        if let Some(id) = &context.job_id {
            return watchers.jobs.then(|| id.clone());
        }
        context
            .trace
            .as_ref()
            .filter(|_| watchers.schedule)
            .map(|trace| trace.trace_id.to_string())
    }

    /// Sets the batch, user, workspace and trace of `entry` from `context`.
    pub(crate) fn apply_context(&self, entry: &mut Entry, context: &EventContext) {
        entry.batch_id = self.batch_of(context);
        entry.user_id.clone_from(&context.user_id);
        entry.workspace_id.clone_from(&context.workspace_id);
        entry.trace_id = context
            .trace
            .as_ref()
            .map(|trace| trace.trace_id.to_string());
    }

    /// Records one telemetry record.
    pub(crate) fn record(&self, record: &TelemetryRecord) {
        if crate::logbook::in_own_work() {
            return;
        }
        let context = &record.context;
        if self.options.ignore.framework
            && crate::framework::is_unscoped(context)
            && matches!(&record.event, TelemetryEvent::QueryExecuted(query)
                if crate::framework::is_framework_statement(&query.sql))
        {
            return;
        }
        if context
            .request_id
            .as_deref()
            .is_some_and(|id| self.is_ignored(id))
        {
            return;
        }
        let Some(recorded) = self.recorded(record) else {
            return;
        };
        let mut entry = Entry::new(recorded.at, recorded.content);
        self.apply_context(&mut entry, context);
        if let Some(batch) = recorded.heads {
            entry.batch_id = Some(batch);
        }
        // A request's own event knows its user and workspace best.
        if let TelemetryEvent::RequestHandled(event) = &record.event {
            if event.user_id.is_some() {
                entry.user_id.clone_from(&event.user_id);
            }
            if event.workspace_id.is_some() {
                entry.workspace_id.clone_from(&event.workspace_id);
            }
        }
        entry.duration_ms = recorded.duration_ms;
        entry.failed = recorded.failed;
        entry.slow = recorded.slow;
        entry.tags = recorded.tags;
        if entry.kind().is_batch() {
            if let Some(user) = &entry.user_id {
                let tag = format!("user:{user}");
                if !entry.tags.contains(&tag) {
                    entry.tags.insert(0, tag);
                }
            }
            if let Some(workspace) = &entry.workspace_id {
                let tag = format!("workspace:{workspace}");
                if !entry.tags.contains(&tag) {
                    entry.tags.push(tag);
                }
            }
        }
        self.send(entry);
    }

    /// The content of one record, or `None` when it is not recorded.
    fn recorded(&self, record: &TelemetryRecord) -> Option<Recorded> {
        let at = record.at;
        Some(match &record.event {
            TelemetryEvent::RequestHandled(e) => {
                if self.ignores_path(&e.path) {
                    self.mark_ignored(&e.request_id);
                    return None;
                }
                let capture = self.take_capture(&e.request_id);
                let replay_of = self.take_replay(&e.request_id);
                let mut tags = Vec::new();
                if let Some(route) = &e.route {
                    tags.push(format!("route:{route}"));
                }
                tags.push(format!("status:{}", e.status));
                if replay_of.is_some() {
                    tags.push("replay".to_owned());
                }
                let (request, response) = match capture {
                    Some(capture) => (Some(capture.request), Some(capture.response)),
                    None => (None, None),
                };
                let mut recorded = Recorded::new(
                    Content::Request(RequestEntry {
                        method: e.method.clone(),
                        path: e.path.clone(),
                        route: e.route.clone(),
                        status: e.status,
                        request_id: e.request_id.clone(),
                        client_ip: e.client_ip.clone(),
                        duration_ms: ms(e.duration),
                        request,
                        response,
                        replay_of,
                        summary: Default::default(),
                    }),
                    at,
                )
                .lasting(at, e.duration);
                recorded.failed = e.status >= 500;
                recorded.tags = tags;
                recorded.heads = Some(e.request_id.clone());
                recorded
            }
            TelemetryEvent::QueryExecuted(e) => {
                let duration_ms = ms(e.duration);
                let slow = duration_ms > self.options.slow_query_ms as f64;
                let mut recorded = Recorded::new(
                    Content::Query(QueryEntry {
                        sql: e.sql.clone(),
                        connection: e.connection.clone(),
                        rows: e.rows,
                        duration_ms,
                        failed: e.failed,
                        slow,
                    }),
                    at,
                )
                .lasting(at, e.duration);
                recorded.failed = e.failed;
                recorded.slow = slow;
                recorded
            }
            TelemetryEvent::JobProcessed(e) => self.job(
                at,
                e.duration,
                JobPart {
                    queue: &e.queue,
                    job_type: &e.job_type,
                    job_id: &e.job_id,
                    attempt: e.attempt,
                    outcome: JobOutcome::Processed,
                    error: None,
                },
            ),
            TelemetryEvent::JobFailed(e) => {
                let outcome = if e.will_retry {
                    JobOutcome::Released
                } else {
                    JobOutcome::Failed
                };
                self.job(
                    at,
                    e.duration,
                    JobPart {
                        queue: &e.queue,
                        job_type: &e.job_type,
                        job_id: &e.job_id,
                        attempt: e.attempt,
                        outcome,
                        error: Some(&e.error),
                    },
                )
            }
            TelemetryEvent::ScheduledTaskRan(e) => {
                let mut recorded = Recorded::new(
                    Content::Schedule(ScheduleEntry {
                        task: e.task.clone(),
                        tick: e.tick,
                        error: e.error.clone(),
                        duration_ms: ms(e.duration),
                        summary: Default::default(),
                    }),
                    at,
                )
                .lasting(at, e.duration);
                recorded.failed = e.error.is_some();
                recorded.tags = vec![format!("schedule:{}", e.task)];
                recorded
            }
            TelemetryEvent::MailSent(e) => Recorded::new(
                Content::Mail(MailEntry {
                    from: e.from.clone(),
                    to: e.to.clone(),
                    cc: e.cc.clone(),
                    bcc: e.bcc.clone(),
                    subject: e.subject.clone(),
                    text: e.text.clone(),
                    html: e.html.clone(),
                    transport: e.transport.clone(),
                }),
                at,
            )
            .lasting(at, e.duration),
            TelemetryEvent::NotificationSent(e) => Recorded::new(
                Content::Notification(NotificationEntry {
                    notification_id: e.notification_id.clone(),
                    notification_type: e.notification_type.clone(),
                    notifiable_type: e.notifiable_type.clone(),
                    notifiable_id: e.notifiable_id.clone(),
                    channel: e.channel.clone(),
                }),
                at,
            ),
            TelemetryEvent::Cache(e) => Recorded::new(
                Content::Cache(CacheEntry {
                    operation: e.operation.as_str().to_owned(),
                    key: e.key.clone(),
                    store: e.store.clone(),
                }),
                at,
            ),
            TelemetryEvent::HttpClientRequest(e) => {
                let mut recorded = Recorded::new(
                    Content::HttpClient(HttpClientEntry {
                        method: e.method.clone(),
                        url: e.url.clone(),
                        status: e.status,
                        attempts: e.attempts,
                        error: e.error.clone(),
                    }),
                    at,
                )
                .lasting(at, e.duration);
                recorded.failed = e.error.is_some() || e.status.is_some_and(|s| s >= 500);
                recorded
            }
            TelemetryEvent::ExceptionReported(e) => Recorded::new(
                Content::Exception(ExceptionEntry {
                    message: e.message.clone(),
                    chain: e.chain.clone(),
                    location: e.location.clone(),
                    panic: e.panic,
                }),
                at,
            ),
            TelemetryEvent::Log(e) => {
                if e.level < self.options.min_log_level() || e.target.starts_with(OWN_TARGET) {
                    return None;
                }
                Recorded::new(
                    Content::Log(LogEntry {
                        level: e.level.as_str().to_owned(),
                        target: e.target.clone(),
                        message: e.message.clone(),
                        fields: e.fields.clone(),
                    }),
                    at,
                )
            }
            TelemetryEvent::EventEmitted(e) => Recorded::new(
                Content::Event(EventEntry {
                    name: e.name.clone(),
                    payload: e.payload.clone(),
                    listeners: e.listeners.clone(),
                    queued: e.queued.clone(),
                    failed: e.failed.clone(),
                }),
                at,
            )
            .lasting(at, e.duration),
            _ => return None,
        })
    }

    fn job(&self, at: DateTime<Utc>, duration: Duration, part: JobPart<'_>) -> Recorded {
        let mut recorded = Recorded::new(
            Content::Job(JobEntry {
                queue: part.queue.to_owned(),
                job_type: part.job_type.to_owned(),
                job_id: part.job_id.to_owned(),
                attempt: part.attempt,
                outcome: part.outcome,
                error: part.error.map(str::to_owned),
                duration_ms: ms(duration),
                summary: Default::default(),
            }),
            at,
        )
        .lasting(at, duration);
        recorded.failed = part.outcome != JobOutcome::Processed;
        recorded.tags = vec![
            format!("job:{}", part.job_type),
            format!("queue:{}", part.queue),
        ];
        recorded.heads = Some(part.job_id.to_owned());
        recorded
    }

    /// Records the changes of one unit of work.
    fn record_changes(&self, uow: &Uow, changes: &[ChangeEvent]) {
        let mut context = Telemetry::current_context();
        if context.request_id.is_none() && context.job_id.is_none() {
            context.request_id = uow.request_id().map(str::to_owned);
        }
        if context.user_id.is_none() {
            context.user_id = uow.actor().map(|actor| actor.to_string());
        }
        if context
            .request_id
            .as_deref()
            .is_some_and(|id| self.is_ignored(id))
        {
            return;
        }
        for change in changes {
            let mut entry = Entry::new(
                Utc::now(),
                Content::Model(ModelEntry {
                    entity: change.entity.clone(),
                    id: change.id.clone(),
                    op: change.op.as_str().to_owned(),
                    old: change.old.clone(),
                    new: change.new.clone(),
                }),
            );
            self.apply_context(&mut entry, &context);
            entry.tags = vec![model_tag(&change.entity, &change.id)];
            self.send(entry);
        }
    }
}

/// The parts of a job event.
struct JobPart<'a> {
    queue: &'a str,
    job_type: &'a str,
    job_id: &'a str,
    attempt: u32,
    outcome: JobOutcome,
    error: Option<&'a str>,
}

/// The tag of a changed record: the entity in the singular and its id
/// (`deals` and `0199c1d2-..` give `deal:0199c1d2-..`).
pub(crate) fn model_tag(entity: &str, id: &str) -> String {
    format!("{}:{id}", singular(entity))
}

/// A table name in the singular: `companies` -> `company`, `deals` ->
/// `deal`, `addresses` -> `address`; names ending in `ss` or `us` stay as
/// they are.
pub(crate) fn singular(entity: &str) -> String {
    if let Some(stem) = entity.strip_suffix("ies")
        && !stem.is_empty()
    {
        return format!("{stem}y");
    }
    for suffix in ["sses", "shes", "ches", "xes"] {
        if entity.ends_with(suffix) {
            return entity[..entity.len() - 2].to_owned();
        }
    }
    match entity.strip_suffix('s') {
        Some(stem) if !stem.is_empty() && !stem.ends_with(['s', 'u']) => stem.to_owned(),
        _ => entity.to_owned(),
    }
}

impl ChangeObserver for Logbook {
    fn observe<'a>(
        &'a self,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
        if let Some(active) = self.active()
            && active.options.watchers.models
        {
            active.record_changes(uow, changes);
        }
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_become_singular_tags() {
        assert_eq!(singular("deals"), "deal");
        assert_eq!(singular("companies"), "company");
        assert_eq!(singular("activities"), "activity");
        assert_eq!(singular("addresses"), "address");
        assert_eq!(singular("boxes"), "box");
        assert_eq!(singular("status"), "status");
        assert_eq!(singular("news"), "new");
        assert_eq!(singular("person"), "person");
        assert_eq!(model_tag("deals", "0199"), "deal:0199");
    }
}
