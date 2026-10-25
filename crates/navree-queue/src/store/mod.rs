//! The drivers behind [`Queue`](crate::Queue).

mod database;
mod memory;

pub(crate) use database::DatabaseStore;
pub(crate) use memory::MemoryStore;

use navree_contracts::JobEnvelope;

use crate::record::{FailedJob, NewJob, ReservedJob, RetryTarget};
use crate::{QueueError, QueueFake};

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
}

/// The envelope a fake records for `job` (its delay rounded up to whole
/// seconds).
fn envelope_of(job: NewJob) -> JobEnvelope {
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
