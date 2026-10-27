//! The `fleet` migration set: `fleet_masters`, `fleet_state`, `fleet_jobs`
//! and `fleet_metrics`. The last two are history: TimescaleDB hypertables
//! on their Unix millisecond columns.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Hypertable, KeyStrategy, Migration, Schema, SchemaMigration, SchemaResult,
};
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The running masters, their supervisors and processes, and heartbeats.
pub const MASTERS_TABLE: &str = "fleet_masters";
/// Paused supervisors and pending commands (`terminate`), shared by every
/// server.
pub const STATE_TABLE: &str = "fleet_state";
/// Recently finished job attempts.
pub const JOBS_TABLE: &str = "fleet_jobs";
/// Per-queue and per-job-type metric snapshots.
pub const METRICS_TABLE: &str = "fleet_metrics";

/// The migrations `FleetModule` contributes (module name `fleet`, after
/// `queue`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("fleet")
        .after(["queue"])
        .migration(CreateFleetTables)
        .migration(SchemaMigration::new(
            "m20261027_000002_convert_fleet_history_to_hypertables",
            ConvertHistoryToHypertables,
        ))
}

/// An hour, in the millisecond units of Fleet's time columns.
const HOUR_MS: i64 = 60 * 60 * 1000;

/// How Fleet's history is partitioned on TimescaleDB: finished jobs by
/// `finished_at`, an hour per chunk (they are kept an hour by default), and
/// metric snapshots by `at`, a day per chunk (kept a week). Both columns
/// hold Unix milliseconds, so the chunk intervals are whole numbers of
/// them. `Fleet::trim` drops whole chunks.
pub(crate) fn hypertables() -> [(&'static str, Hypertable); 2] {
    let table = |column: &str, interval: i64| {
        let mut hypertable = Hypertable::on(column);
        hypertable
            .chunk_interval(interval.to_string())
            .keys(KeyStrategy::Extend)
            .migrate_data();
        hypertable
    };
    [
        (JOBS_TABLE, table("finished_at", HOUR_MS)),
        (METRICS_TABLE, table("at", 24 * HOUR_MS)),
    ]
}

/// Makes `fleet_jobs` and `fleet_metrics` hypertables on TimescaleDB (their
/// primary keys become `(id, finished_at)` and `(id, at)`); elsewhere they
/// stay regular tables with their time indexes, and the migration reports
/// a notice.
///
/// Reverting it keeps the hypertables: TimescaleDB cannot turn one back
/// into a plain table in place, and the tables work the same either way.
/// Running it again changes nothing.
struct ConvertHistoryToHypertables;

impl Migration for ConvertHistoryToHypertables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        for (table, hypertable) in hypertables() {
            if !schema.is_hypertable(table).await? {
                schema.convert_to_hypertable(table, hypertable).await?;
            }
        }
        Ok(())
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
        Ok(())
    }
}

/// Creates the four tables. Times are Unix milliseconds (`BIGINT`), as in
/// the queue's tables.
struct CreateFleetTables;

impl MigrationName for CreateFleetTables {
    fn name(&self) -> &str {
        "m20261027_000001_create_fleet_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateFleetTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MASTERS_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("pid").big_integer().not_null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("supervisors").text().not_null())
                    .col(ColumnDef::new("started_at").big_integer().not_null())
                    .col(ColumnDef::new("heartbeat_at").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(STATE_TABLE)
                    .col(ColumnDef::new("key").string().not_null().primary_key())
                    .col(ColumnDef::new("value").text().not_null())
                    .col(ColumnDef::new("updated_at").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(JOBS_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("job_id").string().not_null())
                    .col(ColumnDef::new("queue").string().not_null())
                    .col(ColumnDef::new("job_type").string().not_null())
                    .col(ColumnDef::new("attempt").big_integer().not_null())
                    .col(ColumnDef::new("outcome").string().not_null())
                    .col(ColumnDef::new("runtime_ms").big_integer().not_null())
                    .col(ColumnDef::new("finished_at").big_integer().not_null())
                    .col(ColumnDef::new("master").string().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("fleet_jobs_finished_at_index")
                    .table(JOBS_TABLE)
                    .col("finished_at")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(METRICS_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("at").big_integer().not_null())
                    .col(ColumnDef::new("kind").string().not_null())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("processed").big_integer().not_null())
                    .col(ColumnDef::new("failed").big_integer().not_null())
                    .col(ColumnDef::new("runtime_avg_ms").big_integer().not_null())
                    .col(ColumnDef::new("runtime_p95_ms").big_integer().not_null())
                    .col(ColumnDef::new("pending").big_integer().null())
                    .col(ColumnDef::new("wait_ms").big_integer().null())
                    .col(ColumnDef::new("master").string().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("fleet_metrics_at_index")
                    .table(METRICS_TABLE)
                    .col("at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [METRICS_TABLE, JOBS_TABLE, STATE_TABLE, MASTERS_TABLE] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
