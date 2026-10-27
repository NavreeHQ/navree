//! The drivers behind [`Queue`](crate::Queue) and the public
//! [`QueueStore`] trait other crates implement drivers with.

mod database;
mod memory;

use std::sync::Arc;

pub(crate) use database::DatabaseStore;
pub(crate) use memory::MemoryStore;

use navree_contracts::{BoxFuture, JobEnvelope};

use crate::record::{
    FailedJob, NewJob, PendingJob, PendingQuery, QueueStatus, ReservedJob, RetryTarget, from_millis,
};
use crate::{QueueError, QueueFake};

/// A queue driver implemented outside this crate (navree-redis's Redis
/// driver is one): where jobs wait, are reserved by workers and end up when
/// they fail. Times are Unix milliseconds from the app's clock, passed in by
/// the queue; a store never reads its own clock to decide what is due.
///
/// The built-in `database` and `memory` drivers have the same semantics,
/// which a store must keep:
///
/// - **Order.** [`reserve`](QueueStore::reserve) takes the job of the queue
///   with the smallest `(available_at, id)` among those a worker may take:
///   unreserved jobs whose `available_at` has come, and reserved jobs whose
///   reservation is older than [`NewJob::stale_after_ms`] (a crashed
///   worker's). Ids are UUID v7, so equal times run in dispatch order.
/// - **Atomic reservations.** Two workers never reserve the same job at the
///   same time. Reserving counts an attempt and records the worker.
/// - **Ownership.** [`release`](QueueStore::release) and
///   [`fail`](QueueStore::fail) act only while `worker` still holds the
///   reservation, and return `false` otherwise (the job was taken over).
/// - **Unique jobs.** A job with a [`unique_key`](NewJob::unique_key) is not
///   stored while a pending job with the same key blocks it
///   ([`NewJob::blocked_by`]); `insert` returns the pending job's id. An
///   older pending job that no longer blocks loses its key.
/// - **Failed jobs** are kept until [`retry`](QueueStore::retry) (which puts
///   them back with [`FailedJob::requeued`]) or
///   [`flush_failed`](QueueStore::flush_failed).
///
/// Select a store with `queue.driver`: `redis` resolves the provider of
/// `dyn QueueStore` named `redis`. Jobs dispatched inside a unit of work
/// are inserted after it commits.
pub trait QueueStore: Send + Sync + 'static {
    /// Stores `job`; returns its id, or the id of the pending unique job
    /// that takes its place.
    fn insert(&self, job: NewJob) -> BoxFuture<'_, Result<String, QueueError>>;

    /// Reserves the next job of `queue` for `worker` at `now`: increments
    /// its attempts and records the reservation. `None` when no job is
    /// available.
    fn reserve<'a>(
        &'a self,
        queue: &'a str,
        worker: &'a str,
        now: i64,
    ) -> BoxFuture<'a, Result<Option<ReservedJob>, QueueError>>;

    /// Deletes `job`, which finished.
    fn complete<'a>(&'a self, job: &'a ReservedJob) -> BoxFuture<'a, Result<(), QueueError>>;

    /// Unreserves `job`, available again at `available_at`. `false` when
    /// `worker` no longer holds the reservation.
    fn release<'a>(
        &'a self,
        job: &'a ReservedJob,
        worker: &'a str,
        available_at: i64,
    ) -> BoxFuture<'a, Result<bool, QueueError>>;

    /// Moves `job` to the failed jobs with `error`, failed at `now`.
    /// `false` when `worker` no longer holds the reservation.
    fn fail<'a>(
        &'a self,
        job: &'a ReservedJob,
        worker: &'a str,
        error: &'a str,
        now: i64,
    ) -> BoxFuture<'a, Result<bool, QueueError>>;

    /// Every failed job, newest first (by `failed_at`, then id).
    fn failed(&self) -> BoxFuture<'_, Result<Vec<FailedJob>, QueueError>>;

    /// Moves the targeted failed jobs back to their queues at `now`
    /// ([`FailedJob::requeued`]); returns how many (0 for an unknown id).
    fn retry<'a>(
        &'a self,
        target: &'a RetryTarget,
        now: i64,
    ) -> BoxFuture<'a, Result<u64, QueueError>>;

    /// Deletes every failed job; returns how many.
    fn flush_failed(&self) -> BoxFuture<'_, Result<u64, QueueError>>;

    /// How many jobs of `queue` are not finished: waiting, delayed or
    /// reserved.
    fn pending_count<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<u64, QueueError>>;
}

/// The driver of a queue.
pub(crate) enum Store {
    Database(DatabaseStore),
    Memory(MemoryStore),
    /// Records jobs, never runs them.
    Fake(QueueFake),
    /// A driver from another crate (`queue.driver = "redis"`), named for
    /// `Debug`.
    Custom(&'static str, Arc<dyn QueueStore>),
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Store::Database(db) => f.debug_tuple("Database").field(db).finish(),
            Store::Memory(memory) => f.debug_tuple("Memory").field(memory).finish(),
            Store::Fake(fake) => f.debug_tuple("Fake").field(fake).finish(),
            Store::Custom(name, _) => f.debug_tuple("Custom").field(name).finish(),
        }
    }
}

impl Store {
    /// The driver's name, for `Debug`.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Store::Database(_) => "Database",
            Store::Memory(_) => "Memory",
            Store::Fake(_) => "Fake",
            Store::Custom(name, _) => name,
        }
    }

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
            Store::Custom(_, store) => store.insert(job).await,
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
            Store::Custom(_, store) => store.reserve(queue, worker, now).await,
        }
    }

    pub(crate) async fn complete(&self, job: &ReservedJob) -> Result<(), QueueError> {
        match self {
            Store::Database(db) => db.complete(&job.id).await,
            Store::Memory(memory) => {
                memory.complete(&job.id);
                Ok(())
            }
            Store::Fake(_) => Ok(()),
            Store::Custom(_, store) => store.complete(job).await,
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
            Store::Custom(_, store) => store.release(job, worker, available_at).await,
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
            Store::Custom(_, store) => store.fail(job, worker, error, now).await,
        }
    }

    pub(crate) async fn failed(&self) -> Result<Vec<FailedJob>, QueueError> {
        match self {
            Store::Database(db) => db.failed().await,
            Store::Memory(memory) => Ok(memory.failed()),
            Store::Fake(_) => Ok(Vec::new()),
            Store::Custom(_, store) => store.failed().await,
        }
    }

    /// Moves the targeted failed jobs back to their queues; returns how many.
    pub(crate) async fn retry(&self, target: &RetryTarget, now: i64) -> Result<u64, QueueError> {
        let retried = match self {
            Store::Database(db) => db.retry(target, now).await?,
            Store::Memory(memory) => memory.retry(target, now),
            Store::Fake(_) => 0,
            Store::Custom(_, store) => store.retry(target, now).await?,
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
            Store::Custom(_, store) => store.flush_failed().await,
        }
    }

    pub(crate) async fn pending_count(&self, queue: &str) -> Result<u64, QueueError> {
        match self {
            Store::Database(db) => db.pending_count(queue).await,
            Store::Memory(memory) => Ok(memory.pending_count(queue)),
            Store::Fake(fake) => Ok(fake.count_on(queue)),
            Store::Custom(_, store) => store.pending_count(queue).await,
        }
    }

    /// `queue`'s waiting, delayed and running jobs at `now`. A fake's
    /// recorded jobs all count as waiting since `now`.
    pub(crate) async fn status(&self, queue: &str, now: i64) -> Result<QueueStatus, QueueError> {
        match self {
            Store::Database(db) => db.status(queue, now).await,
            Store::Memory(memory) => Ok(memory.status(queue, now)),
            Store::Fake(fake) => {
                let waiting = fake.count_on(queue);
                Ok(QueueStatus {
                    queue: queue.to_owned(),
                    waiting,
                    delayed: 0,
                    running: 0,
                    oldest_available_at: (waiting > 0).then(|| from_millis(now)),
                    at: from_millis(now),
                })
            }
            // `QueueStore` reports only how many jobs are pending: they
            // all count as waiting, with no oldest wait.
            Store::Custom(_, store) => Ok(QueueStatus {
                queue: queue.to_owned(),
                waiting: store.pending_count(queue).await?,
                delayed: 0,
                running: 0,
                oldest_available_at: None,
                at: from_millis(now),
            }),
        }
    }

    pub(crate) async fn pending_jobs(
        &self,
        query: &PendingQuery,
        now: i64,
    ) -> Result<Vec<PendingJob>, QueueError> {
        match self {
            Store::Database(db) => db.pending_jobs(query, now).await,
            Store::Memory(memory) => Ok(memory.pending_jobs(query, now)),
            Store::Fake(_) | Store::Custom(..) => Ok(Vec::new()),
        }
    }

    pub(crate) async fn pending_job(
        &self,
        id: &str,
        now: i64,
    ) -> Result<Option<PendingJob>, QueueError> {
        match self {
            Store::Database(db) => db.pending_job(id, now).await,
            Store::Memory(memory) => Ok(memory.pending_job(id, now)),
            Store::Fake(_) | Store::Custom(..) => Ok(None),
        }
    }

    pub(crate) async fn failed_job(&self, id: &str) -> Result<Option<FailedJob>, QueueError> {
        match self {
            Store::Database(db) => db.failed_job(id).await,
            Store::Memory(memory) => Ok(memory.failed_job(id)),
            Store::Fake(_) => Ok(None),
            Store::Custom(_, store) => {
                Ok(store.failed().await?.into_iter().find(|job| job.id == id))
            }
        }
    }

    /// Deletes failed job `id`; [`QueueError::FailedJobNotFound`] when
    /// there is none.
    pub(crate) async fn forget(&self, id: &str) -> Result<(), QueueError> {
        let deleted = match self {
            Store::Database(db) => db.forget(id).await?,
            Store::Memory(memory) => memory.forget(id),
            Store::Fake(_) => false,
            Store::Custom(name, _) => {
                return Err(QueueError::Store(
                    format!("the {name} queue driver cannot forget one failed job").into(),
                ));
            }
        };
        if deleted {
            Ok(())
        } else {
            Err(QueueError::FailedJobNotFound { id: id.to_owned() })
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
