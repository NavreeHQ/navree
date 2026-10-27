//! The `lighthouse` migration set: five tables prefixed `lighthouse_`.
//!
//! With `lighthouse.database_url` set, Lighthouse also creates the same
//! tables (when missing) in that database as it connects at boot, and
//! reads and writes there.

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the Lighthouse migration set.
pub const LIGHTHOUSE_MIGRATIONS: &str = "lighthouse";

/// The Lighthouse migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(LIGHTHOUSE_MIGRATIONS).migration(CreateLighthouseTables)
}

/// Creates the five tables.
pub(crate) struct CreateLighthouseTables;

impl MigrationName for CreateLighthouseTables {
    fn name(&self) -> &str {
        "m20261027_000001_create_lighthouse_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateLighthouseTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_tables(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_tables(manager).await
    }
}

fn timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

fn nullable_timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .null()
        .to_owned()
}

fn nullable_string(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).string().null().to_owned()
}

fn text(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).text().not_null().to_owned()
}

fn nullable_text(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).text().null().to_owned()
}

/// Creates the tables (if they do not exist yet).
pub(crate) async fn create_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table("lighthouse_issues")
                .if_not_exists()
                .col(ColumnDef::new("id").uuid().not_null().primary_key())
                .col(
                    ColumnDef::new("fingerprint")
                        .string()
                        .not_null()
                        .unique_key(),
                )
                .col(ColumnDef::new("kind").string().not_null())
                .col(ColumnDef::new("level").string().not_null())
                .col(ColumnDef::new("status").string().not_null())
                .col(ColumnDef::new("title").string().not_null())
                .col(ColumnDef::new("culprit").string().not_null())
                .col(text("message"))
                .col(text("chain"))
                .col(nullable_string("location"))
                .col(nullable_string("error_type"))
                .col(ColumnDef::new("count").big_integer().not_null())
                .col(timestamp("first_seen"))
                .col(timestamp("last_seen"))
                .col(nullable_string("first_release"))
                .col(nullable_string("last_release"))
                .col(nullable_string("environment"))
                .col(nullable_timestamp("resolved_at"))
                .col(nullable_string("resolved_in_release"))
                .col(nullable_timestamp("ignored_until"))
                .col(ColumnDef::new("ignore_remaining").big_integer().null())
                .col(nullable_timestamp("regressed_at"))
                .col(nullable_text("comment"))
                .col(timestamp("created_at"))
                .col(timestamp("updated_at"))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("lighthouse_issues_status_last_seen_index")
                .table("lighthouse_issues")
                .col("status")
                .col("last_seen")
                .to_owned(),
        )
        .await?;
    manager
        .create_table(
            Table::create()
                .table("lighthouse_occurrences")
                .if_not_exists()
                .col(ColumnDef::new("id").uuid().not_null().primary_key())
                .col(ColumnDef::new("issue_id").uuid().not_null())
                .col(timestamp("at"))
                .col(ColumnDef::new("level").string().not_null())
                .col(text("message"))
                .col(text("chain"))
                .col(nullable_text("backtrace"))
                .col(nullable_string("location"))
                .col(nullable_string("request_id"))
                .col(nullable_string("job_id"))
                .col(nullable_string("trace_id"))
                .col(nullable_string("user_id"))
                .col(nullable_string("workspace_id"))
                .col(nullable_string("release"))
                .col(nullable_string("environment"))
                .col(nullable_string("server"))
                .col(nullable_string("process"))
                .col(nullable_string("method"))
                .col(nullable_string("route"))
                .col(nullable_string("path"))
                .col(ColumnDef::new("status").integer().null())
                .col(ColumnDef::new("sample_id").uuid().null())
                .col(text("context"))
                .foreign_key(
                    ForeignKey::create()
                        .from("lighthouse_occurrences", "issue_id")
                        .to("lighthouse_issues", "id")
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("lighthouse_occurrences_issue_at_index")
                .table("lighthouse_occurrences")
                .col("issue_id")
                .col("at")
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("lighthouse_occurrences_at_index")
                .table("lighthouse_occurrences")
                .col("at")
                .to_owned(),
        )
        .await?;
    manager
        .create_table(
            Table::create()
                .table("lighthouse_samples")
                .if_not_exists()
                .col(ColumnDef::new("id").uuid().not_null().primary_key())
                .col(ColumnDef::new("kind").string().not_null())
                .col(ColumnDef::new("name").string().not_null())
                .col(timestamp("at"))
                .col(ColumnDef::new("duration_ms").double().not_null())
                .col(ColumnDef::new("status").integer().null())
                .col(ColumnDef::new("outcome").string().not_null())
                .col(ColumnDef::new("slow").boolean().not_null())
                .col(nullable_string("request_id"))
                .col(nullable_string("job_id"))
                .col(nullable_string("trace_id"))
                .col(nullable_string("user_id"))
                .col(nullable_string("workspace_id"))
                .col(nullable_string("release"))
                .col(nullable_string("environment"))
                .col(nullable_string("server"))
                .col(nullable_string("process"))
                .col(text("spans"))
                .col(text("context"))
                .col(nullable_text("error"))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("lighthouse_samples_kind_name_duration_index")
                .table("lighthouse_samples")
                .col("kind")
                .col("name")
                .col("duration_ms")
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("lighthouse_samples_at_index")
                .table("lighthouse_samples")
                .col("at")
                .to_owned(),
        )
        .await?;
    manager
        .create_table(
            Table::create()
                .table("lighthouse_aggregates")
                .if_not_exists()
                .col(ColumnDef::new("id").uuid().not_null().primary_key())
                .col(ColumnDef::new("resolution").integer().not_null())
                .col(timestamp("bucket"))
                .col(ColumnDef::new("kind").string().not_null())
                .col(ColumnDef::new("key_hash").string().not_null())
                .col(text("key"))
                .col(ColumnDef::new("process").string().not_null())
                .col(ColumnDef::new("count").big_integer().not_null())
                .col(ColumnDef::new("errors").big_integer().not_null())
                .col(ColumnDef::new("extra").big_integer().not_null())
                .col(ColumnDef::new("total_ms").double().not_null())
                .col(ColumnDef::new("max_ms").double().not_null())
                .col(text("histogram"))
                .col(nullable_text("detail"))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("lighthouse_aggregates_row_unique")
                .table("lighthouse_aggregates")
                .col("resolution")
                .col("bucket")
                .col("kind")
                .col("key_hash")
                .col("process")
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("lighthouse_aggregates_kind_bucket_index")
                .table("lighthouse_aggregates")
                .col("kind")
                .col("resolution")
                .col("bucket")
                .to_owned(),
        )
        .await?;
    manager
        .create_table(
            Table::create()
                .table("lighthouse_alerts")
                .if_not_exists()
                .col(ColumnDef::new("id").uuid().not_null().primary_key())
                .col(ColumnDef::new("issue_id").uuid().null())
                .col(ColumnDef::new("kind").string().not_null())
                .col(ColumnDef::new("channel").string().not_null())
                .col(ColumnDef::new("subject_key").string().not_null())
                .col(ColumnDef::new("status").string().not_null())
                .col(ColumnDef::new("attempts").integer().not_null().default(0))
                .col(nullable_text("error"))
                .col(text("payload"))
                .col(timestamp("created_at"))
                .col(nullable_timestamp("delivered_at"))
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("lighthouse_alerts_subject_kind_index")
                .table("lighthouse_alerts")
                .col("subject_key")
                .col("kind")
                .col("created_at")
                .to_owned(),
        )
        .await
}

/// Drops the tables.
async fn drop_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for table in [
        "lighthouse_alerts",
        "lighthouse_aggregates",
        "lighthouse_samples",
        "lighthouse_occurrences",
        "lighthouse_issues",
    ] {
        manager
            .drop_table(Table::drop().table(table).if_exists().to_owned())
            .await?;
    }
    Ok(())
}
