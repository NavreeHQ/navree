//! Workers: [`Queue::work`], [`WorkOptions`] and [`WorkStats`].

use std::any::Any;
use std::fmt;
use std::ops::AddAssign;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::FutureExt;
use navree_contracts::{JobContext, JobRegistration};
use navree_kernel::AppContext;
use navree_telemetry::{EventKind, JobFailed, JobProcessed, TelemetryContext, TelemetryEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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
}

impl AddAssign for WorkStats {
    fn add_assign(&mut self, other: WorkStats) {
        self.processed += other.processed;
        self.succeeded += other.succeeded;
        self.released += other.released;
        self.failed += other.failed;
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
}

impl AttemptOutcome {
    /// The name in logs and reports: `succeeded`, `released` or `failed`.
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptOutcome::Succeeded => "succeeded",
            AttemptOutcome::Released => "released",
            AttemptOutcome::Failed => "failed",
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

/// The jobs the workers of one `work` call may still take.
struct Budget(AtomicU64);

impl Budget {
    fn new(max_jobs: Option<u64>) -> Self {
        Self(AtomicU64::new(max_jobs.unwrap_or(u64::MAX)))
    }

    /// Takes one job slot; false when none are left.
    fn take(&self) -> bool {
        let mut left = self.0.load(Ordering::SeqCst);
        while left > 0 {
            match self
                .0
                .compare_exchange_weak(left, left - 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(actual) => left = actual,
            }
        }
        false
    }

    /// Returns a slot that found no job.
    fn give_back(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
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
        let job_ctx = JobContext {
            app: ctx.clone(),
            job_id: job.id.clone(),
            attempt: job.attempts,
        };
        let limit = Duration::from_secs(job.options.timeout_secs);
        match execute(registration, job.payload.clone(), job_ctx, limit).await {
            Ok(()) => {
                self.inner.store.complete(&job).await?;
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
            Err(error) if job.attempts >= job.options.max_attempts.max(1) => {
                self.fail(&job, worker, &error, started).await?;
                Ok(AttemptOutcome::Failed)
            }
            Err(error) => {
                self.release(&job, worker, &error, started).await?;
                Ok(AttemptOutcome::Released)
            }
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
        let released = self.inner.store.release(job, worker, available_at).await?;
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
            .inner
            .store
            .fail(job, worker, error, self.now_ms())
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
                        // The job ran (or was skipped) but its outcome could not be
                        // stored; the reservation goes stale and another worker
                        // retries it.
                        Err(error) => {
                            if !self.recover(error, "cannot record a job's outcome").await? {
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

    /// A driver error: returned when the worker stops on an empty queue;
    /// otherwise logged, then the worker waits a poll interval. `Ok(false)`
    /// when the app shuts down meanwhile.
    async fn recover(&self, error: QueueError, what: &str) -> Result<bool, QueueError> {
        if self.finite {
            return Err(error);
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
) -> Result<(), String> {
    let run = registration.run;
    let attempt = AssertUnwindSafe(async move { run(payload, ctx).await }).catch_unwind();
    let result = if limit.is_zero() {
        attempt.await
    } else {
        match tokio::time::timeout(limit, attempt).await {
            Ok(result) => result,
            Err(_) => {
                return Err(format!("the job timed out after {} s", limit.as_secs()));
            }
        }
    };
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error_chain(&*error)),
        Err(panic) => Err(format!("the job panicked: {}", panic_message(&*panic))),
    }
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
