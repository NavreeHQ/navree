//! The `audit` migration set: the `audit_logs` table, a TimescaleDB
//! hypertable on `created_at`, written with the schema builder.
//!
//! The names are the ones the module's migrations always had, so a
//! database migrated by an earlier version finds nothing pending. The
//! columns are the types those migrations created: UUIDs as `BINARY(16)`
//! on MySQL and MariaDB (where navree-db writes UUIDs as 16 bytes), text
//! as `VARCHAR` without a length on PostgreSQL and SQLite and
//! `VARCHAR(255)` on MySQL, JSON as `jsonb` on PostgreSQL.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Hypertable, KeyStrategy, Migration, Schema, SchemaMigration,
    SchemaResult,
};

use crate::policy::AUDIT_TABLE;

/// The migrations of `AuditModule` (module name `audit`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("audit")
        .migration(SchemaMigration::new(
            "m20261025_000001_create_audit_logs",
            CreateAuditLogs,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000001_add_impersonator_id_to_audit_logs",
            AddImpersonatorId,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000002_convert_audit_logs_to_hypertable",
            ConvertAuditLogsToHypertable,
        ))
}

/// How `audit_logs` is partitioned on TimescaleDB: by `created_at`, a
/// week per chunk (the audit trail grows steadily and is kept for a year by
/// default, so about fifty chunks). `audit:prune` drops whole chunks.
pub(crate) fn hypertable() -> Hypertable {
    let mut audit = Hypertable::on("created_at");
    audit
        .chunk_interval("7 days")
        .keys(KeyStrategy::Extend)
        .migrate_data();
    audit
}

/// Makes `audit_logs` a hypertable on TimescaleDB: its primary key becomes
/// `(id, created_at)` and its rows move into chunks. Elsewhere it stays a
/// regular table with its `created_at` index, and the migration reports a
/// notice.
///
/// Reverting it keeps the hypertable: TimescaleDB cannot turn one back into
/// a plain table in place, and the table works the same either way. Running
/// it again leaves a hypertable as it is.
struct ConvertAuditLogsToHypertable;

impl Migration for ConvertAuditLogsToHypertable {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        if schema.is_hypertable(AUDIT_TABLE).await? {
            return Ok(());
        }
        schema
            .convert_to_hypertable(AUDIT_TABLE, hypertable())
            .await
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
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

/// Creates `audit_logs` with indexes on (subject_type, subject_id),
/// actor_id and created_at. No foreign key to `users`, so auditing works
/// without navree-auth and entries outlive deleted users.
struct CreateAuditLogs;

impl Migration for CreateAuditLogs {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create(AUDIT_TABLE, |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "kind", dialect);
                varchar(t, "action", dialect);
                uuid(t, "actor_id", dialect)
                    .nullable()
                    .index_named("idx_audit_logs_actor_id");
                varchar(t, "subject_type", dialect).nullable();
                varchar(t, "subject_id", dialect).nullable();
                t.jsonb("old_values").nullable();
                t.jsonb("new_values").nullable();
                varchar(t, "request_id", dialect).nullable();
                varchar(t, "ip", dialect).nullable();
                t.text("user_agent").nullable();
                t.jsonb("metadata");
                t.timestamp_tz("created_at")
                    .index_named("idx_audit_logs_created_at");
                t.index(["subject_type", "subject_id"])
                    .name("idx_audit_logs_subject");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(AUDIT_TABLE).await
    }
}

/// Adds `impersonator_id` (who was impersonating the actor) with an index.
struct AddImpersonatorId;

impl Migration for AddImpersonatorId {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .table(AUDIT_TABLE, |t| {
                uuid(t, "impersonator_id", dialect)
                    .nullable()
                    .index_named("idx_audit_logs_impersonator_id");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table(AUDIT_TABLE, |t| {
                t.drop_index("idx_audit_logs_impersonator_id");
                t.drop_column("impersonator_id");
            })
            .await
    }
}
