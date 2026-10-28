//! Creates the `projects` table and, on PostgreSQL, its row-level security
//! policy: a transaction sees only the rows of the workspace it is in
//! (`navree.workspace`, set for every workspace request).

use navree::workspace::RlsPolicy;
use sea_orm::{ConnectionTrait, DbBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Projects::Table)
                    .col(ColumnDef::new(Projects::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Projects::WorkspaceId).uuid().not_null())
                    .col(ColumnDef::new(Projects::Name).string().not_null())
                    .col(ColumnDef::new(Projects::Description).text().null())
                    .col(ColumnDef::new(Projects::Status).string().not_null())
                    .col(ColumnDef::new(Projects::CreatedBy).uuid().not_null())
                    .col(
                        ColumnDef::new(Projects::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Projects::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_projects_workspace_id")
                    .table(Projects::Table)
                    .col(Projects::WorkspaceId)
                    .to_owned(),
            )
            .await?;
        if manager.get_database_backend() == DbBackend::Postgres {
            manager
                .get_connection()
                .execute_unprepared(&RlsPolicy::new("projects").create_sql())
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Projects::Table).to_owned())
            .await
    }
}

/// The table and column names (`Projects::Table` is `projects`).
#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    WorkspaceId,
    Name,
    Description,
    Status,
    CreatedBy,
    CreatedAt,
    UpdatedAt,
}
