//! # navree-fleet
//!
//! `FleetModule`: a queue worker supervisor and dashboard. One command,
//! `fleet`, starts a **master** that keeps the right number of worker
//! processes on each queue, moves workers to the queues that need them,
//! restarts workers that exit and recycles them before they grow. A
//! dashboard at `/_fleet` shows each queue's throughput, wait time and
//! runtimes, the failed jobs (retry or forget), and pauses and continues
//! the workers.
//!
//! - **Supervisors** ([`SupervisorOptions`], `fleet.supervisors.<name>`):
//!   a group of workers with one configuration: its queues, how it
//!   [`Balance`]s processes over them (`auto`, `simple`, `off`),
//!   `min_processes` per queue, `max_processes` in all, the concurrency
//!   of each process and the recycling limits (`max_jobs`,
//!   `max_memory_mb`, `max_time_secs`).
//! - **The master** ([`Master`], command `fleet`) starts each worker as a
//!   child process of the app's own binary (`fleet:work`, a `queue:work`
//!   with Fleet's limits), tells it to pause, continue or stop over its
//!   stdin and reads its reports (`@fleet` lines) from its stdout. It runs
//!   no jobs itself. `auto` supervisors rebalance every
//!   `fleet.balance_cooldown_secs` from each queue's workload (pending
//!   jobs times their recent average runtime), moving each queue by at
//!   most `fleet.balance_max_shift` processes per step
//!   ([`next_allocation`], a pure function). A worker that reaches a
//!   limit finishes its jobs and exits, and a fresh one starts; a worker
//!   that fails is restarted after a delay growing from 1 to 60 seconds.
//!   Ctrl-C or SIGTERM stops every worker gracefully.
//! - **Shared state** in the database (`fleet_masters`, `fleet_state`): the
//!   masters' heartbeats, paused supervisors and `fleet:terminate`, so
//!   pausing and terminating reach every server.
//! - **Metrics**: workers turn navree-queue's telemetry events
//!   (`JobProcessed`, `JobFailed`) into [`CompletedJob`]s and report them;
//!   the master stores them (`fleet_jobs`, kept `fleet.trim.recent_minutes`)
//!   and takes a snapshot per queue and job type every
//!   `fleet.snapshot_secs` (`fleet_metrics`, kept `fleet.trim.metrics_days`).
//!   With `ObserveModule`, `GET /metrics` has Fleet's gauges, read from the
//!   database at scrape time.
//! - **Long waits**: a master emits [`LongWaitDetected`] (`fleet.long_wait`)
//!   when a queue's oldest job waited longer than its `fleet.long_wait`
//!   threshold, at most once per queue per cooldown across masters.
//! - **The dashboard** at `fleet.path` (`/_fleet`): a page rendered with
//!   navree-view and a JSON API under `{path}/api`, behind the abilities
//!   `view` and `operate` on the access subject `Fleet` (`fleet.view`,
//!   `fleet.operate`).
//! - **The service** [`Fleet`]: pause and resume, terminate, masters,
//!   status, workload, long waits, failed jobs (list, retry, retry all,
//!   forget), pending and completed jobs, metrics.
//! - **Commands**: `fleet`, `fleet:status`, `fleet:pause [supervisor]`,
//!   `fleet:continue [supervisor]`, `fleet:terminate`, `fleet:forget <id>`.
//! - **Testing**: [`testing::FakeSpawner`] drives a [`Master`] without
//!   processes; [`InProcessSpawner`] runs real workers as tasks.
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_db::DatabaseModule;
//! use navree_fleet::FleetModule;
//! use navree_kernel::Navree;
//! use navree_queue::QueueModule;
//!
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     // `<app> fleet` runs the master; `<app> fleet:status` reports.
//!     Navree::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(DatabaseModule::from_config())
//!         .module(QueueModule::from_config())
//!         .module(FleetModule::from_config())
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]

mod balance;
mod commands;
mod dashboard;
mod error;
mod events;
mod fleet;
mod gauges;
pub mod master;
mod migration;
mod module;
mod options;
mod protocol;
mod record;
mod recorder;
mod redact;
mod snapshot;
mod spawner;
mod store;
pub mod testing;
mod worker;
mod workload;

pub use balance::{Allocation, initial_allocation, next_allocation, target_allocation};
pub use dashboard::{FleetSubject, OPERATE, VIEW};
pub use error::FleetError;
pub use events::LongWaitDetected;
pub use fleet::{Fleet, LIST_LIMIT, LongestWait, MASTER_TIMEOUT, Overview, RUNTIME_WINDOW};
pub use gauges::{
    FAILED_JOBS, JOB_RUNTIME, PROCESSES, QUEUE_PENDING, QUEUE_THROUGHPUT, QUEUE_WAIT,
};
pub use master::{Master, MasterSettings, MasterSummary};
pub use migration::{JOBS_TABLE, MASTERS_TABLE, METRICS_TABLE, STATE_TABLE};
pub use module::FleetModule;
pub use options::{
    Balance, DEFAULT_SUPERVISOR, FleetOptions, Supervisor, SupervisorOptions, TrimOptions,
};
pub use protocol::{ExitReason, REPORT_PREFIX, WorkerCommand, WorkerReport, WorkerSpec};
pub use record::{
    CompletedJob, FleetStatus, MasterReport, MetricKind, MetricPoint, MetricSeries, MetricSnapshot,
    MetricsPeriod, ProcessReport, SupervisorReport,
};
pub use recorder::JobRecorder;
pub use redact::REDACTED;
pub use spawner::{InProcessSpawner, ProcessSpawner, Spawner, WorkerHandle};
pub use workload::{ASSUMED_RUNTIME_MS, QueueWorkload};
