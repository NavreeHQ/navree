//! The `schedule_runs` table: its migration and the statements the scheduler runs.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{ColumnDef, Expr, ExprTrait, Index, OnConflict, Query, Table};
use sea_orm::{ConnectionTrait, DbErr};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The table recording every claimed tick.
pub const RUNS_TABLE: &str = "schedule_runs";

/// The `migration set` name of this module (`keel_migrations.module`).
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

impl MigrationName for CreateScheduleRuns {
    fn name(&self) -> &str {
        "m20261025_000001_create_schedule_runs"
    }
}

#[async_trait]
impl MigrationTrait for CreateScheduleRuns {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RUNS_TABLE)
                    .col(ColumnDef::new("task").string_len(191).not_null())
                    .col(ColumnDef::new("tick").timestamp_with_time_zone().not_null())
                    .col(
                        ColumnDef::new("started_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new("finished_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("status").string_len(16).not_null())
                    .col(ColumnDef::new("error").text().null())
                    .primary_key(Index::create().col("task").col("tick"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_schedule_runs_task_status")
                    .table(RUNS_TABLE)
                    .col("task")
                    .col("status")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RUNS_TABLE).to_owned())
            .await
    }
}

/// Claims `tick` of `task` (`INSERT … ON CONFLICT DO NOTHING`): true when
/// this call inserted the row, false when another scheduler already had.
pub(crate) async fn claim<C: ConnectionTrait>(
    conn: &C,
    task: &str,
    tick: DateTime<Utc>,
    started_at: DateTime<Utc>,
) -> Result<bool, DbErr> {
    let mut insert = Query::insert();
    insert
        .into_table(RUNS_TABLE)
        .columns(["task", "tick", "started_at", "status"])
        .values([task.into(), tick.into(), started_at.into(), RUNNING.into()])
        .map_err(|error| DbErr::Custom(error.to_string()))?
        .on_conflict(
            OnConflict::columns(["task", "tick"])
                .do_nothing()
                .to_owned(),
        );
    Ok(conn.execute(&insert).await?.rows_affected() == 1)
}

/// Records how the run of `tick` of `task` ended (`error` is `None` on success).
pub(crate) async fn finish<C: ConnectionTrait>(
    conn: &C,
    task: &str,
    tick: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    error: Option<&str>,
) -> Result<(), DbErr> {
    let status = if error.is_some() { FAILED } else { SUCCEEDED };
    conn.execute(
        Query::update()
            .table(RUNS_TABLE)
            .values([
                ("finished_at", finished_at.into()),
                ("status", status.into()),
                ("error", error.map(str::to_owned).into()),
            ])
            .and_where(Expr::col("task").eq(task))
            .and_where(Expr::col("tick").eq(tick)),
    )
    .await?;
    Ok(())
}

/// Whether `task` has a `running` row for a tick other than `tick` that
/// started after `since` (older rows are left behind by crashed processes).
pub(crate) async fn running_elsewhere<C: ConnectionTrait>(
    conn: &C,
    task: &str,
    tick: DateTime<Utc>,
    since: DateTime<Utc>,
) -> Result<bool, DbErr> {
    let rows = conn
        .query_all(
            Query::select()
                .column("started_at")
                .from(RUNS_TABLE)
                .and_where(Expr::col("task").eq(task))
                .and_where(Expr::col("status").eq(RUNNING))
                .and_where(Expr::col("tick").ne(tick)),
        )
        .await?;
    for row in rows {
        let started_at: DateTime<Utc> = row.try_get("", "started_at")?;
        if started_at > since {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Deletes the runs of ticks before `before`; returns how many.
pub(crate) async fn prune<C: ConnectionTrait>(
    conn: &C,
    before: DateTime<Utc>,
) -> Result<u64, DbErr> {
    let result = conn
        .execute(
            Query::delete()
                .from_table(RUNS_TABLE)
                .and_where(Expr::col("tick").lt(before)),
        )
        .await?;
    Ok(result.rows_affected())
}
