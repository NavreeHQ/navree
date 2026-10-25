//! The `queue` migration set: tables `jobs` and `failed_jobs`.

use keel_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The table of queued jobs.
pub const JOBS_TABLE: &str = "jobs";

/// The table of failed jobs.
pub const FAILED_JOBS_TABLE: &str = "failed_jobs";

/// The migrations `QueueModule` contributes (module name `queue`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("queue").migration(CreateQueueTables)
}

/// Creates `jobs` and `failed_jobs`. Times are Unix milliseconds (`BIGINT`),
/// so reservation expiry is plain arithmetic on every backend.
struct CreateQueueTables;

impl MigrationName for CreateQueueTables {
    fn name(&self) -> &str {
        "m20261025_000001_create_queue_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateQueueTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(JOBS_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("queue").string().not_null())
                    .col(ColumnDef::new("job_type").string().not_null())
                    .col(ColumnDef::new("payload").text().not_null())
                    .col(ColumnDef::new("options").text().not_null())
                    .col(
                        ColumnDef::new("attempts")
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new("max_attempts").big_integer().not_null())
                    .col(ColumnDef::new("timeout_secs").big_integer().not_null())
                    .col(ColumnDef::new("available_at").big_integer().not_null())
                    .col(ColumnDef::new("reserved_at").big_integer().null())
                    .col(ColumnDef::new("reserved_by").string().null())
                    .col(ColumnDef::new("unique_key").string().null().unique_key())
                    .col(ColumnDef::new("created_at").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("jobs_queue_available_at_index")
                    .table(JOBS_TABLE)
                    .col("queue")
                    .col("available_at")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(FAILED_JOBS_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("queue").string().not_null())
                    .col(ColumnDef::new("job_type").string().not_null())
                    .col(ColumnDef::new("payload").text().not_null())
                    .col(ColumnDef::new("options").text().not_null())
                    .col(ColumnDef::new("error").text().not_null())
                    .col(ColumnDef::new("attempts").big_integer().not_null())
                    .col(ColumnDef::new("failed_at").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("failed_jobs_failed_at_index")
                    .table(FAILED_JOBS_TABLE)
                    .col("failed_at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(FAILED_JOBS_TABLE).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(JOBS_TABLE).to_owned())
            .await
    }
}
