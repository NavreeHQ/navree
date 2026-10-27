//! Job batches: [`Batch`] (what to dispatch) and [`JobBatch`] (a dispatched
//! batch's progress, as stored in `job_batches`).

use chrono::{DateTime, Utc};
use navree_contracts::{Job, JobEnvelope};
use navree_db::Uow;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::record::from_millis;
use crate::{Queue, QueueError};

/// A group of jobs dispatched together and followed as one: how many are
/// still pending, how many failed, whether the group was cancelled, plus
/// callbacks for its outcome. Dispatch it with [`dispatch`](Batch::dispatch)
/// or [`dispatch_in`](Batch::dispatch_in) (or the same methods on
/// [`Queue`]: [`dispatch_batch`](crate::Queue::dispatch_batch),
/// [`dispatch_batch_in`](crate::Queue::dispatch_batch_in)).
///
/// ```no_run
/// # use navree_contracts::{BoxError, Job, JobContext};
/// # use serde::{Deserialize, Serialize};
/// # #[derive(Serialize, Deserialize)] struct ImportChunk { rows: Vec<u32> }
/// # impl Job for ImportChunk { const NAME: &'static str = "import.chunk"; async fn handle(self, _: JobContext) -> Result<(), BoxError> { Ok(()) } }
/// # #[derive(Serialize, Deserialize)] struct FinishImport { id: u32 }
/// # impl Job for FinishImport { const NAME: &'static str = "import.finish"; async fn handle(self, _: JobContext) -> Result<(), BoxError> { Ok(()) } }
/// # #[derive(Serialize, Deserialize)] struct ReportImportFailure { id: u32 }
/// # impl Job for ReportImportFailure { const NAME: &'static str = "import.failed"; async fn handle(self, _: JobContext) -> Result<(), BoxError> { Ok(()) } }
/// use navree_queue::{Batch, Queue};
///
/// # async fn example(queue: &Queue, uow: &navree_db::Uow) -> Result<(), navree_queue::QueueError> {
/// let chunks = vec![vec![1, 2], vec![3, 4]];
/// let batch = Batch::new("recipients.import")
///     .add(chunks.into_iter().map(|rows| ImportChunk { rows }))
///     .then(FinishImport { id: 7 })
///     .catch(ReportImportFailure { id: 7 })
///     .dispatch_in(queue, uow)
///     .await?;
/// println!("batch {} has {} jobs", batch.id, batch.total_jobs);
/// # Ok(())
/// # }
/// ```
///
/// - The batch **finishes** when each of its jobs has succeeded, failed
///   (used its attempts) or been skipped.
/// - **`then`** runs once it finishes with no failed job and without being
///   cancelled; **`catch`** runs as soon as the first job fails;
///   **`finally`** runs once it finishes, whatever happened.
/// - The first failed job **cancels** the batch, unless it
///   [`allow_failures`](Batch::allow_failures): the jobs that have not
///   started yet are then skipped. [`Queue::cancel_batch`](crate::Queue::cancel_batch)
///   cancels it on demand; running jobs see it through
///   `ctx.is_cancelled()` and `ctx.progress(..)`.
/// - Jobs in a batch are never unique (`unique_for_secs` is ignored), so
///   every job counts.
#[derive(Clone, Debug)]
pub struct Batch {
    pub(crate) name: String,
    pub(crate) jobs: Vec<JobEnvelope>,
    pub(crate) callbacks: Callbacks,
    pub(crate) allow_failures: bool,
    pub(crate) operation_id: Option<Uuid>,
    /// The first job that could not be serialized; dispatching fails with it.
    pub(crate) error: Option<String>,
}

/// A batch's callback jobs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Callbacks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) then: Option<JobEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) catch: Option<JobEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) finally: Option<JobEnvelope>,
}

impl Batch {
    /// An empty batch named `name` (`recipients.import`): what logs,
    /// `queue:batches` and [`JobBatch::name`] show.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            jobs: Vec::new(),
            callbacks: Callbacks::default(),
            allow_failures: false,
            operation_id: None,
            error: None,
        }
    }

    /// Adds `jobs` to the batch.
    // The builder's verb (`Batch::new(..).add(jobs)`); it takes jobs, not
    // another batch, so `std::ops::Add` does not fit.
    #[allow(clippy::should_implement_trait)]
    pub fn add<J: Job>(mut self, jobs: impl IntoIterator<Item = J>) -> Self {
        for job in jobs {
            if let Some(envelope) = self.envelope(&job) {
                self.jobs.push(envelope);
            }
        }
        self
    }

    /// Adds one job to the batch.
    pub fn push<J: Job>(self, job: J) -> Self {
        self.add([job])
    }

    /// The job to dispatch once the batch finishes with no failed job and
    /// without being cancelled.
    pub fn then<J: Job>(mut self, job: J) -> Self {
        self.callbacks.then = self.envelope(&job);
        self
    }

    /// The job to dispatch as soon as one of the batch's jobs fails.
    pub fn catch<J: Job>(mut self, job: J) -> Self {
        self.callbacks.catch = self.envelope(&job);
        self
    }

    /// The job to dispatch once the batch finishes, whatever happened.
    pub fn finally<J: Job>(mut self, job: J) -> Self {
        self.callbacks.finally = self.envelope(&job);
        self
    }

    /// Keeps running the other jobs when one fails (by default the first
    /// failure cancels the batch). `then` still runs only when no job
    /// failed.
    pub fn allow_failures(mut self) -> Self {
        self.allow_failures = true;
        self
    }

    /// Ties the batch to the operation `id` (see
    /// [`Operations`](crate::Operations)): its progress becomes the
    /// operation's, its jobs and callbacks report into it, and the
    /// operation finishes when the batch and its callbacks have run.
    /// [`Operations::dispatch_batch`](crate::Operations::dispatch_batch)
    /// sets it for you.
    pub fn operation(mut self, id: Uuid) -> Self {
        self.operation_id = Some(id);
        self
    }

    /// How many jobs the batch has.
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// Whether the batch has no jobs (it finishes as soon as it is
    /// dispatched).
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// The batch's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Dispatches the batch on `queue`: the fluent form of
    /// [`Queue::dispatch_batch`] (it joins the current unit of work the
    /// same way).
    pub async fn dispatch(self, queue: &Queue) -> Result<JobBatch, QueueError> {
        queue.dispatch_batch(self).await
    }

    /// Dispatches the batch on `queue` as part of `uow`: the fluent form of
    /// [`Queue::dispatch_batch_in`]. Nothing is stored if `uow` rolls back.
    pub async fn dispatch_in(self, queue: &Queue, uow: &Uow) -> Result<JobBatch, QueueError> {
        queue.dispatch_batch_in(uow, self).await
    }

    fn envelope<J: Job>(&mut self, job: &J) -> Option<JobEnvelope> {
        match serde_json::to_value(job) {
            Ok(payload) => Some(JobEnvelope {
                job_type: J::NAME.to_owned(),
                payload,
                options: J::options(),
                delay_secs: None,
                unique_key: None,
            }),
            Err(error) => {
                self.error
                    .get_or_insert_with(|| format!("job `{}`: {error}", J::NAME));
                None
            }
        }
    }

    /// The serialization error of the first job that had one.
    pub(crate) fn check(&self) -> Result<(), QueueError> {
        match &self.error {
            Some(error) => Err(QueueError::Payload(serde::de::Error::custom(error))),
            None => Ok(()),
        }
    }
}

/// A dispatched batch, as stored in `job_batches`: what
/// [`Queue::find_batch`](crate::Queue::find_batch) and
/// [`Queue::batches`](crate::Queue::batches) return.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JobBatch {
    /// The batch id.
    pub id: String,
    /// The name given to [`Batch::new`].
    pub name: String,
    /// How many jobs the batch has.
    pub total_jobs: u64,
    /// How many have not finished yet.
    pub pending_jobs: u64,
    /// How many failed (used their attempts).
    pub failed_jobs: u64,
    /// The ids of the failed jobs (see `queue:failed`).
    pub failed_job_ids: Vec<String>,
    /// Whether a failed job leaves the others running.
    pub allow_failures: bool,
    /// The operation the batch does the work of.
    pub operation_id: Option<Uuid>,
    /// When it was dispatched.
    pub created_at: DateTime<Utc>,
    /// When it was cancelled (by a failure or on demand).
    pub cancelled_at: Option<DateTime<Utc>>,
    /// When its last job finished.
    pub finished_at: Option<DateTime<Utc>>,
}

impl JobBatch {
    /// Jobs that have finished (succeeded, failed or skipped).
    pub fn processed_jobs(&self) -> u64 {
        self.total_jobs.saturating_sub(self.pending_jobs)
    }

    /// Finished jobs as a whole percentage of all jobs (100 for an empty
    /// batch).
    pub fn progress(&self) -> u8 {
        if self.total_jobs == 0 {
            return 100;
        }
        let percent = u128::from(self.processed_jobs()) * 100 / u128::from(self.total_jobs);
        u8::try_from(percent).unwrap_or(100)
    }

    /// Whether every job has finished.
    pub fn finished(&self) -> bool {
        self.finished_at.is_some()
    }

    /// Whether the batch was cancelled.
    pub fn cancelled(&self) -> bool {
        self.cancelled_at.is_some()
    }

    /// Whether any job failed.
    pub fn has_failures(&self) -> bool {
        self.failed_jobs > 0
    }
}

/// How one of a batch's jobs ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Finish {
    /// It ran and succeeded.
    Succeeded,
    /// It did not run (its batch or operation was cancelled) or stopped
    /// because of that.
    Skipped,
    /// It used its attempts.
    Failed,
}

/// A batch row as the drivers keep it.
#[derive(Clone, Debug)]
pub(crate) struct BatchRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) total_jobs: u64,
    pub(crate) pending_jobs: u64,
    pub(crate) failed_jobs: u64,
    pub(crate) failed_job_ids: Vec<String>,
    pub(crate) callbacks: Callbacks,
    /// Callbacks dispatched and not finished yet.
    pub(crate) callbacks_pending: u64,
    /// Callbacks that failed.
    pub(crate) callbacks_failed: u64,
    pub(crate) allow_failures: bool,
    pub(crate) operation_id: Option<String>,
    /// Unix milliseconds.
    pub(crate) created_at: i64,
    pub(crate) cancelled_at: Option<i64>,
    pub(crate) finished_at: Option<i64>,
}

/// What finishing a reserved batch job did.
#[derive(Clone, Debug)]
pub(crate) enum Finished {
    /// The worker no longer held the reservation: nothing changed.
    Lost,
    /// The job is gone (into the failed jobs when it failed); the batch
    /// step, when its batch still exists.
    Done(Option<Box<BatchStep>>),
}

/// What finishing a batch job or callback changed.
#[derive(Clone, Debug)]
pub(crate) struct BatchStep {
    /// The batch afterwards.
    pub(crate) batch: BatchRow,
    /// The batch and every callback it dispatched have now all finished
    /// (true for exactly one step of a batch).
    pub(crate) settled_now: bool,
}

impl BatchRow {
    /// A new batch row with `total_jobs` jobs, created at `now`.
    pub(crate) fn new(id: String, batch: &Batch, now: i64) -> Self {
        let total = batch.jobs.len() as u64;
        Self {
            id,
            name: batch.name.clone(),
            total_jobs: total,
            pending_jobs: total,
            failed_jobs: 0,
            failed_job_ids: Vec::new(),
            callbacks: batch.callbacks.clone(),
            callbacks_pending: 0,
            callbacks_failed: 0,
            allow_failures: batch.allow_failures,
            operation_id: batch.operation_id.map(|id| id.to_string()),
            created_at: now,
            cancelled_at: None,
            finished_at: None,
        }
    }

    /// For a batch with no jobs: it finishes as it is created. Returns the
    /// callbacks due.
    pub(crate) fn start_empty(&mut self, now: i64) -> Vec<JobEnvelope> {
        if self.total_jobs == 0 {
            self.finish(now)
        } else {
            Vec::new()
        }
    }

    /// Records that job `job_id` ended with `finish` at `now`; returns the
    /// callbacks due now (counted as pending).
    pub(crate) fn job_finished(
        &mut self,
        job_id: &str,
        finish: Finish,
        now: i64,
    ) -> Vec<JobEnvelope> {
        if self.pending_jobs == 0 {
            return Vec::new();
        }
        self.pending_jobs -= 1;
        let mut due = Vec::new();
        if finish == Finish::Failed {
            self.failed_jobs += 1;
            self.failed_job_ids.push(job_id.to_owned());
            if self.failed_jobs == 1
                && let Some(catch) = &self.callbacks.catch
            {
                due.push(catch.clone());
            }
            if !self.allow_failures && self.cancelled_at.is_none() {
                self.cancelled_at = Some(now);
            }
        }
        self.callbacks_pending += due.len() as u64;
        if self.pending_jobs == 0 {
            due.extend(self.finish(now));
        }
        due
    }

    /// Records that one of the batch's callbacks ended.
    pub(crate) fn callback_finished(&mut self, failed: bool) {
        self.callbacks_pending = self.callbacks_pending.saturating_sub(1);
        if failed {
            self.callbacks_failed += 1;
        }
    }

    /// Cancels the batch at `now` unless it finished or was cancelled;
    /// whether it changed.
    pub(crate) fn cancel(&mut self, now: i64) -> bool {
        if self.cancelled_at.is_some() || self.finished_at.is_some() {
            return false;
        }
        self.cancelled_at = Some(now);
        true
    }

    /// Whether the batch and the callbacks it dispatched have all finished.
    pub(crate) fn settled(&self) -> bool {
        self.finished_at.is_some() && self.callbacks_pending == 0
    }

    /// Marks the batch finished; returns `then` (when it applies) and
    /// `finally`, counted as pending callbacks.
    fn finish(&mut self, now: i64) -> Vec<JobEnvelope> {
        let mut due = Vec::new();
        if self.finished_at.is_none() {
            self.finished_at = Some(now);
            if self.failed_jobs == 0
                && self.cancelled_at.is_none()
                && let Some(then) = &self.callbacks.then
            {
                due.push(then.clone());
            }
            if let Some(finally) = &self.callbacks.finally {
                due.push(finally.clone());
            }
        }
        self.callbacks_pending += due.len() as u64;
        due
    }

    /// The public view.
    pub(crate) fn to_batch(&self) -> JobBatch {
        JobBatch {
            id: self.id.clone(),
            name: self.name.clone(),
            total_jobs: self.total_jobs,
            pending_jobs: self.pending_jobs,
            failed_jobs: self.failed_jobs,
            failed_job_ids: self.failed_job_ids.clone(),
            allow_failures: self.allow_failures,
            operation_id: self
                .operation_id
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok()),
            created_at: from_millis(self.created_at),
            cancelled_at: self.cancelled_at.map(from_millis),
            finished_at: self.finished_at.map(from_millis),
        }
    }
}
