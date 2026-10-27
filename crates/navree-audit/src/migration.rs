//! The `audit` migration set: the `audit_logs` table, a TimescaleDB
//! hypertable on `created_at`.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Hypertable, KeyStrategy, Migration, Schema, SchemaMigration, SchemaResult,
};
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

use crate::policy::AUDIT_TABLE;

/// The migrations of `AuditModule` (module name `audit`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("audit")
        .migration(CreateAuditLogs)
        .migration(AddImpersonatorId)
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

/// Creates `audit_logs` with indexes on (subject_type, subject_id),
/// actor_id and created_at. No foreign key to `users`, so auditing works
/// without navree-auth and entries outlive deleted users.
struct CreateAuditLogs;

impl MigrationName for CreateAuditLogs {
    fn name(&self) -> &str {
        "m20261025_000001_create_audit_logs"
    }
}

#[async_trait]
impl MigrationTrait for CreateAuditLogs {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AUDIT_TABLE)
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("kind").string().not_null())
                    .col(ColumnDef::new("action").string().not_null())
                    .col(ColumnDef::new("actor_id").uuid().null())
                    .col(ColumnDef::new("subject_type").string().null())
                    .col(ColumnDef::new("subject_id").string().null())
                    .col(ColumnDef::new("old_values").json_binary().null())
                    .col(ColumnDef::new("new_values").json_binary().null())
                    .col(ColumnDef::new("request_id").string().null())
                    .col(ColumnDef::new("ip").string().null())
                    .col(ColumnDef::new("user_agent").text().null())
                    .col(ColumnDef::new("metadata").json_binary().not_null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        for (name, columns) in [
            (
                "idx_audit_logs_subject",
                &["subject_type", "subject_id"][..],
            ),
            ("idx_audit_logs_actor_id", &["actor_id"][..]),
            ("idx_audit_logs_created_at", &["created_at"][..]),
        ] {
            let mut index = Index::create();
            index.name(name).table(AUDIT_TABLE);
            for column in columns {
                index.col(*column);
            }
            manager.create_index(index).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AUDIT_TABLE).to_owned())
            .await
    }
}

/// Adds `impersonator_id` (who was impersonating the actor) with an index.
struct AddImpersonatorId;

impl MigrationName for AddImpersonatorId {
    fn name(&self) -> &str {
        "m20261027_000001_add_impersonator_id_to_audit_logs"
    }
}

#[async_trait]
impl MigrationTrait for AddImpersonatorId {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AUDIT_TABLE)
                    .add_column(ColumnDef::new("impersonator_id").uuid().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_logs_impersonator_id")
                    .table(AUDIT_TABLE)
                    .col("impersonator_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_audit_logs_impersonator_id")
                    .table(AUDIT_TABLE)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(AUDIT_TABLE)
                    .drop_column("impersonator_id")
                    .to_owned(),
            )
            .await
    }
}
