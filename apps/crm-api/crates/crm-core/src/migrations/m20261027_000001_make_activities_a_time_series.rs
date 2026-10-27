//! Declares `activities`, the CRM's timeline, a time series on
//! `created_at`.
//!
//! Activities are written once, completed or edited now and then, and read
//! by time (the newest on the dashboard, a record's history, today's
//! tasks), so the table takes the time-series engine:
//!
//! - **TimescaleDB:** a hypertable partitioned by `created_at`, a month per
//!   chunk (a sales team writes a few thousand activities a month, so the
//!   recent chunks stay small and in memory). Its primary key becomes
//!   `(id, created_at)`, as TimescaleDB requires; ids stay unique (UUID v7)
//!   and nothing points at activities with a foreign key. Existing rows
//!   move into chunks. No compression and no retention: tasks are
//!   completed long after they are written, and the history is kept.
//! - **PostgreSQL without TimescaleDB, MySQL, SQLite:** the table stays as
//!   it is, with an index on `created_at`; the migration reports a notice.
//!
//! Reverting it keeps the hypertable: TimescaleDB cannot turn one back
//! into a plain table in place, and the table works the same either way.

use navree::schema::prelude::*;

/// The time column of the activity timeline.
pub const TIME_COLUMN: &str = "created_at";

/// `m20261027_000001_make_activities_a_time_series`.
pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        if schema.is_hypertable("activities").await? {
            return Ok(());
        }
        schema
            .table("activities", |t| {
                t.hypertable(TIME_COLUMN)
                    .chunk_interval("30 days")
                    .keys(KeyStrategy::Extend)
                    .migrate_data();
            })
            .await
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
        Ok(())
    }
}
