//! # navree-queue
//!
//! `QueueModule`: database and memory job queues, workers, retries with
//! backoff, unique and delayed jobs.
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
//!   share them) and `memory` (one process, for tests and development).
//!   Both behave the same.
//! - Workers ([`Queue::work`], command `queue:work`) reserve jobs atomically
//!   (SQLite: `UPDATE … WHERE id = (SELECT … LIMIT 1) RETURNING`;
//!   PostgreSQL adds `FOR UPDATE SKIP LOCKED`), run each attempt within the
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
//! - Every attempt is logged at target `navree::job` with the job type
//!   (`name`), id, queue, attempt, `duration_ms` and `outcome`
//!   (`succeeded`, `released` or `failed`); payloads are never logged.
//! - Introspection: [`Queue::status`] (a queue's waiting, delayed and
//!   running jobs and how long its oldest job has waited),
//!   [`Queue::pending_jobs`] and [`Queue::pending_job`], [`Queue::failed_job`]
//!   and [`Queue::forget`] (delete one failed job).
//! - Supervision: [`Queue::work_supervised`] runs workers for a supervisor
//!   (navree-fleet): its own concurrency and stop token, and [`WorkHooks`]
//!   asked before each reservation (pause) and told every [`Attempt`].
//! - Commands: `queue:work [--queue a,b] [--once] [--stop-when-empty]
//!   [--max-jobs N]`, `queue:failed`, `queue:retry <id|all>` and
//!   `queue:flush`.
//! - Testing: [`QueueFake`] records dispatched jobs for assertions;
//!   [`Queue::run_pending`] runs every available job before returning.
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

mod commands;
mod error;
mod fake;
mod migration;
mod module;
mod options;
mod queue;
mod record;
mod store;
mod worker;

pub use error::QueueError;
pub use fake::QueueFake;
pub use migration::{FAILED_JOBS_TABLE, JOBS_TABLE};
pub use module::QueueModule;
pub use options::{QueueDriver, QueueOptions};
pub use queue::Queue;
pub use record::{
    FailedJob, PendingJob, PendingQuery, PendingState, QueueStatus, RetryTarget,
};
pub use worker::{
    Attempt, AttemptOutcome, STALE_RESERVATION_GRACE_SECS, Supervision, WorkHooks, WorkOptions,
    WorkStats,
};
/// The cancellation token [`Supervision::stop`] takes (tokio-util's).
pub use tokio_util::sync::CancellationToken;
