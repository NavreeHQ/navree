//! The `schedule_runs` table: its migrations (a TimescaleDB hypertable on
//! `tick`) and the statements the scheduler runs.

use chrono::{DateTime, Utc};
use navree_db::navree_schema::{Hypertable, KeyStrategy, Migration, Schema, SchemaResult};
use navree_db::{DbError, Executor, Statement, prune_time_series_on};

/// The table recording every claimed tick.
pub const RUNS_TABLE: &str = "schedule_runs";

/// The `migration set` name of this module (`navree_migrations.module`).
pub(crate) const MIGRATION_MODULE: &str = "schedule";

/// `schedule_runs.status` of a run in progress.
pub(crate) const RUNNING: &str = "running";
/// `schedule_runs.status` of a run that returned `Ok`.
pub(crate) const SUCCEEDED: &str = "succeeded";
/// `schedule_runs.status` of a run that failed or panicked.
pub(crate) const FAILED: &str = "failed";

/// Creates `schedule_runs(task, tick, started_at, finished_at, status,
/// error, PRIMARY KEY (task, tick))`.
pub(crate) struct CreateScheduleRuns;

impl CreateScheduleRuns {
    /// The name recorded in `navree_migrations`.
    pub(crate) const NAME: &'static str = "m20261025_000001_create_schedule_runs";
}

impl Migration for CreateScheduleRuns {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(RUNS_TABLE, |t| {
                t.string("task").length(191);
                t.timestamp_tz("tick");
                t.timestamp_tz("started_at");
                t.timestamp_tz("finished_at").nullable();
                t.string("status").length(16);
                t.text("error").nullable();
                t.primary(["task", "tick"]);
                t.index(["task", "status"])
                    .name("idx_schedule_runs_task_status");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(RUNS_TABLE).await
    }
}

/// Claims `tick` of `task` (`INSERT … ON CONFLICT DO NOTHING`): true when
/// this call inserted the row, false when another scheduler already had.
pub(crate) async fn claim<E: Executor + ?Sized>(
    executor: &E,
    task: &str,
    tick: DateTime<Utc>,
    started_at: DateTime<Utc>,
) -> Result<bool, DbError> {
    // MySQL's `INSERT IGNORE` skips a duplicate key (and only that, here:
    // every column is bound and checked by the table).
    let insert = match executor.backend() {
        navree_db::Backend::MySql => "INSERT IGNORE INTO",
        _ => "INSERT INTO",
    };
    let conflict = match executor.backend() {
        navree_db::Backend::MySql => "",
        _ => " ON CONFLICT (task, tick) DO NOTHING",
    };
    let statement = Statement::portable(
        format!(
            "{insert} {RUNS_TABLE} (task, tick, started_at, status) VALUES (?, ?, ?, ?){conflict}"
        ),
        [task.into(), tick.into(), started_at.into(), RUNNING.into()],
    );
    Ok(executor.execute(&statement).await?.rows_affected == 1)
}

/// Records how the run of `tick` of `task` ended (`error` is `None` on success).
pub(crate) async fn finish<E: Executor + ?Sized>(
    executor: &E,
    task: &str,
    tick: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    error: Option<&str>,
) -> Result<(), DbError> {
    let status = if error.is_some() { FAILED } else { SUCCEEDED };
    let statement = Statement::portable(
        format!(
            "UPDATE {RUNS_TABLE} SET finished_at = ?, status = ?, error = ? WHERE task = ? AND tick = ?"
        ),
        [
            finished_at.into(),
            status.into(),
            error.map(str::to_owned).into(),
            task.into(),
            tick.into(),
        ],
    );
    executor.execute(&statement).await?;
    Ok(())
}

/// Whether `task` has a `running` row for a tick other than `tick` that
/// started after `since` (older rows are left behind by crashed processes).
pub(crate) async fn running_elsewhere<E: Executor + ?Sized>(
    executor: &E,
    task: &str,
    tick: DateTime<Utc>,
    since: DateTime<Utc>,
) -> Result<bool, DbError> {
    let statement = Statement::portable(
        format!("SELECT started_at FROM {RUNS_TABLE} WHERE task = ? AND status = ? AND tick <> ?"),
        [task.into(), RUNNING.into(), tick.into()],
    );
    for row in executor.fetch_all(&statement).await? {
        let started_at: DateTime<Utc> = row.get("started_at")?;
        if started_at > since {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Deletes the runs of ticks before `before`; returns how many. On
/// TimescaleDB, where `schedule_runs` is a hypertable, the days entirely
/// before `before` go as whole chunks.
pub(crate) async fn prune<E: Executor + ?Sized>(
    executor: &E,
    before: DateTime<Utc>,
) -> Result<u64, DbError> {
    prune_time_series_on(executor, RUNS_TABLE, "tick", before).await
}

/// How `schedule_runs` is partitioned on TimescaleDB: by `tick`, a day per
/// chunk (runs are kept a week, [`RUN_RETENTION`](crate::RUN_RETENTION)).
/// Its primary key, `(task, tick)`, already includes the time column.
pub(crate) fn hypertable() -> Hypertable {
    let mut runs = Hypertable::on("tick");
    runs.chunk_interval("1 day")
        .keys(KeyStrategy::Extend)
        .migrate_data();
    runs
}

/// Makes `schedule_runs` a hypertable on TimescaleDB; elsewhere it stays a
/// regular table, given an index on `tick` for pruning, and the migration
/// reports a notice.
///
/// Reverting it keeps the hypertable (and the index): TimescaleDB cannot
/// turn one back into a plain table in place, and the table works the same
/// either way. Running it again changes nothing.
pub(crate) struct ConvertScheduleRunsToHypertable;

impl ConvertScheduleRunsToHypertable {
    /// The name recorded in `navree_migrations`.
    pub(crate) const NAME: &'static str = "m20261027_000001_convert_schedule_runs_to_hypertable";
}

impl Migration for ConvertScheduleRunsToHypertable {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        if schema.is_hypertable(RUNS_TABLE).await? {
            return Ok(());
        }
        schema.convert_to_hypertable(RUNS_TABLE, hypertable()).await
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
        Ok(())
    }
}
