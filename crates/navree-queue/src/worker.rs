//! Workers: [`Queue::work`], [`WorkOptions`] and [`WorkStats`].

use std::any::Any;
use std::fmt;
use std::future::Future;
use std::ops::AddAssign;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::FutureExt;
use navree_contracts::{JobCancelled, JobContext, JobRegistration};
use navree_kernel::AppContext;
use navree_telemetry::{EventKind, JobFailed, JobProcessed, TelemetryContext, TelemetryEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::batch::{BatchRow, Finish, Finished};
use crate::record::ReservedJob;
use crate::store::Store;
use crate::{Queue, QueueError};

/// A reservation older than the job's `timeout_secs` plus this many seconds
/// is considered abandoned (its worker crashed or was killed): another
/// worker may take the job, and that counts as a new attempt. A live worker
/// never holds a job that long, because each attempt is cut off at its
/// timeout.
pub const STALE_RESERVATION_GRACE_SECS: u64 = 30;

/// How a [`Queue::work`] call runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkOptions {
    /// The queues to take jobs from, highest priority first (empty: the
    /// module's [`worker_queues`](crate::QueueOptions::worker_queues)).
    pub queues: Vec<String>,
    /// Runs at most one job, with one worker, then returns (`--once`).
    pub once: bool,
    /// Returns after this many jobs (all workers together).
    pub max_jobs: Option<u64>,
    /// Returns as soon as no job is available, instead of polling until
    /// the app shuts down (`--stop-when-empty`).
    pub stop_when_empty: bool,
}

/// What a [`Queue::work`] call did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkStats {
    /// Attempts run (each attempt of a retried job counts).
    pub processed: u64,
    /// Attempts that succeeded (the job is done).
    pub succeeded: u64,
    /// Failed attempts put back on the queue for another attempt.
    pub released: u64,
    /// Jobs moved to the failed jobs.
    pub failed: u64,
    /// Jobs skipped, or stopped with `JobCancelled`, because their batch or
    /// operation was cancelled.
    pub cancelled: u64,
}

impl AddAssign for WorkStats {
    fn add_assign(&mut self, other: WorkStats) {
        self.processed += other.processed;
        self.succeeded += other.succeeded;
        self.released += other.released;
        self.failed += other.failed;
        self.cancelled += other.cancelled;
    }
}

/// How one attempt of a job ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    /// The job is done.
    Succeeded,
    /// It failed and goes back on its queue for another attempt.
    Released,
    /// It failed for good and moved to the failed jobs.
    Failed,
    /// Skipped, or stopped with `JobCancelled`, because its batch or
    /// operation was cancelled.
    Cancelled,
}

impl AttemptOutcome {
    /// The name in logs and reports: `succeeded`, `released`, `failed` or
    /// `cancelled`.
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptOutcome::Succeeded => "succeeded",
            AttemptOutcome::Released => "released",
            AttemptOutcome::Failed => "failed",
            AttemptOutcome::Cancelled => "cancelled",
        }
    }

    fn stats(self) -> WorkStats {
        let mut stats = WorkStats {
            processed: 1,
            ..WorkStats::default()
        };
        match self {
            AttemptOutcome::Succeeded => stats.succeeded = 1,
            AttemptOutcome::Released => stats.released = 1,
            AttemptOutcome::Failed => stats.failed = 1,
            AttemptOutcome::Cancelled => stats.cancelled = 1,
        }
        stats
    }
}

/// One finished attempt, as a supervised worker tells its
/// [`WorkHooks`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    /// The job id.
    pub job_id: String,
    /// The queue it came from.
    pub queue: String,
    /// The job type.
    pub job_type: String,
    /// Which attempt this was (1 for the first).
    pub attempt: u32,
    /// How it ended.
    pub outcome: AttemptOutcome,
    /// How long it ran, in milliseconds.
    pub runtime_ms: u64,
}

/// What a supervisor (navree-fleet's workers) puts around a worker: asked
/// before every reservation, told after every attempt. Both run on the
/// worker's task, so keep them quick.
pub trait WorkHooks: Send + Sync + 'static {
    /// Whether the worker may take a job now. `false` holds it (a paused
    /// supervisor): it takes nothing and asks again after a poll interval
    /// (a worker that stops when empty stops instead). Default: `true`.
    fn may_reserve(&self) -> bool {
        true
    }

    /// Told after each attempt, whatever its outcome.
    fn attempted(&self, attempt: &Attempt) {
        let _ = attempt;
    }
}

/// How [`Queue::work_supervised`] runs besides its [`WorkOptions`].
#[derive(Clone)]
pub struct Supervision {
    /// Concurrent workers (`None`: [`QueueOptions::workers`](crate::QueueOptions::workers)).
    pub workers: Option<usize>,
    /// Cancelled to stop the call as the app's shutdown would: no new job
    /// is taken, running jobs finish.
    pub stop: CancellationToken,
    /// Asked before each reservation, told after each attempt.
    pub hooks: Arc<dyn WorkHooks>,
}

impl fmt::Debug for Supervision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Supervision")
            .field("workers", &self.workers)
            .field("stopped", &self.stop.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// How a job's handler ended.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ran {
    Succeeded,
    /// It returned [`JobCancelled`].
    Cancelled,
    /// An error, a panic or a timeout.
    Failed(String),
}

/// The jobs the workers of one `work` call may still take.
struct Budget {
    left: AtomicU64,
    /// Whether the call has a limit, so that it ends by itself.
    limited: bool,
}

impl Budget {
    fn new(max_jobs: Option<u64>) -> Self {
        Self {
            left: AtomicU64::new(max_jobs.unwrap_or(u64::MAX)),
            limited: max_jobs.is_some(),
        }
    }

    /// Takes one job slot; false when none are left.
    fn take(&self) -> bool {
        let mut left = self.left.load(Ordering::SeqCst);
        while left > 0 {
            match self.left.compare_exchange_weak(
                left,
                left - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return true,
                Err(actual) => left = actual,
            }
        }
        false
    }

    /// Returns a slot that found no job.
    fn give_back(&self) {
        self.left.fetch_add(1, Ordering::SeqCst);
    }
}

impl Queue {
    /// Runs every job available now, one at a time in queue order, and
    /// returns when none is left (jobs released for a later retry and
    /// delayed jobs stay queued). For tests: `dispatch`, then
    /// `run_pending`, then assert.
    pub async fn run_pending(&self) -> Result<WorkStats, QueueError> {
        let opts = WorkOptions {
            stop_when_empty: true,
            ..WorkOptions::default()
        };
        self.work_with(opts, 1, None).await
    }

    /// [`work`](Queue::work) under a supervisor (navree-fleet's workers):
    /// with [`Supervision::workers`] concurrent workers, stopped by
    /// [`Supervision::stop`] as well as by the app's shutdown, asking the
    /// hooks before each reservation (a paused supervisor holds its
    /// workers) and telling them every attempt.
    pub async fn work_supervised(
        &self,
        opts: WorkOptions,
        supervision: Supervision,
    ) -> Result<WorkStats, QueueError> {
        let workers = if opts.once {
            1
        } else {
            supervision
                .workers
                .unwrap_or(self.inner.options.workers)
                .max(1)
        };
        self.work_with(opts, workers, Some(supervision)).await
    }

    /// Runs jobs with [`workers`](crate::QueueOptions::workers) concurrent
    /// workers (one with [`once`](WorkOptions::once)) until the app shuts
    /// down, [`max_jobs`](WorkOptions::max_jobs) jobs ran, or, with
    /// [`stop_when_empty`](WorkOptions::stop_when_empty) or `once`, no job
    /// is available. A running job is always finished first.
    ///
    /// Each job runs with the handler registered for its type, bounded by
    /// its `timeout_secs`; a panic counts as a failed attempt.
    ///
    /// Errors of the driver end the call when it would stop on an empty
    /// queue; otherwise they are logged and the worker polls again.
    ///
    /// A job's outcome is stored for as long as its worker holds it, whatever
    /// the database says for now. Past that, another worker may take the job and
    /// it runs again: the worker logs an error naming it, and a call limited by
    /// `once`, `stop_when_empty` or `max_jobs` returns the error, never `Ok`.
    ///
    /// A [`QueueFake`](crate::QueueFake) queue runs nothing and returns at once.
    pub async fn work(&self, opts: WorkOptions) -> Result<WorkStats, QueueError> {
        let workers = if opts.once {
            1
        } else {
            self.inner.options.workers.max(1)
        };
        self.work_with(opts, workers, None).await
    }

    /// [`work`](Queue::work) with `workers` concurrent workers, supervised
    /// or not.
    async fn work_with(
        &self,
        opts: WorkOptions,
        workers: usize,
        supervision: Option<Supervision>,
    ) -> Result<WorkStats, QueueError> {
        if matches!(self.inner.store, Store::Fake(_)) {
            return Ok(WorkStats::default());
        }
        let ctx = self.app()?;
        let queues = if opts.queues.is_empty() {
            self.inner.options.worker_queues()
        } else {
            opts.queues.clone()
        };
        let budget = Arc::new(Budget::new(if opts.once { Some(1) } else { opts.max_jobs }));
        let shutdown = ctx.shutdown_token();
        // Stops on the app's shutdown, or on the supervisor's stop.
        let token = CancellationToken::new();
        let hooks = supervision.as_ref().map(|s| s.hooks.clone());
        let linked = {
            let token = token.clone();
            let stop = supervision.map(|s| s.stop);
            tokio::spawn(async move {
                let stopped = async {
                    match stop {
                        Some(stop) => stop.cancelled().await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    () = shutdown.cancelled() => token.cancel(),
                    () = stopped => token.cancel(),
                    () = token.cancelled() => {}
                }
            })
        };
        let mut set = JoinSet::new();
        for _ in 0..workers {
            let worker = Worker {
                queue: self.clone(),
                ctx: ctx.clone(),
                queues: queues.clone(),
                finite: opts.once || opts.stop_when_empty,
                budget: budget.clone(),
                token: token.clone(),
                hooks: hooks.clone(),
                id: format!("worker-{}", Uuid::now_v7()),
                busy: AtomicU32::new(0),
            };
            set.spawn(worker.run());
        }
        let result = Self::join_workers(set).await;
        token.cancel();
        linked.abort();
        result
    }

    /// Waits for every worker of a `work` call; their stats summed, or the
    /// first error.
    async fn join_workers(
        mut set: JoinSet<Result<WorkStats, QueueError>>,
    ) -> Result<WorkStats, QueueError> {
        let mut stats = WorkStats::default();
        let mut first_error = None;
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(worker_stats)) => stats += worker_stats,
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(join) if join.is_panic() => std::panic::resume_unwind(join.into_panic()),
                Err(_) => {}
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(stats),
        }
    }

    /// Runs one reserved job in its own telemetry context (the job id, and
    /// a span in the trace it was dispatched in, or a new trace), and
    /// records the outcome.
    async fn run_reserved(
        &self,
        ctx: &AppContext,
        job: ReservedJob,
        worker: &str,
    ) -> Result<AttemptOutcome, QueueError> {
        let mut context =
            TelemetryContext::new(self.inner.telemetry.clone()).with_job_id(job.id.as_str());
        if let Some(trace) = self.inner.policy.resume(job.trace_parent.as_deref()) {
            context = context.with_trace(trace);
        }
        context.scope(self.run_attempt(ctx, job, worker)).await
    }

    /// Runs one reserved job and records the outcome.
    async fn run_attempt(
        &self,
        ctx: &AppContext,
        job: ReservedJob,
        worker: &str,
    ) -> Result<AttemptOutcome, QueueError> {
        let started = Instant::now();
        if job.attempts > job.options.max_attempts.max(1) {
            let error = format!(
                "the job was attempted too many times ({} of {}): the worker running its last attempt stopped (crashed or was killed) before it finished",
                job.attempts, job.options.max_attempts
            );
            self.fail(&job, worker, &error, started).await?;
            return Ok(AttemptOutcome::Failed);
        }
        let Some(registration) = self.inner.registry.get(job.job_type.as_str()) else {
            let error = format!(
                "no handler is registered for job type `{}`; register it with `m.job::<J>()`",
                job.job_type
            );
            self.fail(&job, worker, &error, started).await?;
            return Ok(AttemptOutcome::Failed);
        };
        if self.is_skipped(ctx, &job).await? {
            self.finish(&job, worker, Finish::Skipped, "", started)
                .await?;
            log_cancelled(
                &job,
                started,
                "the job's batch or operation was cancelled; skipped",
            );
            return Ok(AttemptOutcome::Cancelled);
        }
        let job_ctx = JobContext {
            app: ctx.clone(),
            job_id: job.id.clone(),
            attempt: job.attempts,
        };
        let limit = Duration::from_secs(job.options.timeout_secs);
        let running = self.begin(ctx, &job).await;
        let ran = execute(registration, job.payload.clone(), job_ctx, limit).await;
        self.end(running).await;
        match ran {
            Ran::Succeeded => {
                self.finish(&job, worker, Finish::Succeeded, "", started)
                    .await?;
                self.inner.telemetry.emit_with(EventKind::JobProcessed, || {
                    TelemetryEvent::JobProcessed(JobProcessed {
                        queue: job.queue.clone(),
                        job_type: job.job_type.clone(),
                        job_id: job.id.clone(),
                        attempt: job.attempts,
                        duration: started.elapsed(),
                    })
                });
                tracing::info!(
                    target: "navree::job",
                    name = %job.job_type,
                    job_id = %job.id,
                    queue = %job.queue,
                    attempt = job.attempts,
                    duration_ms = elapsed_ms(started),
                    outcome = "succeeded",
                    "job succeeded"
                );
                Ok(AttemptOutcome::Succeeded)
            }
            Ran::Cancelled => {
                self.finish(&job, worker, Finish::Skipped, "", started)
                    .await?;
                log_cancelled(
                    &job,
                    started,
                    "the job stopped: its batch or operation was cancelled",
                );
                Ok(AttemptOutcome::Cancelled)
            }
            Ran::Failed(error) if job.attempts >= job.options.max_attempts.max(1) => {
                self.fail(&job, worker, &error, started).await?;
                Ok(AttemptOutcome::Failed)
            }
            Ran::Failed(error) => {
                self.release(&job, worker, &error, started).await?;
                Ok(AttemptOutcome::Released)
            }
        }
    }

    /// Whether `job` must not run: one of a cancelled batch's jobs, or the
    /// work of a cancelled operation.
    async fn is_skipped(&self, ctx: &AppContext, job: &ReservedJob) -> Result<bool, QueueError> {
        if let Some(batch_id) = job.membership.in_batch()
            && let Some(row) = self.inner.store.find_batch(batch_id).await?
            && row.cancelled_at.is_some()
        {
            return Ok(true);
        }
        crate::operations::skips(ctx, job).await
    }

    /// Records the end of `job`: deleted (or moved to the failed jobs), and
    /// for a batch's job or callback, its batch updated and the callbacks
    /// that became due queued, in one step. Then the effect on its
    /// operation. Returns false when the worker no longer held the job.
    async fn finish(
        &self,
        job: &ReservedJob,
        worker: &str,
        finish: Finish,
        error: &str,
        started: Instant,
    ) -> Result<bool, QueueError> {
        let now = self.now_ms();
        let held = held_until(job, started);
        let mut step = None;
        if job.membership.batch_id.is_some() {
            let prepare = |envelope, row: &BatchRow| self.prepare_callback(envelope, row);
            match storing(held, || {
                self.inner
                    .store
                    .finish_batch_job(job, worker, finish, error, now, &prepare)
            })
            .await?
            {
                Finished::Lost => return Ok(false),
                Finished::Done(done) => step = done,
            }
        } else {
            match finish {
                Finish::Succeeded | Finish::Skipped => {
                    storing(held, || self.inner.store.complete(job)).await?;
                }
                Finish::Failed => {
                    if !storing(held, || self.inner.store.fail(job, worker, error, now)).await? {
                        return Ok(false);
                    }
                }
            }
        }
        if let Ok(app) = self.app() {
            let batch = step.as_ref().map(|step| &step.batch);
            crate::operations::job_finished(&app, job, finish, batch).await;
        }
        if let Some(step) = step
            && step.settled_now
        {
            self.batch_settled(&step.batch).await;
        }
        Ok(true)
    }

    /// A batch and every callback it dispatched have finished: log it and
    /// finish its operation.
    async fn batch_settled(&self, row: &BatchRow) {
        tracing::info!(
            target: "navree::job",
            batch_id = %row.id,
            name = %row.name,
            total_jobs = row.total_jobs,
            failed_jobs = row.failed_jobs,
            cancelled = row.cancelled_at.is_some(),
            "batch finished"
        );
        if let Ok(app) = self.app() {
            crate::operations::batch_settled(&app, row).await;
        }
    }

    /// Puts `job` back on its queue, available after its backoff, and logs it.
    async fn release(
        &self,
        job: &ReservedJob,
        worker: &str,
        error: &str,
        started: Instant,
    ) -> Result<(), QueueError> {
        let delay = job.options.backoff.delay_for(job.attempts);
        let delay_ms = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
        let available_at = self.now_ms().saturating_add(delay_ms);
        let released = storing(held_until(job, started), || {
            self.inner.store.release(job, worker, available_at)
        })
        .await?;
        if released {
            self.report_failure(job, error, started, true);
            tracing::warn!(
                target: "navree::job",
                name = %job.job_type,
                job_id = %job.id,
                queue = %job.queue,
                attempt = job.attempts,
                duration_ms = elapsed_ms(started),
                outcome = "released",
                retry_in_secs = delay.as_secs(),
                error,
                "job attempt failed; it will be retried"
            );
        } else {
            lost_reservation(job, worker);
        }
        Ok(())
    }

    /// Moves `job` to the failed jobs and logs it.
    async fn fail(
        &self,
        job: &ReservedJob,
        worker: &str,
        error: &str,
        started: Instant,
    ) -> Result<(), QueueError> {
        let moved = self
            .finish(job, worker, Finish::Failed, error, started)
            .await?;
        if moved {
            self.report_failure(job, error, started, false);
            tracing::error!(
                target: "navree::job",
                name = %job.job_type,
                job_id = %job.id,
                queue = %job.queue,
                attempt = job.attempts,
                duration_ms = elapsed_ms(started),
                outcome = "failed",
                error,
                "job failed"
            );
        } else {
            lost_reservation(job, worker);
        }
        Ok(())
    }
}

impl Queue {
    /// Emits a [`JobFailed`] for a failed attempt of `job`.
    fn report_failure(&self, job: &ReservedJob, error: &str, started: Instant, will_retry: bool) {
        self.inner.telemetry.emit_with(EventKind::JobFailed, || {
            TelemetryEvent::JobFailed(JobFailed {
                queue: job.queue.clone(),
                job_type: job.job_type.clone(),
                job_id: job.id.clone(),
                attempt: job.attempts,
                duration: started.elapsed(),
                error: error.to_owned(),
                will_retry,
            })
        });
    }
}

/// How many times a worker tries again when the database is busy, and how long
/// it waits in between. SQLite has already waited `busy_timeout` by then.
const BUSY_RETRIES: u32 = 5;
const BUSY_RETRY_DELAY: Duration = Duration::from_millis(250);

/// Whether `error` is the database being busy for now.
fn is_busy(error: &QueueError) -> bool {
    matches!(error, QueueError::Db(error) if error.is_retryable())
}

/// Until when a worker holds `job`, reserved about when `started` was taken: after
/// that its reservation is stale and another worker may take it.
fn held_until(job: &ReservedJob, started: Instant) -> Instant {
    let held = Duration::from_millis(
        u64::try_from(crate::record::stale_after_ms(&job.options)).unwrap_or(u64::MAX),
    );
    started.checked_add(held).unwrap_or(started)
}

/// Stores what became of a job, trying again while the database is busy and the
/// worker still holds it (until `held`): the job has run, and if this is lost
/// its reservation goes stale and the job runs again.
async fn storing<T, Fut>(held: Instant, mut store: impl FnMut() -> Fut) -> Result<T, QueueError>
where
    Fut: Future<Output = Result<T, QueueError>>,
{
    loop {
        match store().await {
            Err(error) if is_busy(&error) && Instant::now() < held => {
                tracing::warn!(target: "navree::job", %error, "cannot store a job's outcome; trying again");
                tokio::time::sleep(BUSY_RETRY_DELAY).await;
            }
            outcome => return outcome,
        }
    }
}

/// One worker loop of a `work` call.
struct Worker {
    queue: Queue,
    ctx: AppContext,
    queues: Vec<String>,
    /// Stops when no job is available.
    finite: bool,
    budget: Arc<Budget>,
    token: CancellationToken,
    /// A supervisor's hooks.
    hooks: Option<Arc<dyn WorkHooks>>,
    id: String,
    /// Tries in a row that found the database busy.
    busy: AtomicU32,
}

impl Worker {
    async fn run(self) -> Result<WorkStats, QueueError> {
        let mut stats = WorkStats::default();
        while !self.token.is_cancelled() {
            if self
                .hooks
                .as_ref()
                .is_some_and(|hooks| !hooks.may_reserve())
            {
                // Held by the supervisor (paused).
                if self.finite || !self.idle().await {
                    break;
                }
                continue;
            }
            if !self.budget.take() {
                break;
            }
            match self.reserve().await {
                Ok(Some(job)) => {
                    self.busy.store(0, Ordering::Relaxed);
                    let started = Instant::now();
                    let (job_id, queue, job_type, attempt) = (
                        job.id.clone(),
                        job.queue.clone(),
                        job.job_type.clone(),
                        job.attempts,
                    );
                    match self.queue.run_reserved(&self.ctx, job, &self.id).await {
                        Ok(outcome) => {
                            stats += outcome.stats();
                            if let Some(hooks) = &self.hooks {
                                hooks.attempted(&Attempt {
                                    job_id,
                                    queue,
                                    job_type,
                                    attempt,
                                    outcome,
                                    runtime_ms: elapsed_ms(started),
                                });
                            }
                        }
                        // The job ran (or was skipped) and its outcome could not be
                        // stored while the worker held it: its reservation goes stale
                        // and another worker runs it again. A call that ends by itself
                        // says so, and never returns `Ok` for it.
                        Err(error) => {
                            tracing::error!(
                                target: "navree::job",
                                %error,
                                worker = %self.id,
                                name = %job_type,
                                job_id = %job_id,
                                queue = %queue,
                                attempt,
                                "cannot store a job's outcome; the job will run again"
                            );
                            if self.finite || self.budget.limited {
                                return Err(error);
                            }
                            if !self.idle().await {
                                break;
                            }
                        }
                    }
                }
                Ok(None) => {
                    self.budget.give_back();
                    if self.finite || !self.idle().await {
                        break;
                    }
                }
                Err(error) => {
                    self.budget.give_back();
                    if !self.recover(error, "cannot reserve a job").await? {
                        break;
                    }
                }
            }
        }
        Ok(stats)
    }

    /// A driver error finding a job: returned when the worker stops on an empty
    /// queue, unless it is the database being busy for now (a lock held longer
    /// than it waits, a deadlock), which is tried again a few times; otherwise
    /// logged, then the worker waits a poll interval. `Ok(false)` when the app
    /// shuts down meanwhile.
    async fn recover(&self, error: QueueError, what: &str) -> Result<bool, QueueError> {
        if self.finite {
            if !is_busy(&error) || self.busy.fetch_add(1, Ordering::Relaxed) >= BUSY_RETRIES {
                return Err(error);
            }
            tracing::warn!(target: "navree::job", %error, worker = %self.id, "{what}; trying again");
            tokio::time::sleep(BUSY_RETRY_DELAY).await;
            return Ok(!self.token.is_cancelled());
        }
        tracing::error!(target: "navree::job", %error, worker = %self.id, "{what}");
        Ok(self.idle().await)
    }

    /// The next available job, from the first queue that has one.
    async fn reserve(&self) -> Result<Option<ReservedJob>, QueueError> {
        for queue in &self.queues {
            let now = self.queue.now_ms();
            if let Some(job) = self.queue.inner.store.reserve(queue, &self.id, now).await? {
                return Ok(Some(job));
            }
        }
        Ok(None)
    }

    /// Waits one poll interval; false when the app shuts down meanwhile.
    async fn idle(&self) -> bool {
        tokio::select! {
            () = self.token.cancelled() => false,
            () = tokio::time::sleep(self.queue.inner.options.poll_interval()) => true,
        }
    }
}

/// Runs the handler, bounded by `limit` (none when zero); panics and
/// timeouts become errors.
async fn execute(
    registration: &JobRegistration,
    payload: Value,
    ctx: JobContext,
    limit: Duration,
) -> Ran {
    let run = registration.run;
    let attempt = AssertUnwindSafe(async move { run(payload, ctx).await }).catch_unwind();
    let result = if limit.is_zero() {
        attempt.await
    } else {
        match tokio::time::timeout(limit, attempt).await {
            Ok(result) => result,
            Err(_) => {
                return Ran::Failed(format!("the job timed out after {} s", limit.as_secs()));
            }
        }
    };
    match result {
        Ok(Ok(())) => Ran::Succeeded,
        Ok(Err(error)) if error.downcast_ref::<JobCancelled>().is_some() => Ran::Cancelled,
        Ok(Err(error)) => Ran::Failed(error_chain(&*error)),
        Err(panic) => Ran::Failed(format!("the job panicked: {}", panic_message(&*panic))),
    }
}

fn log_cancelled(job: &ReservedJob, started: Instant, message: &str) {
    tracing::info!(
        target: "navree::job",
        name = %job.job_type,
        job_id = %job.id,
        queue = %job.queue,
        attempt = job.attempts,
        duration_ms = elapsed_ms(started),
        outcome = "cancelled",
        "{message}"
    );
}

/// `error: source: source…`.
fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !message.ends_with(&cause_text) {
            message.push_str(": ");
            message.push_str(&cause_text);
        }
        source = cause.source();
    }
    message
}

fn panic_message(panic: &(dyn Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "a non-string panic payload".to_owned()
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn lost_reservation(job: &ReservedJob, worker: &str) {
    tracing::warn!(
        target: "navree::job",
        name = %job.job_type,
        job_id = %job.id,
        worker,
        "the job's reservation was taken over by another worker (it ran longer than its timeout); leaving it to that worker"
    );
}
