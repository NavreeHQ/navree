//! Operations: user-visible long work (an import, an export, a bulk
//! update) with an owner, a workspace, a status, progress, a result and a
//! heartbeat, stored in `operations`.

mod controller;
mod hooks;
mod sql;

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_contracts::{BoxError, Broadcaster, Job, JobEnvelope, Principal, permitted};
use navree_db::{Db, Uow};
use navree_kernel::{AppContext, WeakAppContext};
use sea_orm::{ConnectionTrait, Statement};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use utoipa::ToSchema;
use uuid::Uuid;

pub(crate) use controller::authorize_channel;
pub use controller::{OPERATION_EVENT, OperationsController};
pub(crate) use hooks::{QueueMonitor, RunningJobs, batch_settled, job_finished, skips};

use crate::batch::{Batch, JobBatch};
use crate::{OperationsOptions, Queue, QueueError};

/// The realtime event every change of an operation is broadcast as, on the
/// private channel `operations.{id}`.
pub const OPERATION_UPDATED_EVENT: &str = "operation.updated";

/// Where an operation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// Started, waiting for its work to begin.
    Pending,
    /// Its work is running.
    Running,
    /// It finished; `result` may hold what it produced.
    Succeeded,
    /// It failed; `error` says why.
    Failed,
    /// It was cancelled.
    Cancelled,
}

impl OperationStatus {
    /// Whether the operation has finished (succeeded, failed or was
    /// cancelled): it never changes again.
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            OperationStatus::Succeeded | OperationStatus::Failed | OperationStatus::Cancelled
        )
    }

    /// The stored name (`running`).
    pub fn as_str(self) -> &'static str {
        match self {
            OperationStatus::Pending => "pending",
            OperationStatus::Running => "running",
            OperationStatus::Succeeded => "succeeded",
            OperationStatus::Failed => "failed",
            OperationStatus::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "pending" => OperationStatus::Pending,
            "running" => OperationStatus::Running,
            "succeeded" => OperationStatus::Succeeded,
            "failed" => OperationStatus::Failed,
            "cancelled" => OperationStatus::Cancelled,
            _ => return None,
        })
    }
}

impl fmt::Display for OperationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An operation to start with [`Operations::start`]:
/// `Operation::new("export").owner(user_id).workspace(workspace_id)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Operation {
    pub(crate) kind: String,
    pub(crate) owner_id: Option<Uuid>,
    pub(crate) workspace_id: Option<Uuid>,
    pub(crate) total: u64,
    pub(crate) note: String,
    pub(crate) idempotent: bool,
}

impl Operation {
    /// An operation of kind `kind` (`export`, `recipients.import`), for
    /// people and front ends to tell operations apart.
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            ..Self::default()
        }
    }

    /// The user who started it: they may always see and cancel it.
    pub fn owner(mut self, user_id: Uuid) -> Self {
        self.owner_id = Some(user_id);
        self
    }

    /// The workspace it belongs to: members whose role there grants
    /// `operations.view` may see it (`operations.cancel` to cancel it).
    pub fn workspace(mut self, workspace_id: Uuid) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    /// The units of work, when known up front (rows to import).
    pub fn total(mut self, total: u64) -> Self {
        self.total = total;
        self
    }

    /// The first progress note (`"Waiting for a worker"`).
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    /// Its job may safely run again from the start: when the operation
    /// stops responding and no job of it is left in the queue, the
    /// watchdog dispatches the job again (up to
    /// `queue.operations.max_redispatches` times) instead of failing it.
    pub fn idempotent(mut self) -> Self {
        self.idempotent = true;
        self
    }
}

/// How far an operation got.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OperationProgress {
    /// Units done.
    pub done: u64,
    /// Units in all (0 when unknown).
    pub total: u64,
    /// `done` as a whole percentage of `total` (0 when `total` is 0).
    pub percent: u8,
    /// What the operation is doing, for people.
    pub note: String,
}

/// A stored operation: what [`Operations`] returns, `GET
/// /api/operations/{id}` answers, the SSE stream sends and
/// `operation.updated` broadcasts.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct OperationRecord {
    /// The operation id.
    pub id: Uuid,
    /// Its kind (`export`).
    pub kind: String,
    /// Where it is.
    pub status: OperationStatus,
    /// How far it got.
    pub progress: OperationProgress,
    /// What it produced (set by [`Operations::complete`]).
    pub result: Option<Value>,
    /// Why it failed.
    pub error: Option<String>,
    /// The user who started it.
    pub owner_id: Option<Uuid>,
    /// The workspace it belongs to.
    pub workspace_id: Option<Uuid>,
    /// The batch doing its work.
    pub batch_id: Option<String>,
    /// When it was started.
    pub created_at: DateTime<Utc>,
    /// When its work began.
    pub started_at: Option<DateTime<Utc>>,
    /// When its work last showed signs of life.
    pub heartbeat_at: Option<DateTime<Utc>>,
    /// When it finished.
    pub finished_at: Option<DateTime<Utc>>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// Bumped on every change (the SSE stream's event id).
    #[serde(skip)]
    pub(crate) version: i64,
    /// The job to dispatch again when an idempotent operation stalls.
    #[serde(skip)]
    pub(crate) job: Option<Value>,
    #[serde(skip)]
    pub(crate) idempotent: bool,
    #[serde(skip)]
    pub(crate) redispatches: u32,
}

impl OperationRecord {
    /// Whether the operation has finished.
    pub fn is_finished(&self) -> bool {
        self.status.is_finished()
    }

    /// The private realtime channel its changes are broadcast on,
    /// `private-operations.{id}`.
    pub fn channel(&self) -> String {
        channel_name(self.id)
    }
}

/// `private-operations.{id}`.
pub(crate) fn channel_name(id: Uuid) -> String {
    format!("private-operations.{id}")
}

/// What one [`Operations::sweep`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SweepReport {
    /// Operations that stopped responding and were marked failed.
    pub failed: u64,
    /// Idempotent operations whose job was dispatched again.
    pub redispatched: u64,
    /// Operations whose batch had finished, now finished too.
    pub finished: u64,
}

/// User-visible long work: provided and exported by
/// [`QueueModule`](crate::QueueModule) (it needs `DatabaseModule`).
///
/// ```no_run
/// # use navree_contracts::{BoxError, Job, JobContext};
/// # use serde::{Deserialize, Serialize};
/// # #[derive(Serialize, Deserialize)] struct ExportDeals { operation: uuid::Uuid }
/// # impl Job for ExportDeals { const NAME: &'static str = "deals.export"; async fn handle(self, _: JobContext) -> Result<(), BoxError> { Ok(()) } }
/// use navree_queue::{Operation, Operations};
///
/// # async fn example(operations: &Operations, uow: &navree_db::Uow, user: uuid::Uuid) -> Result<(), navree_queue::QueueError> {
/// let op = operations
///     .start(uow, Operation::new("deals.export").owner(user))
///     .await?;
/// operations
///     .dispatch(uow, &op, ExportDeals { operation: op.id })
///     .await?;
/// // The client follows GET /api/operations/{op.id}/events.
/// # Ok(())
/// # }
/// ```
///
/// - [`start`](Operations::start) stores it `pending`;
///   [`dispatch`](Operations::dispatch) and
///   [`dispatch_batch`](Operations::dispatch_batch) queue its work.
/// - Its job's `ctx.progress(done, total, note)` makes it `running` and
///   records the progress; while the job runs the worker beats its
///   heartbeat every `queue.operations.heartbeat_secs`.
/// - It finishes when its job ends: `succeeded` (keeping any result set
///   with [`complete`](Operations::complete)), `failed`, or `cancelled`.
///   With a batch it finishes once the batch and its callbacks have run.
/// - [`cancel`](Operations::cancel) is cooperative: jobs not started are
///   skipped, running ones get `JobCancelled` from their next
///   `ctx.progress`.
/// - Every change is broadcast as `operation.updated` on
///   `private-operations.{id}` (when the app has a `dyn Broadcaster`).
pub struct Operations {
    db: Option<Db>,
    queue: Queue,
    options: OperationsOptions,
    changes: broadcast::Sender<Uuid>,
    app: Option<WeakAppContext>,
}

/// How many change notices the SSE streams may fall behind before they
/// reload.
const CHANGE_BUFFER: usize = 1024;

impl Operations {
    pub(crate) fn new(
        db: Option<Db>,
        queue: Queue,
        options: OperationsOptions,
        app: Option<WeakAppContext>,
    ) -> Self {
        let (changes, _) = broadcast::channel(CHANGE_BUFFER);
        Self {
            db,
            queue,
            options,
            changes,
            app,
        }
    }

    /// The options operations run with (`queue.operations`).
    pub fn options(&self) -> &OperationsOptions {
        &self.options
    }

    /// The app database; [`QueueError::NoDatabase`] in an app without
    /// `DatabaseModule`.
    fn db(&self) -> Result<&Db, QueueError> {
        self.db.as_ref().ok_or(QueueError::NoDatabase)
    }

    /// Stores `operation` as `pending`, as part of `uow`; returns it.
    pub async fn start(
        &self,
        uow: &Uow,
        operation: Operation,
    ) -> Result<OperationRecord, QueueError> {
        let now = self.queue.now_ms();
        let record = sql::new_record(Uuid::now_v7(), &operation, now);
        let tx = uow.tx().await?;
        tx.execute_raw(sql::insert(uow.db().backend(), &record)?)
            .await?;
        Ok(record)
    }

    /// Queues `job` as the work of `operation`, as part of `uow`; returns
    /// the job id. The job's progress reports go to the operation, and the
    /// operation finishes when the job ends. For an
    /// [`idempotent`](Operation::idempotent) operation the job is kept so
    /// the watchdog can dispatch it again.
    pub async fn dispatch<J: Job>(
        &self,
        uow: &Uow,
        operation: &OperationRecord,
        job: J,
    ) -> Result<String, QueueError> {
        let envelope = JobEnvelope {
            job_type: J::NAME.to_owned(),
            payload: serde_json::to_value(&job).map_err(QueueError::Payload)?,
            options: J::options(),
            delay_secs: None,
            unique_key: None,
        };
        if operation.idempotent {
            let tx = uow.tx().await?;
            tx.execute_raw(sql::set_job(uow.db().backend(), operation.id, &envelope)?)
                .await?;
        }
        self.queue
            .push_for_operation_in(uow, envelope, operation.id)
            .await
    }

    /// Dispatches `batch` as the work of `operation`, as part of `uow`
    /// (see [`Batch::operation`]): the operation's progress follows the
    /// batch's finished jobs, and it finishes once the batch and its
    /// callbacks have run.
    pub async fn dispatch_batch(
        &self,
        uow: &Uow,
        operation: &OperationRecord,
        batch: Batch,
    ) -> Result<JobBatch, QueueError> {
        let total = batch.len() as u64;
        let batch = self
            .queue
            .dispatch_batch_in(uow, batch.operation(operation.id))
            .await?;
        let tx = uow.tx().await?;
        tx.execute_raw(sql::set_batch(
            uow.db().backend(),
            operation.id,
            &batch.id,
            total,
            self.queue.now_ms(),
        ))
        .await?;
        Ok(batch)
    }

    /// The operation `id`.
    pub async fn find(&self, id: Uuid) -> Result<Option<OperationRecord>, QueueError> {
        sql::find(self.db()?.conn(), self.db()?.backend(), id).await
    }

    /// The operations `owner` started, newest first, at most `limit`.
    pub async fn for_owner(
        &self,
        owner: Uuid,
        limit: usize,
    ) -> Result<Vec<OperationRecord>, QueueError> {
        sql::for_owner(self.db()?.conn(), self.db()?.backend(), owner, limit).await
    }

    /// Records progress: the operation becomes `running` with `done` of
    /// `total` and `note`, and its heartbeat is renewed. A finished
    /// operation does not change. Returns the operation afterwards.
    pub async fn progress(
        &self,
        id: Uuid,
        done: u64,
        total: u64,
        note: impl Into<String>,
    ) -> Result<Option<OperationRecord>, QueueError> {
        let now = self.queue.now_ms();
        let stmt = sql::progress(self.db()?.backend(), id, done, total, &note.into(), now);
        self.update(id, stmt).await
    }

    /// Renews the heartbeat of a pending or running operation (work that
    /// runs outside the queue calls this to show it is alive).
    pub async fn heartbeat(&self, id: Uuid) -> Result<(), QueueError> {
        let stmt = sql::heartbeat(self.db()?.backend(), id, self.queue.now_ms());
        self.execute(stmt).await.map(|_| ())
    }

    /// Finishes the operation as `succeeded` with `result` (what a front
    /// end needs: a download URL, counts). A finished operation does not
    /// change. Returns the operation afterwards.
    pub async fn complete(
        &self,
        id: Uuid,
        result: impl Serialize,
    ) -> Result<Option<OperationRecord>, QueueError> {
        let result = serde_json::to_value(result).map_err(QueueError::Payload)?;
        let now = self.queue.now_ms();
        let stmt = sql::finish(
            self.db()?.backend(),
            id,
            OperationStatus::Succeeded,
            Some(&result),
            None,
            now,
        )?;
        self.update(id, stmt).await
    }

    /// Finishes the operation as `failed` with `error`, a message for
    /// people. A finished operation does not change.
    pub async fn fail(
        &self,
        id: Uuid,
        error: impl Into<String>,
    ) -> Result<Option<OperationRecord>, QueueError> {
        let now = self.queue.now_ms();
        let error = error.into();
        let stmt = sql::finish(
            self.db()?.backend(),
            id,
            OperationStatus::Failed,
            None,
            Some(&error),
            now,
        )?;
        self.update(id, stmt).await
    }

    /// Cancels the operation: it becomes `cancelled` at once, its batch is
    /// cancelled, its jobs that have not started are skipped, and running
    /// ones stop at their next `ctx.progress` (cooperative). A finished
    /// operation does not change. Returns the operation afterwards (`None`
    /// when there is none).
    pub async fn cancel(&self, id: Uuid) -> Result<Option<OperationRecord>, QueueError> {
        let now = self.queue.now_ms();
        let stmt = sql::finish(
            self.db()?.backend(),
            id,
            OperationStatus::Cancelled,
            None,
            None,
            now,
        )?;
        let record = self.update(id, stmt).await?;
        if let Some(record) = &record
            && record.status == OperationStatus::Cancelled
            && let Some(batch_id) = &record.batch_id
        {
            self.queue.cancel_batch(batch_id).await?;
        }
        Ok(record)
    }

    /// The watchdog (command `queue:watchdog`; schedule it every minute).
    /// For each pending or running operation without a heartbeat for
    /// `queue.operations.stale_after_secs`:
    ///
    /// - a job of it still in the queue means the queue still owns the
    ///   work (a crashed worker's reservation is taken over by another):
    ///   left alone;
    /// - its batch finished: it finishes the way the batch did;
    /// - [`idempotent`](Operation::idempotent) with a job: the job is
    ///   dispatched again (at most `max_redispatches` times);
    /// - otherwise: `failed`, "The operation stopped responding.".
    ///
    /// Each step first claims the operation on its row (it must still be
    /// stale), so sweeps that overlap (two nodes, or the schedule and a
    /// manual `queue:watchdog`) restart or fail an operation once, and a
    /// heartbeat that arrives during the sweep keeps it alive.
    pub async fn sweep(&self) -> Result<SweepReport, QueueError> {
        let now = self.queue.now_ms();
        let stale_ms =
            i64::try_from(self.options.stale_after_secs.saturating_mul(1000)).unwrap_or(i64::MAX);
        let before = now.saturating_sub(stale_ms);
        let backend = self.db()?.backend();
        let stale = sql::stale(self.db()?.conn(), backend, before).await?;
        let mut report = SweepReport::default();
        for record in stale {
            let id = record.id.to_string();
            if self.queue.inner.store.count_for_operation(&id).await? > 0 {
                continue;
            }
            if let Some(batch_id) = &record.batch_id
                && let Some(row) = self.queue.inner.store.find_batch(batch_id).await?
                && row.settled()
            {
                let finish = self.batch_finish(record.id, &row, now)?;
                if self.apply(record.id, finish).await?.0 {
                    report.finished += 1;
                }
                continue;
            }
            if record.idempotent
                && let Some(job) = record
                    .job
                    .clone()
                    .and_then(|job| serde_json::from_value::<JobEnvelope>(job).ok())
                && record.redispatches < self.options.max_redispatches
            {
                let claim = sql::redispatched(backend, record.id, before, now);
                if !self.apply(record.id, claim).await?.0 {
                    // Another sweep restarted it, or it came back to life.
                    continue;
                }
                self.queue.push_for_operation(job, record.id).await?;
                tracing::warn!(
                    target: "navree::job",
                    operation_id = %record.id,
                    kind = %record.kind,
                    redispatches = record.redispatches + 1,
                    "operation stopped responding; its job was dispatched again"
                );
                report.redispatched += 1;
                continue;
            }
            let stalled = sql::stalled(backend, record.id, STALLED_MESSAGE, before, now);
            if !self.apply(record.id, stalled).await?.0 {
                continue;
            }
            tracing::warn!(
                target: "navree::job",
                operation_id = %record.id,
                kind = %record.kind,
                "operation stopped responding; marked failed"
            );
            report.failed += 1;
        }
        Ok(report)
    }

    /// Deletes the operations that finished more than `older_than` ago;
    /// returns how many.
    pub async fn prune(&self, older_than: Duration) -> Result<u64, QueueError> {
        let before = self
            .queue
            .now_ms()
            .saturating_sub(i64::try_from(older_than.as_millis()).unwrap_or(i64::MAX));
        let stmt = sql::prune(self.db()?.backend(), before);
        Ok(self.db()?.conn().execute_raw(stmt).await?.rows_affected())
    }

    /// Whether `principal` may see `operation`: its owner, or a holder of
    /// `queue.operations.view_permission` (`operations.view`) app-wide or
    /// in the operation's workspace.
    pub async fn can_view(
        &self,
        principal: &Principal,
        operation: &OperationRecord,
    ) -> Result<bool, BoxError> {
        self.allowed(principal, operation, &self.options.view_permission)
            .await
    }

    /// Whether `principal` may cancel `operation`: its owner, or a holder
    /// of `queue.operations.cancel_permission` (`operations.cancel`)
    /// app-wide or in the operation's workspace.
    pub async fn can_cancel(
        &self,
        principal: &Principal,
        operation: &OperationRecord,
    ) -> Result<bool, BoxError> {
        self.allowed(principal, operation, &self.options.cancel_permission)
            .await
    }

    async fn allowed(
        &self,
        principal: &Principal,
        operation: &OperationRecord,
        permission: &str,
    ) -> Result<bool, BoxError> {
        if operation.owner_id == Some(principal.user_id) {
            return Ok(true);
        }
        let Some(app) = self.app() else {
            return Ok(false);
        };
        permitted(&app, principal, permission, operation.workspace_id).await
    }

    /// A receiver of the ids of operations that changed in this process.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Uuid> {
        self.changes.subscribe()
    }

    /// Finishes the operation the way its settled batch went.
    pub(crate) async fn finish_from_batch(
        &self,
        id: Uuid,
        row: &crate::batch::BatchRow,
    ) -> Result<Option<OperationRecord>, QueueError> {
        let stmt = self.batch_finish(id, row, self.queue.now_ms())?;
        self.update(id, stmt).await
    }

    /// The statement that finishes operation `id` the way its settled batch
    /// `row` went.
    fn batch_finish(
        &self,
        id: Uuid,
        row: &crate::batch::BatchRow,
        now: i64,
    ) -> Result<Statement, QueueError> {
        let backend = self.db()?.backend();
        if row.cancelled_at.is_some() && row.failed_jobs == 0 {
            sql::finish(backend, id, OperationStatus::Cancelled, None, None, now)
        } else if row.failed_jobs > 0 && !row.allow_failures {
            let error = format!("{} of {} jobs failed.", row.failed_jobs, row.total_jobs);
            sql::finish(
                backend,
                id,
                OperationStatus::Failed,
                None,
                Some(&error),
                now,
            )
        } else if row.callbacks_failed > 0 {
            let error = "A follow-up job of the batch failed.";
            sql::finish(backend, id, OperationStatus::Failed, None, Some(error), now)
        } else {
            sql::finish(backend, id, OperationStatus::Succeeded, None, None, now)
        }
    }

    /// Records a batch's finished jobs as the operation's progress.
    pub(crate) async fn batch_progress(
        &self,
        id: Uuid,
        row: &crate::batch::BatchRow,
    ) -> Result<Option<OperationRecord>, QueueError> {
        let now = self.queue.now_ms();
        let done = row.total_jobs.saturating_sub(row.pending_jobs);
        let stmt = sql::counts(self.db()?.backend(), id, done, row.total_jobs, now);
        self.update(id, stmt).await
    }

    /// Records a progress note (and renews the heartbeat) without
    /// changing the counts: what a batch job's `ctx.progress` does.
    pub(crate) async fn note(
        &self,
        id: Uuid,
        note: &str,
    ) -> Result<Option<OperationRecord>, QueueError> {
        let stmt = sql::note(self.db()?.backend(), id, note, self.queue.now_ms());
        self.update(id, stmt).await
    }

    /// Finishes the operation as `succeeded` (its own job succeeded),
    /// keeping a result already set.
    pub(crate) async fn job_succeeded(
        &self,
        id: Uuid,
    ) -> Result<Option<OperationRecord>, QueueError> {
        let now = self.queue.now_ms();
        let stmt = sql::finish(
            self.db()?.backend(),
            id,
            OperationStatus::Succeeded,
            None,
            None,
            now,
        )?;
        self.update(id, stmt).await
    }

    /// Marks the operation running (its job started).
    pub(crate) async fn mark_running(
        &self,
        id: Uuid,
    ) -> Result<Option<OperationRecord>, QueueError> {
        let stmt = sql::running(self.db()?.backend(), id, self.queue.now_ms());
        self.update(id, stmt).await
    }

    /// The status of operation `id`.
    pub(crate) async fn status(&self, id: Uuid) -> Result<Option<OperationStatus>, QueueError> {
        sql::status(self.db()?.conn(), self.db()?.backend(), id).await
    }

    fn app(&self) -> Option<AppContext> {
        self.app.as_ref().and_then(WeakAppContext::upgrade)
    }

    /// Runs `stmt` in the current unit of work when there is an
    /// unfinished one, else on its own; returns the rows it changed.
    async fn execute(&self, stmt: Statement) -> Result<u64, QueueError> {
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            let tx = uow.tx().await?;
            return Ok(tx.execute_raw(stmt).await?.rows_affected());
        }
        Ok(self.db()?.conn().execute_raw(stmt).await?.rows_affected())
    }

    /// Runs an update of operation `id` (in the current unit of work when
    /// there is an unfinished one) and returns the operation afterwards.
    /// When it changed, the change is announced: to this process's SSE
    /// streams and as `operation.updated` on its channel, after the commit
    /// of the unit of work it joined.
    async fn update(
        &self,
        id: Uuid,
        stmt: Statement,
    ) -> Result<Option<OperationRecord>, QueueError> {
        Ok(self.apply(id, stmt).await?.1)
    }

    /// [`update`](Operations::update), also telling whether `stmt` changed
    /// the operation.
    async fn apply(
        &self,
        id: Uuid,
        stmt: Statement,
    ) -> Result<(bool, Option<OperationRecord>), QueueError> {
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            let tx = uow.tx().await?;
            let changed = tx.execute_raw(stmt).await?.rows_affected() > 0;
            let record = sql::find(&*tx, uow.db().backend(), id).await?;
            drop(tx);
            if changed && let Some(record) = record.clone() {
                let announcer = self.announcer();
                uow.after_commit(move || {
                    Box::pin(async move { announcer.announce(&record).await })
                });
            }
            return Ok((changed, record));
        }
        let changed = self.db()?.conn().execute_raw(stmt).await?.rows_affected() > 0;
        let record = self.find(id).await?;
        if changed && let Some(record) = &record {
            self.announcer().announce(record).await;
        }
        Ok((changed, record))
    }

    fn announcer(&self) -> Announcer {
        Announcer {
            changes: self.changes.clone(),
            app: self.app.clone(),
        }
    }
}

/// Announces changed operations.
struct Announcer {
    changes: broadcast::Sender<Uuid>,
    app: Option<WeakAppContext>,
}

impl Announcer {
    async fn announce(&self, record: &OperationRecord) {
        // No receiver is not an error.
        let _ = self.changes.send(record.id);
        let Some(app) = self.app.as_ref().and_then(WeakAppContext::upgrade) else {
            return;
        };
        let Some(broadcaster) = app.try_get::<dyn Broadcaster>().await else {
            return;
        };
        let data = match serde_json::to_value(record) {
            Ok(data) => data,
            Err(error) => {
                tracing::warn!(target: "navree::job", %error, "cannot serialize an operation");
                return;
            }
        };
        if let Err(error) = broadcaster
            .broadcast(
                vec![channel_name(record.id)],
                OPERATION_UPDATED_EVENT.to_owned(),
                data,
            )
            .await
        {
            tracing::warn!(target: "navree::job", operation_id = %record.id, %error, "cannot broadcast an operation update");
        }
    }
}

/// The error of an operation the watchdog found stalled.
pub const STALLED_MESSAGE: &str = "The operation stopped responding.";

/// The error of an operation whose job failed.
pub const FAILED_MESSAGE: &str = "The operation failed.";

impl fmt::Debug for Operations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operations")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}
