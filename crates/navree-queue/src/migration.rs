//! The `queue` migration set: tables `jobs`, `failed_jobs`, `job_batches`
//! and `operations`, written with the schema builder.
//!
//! The names are the ones the module's migrations always had, so a
//! database migrated by an earlier version finds nothing pending. Ids are
//! text, times are Unix milliseconds (`BIGINT`), so reservation expiry is
//! plain arithmetic on every backend.

use navree_db::MigrationSet;
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};

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
        .migration(SchemaMigration::new(
            "m20261025_000001_create_queue_tables",
            CreateQueueTables,
        ))
        .migration(SchemaMigration::new(
            "m20261026_000001_add_jobs_trace_parent",
            AddJobsTraceParent,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000001_create_job_batches",
            CreateJobBatches,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000002_create_operations",
            CreateOperations,
        ))
}

/// Adds `jobs.trace_parent`: the W3C `traceparent` of the span that
/// dispatched a job, so its attempts continue that trace (nullable; jobs
/// queued before it start their own trace).
struct AddJobsTraceParent;

impl Migration for AddJobsTraceParent {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table(JOBS_TABLE, |t| {
                t.string("trace_parent").nullable();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table(JOBS_TABLE, |t| t.drop_column("trace_parent"))
            .await
    }
}

/// Creates `job_batches` and adds each job's batch and operation to `jobs`.
struct CreateJobBatches;

impl Migration for CreateJobBatches {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(JOB_BATCHES_TABLE, |t| {
                t.string("id").primary();
                t.string("name");
                t.big_integer("total_jobs");
                t.big_integer("pending_jobs");
                t.big_integer("failed_jobs");
                t.text("failed_job_ids");
                t.text("callbacks");
                t.big_integer("callbacks_pending");
                t.big_integer("callbacks_failed");
                t.boolean("allow_failures");
                t.string("operation_id").nullable();
                t.big_integer("created_at");
                t.big_integer("cancelled_at").nullable();
                t.big_integer("finished_at").nullable();
                t.index(["created_at"]).name("job_batches_created_at_index");
            })
            .await?;
        schema
            .table(JOBS_TABLE, |t| {
                t.string("batch_id").nullable();
                t.boolean("batch_callback").default(false);
                t.string("operation_id").nullable();
                t.index(["operation_id"]).name("jobs_operation_id_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table(JOBS_TABLE, |t| {
                t.drop_index("jobs_operation_id_index");
                t.drop_column(["operation_id", "batch_callback", "batch_id"]);
            })
            .await?;
        schema.drop_if_exists(JOB_BATCHES_TABLE).await
    }
}

/// Creates `operations`: user-visible long work with its owner, workspace,
/// status, progress, result and heartbeat. Times are Unix milliseconds.
struct CreateOperations;

impl Migration for CreateOperations {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(OPERATIONS_TABLE, |t| {
                t.string("id").primary();
                t.string("kind");
                t.string("status");
                t.string("owner_id").nullable();
                t.string("workspace_id").nullable();
                t.big_integer("progress_done");
                t.big_integer("progress_total");
                t.text("progress_note");
                t.text("result").nullable();
                t.text("error").nullable();
                t.string("batch_id").nullable();
                t.text("job").nullable();
                t.boolean("idempotent");
                t.big_integer("redispatches");
                t.big_integer("version");
                t.big_integer("created_at");
                t.big_integer("started_at").nullable();
                t.big_integer("heartbeat_at").nullable();
                t.big_integer("finished_at").nullable();
                t.big_integer("updated_at");
                t.index(["owner_id", "created_at"])
                    .name("operations_owner_id_index");
                t.index(["status", "heartbeat_at"])
                    .name("operations_status_heartbeat_at_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(OPERATIONS_TABLE).await
    }
}

/// Creates `jobs` and `failed_jobs`.
struct CreateQueueTables;

impl Migration for CreateQueueTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(JOBS_TABLE, |t| {
                t.string("id").primary();
                t.string("queue");
                t.string("job_type");
                t.text("payload");
                t.text("options");
                t.big_integer("attempts").default(0);
                t.big_integer("max_attempts");
                t.big_integer("timeout_secs");
                t.big_integer("available_at");
                t.big_integer("reserved_at").nullable();
                t.string("reserved_by").nullable();
                t.string("unique_key").nullable().unique();
                t.big_integer("created_at");
                t.index(["queue", "available_at"])
                    .name("jobs_queue_available_at_index");
            })
            .await?;
        schema
            .create(FAILED_JOBS_TABLE, |t| {
                t.string("id").primary();
                t.string("queue");
                t.string("job_type");
                t.text("payload");
                t.text("options");
                t.text("error");
                t.big_integer("attempts");
                t.big_integer("failed_at");
                t.index(["failed_at"]).name("failed_jobs_failed_at_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(FAILED_JOBS_TABLE).await?;
        schema.drop_if_exists(JOBS_TABLE).await
    }
}
