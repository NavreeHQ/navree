//! What workers do for jobs that belong to an operation or a batch:
//! skipping cancelled work, the running-job registry behind `dyn
//! JobMonitor` (progress and cancellation), the heartbeat, and finishing
//! operations.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use navree_contracts::{BoxFuture, JobCancelled, JobMonitor, JobProgress};
use navree_kernel::AppContext;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::{FAILED_MESSAGE, OperationStatus, Operations};
use crate::batch::{BatchRow, Finish};
use crate::record::ReservedJob;
use crate::{Queue, QueueError};

/// The operation a job does the work of.
fn operation_of(job: &ReservedJob) -> Option<Uuid> {
    job.membership
        .operation_id
        .as_deref()
        .and_then(|id| Uuid::parse_str(id).ok())
}

/// Whether `job` must not run because its operation has finished
/// (cancelled, or failed or completed by someone else). A batch's
/// callbacks always run.
pub(crate) async fn skips(ctx: &AppContext, job: &ReservedJob) -> Result<bool, QueueError> {
    if job.membership.callback {
        return Ok(false);
    }
    let Some(operation) = operation_of(job) else {
        return Ok(false);
    };
    let Some(operations) = ctx.try_get::<Operations>().await else {
        return Ok(false);
    };
    Ok(operations
        .status(operation)
        .await?
        .is_some_and(OperationStatus::is_finished))
}

/// The effect of a finished job on its operation: a batch's job moves the
/// operation's progress to the batch's finished jobs; a job of its own
/// finishes it (`succeeded`, or `failed` once it used its attempts).
pub(crate) async fn job_finished(
    ctx: &AppContext,
    job: &ReservedJob,
    finish: Finish,
    batch: Option<&BatchRow>,
) {
    let Some(operation) = operation_of(job) else {
        return;
    };
    let Some(operations) = ctx.try_get::<Operations>().await else {
        return;
    };
    let result = if job.membership.batch_id.is_some() {
        match batch {
            Some(row) if !job.membership.callback => operations.batch_progress(operation, row).await,
            _ => return,
        }
    } else {
        match finish {
            Finish::Succeeded => operations.job_succeeded(operation).await,
            Finish::Failed => operations.fail(operation, FAILED_MESSAGE).await,
            Finish::Skipped => return,
        }
    };
    if let Err(error) = result {
        tracing::warn!(target: "navree::job", operation_id = %operation, %error, "cannot update an operation");
    }
}

/// A batch and its callbacks finished: its operation finishes the way the
/// batch went.
pub(crate) async fn batch_settled(ctx: &AppContext, row: &BatchRow) {
    let Some(operation) = row
        .operation_id
        .as_deref()
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        return;
    };
    let Some(operations) = ctx.try_get::<Operations>().await else {
        return;
    };
    if let Err(error) = operations.finish_from_batch(operation, row).await {
        tracing::warn!(target: "navree::job", operation_id = %operation, %error, "cannot finish an operation");
    }
}

/// A job a worker of this process is running.
#[derive(Debug)]
pub(crate) struct RunningJob {
    /// The batch, when it is one of a batch's jobs (not a callback).
    batch_id: Option<String>,
    /// Whether it is a batch's callback.
    callback: bool,
    operation_id: Option<Uuid>,
    state: Mutex<Throttle>,
}

/// When progress was last written, and whether cancellation was seen.
#[derive(Debug, Default)]
struct Throttle {
    last_write: Option<Instant>,
    cancelled: bool,
}

/// The running jobs of this process, by id.
#[derive(Debug, Default)]
pub(crate) struct RunningJobs(Mutex<HashMap<String, Arc<RunningJob>>>);

impl RunningJobs {
    fn get(&self, job_id: &str) -> Option<Arc<RunningJob>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(job_id)
            .cloned()
    }

    fn insert(&self, job_id: String, job: Arc<RunningJob>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(job_id, job);
    }

    fn remove(&self, job_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(job_id);
    }
}

/// A job's run: registered while it runs, with its operation's heartbeat.
pub(crate) struct Running {
    job_id: String,
    heartbeat: Option<JoinHandle<()>>,
}

impl Queue {
    /// Registers `job` as running (for `ctx.progress` and
    /// `ctx.is_cancelled`); for an operation's job, marks the operation
    /// running and starts beating its heartbeat.
    pub(crate) async fn begin(&self, ctx: &AppContext, job: &ReservedJob) -> Running {
        let operation_id = operation_of(job);
        self.inner.running.insert(
            job.id.clone(),
            Arc::new(RunningJob {
                batch_id: job.membership.in_batch().map(str::to_owned),
                callback: job.membership.callback,
                operation_id,
                state: Mutex::new(Throttle::default()),
            }),
        );
        let mut heartbeat = None;
        if let Some(operation) = operation_id
            && let Some(operations) = ctx.try_get::<Operations>().await
        {
            if let Err(error) = operations.mark_running(operation).await {
                tracing::warn!(target: "navree::job", operation_id = %operation, %error, "cannot mark an operation running");
            }
            let every = operations.options().heartbeat_interval();
            heartbeat = Some(tokio::spawn(async move {
                let mut ticks = tokio::time::interval(every);
                ticks.tick().await;
                loop {
                    ticks.tick().await;
                    if let Err(error) = operations.heartbeat(operation).await {
                        tracing::warn!(target: "navree::job", operation_id = %operation, %error, "cannot renew an operation's heartbeat");
                    }
                }
            }));
        }
        Running {
            job_id: job.id.clone(),
            heartbeat,
        }
    }

    /// The job's run ended.
    pub(crate) async fn end(&self, running: Running) {
        if let Some(heartbeat) = running.heartbeat {
            heartbeat.abort();
        }
        self.inner.running.remove(&running.job_id);
    }

    /// Records `progress` for a running job; whether its operation or batch
    /// was cancelled.
    async fn report(&self, job: &RunningJob, progress: &JobProgress) -> bool {
        if let Some(operation) = job.operation_id
            && let Some(operations) = self.operations().await
        {
            let record = if job.batch_id.is_some() || job.callback {
                operations.note(operation, &progress.note).await
            } else {
                operations
                    .progress(operation, progress.done, progress.total, progress.note.clone())
                    .await
            };
            match record {
                Ok(Some(record)) if record.status == OperationStatus::Cancelled => return true,
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(target: "navree::job", operation_id = %operation, %error, "cannot record an operation's progress");
                }
            }
        }
        self.batch_cancelled(job).await
    }

    /// Whether a running job's operation or batch was cancelled.
    async fn cancelled(&self, job: &RunningJob) -> bool {
        if let Some(operation) = job.operation_id
            && let Some(operations) = self.operations().await
        {
            match operations.status(operation).await {
                Ok(Some(OperationStatus::Cancelled)) => return true,
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(target: "navree::job", operation_id = %operation, %error, "cannot read an operation");
                }
            }
        }
        self.batch_cancelled(job).await
    }

    async fn batch_cancelled(&self, job: &RunningJob) -> bool {
        let Some(batch_id) = &job.batch_id else {
            return false;
        };
        match self.inner.store.find_batch(batch_id).await {
            Ok(row) => row.is_some_and(|row| row.cancelled_at.is_some()),
            Err(error) => {
                tracing::warn!(target: "navree::job", batch_id = %batch_id, %error, "cannot read a batch");
                false
            }
        }
    }

    async fn operations(&self) -> Option<std::sync::Arc<Operations>> {
        self.app().ok()?.try_get::<Operations>().await
    }
}

/// navree-queue's `dyn JobMonitor`: progress and cancellation for the jobs
/// this process's workers run.
pub(crate) struct QueueMonitor {
    pub(crate) queue: Queue,
}

impl JobMonitor for QueueMonitor {
    fn progress<'a>(
        &'a self,
        job_id: &'a str,
        progress: JobProgress,
    ) -> BoxFuture<'a, Result<(), JobCancelled>> {
        Box::pin(async move {
            let Some(job) = self.queue.inner.running.get(job_id) else {
                return Ok(());
            };
            {
                let mut state = job.state.lock().unwrap_or_else(PoisonError::into_inner);
                if state.cancelled {
                    return Err(JobCancelled);
                }
                let last = progress.total > 0 && progress.done >= progress.total;
                let interval = self.queue.inner.options.operations.progress_interval();
                if !last && state.last_write.is_some_and(|at| at.elapsed() < interval) {
                    return Ok(());
                }
                state.last_write = Some(Instant::now());
            }
            if self.queue.report(&job, &progress).await {
                job.state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .cancelled = true;
                return Err(JobCancelled);
            }
            Ok(())
        })
    }

    fn is_cancelled<'a>(&'a self, job_id: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let Some(job) = self.queue.inner.running.get(job_id) else {
                return false;
            };
            if job
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .cancelled
            {
                return true;
            }
            let cancelled = self.queue.cancelled(&job).await;
            if cancelled {
                job.state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .cancelled = true;
            }
            cancelled
        })
    }
}
