//! [`Queue`]: dispatching jobs and managing failed ones.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use navree_contracts::{BoxError, BoxFuture, Clock, Dispatcher, Job, JobEnvelope, JobRegistration};
use navree_db::Uow;
use navree_kernel::{AppContext, WeakAppContext};
use navree_telemetry::{Telemetry, TelemetryContext, TraceContext, TracePolicy};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::batch::{Batch, BatchRow, JobBatch};
use crate::operations::RunningJobs;
use crate::record::{
    FailedJob, FailedSummary, Membership, NewJob, PendingJob, PendingQuery, QueueStatus,
    RetryTarget,
};
use crate::store::Store;
use crate::{QueueError, QueueOptions};

/// The job queue: provided and exported by [`QueueModule`](crate::QueueModule),
/// which also registers it as the app's `dyn Dispatcher`.
///
/// Jobs are stored by the configured driver until a worker
/// ([`work`](Queue::work), command `queue:work`) runs them with the handler
/// a module registered (`m.job::<J>()`).
///
/// `Queue` is a cheap handle: clones share the same driver.
#[derive(Clone)]
pub struct Queue {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) store: Store,
    pub(crate) options: QueueOptions,
    /// The app's `dyn Clock`, if it registers one.
    clock: Option<Arc<dyn Clock>>,
    /// Job handlers by job type.
    pub(crate) registry: HashMap<&'static str, JobRegistration>,
    /// The app, for job contexts and the shutdown token.
    app: Option<WeakAppContext>,
    /// Where job attempts are reported (`JobProcessed`, `JobFailed`).
    pub(crate) telemetry: Telemetry,
    /// How each attempt's trace starts (`observe.tracing`).
    pub(crate) policy: TracePolicy,
    /// The jobs this process's workers are running.
    pub(crate) running: RunningJobs,
}

impl Queue {
    pub(crate) fn new(
        store: Store,
        options: QueueOptions,
        clock: Option<Arc<dyn Clock>>,
        registrations: impl IntoIterator<Item = JobRegistration>,
        app: Option<WeakAppContext>,
    ) -> Queue {
        let mut registry = HashMap::new();
        for registration in registrations {
            registry.entry(registration.name).or_insert(registration);
        }
        Queue {
            inner: Arc::new(Inner {
                store,
                options,
                clock,
                registry,
                app,
                telemetry: Telemetry::disabled(),
                policy: TracePolicy::default(),
                running: RunningJobs::default(),
            }),
        }
    }

    /// Reports job attempts to `telemetry` and traces them by `policy`
    /// (on a queue no clone was made of yet).
    pub(crate) fn instrumented(mut self, telemetry: Telemetry, policy: TracePolicy) -> Queue {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.telemetry = telemetry;
            inner.policy = policy;
        }
        self
    }

    /// Queues the job in `envelope` as the work of operation `operation`,
    /// as part of `uow`.
    pub(crate) async fn push_for_operation_in(
        &self,
        uow: &Uow,
        envelope: JobEnvelope,
        operation: Uuid,
    ) -> Result<String, QueueError> {
        let delay = envelope_delay(&envelope);
        let mut job = self.prepare(envelope, delay)?;
        job.membership.operation_id = Some(operation.to_string());
        self.store_in(uow, job).await
    }

    /// Queues the job in `envelope` as the work of operation `operation`
    /// (joining the current unit of work like [`dispatch`](Queue::dispatch)).
    pub(crate) async fn push_for_operation(
        &self,
        envelope: JobEnvelope,
        operation: Uuid,
    ) -> Result<String, QueueError> {
        let delay = envelope_delay(&envelope);
        let mut job = self.prepare(envelope, delay)?;
        job.membership.operation_id = Some(operation.to_string());
        self.store(job).await
    }

    /// The options the queue runs with.
    pub fn options(&self) -> &QueueOptions {
        &self.inner.options
    }

    /// Queues `job` to run as soon as a worker is free; returns its id.
    ///
    /// Inside a unit of work's scope ([`Uow::current`]: a request handler,
    /// `Db::transaction`) the job joins it, as with
    /// [`dispatch_in`](Queue::dispatch_in): it is queued only if that unit
    /// of work commits, and workers see it after the commit. (On SQLite
    /// this is also what keeps the insert from waiting for the
    /// transaction's write lock.) Elsewhere, or once the current unit of
    /// work has finished, the job is stored at once. The same holds for
    /// [`later`](Queue::later), [`push`](Queue::push) and `dyn Dispatcher`.
    pub async fn dispatch<J: Job>(&self, job: J) -> Result<String, QueueError> {
        self.push(envelope(&job)?).await
    }

    /// Queues `job` as part of the unit of work `uow`: with the database
    /// driver the row is written in its transaction, otherwise the job is
    /// queued once it commits. Either way a rolled-back unit of work queues
    /// nothing, and workers never see the job before the commit. Returns the
    /// id the job gets.
    pub async fn dispatch_in<J: Job>(&self, uow: &Uow, job: J) -> Result<String, QueueError> {
        self.push_in(uow, envelope(&job)?).await
    }

    /// Queues `job` to run no earlier than `delay` from now (millisecond
    /// precision); returns its id. Joins the current unit of work like
    /// [`dispatch`](Queue::dispatch).
    pub async fn later<J: Job>(&self, job: J, delay: Duration) -> Result<String, QueueError> {
        let job = self.prepare(envelope(&job)?, delay)?;
        self.store(job).await
    }

    /// Queues a serialized job (what `dyn Dispatcher` does), honouring its
    /// `delay_secs`; returns its id. Joins the current unit of work like
    /// [`dispatch`](Queue::dispatch).
    pub async fn push(&self, envelope: JobEnvelope) -> Result<String, QueueError> {
        let delay = envelope_delay(&envelope);
        let job = self.prepare(envelope, delay)?;
        self.store(job).await
    }

    /// Stores `job` in the current unit of work when there is an unfinished
    /// one, at once otherwise. `QueueFake` records at once.
    async fn store(&self, job: NewJob) -> Result<String, QueueError> {
        if !matches!(self.inner.store, Store::Fake(_))
            && let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return self.store_in(&uow, job).await;
        }
        self.inner.store.insert(job).await
    }

    /// Queues a serialized job as part of `uow` (see
    /// [`dispatch_in`](Queue::dispatch_in)); returns its id.
    ///
    /// With the memory and Redis drivers, a unique job is deduplicated when
    /// the unit of work commits, so the id returned here is the new job's id
    /// even if a pending duplicate later takes its place.
    pub async fn push_in(&self, uow: &Uow, envelope: JobEnvelope) -> Result<String, QueueError> {
        let delay = envelope_delay(&envelope);
        let job = self.prepare(envelope, delay)?;
        self.store_in(uow, job).await
    }

    /// Queues a serialized job from inside a
    /// [`ChangeObserver`](navree_db::ChangeObserver), as part of the commit
    /// it observes: `uow` is the unit of work the observer is given, which
    /// runs statements in the transaction being committed. With the
    /// database driver the row is written through it; otherwise the job is
    /// queued once `uow` has committed. A commit that fails queues nothing.
    /// Returns the id the job gets.
    pub async fn push_observed(
        &self,
        uow: &Uow,
        envelope: JobEnvelope,
    ) -> Result<String, QueueError> {
        let delay = envelope_delay(&envelope);
        let job = self.prepare(envelope, delay)?;
        self.store_in(uow, job).await
    }

    /// Stores `job` as part of `uow`.
    async fn store_in(&self, uow: &Uow, job: NewJob) -> Result<String, QueueError> {
        match &self.inner.store {
            Store::Database(store) => store.insert_in(uow, &job).await,
            Store::Memory(_) | Store::Fake(_) | Store::Custom(..) => {
                let id = job.id.clone();
                let queue = self.clone();
                uow.after_commit(move || {
                    Box::pin(async move {
                        let id = job.id.clone();
                        if let Err(error) = queue.inner.store.insert(job).await {
                            tracing::error!(target: "navree::job", job_id = %id, %error, "cannot queue a job after commit");
                        }
                    })
                });
                Ok(id)
            }
        }
    }

    /// Every failed job, newest first.
    pub async fn failed(&self) -> Result<Vec<FailedJob>, QueueError> {
        self.inner.store.failed().await
    }

    /// How many jobs failed, how many of them at or after `since`, and the
    /// newest `limit`, without their payloads: what to read on a page polled
    /// every few seconds, where [`failed`](Queue::failed) would read the
    /// whole table.
    pub async fn failed_summary(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<FailedSummary, QueueError> {
        self.inner.store.failed_summary(since, limit).await
    }

    /// Puts failed jobs back on their queues, to run as soon as possible
    /// with their attempts starting over (same id and options, not unique);
    /// returns how many. `retry("all")` or [`RetryTarget::All`] retries
    /// every failed job; an unknown id is
    /// [`QueueError::FailedJobNotFound`].
    pub async fn retry(&self, target: impl Into<RetryTarget>) -> Result<u64, QueueError> {
        let target = target.into();
        self.inner.store.retry(&target, self.now_ms()).await
    }

    /// Deletes every failed job; returns how many.
    pub async fn flush_failed(&self) -> Result<u64, QueueError> {
        self.inner.store.flush_failed().await
    }

    /// How many jobs of `queue` are not finished yet: waiting, delayed or
    /// running.
    pub async fn pending_count(&self, queue: &str) -> Result<u64, QueueError> {
        self.inner.store.pending_count(queue).await
    }

    /// The failed job with id `id`, if there is one.
    pub async fn failed_job(&self, id: &str) -> Result<Option<FailedJob>, QueueError> {
        self.inner.store.failed_job(id).await
    }

    /// Deletes one failed job (it is not retried);
    /// [`QueueError::FailedJobNotFound`] when there is none with that id.
    pub async fn forget(&self, id: &str) -> Result<(), QueueError> {
        self.inner.store.forget(id).await
    }

    /// `queue` now: how many jobs wait (available now), are delayed
    /// (available later) and run (reserved by a worker), and since when its
    /// oldest waiting job is available, so
    /// [`oldest_wait`](QueueStatus::oldest_wait) is how far the queue is
    /// behind. One query with the database driver.
    pub async fn status(&self, queue: &str) -> Result<QueueStatus, QueueError> {
        self.inner.store.status(queue, self.now_ms()).await
    }

    /// The jobs that are not finished yet, oldest first (by availability):
    /// waiting, delayed and running, filtered by `query`.
    pub async fn pending_jobs(&self, query: &PendingQuery) -> Result<Vec<PendingJob>, QueueError> {
        self.inner.store.pending_jobs(query, self.now_ms()).await
    }

    /// The unfinished job with id `id`, if there is one.
    pub async fn pending_job(&self, id: &str) -> Result<Option<PendingJob>, QueueError> {
        self.inner.store.pending_job(id, self.now_ms()).await
    }

    /// Dispatches `batch`: stores the batch and queues its jobs, all at
    /// once; returns the stored batch. Joins the current unit of work like
    /// [`dispatch`](Queue::dispatch). A batch with no jobs finishes at once
    /// (its `then` and `finally` are queued with it).
    ///
    /// Every job type in the batch, callbacks included, must be registered
    /// ([`QueueError::UnknownJob`] otherwise, and nothing is queued).
    pub async fn dispatch_batch(&self, batch: Batch) -> Result<JobBatch, QueueError> {
        let (row, jobs) = self.prepare_batch(batch)?;
        if !matches!(self.inner.store, Store::Fake(_))
            && let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return self.store_batch_in(&uow, row, jobs).await;
        }
        let batch = row.to_batch();
        self.inner.store.insert_batch(row, jobs).await?;
        Ok(batch)
    }

    /// Dispatches `batch` as part of the unit of work `uow` (see
    /// [`dispatch_in`](Queue::dispatch_in)): nothing is stored if it rolls
    /// back.
    pub async fn dispatch_batch_in(&self, uow: &Uow, batch: Batch) -> Result<JobBatch, QueueError> {
        let (row, jobs) = self.prepare_batch(batch)?;
        self.store_batch_in(uow, row, jobs).await
    }

    /// The batch `id`, with its counts as they are now.
    pub async fn find_batch(&self, id: &str) -> Result<Option<JobBatch>, QueueError> {
        Ok(self
            .inner
            .store
            .find_batch(id)
            .await?
            .map(|row| row.to_batch()))
    }

    /// The most recent batches, newest first, at most `limit`.
    pub async fn batches(&self, limit: usize) -> Result<Vec<JobBatch>, QueueError> {
        Ok(self
            .inner
            .store
            .batches(limit)
            .await?
            .iter()
            .map(BatchRow::to_batch)
            .collect())
    }

    /// Cancels batch `id`: its jobs that have not started are skipped, and
    /// running ones see it through `ctx.is_cancelled()` and
    /// `ctx.progress(..)`. `finally` still runs once every job has ended;
    /// `then` does not. A finished batch stays as it is. Returns the batch
    /// afterwards (`None` when there is none).
    pub async fn cancel_batch(&self, id: &str) -> Result<Option<JobBatch>, QueueError> {
        let now = self.now_ms();
        Ok(self
            .inner
            .store
            .cancel_batch(id, now)
            .await?
            .map(|row| row.to_batch()))
    }

    /// Deletes the batches whose jobs and callbacks all finished more than
    /// `older_than` ago; returns how many.
    pub async fn prune_batches(&self, older_than: Duration) -> Result<u64, QueueError> {
        let before = self
            .now_ms()
            .saturating_sub(i64::try_from(older_than.as_millis()).unwrap_or(i64::MAX));
        self.inner.store.prune_batches(before).await
    }

    /// The batch row and the jobs to store for `batch` (callbacks due at
    /// once included).
    fn prepare_batch(&self, batch: Batch) -> Result<(BatchRow, Vec<NewJob>), QueueError> {
        if let Store::Custom(name, _) = &self.inner.store {
            return Err(QueueError::Store(
                format!("the {name} queue driver does not support batches").into(),
            ));
        }
        batch.check()?;
        let fake = matches!(self.inner.store, Store::Fake(_));
        let callbacks = [
            &batch.callbacks.then,
            &batch.callbacks.catch,
            &batch.callbacks.finally,
        ];
        for callback in callbacks.into_iter().flatten() {
            if !fake && !self.inner.registry.contains_key(callback.job_type.as_str()) {
                return Err(QueueError::UnknownJob {
                    job_type: callback.job_type.clone(),
                });
            }
        }
        let now = self.now_ms();
        let mut row = BatchRow::new(Uuid::now_v7().to_string(), &batch, now);
        let mut jobs = Vec::with_capacity(batch.jobs.len());
        for envelope in batch.jobs {
            let mut job = self.prepare(not_unique(envelope), Duration::ZERO)?;
            job.membership = Membership::batch_job(&row.id, row.operation_id.as_deref());
            jobs.push(job);
        }
        for envelope in row.start_empty(now) {
            jobs.push(self.prepare_callback(envelope, &row)?);
        }
        Ok((row, jobs))
    }

    /// A batch callback that became due, as the job to store.
    pub(crate) fn prepare_callback(
        &self,
        envelope: JobEnvelope,
        row: &BatchRow,
    ) -> Result<NewJob, QueueError> {
        let mut job = self.prepare(not_unique(envelope), Duration::ZERO)?;
        job.membership = Membership::batch_callback(&row.id, row.operation_id.as_deref());
        Ok(job)
    }

    /// Stores a prepared batch as part of `uow`.
    async fn store_batch_in(
        &self,
        uow: &Uow,
        row: BatchRow,
        jobs: Vec<NewJob>,
    ) -> Result<JobBatch, QueueError> {
        let batch = row.to_batch();
        match &self.inner.store {
            Store::Database(_) => self.inner.store.insert_batch_in(uow, &row, &jobs).await?,
            Store::Memory(_) | Store::Fake(_) | Store::Custom(..) => {
                let queue = self.clone();
                uow.after_commit(move || {
                    Box::pin(async move {
                        let id = row.id.clone();
                        if let Err(error) = queue.inner.store.insert_batch(row, jobs).await {
                            tracing::error!(target: "navree::job", batch_id = %id, %error, "cannot queue a batch after commit");
                        }
                    })
                });
            }
        }
        Ok(batch)
    }

    /// The current time in Unix milliseconds (the app's `dyn Clock` when it
    /// has one).
    pub(crate) fn now_ms(&self) -> i64 {
        match &self.inner.clock {
            Some(clock) => clock.now().timestamp_millis(),
            None => chrono::Utc::now().timestamp_millis(),
        }
    }

    /// The app, while it is alive.
    pub(crate) fn app(&self) -> Result<AppContext, QueueError> {
        self.inner
            .app
            .as_ref()
            .and_then(WeakAppContext::upgrade)
            .ok_or(QueueError::ShutDown)
    }

    /// Checks the job type and fills in the queue, times (available after
    /// `delay`) and id.
    fn prepare(&self, envelope: JobEnvelope, delay: Duration) -> Result<NewJob, QueueError> {
        let fake = matches!(self.inner.store, Store::Fake(_));
        if !fake && !self.inner.registry.contains_key(envelope.job_type.as_str()) {
            return Err(QueueError::UnknownJob {
                job_type: envelope.job_type,
            });
        }
        let now = self.now_ms();
        let delay_ms = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
        let mut options = envelope.options;
        if options.queue.trim().is_empty() {
            options.queue.clone_from(&self.inner.options.default_queue);
        }
        let unique_key = match (envelope.unique_key, options.unique_for_secs) {
            (Some(key), _) => Some(key),
            (None, Some(_)) => Some(unique_key(&envelope.job_type, &envelope.payload)),
            (None, None) => None,
        };
        let unique_for_ms = options
            .unique_for_secs
            .map(|secs| i64::try_from(secs.saturating_mul(1000)).unwrap_or(i64::MAX));
        Ok(NewJob {
            id: Uuid::now_v7().to_string(),
            queue: options.queue.clone(),
            job_type: envelope.job_type,
            payload: envelope.payload,
            options,
            available_at: now.saturating_add(delay_ms),
            created_at: now,
            unique_key,
            unique_for_ms,
            trace_parent: current_traceparent(),
            membership: Membership::default(),
        })
    }
}

/// The `traceparent` of the current request's or job's span, when it has a
/// trace: a dispatched job continues it.
fn current_traceparent() -> Option<String> {
    TelemetryContext::with_current(|context| context.trace().map(TraceContext::traceparent))
        .flatten()
}

/// `envelope` without uniqueness: every job of a batch counts.
fn not_unique(mut envelope: JobEnvelope) -> JobEnvelope {
    envelope.unique_key = None;
    envelope.options.unique_for_secs = None;
    envelope
}

/// The envelope's `delay_secs`.
fn envelope_delay(envelope: &JobEnvelope) -> Duration {
    Duration::from_secs(envelope.delay_secs.unwrap_or(0))
}

/// `<job type>:<hex SHA-256 of the JSON payload>`.
fn unique_key(job_type: &str, payload: &serde_json::Value) -> String {
    let digest = Sha256::digest(payload.to_string().as_bytes());
    format!("{job_type}:{}", hex::encode(digest))
}

/// The envelope of `job`.
fn envelope<J: Job>(job: &J) -> Result<JobEnvelope, QueueError> {
    Ok(JobEnvelope {
        job_type: J::NAME.to_owned(),
        payload: serde_json::to_value(job).map_err(QueueError::Payload)?,
        options: J::options(),
        delay_secs: None,
        unique_key: None,
    })
}

impl Dispatcher for Queue {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        Box::pin(async move { Ok(self.push(env).await?) })
    }
}

impl fmt::Debug for Queue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut job_types: Vec<_> = self.inner.registry.keys().collect();
        job_types.sort();
        f.debug_struct("Queue")
            .field("driver", &self.inner.store.name())
            .field("job_types", &job_types)
            .finish_non_exhaustive()
    }
}
