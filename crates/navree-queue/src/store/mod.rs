//! The drivers behind [`Queue`](crate::Queue).

mod batches;
mod database;
mod memory;

pub(crate) use database::{DatabaseStore, statement};
pub(crate) use memory::MemoryStore;

use navree_contracts::JobEnvelope;
use navree_db::Uow;

use crate::batch::{BatchRow, Finish, Finished};
use crate::record::{FailedJob, NewJob, ReservedJob, RetryTarget};
use crate::{QueueError, QueueFake};

/// Turns a batch callback that became due into the job to store.
pub(crate) type PrepareCallback<'a> =
    dyn Fn(JobEnvelope, &BatchRow) -> Result<NewJob, QueueError> + Send + Sync + 'a;

/// The driver of a queue.
#[derive(Debug)]
pub(crate) enum Store {
    Database(DatabaseStore),
    Memory(MemoryStore),
    /// Records jobs, never runs them.
    Fake(QueueFake),
}

impl Store {
    /// Stores `job`; returns its id, or the id of the pending unique job
    /// that takes its place.
    pub(crate) async fn insert(&self, job: NewJob) -> Result<String, QueueError> {
        match self {
            Store::Database(db) => db.insert(&job).await,
            Store::Memory(memory) => Ok(memory.insert(job)),
            Store::Fake(fake) => {
                let id = job.id.clone();
                fake.record(envelope_of(job));
                Ok(id)
            }
        }
    }

    pub(crate) async fn reserve(
        &self,
        queue: &str,
        worker: &str,
        now: i64,
    ) -> Result<Option<ReservedJob>, QueueError> {
        match self {
            Store::Database(db) => db.reserve(queue, worker, now).await,
            Store::Memory(memory) => Ok(memory.reserve(queue, worker, now)),
            Store::Fake(_) => Ok(None),
        }
    }

    pub(crate) async fn complete(&self, id: &str) -> Result<(), QueueError> {
        match self {
            Store::Database(db) => db.complete(id).await,
            Store::Memory(memory) => {
                memory.complete(id);
                Ok(())
            }
            Store::Fake(_) => Ok(()),
        }
    }

    /// Unreserves `job`, available again at `available_at`. Returns false
    /// when `worker` no longer holds the reservation.
    pub(crate) async fn release(
        &self,
        job: &ReservedJob,
        worker: &str,
        available_at: i64,
    ) -> Result<bool, QueueError> {
        match self {
            Store::Database(db) => db.release(&job.id, worker, available_at).await,
            Store::Memory(memory) => Ok(memory.release(&job.id, worker, available_at)),
            Store::Fake(_) => Ok(false),
        }
    }

    /// Moves `job` to the failed jobs. Returns false when `worker` no
    /// longer holds the reservation.
    pub(crate) async fn fail(
        &self,
        job: &ReservedJob,
        worker: &str,
        error: &str,
        now: i64,
    ) -> Result<bool, QueueError> {
        match self {
            Store::Database(db) => db.fail(job, worker, error, now).await,
            Store::Memory(memory) => Ok(memory.fail(job, worker, error, now)),
            Store::Fake(_) => Ok(false),
        }
    }

    pub(crate) async fn failed(&self) -> Result<Vec<FailedJob>, QueueError> {
        match self {
            Store::Database(db) => db.failed().await,
            Store::Memory(memory) => Ok(memory.failed()),
            Store::Fake(_) => Ok(Vec::new()),
        }
    }

    /// Moves the targeted failed jobs back to their queues; returns how many.
    pub(crate) async fn retry(&self, target: &RetryTarget, now: i64) -> Result<u64, QueueError> {
        let retried = match self {
            Store::Database(db) => db.retry(target, now).await?,
            Store::Memory(memory) => memory.retry(target, now),
            Store::Fake(_) => 0,
        };
        match target {
            RetryTarget::Id(id) if retried == 0 => {
                Err(QueueError::FailedJobNotFound { id: id.clone() })
            }
            _ => Ok(retried),
        }
    }

    /// Deletes every failed job; returns how many.
    pub(crate) async fn flush_failed(&self) -> Result<u64, QueueError> {
        match self {
            Store::Database(db) => db.flush_failed().await,
            Store::Memory(memory) => Ok(memory.flush_failed()),
            Store::Fake(_) => Ok(0),
        }
    }

    pub(crate) async fn pending_count(&self, queue: &str) -> Result<u64, QueueError> {
        match self {
            Store::Database(db) => db.pending_count(queue).await,
            Store::Memory(memory) => Ok(memory.pending_count(queue)),
            Store::Fake(fake) => Ok(fake.count_on(queue)),
        }
    }

    /// Stores a batch and its jobs at once.
    pub(crate) async fn insert_batch(
        &self,
        row: BatchRow,
        jobs: Vec<NewJob>,
    ) -> Result<(), QueueError> {
        match self {
            Store::Database(db) => db.insert_batch(&row, &jobs).await,
            Store::Memory(memory) => {
                memory.insert_batch(row, jobs);
                Ok(())
            }
            Store::Fake(fake) => {
                // A batch with no jobs dispatches its callbacks at once:
                // the fake records them like any other dispatch.
                let (callbacks, members): (Vec<_>, Vec<_>) =
                    jobs.into_iter().partition(|job| job.membership.callback);
                fake.record_batch(row, members.into_iter().map(envelope_of).collect());
                for callback in callbacks {
                    fake.record(envelope_of(callback));
                }
                Ok(())
            }
        }
    }

    /// Stores a batch and its jobs in the transaction of `uow` (database
    /// driver only; the others store once it commits).
    pub(crate) async fn insert_batch_in(
        &self,
        uow: &Uow,
        row: &BatchRow,
        jobs: &[NewJob],
    ) -> Result<(), QueueError> {
        match self {
            Store::Database(db) => db.insert_batch_in(uow, row, jobs).await,
            Store::Memory(_) | Store::Fake(_) => Ok(()),
        }
    }

    /// Finishes a reserved job of a batch (see
    /// [`MemoryStore::finish_batch_job`]).
    pub(crate) async fn finish_batch_job(
        &self,
        job: &ReservedJob,
        worker: &str,
        finish: Finish,
        error: &str,
        now: i64,
        prepare: &PrepareCallback<'_>,
    ) -> Result<Finished, QueueError> {
        match self {
            Store::Database(db) => {
                db.finish_batch_job(job, worker, finish, error, now, prepare)
                    .await
            }
            Store::Memory(memory) => {
                memory.finish_batch_job(job, worker, finish, error, now, prepare)
            }
            Store::Fake(_) => Ok(Finished::Lost),
        }
    }

    pub(crate) async fn find_batch(&self, id: &str) -> Result<Option<BatchRow>, QueueError> {
        match self {
            Store::Database(db) => db.find_batch(id).await,
            Store::Memory(memory) => Ok(memory.find_batch(id)),
            Store::Fake(fake) => Ok(fake.find_batch(id)),
        }
    }

    pub(crate) async fn batches(&self, limit: usize) -> Result<Vec<BatchRow>, QueueError> {
        match self {
            Store::Database(db) => db.batches(limit).await,
            Store::Memory(memory) => Ok(memory.batches(limit)),
            Store::Fake(fake) => Ok(fake.batch_rows(limit)),
        }
    }

    pub(crate) async fn cancel_batch(
        &self,
        id: &str,
        now: i64,
    ) -> Result<Option<BatchRow>, QueueError> {
        match self {
            Store::Database(db) => db.cancel_batch(id, now).await,
            Store::Memory(memory) => Ok(memory.cancel_batch(id, now)),
            Store::Fake(fake) => Ok(fake.cancel_batch(id, now)),
        }
    }

    pub(crate) async fn prune_batches(&self, before: i64) -> Result<u64, QueueError> {
        match self {
            Store::Database(db) => db.prune_batches(before).await,
            Store::Memory(memory) => Ok(memory.prune_batches(before)),
            Store::Fake(_) => Ok(0),
        }
    }

    /// How many stored jobs do the work of operation `operation_id`.
    pub(crate) async fn count_for_operation(&self, operation_id: &str) -> Result<u64, QueueError> {
        match self {
            Store::Database(db) => db.count_for_operation(operation_id).await,
            Store::Memory(memory) => Ok(memory.count_for_operation(operation_id)),
            Store::Fake(_) => Ok(0),
        }
    }
}

/// The envelope a fake records for `job` (its delay rounded up to whole
/// seconds).
pub(crate) fn envelope_of(job: NewJob) -> JobEnvelope {
    let delay_ms = job.available_at.saturating_sub(job.created_at).max(0);
    let delay_secs = u64::try_from(delay_ms.div_euclid(1000) + i64::from(delay_ms % 1000 != 0))
        .unwrap_or_default();
    JobEnvelope {
        job_type: job.job_type,
        payload: job.payload,
        options: job.options,
        delay_secs: (delay_secs > 0).then_some(delay_secs),
        unique_key: job.unique_key,
    }
}
