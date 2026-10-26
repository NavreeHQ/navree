//! The `queue` migration set: tables `jobs`, `failed_jobs`, `job_batches`
//! and `operations`.

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The table of queued jobs.
pub const JOBS_TABLE: &str = "jobs";

/// The table of failed jobs.
pub const FAILED_JOBS_TABLE: &str = "failed_jobs";

/// The table of job batches.
pub const JOB_BATCHES_TABLE: &str = "job_batches";

/// The table of operations.
pub const OPERATIONS_TABLE: &str = "operations";

/// The migrations `QueueModule` contributes (module name `queue`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("queue")
        .migration(CreateQueueTables)
        .migration(CreateJobBatches)
        .migration(CreateOperations)
}

/// Creates `job_batches` and adds each job's batch and operation to `jobs`
/// (one column per statement, which SQLite needs).
struct CreateJobBatches;

impl MigrationName for CreateJobBatches {
    fn name(&self) -> &str {
        "m20261026_000001_create_job_batches"
    }
}

#[async_trait]
impl MigrationTrait for CreateJobBatches {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(JOB_BATCHES_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("total_jobs").big_integer().not_null())
                    .col(ColumnDef::new("pending_jobs").big_integer().not_null())
                    .col(ColumnDef::new("failed_jobs").big_integer().not_null())
                    .col(ColumnDef::new("failed_job_ids").text().not_null())
                    .col(ColumnDef::new("callbacks").text().not_null())
                    .col(ColumnDef::new("callbacks_pending").big_integer().not_null())
                    .col(ColumnDef::new("callbacks_failed").big_integer().not_null())
                    .col(ColumnDef::new("allow_failures").boolean().not_null())
                    .col(ColumnDef::new("operation_id").string().null())
                    .col(ColumnDef::new("created_at").big_integer().not_null())
                    .col(ColumnDef::new("cancelled_at").big_integer().null())
                    .col(ColumnDef::new("finished_at").big_integer().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("job_batches_created_at_index")
                    .table(JOB_BATCHES_TABLE)
                    .col("created_at")
                    .to_owned(),
            )
            .await?;
        for column in [
            ColumnDef::new("batch_id").string().null().to_owned(),
            ColumnDef::new("batch_callback")
                .boolean()
                .not_null()
                .default(false)
                .to_owned(),
            ColumnDef::new("operation_id").string().null().to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(JOBS_TABLE)
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }
        manager
            .create_index(
                Index::create()
                    .name("jobs_operation_id_index")
                    .table(JOBS_TABLE)
                    .col("operation_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("jobs_operation_id_index")
                    .table(JOBS_TABLE)
                    .to_owned(),
            )
            .await?;
        for column in ["operation_id", "batch_callback", "batch_id"] {
            manager
                .alter_table(
                    Table::alter()
                        .table(JOBS_TABLE)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        manager
            .drop_table(Table::drop().table(JOB_BATCHES_TABLE).to_owned())
            .await
    }
}

/// Creates `operations`: user-visible long work with its owner, workspace,
/// status, progress, result and heartbeat. Times are Unix milliseconds.
struct CreateOperations;

impl MigrationName for CreateOperations {
    fn name(&self) -> &str {
        "m20261026_000002_create_operations"
    }
}

#[async_trait]
impl MigrationTrait for CreateOperations {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(OPERATIONS_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("kind").string().not_null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("owner_id").string().null())
                    .col(ColumnDef::new("workspace_id").string().null())
                    .col(ColumnDef::new("progress_done").big_integer().not_null())
                    .col(ColumnDef::new("progress_total").big_integer().not_null())
                    .col(ColumnDef::new("progress_note").text().not_null())
                    .col(ColumnDef::new("result").text().null())
                    .col(ColumnDef::new("error").text().null())
                    .col(ColumnDef::new("batch_id").string().null())
                    .col(ColumnDef::new("job").text().null())
                    .col(ColumnDef::new("idempotent").boolean().not_null())
                    .col(ColumnDef::new("redispatches").big_integer().not_null())
                    .col(ColumnDef::new("version").big_integer().not_null())
                    .col(ColumnDef::new("created_at").big_integer().not_null())
                    .col(ColumnDef::new("started_at").big_integer().null())
                    .col(ColumnDef::new("heartbeat_at").big_integer().null())
                    .col(ColumnDef::new("finished_at").big_integer().null())
                    .col(ColumnDef::new("updated_at").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("operations_owner_id_index")
                    .table(OPERATIONS_TABLE)
                    .col("owner_id")
                    .col("created_at")
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("operations_status_heartbeat_at_index")
                    .table(OPERATIONS_TABLE)
                    .col("status")
                    .col("heartbeat_at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(OPERATIONS_TABLE).to_owned())
            .await
    }
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
