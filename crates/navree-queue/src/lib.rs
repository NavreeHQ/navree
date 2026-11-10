//! # navree-queue
//!
//! `QueueModule`: database and memory job queues, workers, retries with
//! backoff, unique and delayed jobs, job batches, and operations
//! (user-visible long work with progress).
//!
//! - [`QueueModule`] is global. It provides [`Queue`] and registers it as the
//!   app's `dyn Dispatcher`, so navree-mail, navree-events, navree-notify and
//!   navree-schedule queue through it without depending on this crate. Its
//!   settings are [`QueueOptions`] (configuration section `queue`).
//! - Jobs implement [`navree_contracts::Job`]; a module registers each job type
//!   with `m.job::<J>()` ([`ModuleBuilderJobsExt`](navree_contracts::ModuleBuilderJobsExt)).
//!   Dispatching a type no module registered fails with
//!   [`QueueError::UnknownJob`].
//! - Drivers ([`QueueDriver`]): `database` (tables `jobs` and `failed_jobs`,
//!   created by the `queue` migration set; workers in several processes
//!   share them), `memory` (one process, for tests and development) and
//!   `redis` (the [`QueueStore`] provider named `redis`, from navree-redis's
//!   `RedisModule`). They behave the same; [`QueueStore`] states the
//!   contract other drivers implement.
//! - Workers ([`Queue::work`], command `queue:work`) reserve jobs atomically
//!   (SQLite: `UPDATE … WHERE id = (SELECT … LIMIT 1) RETURNING`;
//!   PostgreSQL adds `FOR UPDATE SKIP LOCKED`; MySQL, which cannot return
//!   the rows of an `UPDATE`, locks the candidate with `SELECT … FOR UPDATE
//!   SKIP LOCKED` in a short transaction first), run each attempt within the
//!   job's `timeout_secs` (a panic or a timeout is a failed attempt), retry
//!   failed attempts after [`Backoff::delay_for`](navree_contracts::Backoff::delay_for)
//!   and move a job that used its `max_attempts` to the failed jobs
//!   ([`Queue::failed`], [`Queue::retry`], [`Queue::flush_failed`]). A
//!   reservation abandoned by a crashed worker is taken over once it is older
//!   than the job's timeout plus [`STALE_RESERVATION_GRACE_SECS`].
//! - Unique jobs (`unique_for_secs`): while an identical job (same type and
//!   payload, or the envelope's `unique_key`) is pending and inside the
//!   window, dispatching returns the pending job's id instead of queueing.
//! - [`Queue::dispatch_in`] queues as part of a unit of work: nothing is
//!   queued if it rolls back. Plain dispatches (and `dyn Dispatcher`) do
//!   the same with the current unit of work (`navree_db::Uow::current`, e.g.
//!   the request's while a handler runs) when there is an unfinished one.
//! - Batches ([`Batch`], table `job_batches`): jobs dispatched together
//!   and followed as one, with `then`, `catch` and `finally` callbacks;
//!   the first failure cancels the rest unless the batch
//!   [`allow_failures`](Batch::allow_failures). A job reports progress
//!   with `ctx.progress(done, total, note)` and sees a cancelled batch
//!   through it ([`JobCancelled`](navree_contracts::JobCancelled)).
//! - Operations ([`Operations`], table `operations`): user-visible long
//!   work with an owner, a workspace, a status, progress, a result and a
//!   heartbeat, driven by its job or batch. Routes `GET /api/operations`,
//!   `GET /api/operations/{id}`, the SSE stream
//!   `GET /api/operations/{id}/events` and the cooperative cancel
//!   `DELETE /api/operations/{id}` ([`OperationsController`]); every
//!   change is broadcast as `operation.updated` on the private channel
//!   `operations.{id}`. The owner and holders of `operations.view` (app-wide
//!   or in the operation's workspace) may see one. A watchdog
//!   ([`Operations::sweep`]) fails or restarts operations that stopped
//!   responding: scheduled every minute in apps with `ScheduleModule`,
//!   or `queue:watchdog` from system cron.
//! - Every attempt is logged at target `navree::job` with the job type
//!   (`name`), id, queue, attempt, `duration_ms` and `outcome`
//!   (`succeeded`, `released`, `failed` or `cancelled`); payloads are never
//!   logged.
//! - Introspection: [`Queue::status`] (a queue's waiting, delayed and
//!   running jobs and how long its oldest job has waited),
//!   [`Queue::pending_jobs`] and [`Queue::pending_job`], [`Queue::failed_job`]
//!   and [`Queue::forget`] (delete one failed job).
//! - Supervision: [`Queue::work_supervised`] runs workers for a supervisor
//!   (navree-fleet): its own concurrency and stop token, and [`WorkHooks`]
//!   asked before each reservation (pause) and told every [`Attempt`].
//! - Commands: `queue:work [--queue a,b] [--once] [--stop-when-empty]
//!   [--max-jobs N]`, `queue:failed`, `queue:retry <id|all>`,
//!   `queue:flush`, `queue:batches [--limit N]`, `queue:watchdog` and
//!   `queue:prune [--hours N]`.
//! - Testing: [`QueueFake`] records dispatched jobs and batches for
//!   assertions; [`Queue::run_pending`] runs every available job before
//!   returning.
//!
//! ```no_run
//! use navree_contracts::{BoxError, Job, JobContext, JobOptions, ModuleBuilderJobsExt};
//! use navree_kernel::{Navree, Module, ModuleBuilder};
//! use navree_queue::{Queue, QueueModule};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct SendInvoice {
//!     invoice_id: u64,
//! }
//!
//! impl Job for SendInvoice {
//!     const NAME: &'static str = "billing.send_invoice";
//!
//!     fn options() -> JobOptions {
//!         JobOptions {
//!             queue: "emails".into(),
//!             max_attempts: 5,
//!             ..JobOptions::default()
//!         }
//!     }
//!
//!     async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
//!         // Resolve services from `ctx.app`, open a unit of work, ...
//!         let _ = (self.invoice_id, ctx.attempt);
//!         Ok(())
//!     }
//! }
//!
//! struct BillingModule;
//!
//! impl Module for BillingModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.job::<SendInvoice>();
//!     }
//! }
//!
//! # async fn dispatch(queue: &Queue) -> Result<(), navree_queue::QueueError> {
//! // Anywhere with access to the app:
//! let id = queue.dispatch(SendInvoice { invoice_id: 42 }).await?;
//! # let _ = id;
//! # Ok(())
//! # }
//!
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     // `<app> queue:work --queue emails,default` runs the jobs.
//!     Navree::app()
//!         .module(navree_config::ConfigModule::for_root(
//!             navree_config::ConfigSources::defaults(),
//!         ))
//!         .module(navree_db::DatabaseModule::from_config())
//!         .module(QueueModule::from_config())
//!         .module(BillingModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod batch;
mod commands;
mod error;
mod fake;
mod migration;
mod module;
mod operations;
mod options;
mod queue;
mod record;
mod store;
#[cfg(feature = "ui")]
pub mod ui;
mod worker;

pub use batch::{Batch, JobBatch};
pub use error::QueueError;
pub use fake::{QueueFake, RecordedBatch};
pub use migration::{FAILED_JOBS_TABLE, JOB_BATCHES_TABLE, JOBS_TABLE, OPERATIONS_TABLE};
pub use module::QueueModule;
pub use operations::{
    FAILED_MESSAGE, OPERATION_EVENT, OPERATION_UPDATED_EVENT, Operation, OperationProgress,
    OperationRecord, OperationStatus, Operations, OperationsController, STALLED_MESSAGE,
    SweepReport, operation_topic,
};
pub use options::{OperationsOptions, QueueDriver, QueueOptions};
pub use queue::Queue;
pub use record::{
    FailedJob, FailedJobBrief, FailedSummary, Membership, NewJob, PendingJob, PendingQuery,
    PendingState, QueueStatus, ReservedJob, RetryTarget,
};
pub use store::QueueStore;
/// The cancellation token [`Supervision::stop`] takes (tokio-util's).
pub use tokio_util::sync::CancellationToken;
pub use worker::{
    Attempt, AttemptOutcome, STALE_RESERVATION_GRACE_SECS, Supervision, WorkHooks, WorkOptions,
    WorkStats,
};
