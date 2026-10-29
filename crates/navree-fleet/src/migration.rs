//! The `fleet` migration set: `fleet_masters`, `fleet_state`, `fleet_jobs`
//! and `fleet_metrics`. The last two are history: TimescaleDB hypertables
//! on their Unix millisecond columns.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Hypertable, KeyStrategy, Migration, Schema, SchemaMigration, SchemaResult,
};

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
        .migration(SchemaMigration::new(
            "m20261027_000001_create_fleet_tables",
            CreateFleetTables,
        ))
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

impl Migration for CreateFleetTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(MASTERS_TABLE, |t| {
                t.string("id").primary();
                t.string("name");
                t.big_integer("pid");
                t.string("status");
                t.text("supervisors");
                t.big_integer("started_at");
                t.big_integer("heartbeat_at");
            })
            .await?;
        schema
            .create(STATE_TABLE, |t| {
                t.string("key").primary();
                t.text("value");
                t.big_integer("updated_at");
            })
            .await?;
        schema
            .create(JOBS_TABLE, |t| {
                t.string("id").primary();
                t.string("job_id");
                t.string("queue");
                t.string("job_type");
                t.big_integer("attempt");
                t.string("outcome");
                t.big_integer("runtime_ms");
                t.big_integer("finished_at");
                t.string("master").nullable();
                t.index(["finished_at"])
                    .name("fleet_jobs_finished_at_index");
            })
            .await?;
        schema
            .create(METRICS_TABLE, |t| {
                t.string("id").primary();
                t.big_integer("at");
                t.string("kind");
                t.string("name");
                t.big_integer("processed");
                t.big_integer("failed");
                t.big_integer("runtime_avg_ms");
                t.big_integer("runtime_p95_ms");
                t.big_integer("pending").nullable();
                t.big_integer("wait_ms").nullable();
                t.string("master").nullable();
                t.index(["at"]).name("fleet_metrics_at_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in [METRICS_TABLE, JOBS_TABLE, STATE_TABLE, MASTERS_TABLE] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}
