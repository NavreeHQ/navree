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
//!   UTC by default), `without_overlapping` and the task name. Invalid
//!   frequencies, duplicate task names and unknown commands fail the boot.
//! - **Local clocks.** `in_zone("Asia/Kolkata")` and `in_default_zone()`
//!   read the frequency on a zone's clock; `per_workspace_local()` runs the
//!   task once per workspace when that workspace's clock reaches it
//!   (navree-time's `Zones` lists the workspaces and their zones). Inside a
//!   run, [`LocalRun::current`] gives the owner and zone, and
//!   [`Schedule::job_with`] builds a job per owner.
//! - **Cron language.** [`CronExpression`] is Navree's own parser. A
//!   frequency is five fields (`minute hour day-of-month month day-of-week`,
//!   with the names `JAN`-`DEC` and `SUN`-`SAT` in the month and day-of-week
//!   fields) or one nickname (`@yearly`, `@annually`, `@monthly`, `@weekly`,
//!   `@daily`, `@hourly`), in any case. Each field takes `*`, values, ranges
//!   `a-b`, steps `*/s` and `a-b/s`, and comma lists. The day of month also
//!   takes `L`, `LW` and `15W`; the day of week `5L`, `5#2`, `5#L` and `?`.
//!   A day matches when its day of month or its day of week matches (unless
//!   one is `*`); `+MON` asks for both. A seconds or year field, a range that
//!   wraps, empty list items and everything else is rejected with an error
//!   naming the field and the bad token ([`CronError`]). An expression with a
//!   `*` in its minute or hour field follows the wall clock through clock
//!   changes; any other runs each local time once.
//! - [`ScheduleModule`] is global and exports the [`Scheduler`]. It needs
//!   `DatabaseModule`: every run is recorded in [`schedule_runs`](RUNS_TABLE)
//!   (`task, tick, started_at, finished_at, status, error`, primary key
//!   `(task, tick)`), created by the module's `schedule` migration set.
//! - A tick runs a task only if its `INSERT … ON CONFLICT DO NOTHING`
//!   (`INSERT IGNORE` on MySQL) inserted the `(task, tick)` row, so each tick
//!   of each task runs on one node, however many schedulers share the
//!   database.
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
#![cfg_attr(docsrs, feature(doc_cfg))]

mod cron;
mod error;
mod local;
mod module;
mod schedule;
mod scheduler;
mod store;

pub use cron::{CronError, CronExpression, MAX_EXPRESSION_LEN};
pub use error::ScheduleError;
pub use local::LocalRun;
pub use module::ScheduleModule;
pub use schedule::{ModuleBuilderScheduleExt, Schedule, TaskBuilder};
pub use scheduler::{RUN_RETENTION, RunOutcome, RunStatus, Scheduler, SkipReason, TaskInfo};
pub use store::RUNS_TABLE;
