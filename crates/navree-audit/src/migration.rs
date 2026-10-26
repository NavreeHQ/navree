//! The `audit` migration set: the `audit_logs` table.

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

use crate::policy::AUDIT_TABLE;

/// The migrations of `AuditModule` (module name `audit`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("audit").migration(CreateAuditLogs)
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
