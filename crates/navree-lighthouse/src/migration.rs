//! The `lighthouse` migration set: seven tables prefixed `lighthouse_`: the
//! five Lighthouse records into, and the releases and the settings changed
//! from the dashboard.
//! Samples, occurrences and aggregates are time series: TimescaleDB
//! hypertables.
//!
//! With `lighthouse.database_url` set, Lighthouse also creates the same
//! tables (when missing) in that database as it connects at boot, makes
//! the time-series ones hypertables there when it runs TimescaleDB, and
//! reads and writes there.
//!
//! Written with the schema builder. The names are the ones the module's
//! migrations always had, so a database migrated by an earlier version
//! finds nothing pending. The columns are the types those migrations
//! created: UUIDs as `BINARY(16)` on MySQL and MariaDB (where navree-db
//! writes UUIDs as 16 bytes), text of no fixed length as `VARCHAR` without
//! a length on PostgreSQL and SQLite and `VARCHAR(255)` on MySQL.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Hypertable, KeyStrategy, Migration, Schema, SchemaMigration,
    SchemaResult, TimescaleMode,
};

/// The name of the Lighthouse migration set.
pub const LIGHTHOUSE_MIGRATIONS: &str = "lighthouse";

/// The table of sampled requests, jobs and queries.
pub(crate) const SAMPLES_TABLE: &str = "lighthouse_samples";

/// The table of issue occurrences.
pub(crate) const OCCURRENCES_TABLE: &str = "lighthouse_occurrences";

/// The table of per-minute and per-hour aggregates.
pub(crate) const AGGREGATES_TABLE: &str = "lighthouse_aggregates";

/// The Lighthouse migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(LIGHTHOUSE_MIGRATIONS)
        .migration(SchemaMigration::new(
            "m20261027_000001_create_lighthouse_tables",
            CreateLighthouseTables,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000002_convert_lighthouse_events_to_hypertables",
            ConvertEventsToHypertables,
        ))
        .migration(SchemaMigration::new(
            "m20261102_000003_create_lighthouse_releases_and_settings",
            CreateReleasesAndSettings,
        ))
        .migration(SchemaMigration::new(
            "m20261102_000004_index_lighthouse_issue_releases",
            IndexIssueReleases,
        ))
}

/// How Lighthouse's events are partitioned on TimescaleDB, a day per chunk
/// (kept 14 days by default, `lighthouse.retention_days`): samples and
/// occurrences by `at`, aggregates by `bucket`. Their unique keys gain the
/// time column (the aggregates' row key already has it), and
/// `lighthouse:prune` drops whole chunks.
pub(crate) fn hypertables() -> [(&'static str, Hypertable); 3] {
    let table = |column: &str| {
        let mut hypertable = Hypertable::on(column);
        hypertable
            .chunk_interval("1 day")
            .keys(KeyStrategy::Extend)
            .migrate_data();
        hypertable
    };
    [
        (SAMPLES_TABLE, table("at")),
        (OCCURRENCES_TABLE, table("at")),
        (AGGREGATES_TABLE, table("bucket")),
    ]
}

/// Makes the event tables hypertables, unless they are already.
pub(crate) async fn convert_time_series(schema: &Schema<'_>) -> SchemaResult {
    for (table, hypertable) in hypertables() {
        if !schema.is_hypertable(table).await? {
            schema.convert_to_hypertable(table, hypertable).await?;
        }
    }
    Ok(())
}

/// [`convert_time_series`] in the database of `lighthouse.database_url`, as
/// Lighthouse connects at boot: on PostgreSQL where TimescaleDB is (or may
/// be) installed, and where `schema.timescaledb = "require"` asks for it,
/// which then fails the boot when it is missing. Other databases keep the
/// tables as created, without a notice at every boot.
pub(crate) async fn convert_time_series_at_boot(schema: &Schema<'_>) -> SchemaResult {
    let target = schema.target().await?;
    if target.dialect() != Dialect::Postgres {
        return Ok(());
    }
    let options = schema.options();
    let usable = target.timescaledb().is_some()
        || (options.create_extensions && target.timescaledb_installable().is_some());
    match options.timescaledb {
        TimescaleMode::Off => Ok(()),
        TimescaleMode::Require => convert_time_series(schema).await,
        TimescaleMode::Auto if usable => convert_time_series(schema).await,
        TimescaleMode::Auto => Ok(()),
    }
}

/// Makes `lighthouse_samples`, `lighthouse_occurrences` and
/// `lighthouse_aggregates` hypertables on TimescaleDB; elsewhere they stay
/// regular tables with time indexes (aggregates gain one on `bucket`), and
/// the migration reports a notice.
///
/// Reverting it keeps the hypertables: TimescaleDB cannot turn one back
/// into a plain table in place, and the tables work the same either way.
/// Running it again changes nothing.
struct ConvertEventsToHypertables;

impl Migration for ConvertEventsToHypertables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        convert_time_series(schema).await
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
        Ok(())
    }
}

/// Creates `lighthouse_releases` and `lighthouse_settings`, and adds the
/// release a regression happened in to the issues, where they are missing.
struct CreateReleasesAndSettings;

impl Migration for CreateReleasesAndSettings {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        create_config_tables(schema).await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("lighthouse_settings").await?;
        schema.drop_if_exists("lighthouse_releases").await?;
        Ok(())
    }
}

/// Indexes the columns the releases are counted by. On MySQL it also makes a
/// release's name compare byte by byte, as it does on the other databases:
/// `r1` and `R1` are two labels.
struct IndexIssueReleases;

const ISSUE_RELEASE_INDEXES: [(&str, &str); 2] = [
    ("first_release", "lighthouse_issues_first_release_index"),
    (
        "regressed_in_release",
        "lighthouse_issues_regressed_in_release_index",
    ),
];

impl Migration for IndexIssueReleases {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("lighthouse_issues", |t| {
                for (column, index) in ISSUE_RELEASE_INDEXES {
                    t.index([column]).name(index);
                }
            })
            .await?;
        if schema.target().await?.dialect() == Dialect::MySql {
            schema
                .statement(
                    "ALTER TABLE lighthouse_releases MODIFY name VARCHAR(255) \
                     CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL",
                )
                .await?;
            // The issues name their releases the same way, or `r9` and `R9` would
            // count each other's.
            for (column, _) in ISSUE_RELEASE_INDEXES {
                schema
                    .statement(&format!(
                        "ALTER TABLE lighthouse_issues MODIFY {column} VARCHAR(255) \
                         CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL"
                    ))
                    .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("lighthouse_issues", |t| {
                for (_, index) in ISSUE_RELEASE_INDEXES {
                    t.drop_index(index);
                }
            })
            .await
    }
}

/// The five tables, in the order a batch writes them (and the order the
/// down migration locks them in): issues first.
const TABLES: [&str; 5] = [
    "lighthouse_issues",
    "lighthouse_occurrences",
    "lighthouse_samples",
    "lighthouse_aggregates",
    "lighthouse_alerts",
];

/// Creates the five tables.
pub(crate) struct CreateLighthouseTables;

impl Migration for CreateLighthouseTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        create_tables(schema).await
    }

    /// Drops the tables.
    ///
    /// On PostgreSQL every table is locked first, in the order the
    /// recording thread's batches write them (issues, occurrences, samples,
    /// aggregates, alerts). A batch being flushed while the tables go would
    /// otherwise add a chunk to a hypertable that is being dropped, or wait
    /// for a lock the drop holds while the drop waits for one of its own,
    /// and the database would end one of them as a deadlock. Locked first,
    /// in that order, the drop waits for a batch in progress to commit; a
    /// batch that starts after it finds the tables gone, a failed flush
    /// Lighthouse reports and drops.
    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        if schema.target().await?.dialect() == Dialect::Postgres && !schema.is_pretending() {
            let mut existing = Vec::new();
            for table in TABLES {
                if schema.has_table(table).await? {
                    existing.push(table);
                }
            }
            if !existing.is_empty() {
                schema
                    .statement(&format!(
                        "LOCK TABLE {} IN ACCESS EXCLUSIVE MODE",
                        existing.join(", ")
                    ))
                    .await?;
            }
        }
        for table in TABLES.iter().rev() {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}

/// A UUID column: `BINARY(16)` on MySQL and MariaDB, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// A text column of no fixed length: `VARCHAR(255)` on MySQL and MariaDB,
/// `VARCHAR` elsewhere.
fn varchar<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.string(name).length(255)
    } else {
        t.raw_column(name, "VARCHAR")
    }
}

/// Creates the tables that do not exist yet, with their indexes.
pub(crate) async fn create_tables(schema: &Schema<'_>) -> SchemaResult {
    let d = schema.target().await?.dialect();
    if !schema.has_table("lighthouse_issues").await? {
        schema
            .create("lighthouse_issues", |t| {
                uuid(t, "id", d).primary();
                varchar(t, "fingerprint", d).unique();
                varchar(t, "kind", d);
                varchar(t, "level", d);
                varchar(t, "status", d);
                varchar(t, "title", d);
                varchar(t, "culprit", d);
                t.text("message");
                t.text("chain");
                varchar(t, "location", d).nullable();
                varchar(t, "error_type", d).nullable();
                t.big_integer("count");
                t.timestamp_tz("first_seen");
                t.timestamp_tz("last_seen");
                varchar(t, "first_release", d).nullable();
                varchar(t, "last_release", d).nullable();
                varchar(t, "environment", d).nullable();
                t.timestamp_tz("resolved_at").nullable();
                varchar(t, "resolved_in_release", d).nullable();
                // A DATETIME on MySQL: its TIMESTAMP ends in 2038, and an issue can
                // be ignored until 2099.
                t.date_time_tz("ignored_until").nullable();
                t.big_integer("ignore_remaining").nullable();
                t.timestamp_tz("regressed_at").nullable();
                varchar(t, "regressed_in_release", d).nullable();
                t.text("comment").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index(["status", "last_seen"])
                    .name("lighthouse_issues_status_last_seen_index");
            })
            .await?;
    }
    if !schema.has_table("lighthouse_occurrences").await? {
        schema
            .create("lighthouse_occurrences", |t| {
                uuid(t, "id", d).primary();
                uuid(t, "issue_id", d);
                t.timestamp_tz("at");
                varchar(t, "level", d);
                t.text("message");
                t.text("chain");
                t.text("backtrace").nullable();
                varchar(t, "location", d).nullable();
                varchar(t, "request_id", d).nullable();
                varchar(t, "job_id", d).nullable();
                varchar(t, "trace_id", d).nullable();
                varchar(t, "user_id", d).nullable();
                varchar(t, "workspace_id", d).nullable();
                varchar(t, "release", d).nullable();
                varchar(t, "environment", d).nullable();
                varchar(t, "server", d).nullable();
                varchar(t, "process", d).nullable();
                varchar(t, "method", d).nullable();
                varchar(t, "route", d).nullable();
                varchar(t, "path", d).nullable();
                t.integer("status").nullable();
                uuid(t, "sample_id", d).nullable();
                t.text("context");
                t.foreign(["issue_id"])
                    .references(["id"])
                    .on("lighthouse_issues")
                    .cascade_on_delete();
                t.index(["issue_id", "at"])
                    .name("lighthouse_occurrences_issue_at_index");
                t.index(["at"]).name("lighthouse_occurrences_at_index");
            })
            .await?;
    }
    if !schema.has_table("lighthouse_samples").await? {
        schema
            .create("lighthouse_samples", |t| {
                uuid(t, "id", d).primary();
                varchar(t, "kind", d);
                varchar(t, "name", d);
                t.timestamp_tz("at");
                t.double("duration_ms");
                t.integer("status").nullable();
                varchar(t, "outcome", d);
                t.boolean("slow");
                varchar(t, "request_id", d).nullable();
                varchar(t, "job_id", d).nullable();
                varchar(t, "trace_id", d).nullable();
                varchar(t, "user_id", d).nullable();
                varchar(t, "workspace_id", d).nullable();
                varchar(t, "release", d).nullable();
                varchar(t, "environment", d).nullable();
                varchar(t, "server", d).nullable();
                varchar(t, "process", d).nullable();
                t.text("spans");
                t.text("context");
                t.text("error").nullable();
                t.index(["kind", "name", "duration_ms"])
                    .name("lighthouse_samples_kind_name_duration_index");
                t.index(["at"]).name("lighthouse_samples_at_index");
            })
            .await?;
    }
    if !schema.has_table("lighthouse_aggregates").await? {
        schema
            .create("lighthouse_aggregates", |t| {
                uuid(t, "id", d).primary();
                t.integer("resolution");
                t.timestamp_tz("bucket");
                varchar(t, "kind", d);
                varchar(t, "key_hash", d);
                t.text("key");
                varchar(t, "process", d);
                t.big_integer("count");
                t.big_integer("errors");
                t.big_integer("extra");
                t.double("total_ms");
                t.double("max_ms");
                t.text("histogram");
                t.text("detail").nullable();
                t.unique(["resolution", "bucket", "kind", "key_hash", "process"])
                    .name("lighthouse_aggregates_row_unique");
                t.index(["kind", "resolution", "bucket"])
                    .name("lighthouse_aggregates_kind_bucket_index");
            })
            .await?;
    }
    if !schema.has_table("lighthouse_alerts").await? {
        schema
            .create("lighthouse_alerts", |t| {
                uuid(t, "id", d).primary();
                uuid(t, "issue_id", d).nullable();
                varchar(t, "kind", d);
                varchar(t, "channel", d);
                varchar(t, "subject_key", d);
                varchar(t, "status", d);
                t.integer("attempts").default(0);
                t.text("error").nullable();
                t.text("payload");
                t.timestamp_tz("created_at");
                t.timestamp_tz("delivered_at").nullable();
                t.index(["subject_key", "kind", "created_at"])
                    .name("lighthouse_alerts_subject_kind_index");
            })
            .await?;
    }
    create_config_tables(schema).await
}

/// Creates the tables of releases and settings, and the issues' column for
/// the release of a regression, where they are missing.
pub(crate) async fn create_config_tables(schema: &Schema<'_>) -> SchemaResult {
    let d = schema.target().await?.dialect();
    if schema.has_table("lighthouse_issues").await?
        && !schema
            .has_column("lighthouse_issues", "regressed_in_release")
            .await?
    {
        schema
            .table("lighthouse_issues", |t| {
                varchar(t, "regressed_in_release", d).nullable();
            })
            .await?;
    }
    if !schema.has_table("lighthouse_releases").await? {
        schema
            .create("lighthouse_releases", |t| {
                uuid(t, "id", d).primary();
                varchar(t, "name", d);
                varchar(t, "environment", d);
                varchar(t, "commit_sha", d).nullable();
                varchar(t, "url", d).nullable();
                t.text("notes").nullable();
                varchar(t, "source", d);
                t.timestamp_tz("deployed_at");
                t.timestamp_tz("first_seen");
                t.timestamp_tz("last_seen");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.unique(["environment", "name"])
                    .name("lighthouse_releases_environment_name_unique");
                t.index(["deployed_at"])
                    .name("lighthouse_releases_deployed_at_index");
            })
            .await?;
    }
    if !schema.has_table("lighthouse_settings").await? {
        schema
            .create("lighthouse_settings", |t| {
                uuid(t, "id", d).primary();
                varchar(t, "setting_key", d).unique();
                t.text("value");
                varchar(t, "updated_by", d).nullable();
                t.timestamp_tz("updated_at");
            })
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use navree_db::{Db, DbOptions};

    use super::*;

    /// A database an earlier build of Lighthouse created: issues without the
    /// release of a regression, and no tables of releases or settings.
    #[tokio::test]
    async fn an_earlier_database_gains_the_release_tables_and_the_issue_column() {
        let (url, _guard) = navree_testing::temp_sqlite_url();
        let db = Db::connect(&DbOptions {
            url,
            ..DbOptions::default()
        })
        .await
        .unwrap();
        let schema = Schema::new(&db);
        schema
            .create("lighthouse_issues", |t| {
                t.string("id").primary();
                t.string("title");
            })
            .await
            .unwrap();
        assert!(!schema.has_table("lighthouse_releases").await.unwrap());

        create_config_tables(&schema).await.unwrap();
        assert!(schema.has_table("lighthouse_releases").await.unwrap());
        assert!(schema.has_table("lighthouse_settings").await.unwrap());
        assert!(
            schema
                .has_column("lighthouse_issues", "regressed_in_release")
                .await
                .unwrap()
        );

        // Running it again changes nothing.
        create_config_tables(&schema).await.unwrap();
    }
}
