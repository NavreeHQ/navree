//! [`ScheduleError`].

use navree_db::DbError;

/// A scheduling problem. The first four variants are found while the
/// [`Scheduler`](crate::Scheduler) is built, so they fail the boot.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    /// A task's frequency is not a valid cron expression or time.
    #[error(
        "scheduled task `{task}` (module `{module}`) has an invalid schedule `{expr}`: {reason}"
    )]
    InvalidSchedule {
        /// The task name.
        task: String,
        /// The module that scheduled it.
        module: &'static str,
        /// The expression (or `daily_at("…")` call) as written.
        expr: String,
        /// Why it is invalid.
        reason: String,
    },
    /// A task name is empty or longer than 191 bytes (`schedule_runs.task`).
    #[error(
        "a task scheduled by module `{module}` has an invalid name `{task}`: names must be 1 to 191 bytes long"
    )]
    InvalidName {
        /// The task name.
        task: String,
        /// The module that scheduled it.
        module: &'static str,
    },
    /// Two tasks share a name (names key `schedule_runs`).
    #[error(
        "scheduled task `{task}` is registered twice (modules `{first}` and `{second}`); give one of them another `.name(..)`"
    )]
    DuplicateTask {
        /// The task name.
        task: String,
        /// The module that scheduled it first.
        first: &'static str,
        /// The module that scheduled it again.
        second: &'static str,
    },
    /// A command task names a command no module registers.
    #[error(
        "scheduled task `{task}` (module `{module}`) runs the command `{command}`, which no module registers"
    )]
    UnknownCommand {
        /// The task name.
        task: String,
        /// The module that scheduled it.
        module: &'static str,
        /// The missing command.
        command: String,
    },
    /// The app that owns the scheduler has been dropped.
    #[error("the app owning this scheduler has been dropped")]
    AppDropped,
    /// Reading or writing `schedule_runs` failed.
    #[error("schedule_runs: {0}")]
    Db(#[from] DbError),
}
