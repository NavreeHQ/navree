//! [`Scheduler`]: decides which tasks are due and runs them.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, DurationRound, TimeDelta, Timelike, Utc};
use croner::Cron;
use croner::parser::{CronParser, Seconds};
use futures_util::future::join_all;
use keel_contracts::Clock;
use keel_db::Db;
use keel_kernel::{AppContext, BoxError, CommandFn, WeakAppContext};
use tokio::sync::Mutex;
use tokio::task::JoinError;
use tokio_util::sync::CancellationToken;

use crate::schedule::{Schedule, TaskFn, TaskKind};
use crate::{ScheduleError, store};

/// The outcome of one due task in a [`Scheduler::tick`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOutcome {
    /// The task name.
    pub task: String,
    /// The tick (the minute the task was due at).
    pub tick: DateTime<Utc>,
    /// What happened.
    pub status: RunStatus,
}

/// What happened to a due task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
    /// It ran and returned `Ok`.
    Succeeded,
    /// It returned an error or panicked (the message, also stored in
    /// `schedule_runs.error`).
    Failed(String),
    /// It was due but did not run.
    Skipped(SkipReason),
}

/// Why a due task did not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// Another scheduler (or an earlier tick call) already claimed this tick.
    AlreadyClaimed,
    /// The task is marked `without_overlapping` and an earlier run is still running.
    Overlapping,
}

/// A scheduled task as reported by [`Scheduler::list`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskInfo {
    /// The task name.
    pub name: String,
    /// The module that scheduled it.
    pub module: &'static str,
    /// The cron expression (UTC).
    pub expr: String,
    /// The first due minute after the time given to `list`.
    pub next_run: Option<DateTime<Utc>>,
    /// Whether it skips ticks while an earlier run is still running.
    pub without_overlapping: bool,
}

/// Runs the tasks every module scheduled (see
/// [`ModuleBuilderScheduleExt`](crate::ModuleBuilderScheduleExt)).
///
/// Provided and exported by `ScheduleModule`. Ticks are whole minutes, UTC.
pub struct Scheduler {
    app: WeakAppContext,
    db: Arc<Db>,
    clock: Arc<dyn Clock>,
    tasks: Vec<Arc<Task>>,
}

/// A validated task.
struct Task {
    name: String,
    module: &'static str,
    expr: String,
    cron: Cron,
    body: Body,
    without_overlapping: bool,
    /// Held while a run of a `without_overlapping` task is in progress.
    running: Arc<Mutex<()>>,
}

impl Scheduler {
    /// Collects and validates the schedules contributed to `app`.
    pub(crate) fn new(
        app: &AppContext,
        db: Arc<Db>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, ScheduleError> {
        let parser = CronParser::builder().seconds(Seconds::Disallowed).build();
        let mut tasks = Vec::new();
        let mut owners: HashMap<String, &'static str> = HashMap::new();
        let commands = app.commands();
        for contributed in app.contributions::<Schedule>() {
            let module = contributed.module.name;
            for def in &contributed.value.tasks {
                if def.name.is_empty() || def.name.len() > MAX_NAME_LEN {
                    return Err(ScheduleError::InvalidName {
                        task: def.name.clone(),
                        module,
                    });
                }
                if let Some(first) = owners.insert(def.name.clone(), module) {
                    return Err(ScheduleError::DuplicateTask {
                        task: def.name.clone(),
                        first,
                        second: module,
                    });
                }
                let invalid = |reason: String| ScheduleError::InvalidSchedule {
                    task: def.name.clone(),
                    module,
                    expr: def.expr.clone(),
                    reason,
                };
                if let Some(reason) = &def.invalid {
                    return Err(invalid(reason.clone()));
                }
                let cron = parser
                    .parse(&def.expr)
                    .map_err(|error| invalid(error.to_string()))?;
                let body = match &def.kind {
                    TaskKind::Run(f) => Body::Run(f.clone()),
                    TaskKind::Command { name, args } => {
                        let spec = commands.iter().find(|c| c.name == name).ok_or_else(|| {
                            ScheduleError::UnknownCommand {
                                task: def.name.clone(),
                                module,
                                command: name.clone(),
                            }
                        })?;
                        Body::Command(spec.run.clone(), args.clone())
                    }
                };
                tasks.push(Arc::new(Task {
                    name: def.name.clone(),
                    module,
                    expr: def.expr.clone(),
                    cron,
                    body,
                    without_overlapping: def.without_overlapping,
                    running: Arc::new(Mutex::new(())),
                }));
            }
        }
        Ok(Self {
            app: app.downgrade(),
            db,
            clock,
            tasks,
        })
    }

    /// Runs every task due at the minute of `now` and waits for them;
    /// tasks that are not due are left out of the result.
    ///
    /// Each due task first claims the tick in `schedule_runs`; only the
    /// scheduler whose insert succeeded runs it. Due tasks run concurrently,
    /// each in its own task on the app's tracker (a panic becomes a failed
    /// run). Fails only when the app is gone or a tick cannot be claimed.
    pub async fn tick(&self, now: DateTime<Utc>) -> Result<Vec<RunOutcome>, ScheduleError> {
        let app = self.app.upgrade().ok_or(ScheduleError::AppDropped)?;
        let tick = minute_of(now);
        if tick.minute() == 0 {
            self.prune(tick).await;
        }
        let due = self
            .tasks
            .iter()
            .filter(|task| task.cron.is_time_matching(&tick).unwrap_or(false))
            .map(|task| self.run_due(&app, task.clone(), tick));
        join_all(due).await.into_iter().collect()
    }

    /// The current time on the scheduler's clock (the app's `dyn Clock`,
    /// else the system clock).
    pub fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// Ticks at every minute boundary until `shutdown` is cancelled (what
    /// `schedule:work` runs). Each tick runs in the background on the app's
    /// task tracker, so a slow task never delays the next tick and graceful
    /// shutdown waits for running tasks. After a stall (a suspended machine)
    /// missed minutes are skipped, not caught up.
    pub async fn work(self: Arc<Self>, shutdown: CancellationToken) -> Result<(), ScheduleError> {
        let tracker = self
            .app
            .upgrade()
            .ok_or(ScheduleError::AppDropped)?
            .tracker();
        let minute = TimeDelta::minutes(1);
        let mut last: Option<DateTime<Utc>> = None;
        loop {
            let now = self.clock.now();
            let mut next = minute_of(now) + minute;
            if let Some(last) = last {
                next = next.max(last + minute);
            }
            let wait = (next - now).to_std().unwrap_or_default();
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = tokio::time::sleep(wait) => {}
            }
            let tick = next.max(minute_of(self.clock.now()));
            last = Some(tick);
            let scheduler = self.clone();
            tracker.spawn(async move {
                if let Err(error) = scheduler.tick(tick).await {
                    tracing::error!(target: "keel::schedule", %tick, %error, "scheduler tick failed");
                }
            });
        }
    }

    /// Every task, in registration order (modules in dependency order),
    /// with its next due minute after `now`.
    pub fn list(&self, now: DateTime<Utc>) -> Vec<TaskInfo> {
        self.tasks
            .iter()
            .map(|task| TaskInfo {
                name: task.name.clone(),
                module: task.module,
                expr: task.expr.clone(),
                next_run: task.cron.find_next_occurrence(&now, false).ok(),
                without_overlapping: task.without_overlapping,
            })
            .collect()
    }

    /// Deletes runs older than [`RUN_RETENTION`] (once an hour, from `tick`).
    async fn prune(&self, tick: DateTime<Utc>) {
        match store::prune(self.db.conn(), tick - RUN_RETENTION).await {
            Ok(0) => {}
            Ok(deleted) => {
                tracing::debug!(target: "keel::schedule", deleted, "pruned old schedule runs");
            }
            Err(error) => {
                tracing::warn!(target: "keel::schedule", %error, "cannot prune old schedule runs");
            }
        }
    }

    /// Claims, runs and records one due task.
    async fn run_due(
        &self,
        app: &AppContext,
        task: Arc<Task>,
        tick: DateTime<Utc>,
    ) -> Result<RunOutcome, ScheduleError> {
        let conn = self.db.conn();
        let outcome = |status| RunOutcome {
            task: task.name.clone(),
            tick,
            status,
        };
        let mut overlap_guard = None;
        if task.without_overlapping {
            let since = self.clock.now() - OVERLAP_EXPIRY;
            let guard = task.running.clone().try_lock_owned().ok();
            if guard.is_none() || store::running_elsewhere(conn, &task.name, tick, since).await? {
                tracing::debug!(target: "keel::schedule", task = %task.name, %tick, "still running, skipped");
                return Ok(outcome(RunStatus::Skipped(SkipReason::Overlapping)));
            }
            overlap_guard = guard;
        }
        if !store::claim(conn, &task.name, tick, self.clock.now()).await? {
            tracing::debug!(target: "keel::schedule", task = %task.name, %tick, "tick already claimed, skipped");
            return Ok(outcome(RunStatus::Skipped(SkipReason::AlreadyClaimed)));
        }

        let started = Instant::now();
        let run = execute(task.clone(), app.clone(), tick);
        let handle = app.tracker().spawn(async move {
            // Released when the run ends, even if this tick is dropped meanwhile.
            let _overlap_guard = overlap_guard;
            run.await
        });
        let error = match handle.await {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error_chain(&*error)),
            Err(join) => Some(panic_message(join)),
        };
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let stored = error.as_deref().map(|e| truncate(e, MAX_ERROR_LEN));
        if let Err(db_error) = store::finish(conn, &task.name, tick, self.clock.now(), stored).await
        {
            tracing::warn!(target: "keel::schedule", task = %task.name, %tick, error = %db_error, "cannot record the end of a scheduled run");
        }
        Ok(match error {
            None => {
                tracing::info!(target: "keel::schedule", task = %task.name, %tick, duration_ms, outcome = "succeeded", "scheduled task finished");
                outcome(RunStatus::Succeeded)
            }
            Some(error) => {
                tracing::error!(target: "keel::schedule", task = %task.name, %tick, duration_ms, outcome = "failed", %error, "scheduled task failed");
                outcome(RunStatus::Failed(error))
            }
        })
    }
}

impl fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scheduler")
            .field(
                "tasks",
                &self.tasks.iter().map(|t| &t.name).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

/// `now` truncated to the minute.
fn minute_of(now: DateTime<Utc>) -> DateTime<Utc> {
    now.duration_trunc(TimeDelta::minutes(1)).unwrap_or(now)
}

/// What a validated task runs.
enum Body {
    /// A function or job task.
    Run(TaskFn),
    /// A registered command and its arguments.
    Command(CommandFn, Vec<String>),
}

/// Runs a task's body.
async fn execute(task: Arc<Task>, app: AppContext, tick: DateTime<Utc>) -> Result<(), BoxError> {
    match &task.body {
        Body::Run(f) => f(app, format!("{}@{}", task.name, tick.to_rfc3339())).await,
        Body::Command(run, args) => run(app, args.clone()).await,
    }
}

/// `schedule_runs` rows are kept this long (pruned at the top of each hour).
pub const RUN_RETENTION: TimeDelta = TimeDelta::days(7);

/// A `running` row older than this no longer blocks a `without_overlapping`
/// task (its process probably crashed).
const OVERLAP_EXPIRY: TimeDelta = TimeDelta::hours(24);

/// The longest task name (`schedule_runs.task` is `VARCHAR(191)`).
const MAX_NAME_LEN: usize = 191;

/// Stored error messages are cut to this many bytes.
const MAX_ERROR_LEN: usize = 4000;

/// `error: source: source of source ...`, leaving out sources whose text
/// the message already contains.
fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !message.contains(&text) {
            message.push_str(": ");
            message.push_str(&text);
        }
        source = cause.source();
    }
    message
}

/// The message of a panicked (or cancelled) run.
fn panic_message(error: JoinError) -> String {
    if !error.is_panic() {
        return "the scheduled task was cancelled".to_owned();
    }
    let payload = error.into_panic();
    let detail = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a non-string payload".to_owned());
    format!("the scheduled task panicked: {detail}")
}

/// `text` cut to at most `max` bytes, on a character boundary.
fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_text_and_cuts_on_char_boundaries() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdef", 3), "abc");
        // `é` is two bytes: cutting inside it backs off to before it.
        assert_eq!(truncate("aé", 2), "a");
    }

    #[test]
    fn error_chain_skips_sources_the_message_already_shows() {
        // thiserror's `#[error("...: {0}")]` with `#[from]` prints its source too.
        let error = ScheduleError::Db(sea_orm::DbErr::Custom("disk I/O error".to_owned()));
        let chain = error_chain(&error);
        assert_eq!(chain.matches("disk I/O error").count(), 1, "{chain}");
    }

    #[test]
    fn minute_of_truncates_seconds_and_nanos() {
        let now = DateTime::parse_from_rfc3339("2026-10-25T02:30:59.999Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(minute_of(now).to_rfc3339(), "2026-10-25T02:30:00+00:00");
    }
}
