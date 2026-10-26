//! # navree-schedule
//!
//! `ScheduleModule`: cron schedules with overlap prevention and
//! single-node runs.
//!
//! - Modules declare tasks with
//!   [`m.schedule(|s| ...)`](ModuleBuilderScheduleExt::schedule): a job
//!   ([`Schedule::job`], dispatched through `dyn Dispatcher`, or run inline
//!   when none is registered; its `JobRegistration` is contributed for the
//!   queue's workers), an async function ([`Schedule::task`]) or a
//!   registered CLI command ([`Schedule::command`]). [`TaskBuilder`] sets the
//!   frequency (`cron`, `every_minute`, `hourly_at`, `daily_at("02:00")`, …,
//!   all UTC), `without_overlapping` and the task name. Invalid
//!   frequencies, duplicate task names and unknown commands fail the boot.
//! - [`ScheduleModule`] is global and exports the [`Scheduler`]. It needs
//!   `DatabaseModule`: every run is recorded in [`schedule_runs`](RUNS_TABLE)
//!   (`task, tick, started_at, finished_at, status, error`, primary key
//!   `(task, tick)`), created by the module's `schedule` migration set.
//! - A tick runs a task only if its `INSERT … ON CONFLICT DO NOTHING`
//!   inserted the `(task, tick)` row, so each tick of each task runs on one
//!   node, however many schedulers share the database.
//!   `without_overlapping` also skips the tick while an earlier run is
//!   still `running` (an in-process lock plus the table; `running` rows
//!   older than 24 hours are taken as left over from a crash). Rows older
//!   than [`RUN_RETENTION`] are pruned at the top of each hour.
//! - Time comes from the app's `dyn Clock` (`SystemClock` when none is
//!   registered), so tests drive the scheduler with `FixedClock` and
//!   [`Scheduler::tick`].
//! - Commands: `schedule:run` (one tick; call it every minute from system
//!   cron), `schedule:work` (a foreground loop aligned to minute boundaries,
//!   until shutdown) and `schedule:list`.
//! - Runs are logged at target `navree::schedule` (task, tick, duration,
//!   outcome, error).
//!
//! ```no_run
//! use navree_db::{DatabaseModule, DbOptions};
//! use navree_kernel::{Navree, Module, ModuleBuilder};
//! use navree_schedule::{ModuleBuilderScheduleExt, ScheduleModule};
//!
//! struct ReportsModule;
//!
//! impl Module for ReportsModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.schedule(|s| {
//!             s.task("reports.refresh", |_ctx| async { Ok(()) })
//!                 .daily_at("02:00")
//!                 .without_overlapping();
//!             s.command("audit:prune", &[]).daily_at("03:00");
//!         });
//!     }
//! }
//!
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     // `my-app schedule:work`, or `my-app schedule:run` every minute from cron.
//!     Navree::app()
//!         .module(DatabaseModule::for_root(DbOptions::default()))
//!         .module(ScheduleModule::for_root())
//!         .module(ReportsModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]

mod error;
mod module;
mod schedule;
mod scheduler;
mod store;

pub use error::ScheduleError;
pub use module::ScheduleModule;
pub use schedule::{ModuleBuilderScheduleExt, Schedule, TaskBuilder};
pub use scheduler::{RUN_RETENTION, RunOutcome, RunStatus, Scheduler, SkipReason, TaskInfo};
pub use store::RUNS_TABLE;
