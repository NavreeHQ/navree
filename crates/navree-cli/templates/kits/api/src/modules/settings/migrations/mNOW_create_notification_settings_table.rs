//! Creates the `notification_settings` table (one row per member and
//! workspace) and, on PostgreSQL, its row-level security policy.

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
                    .table(NotificationSettings::Table)
                    .col(
                        ColumnDef::new(NotificationSettings::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NotificationSettings::WorkspaceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NotificationSettings::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(NotificationSettings::ProjectActivity)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NotificationSettings::WeeklySummary)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NotificationSettings::ProductUpdates)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NotificationSettings::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NotificationSettings::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_notification_settings_workspace_user")
                    .table(NotificationSettings::Table)
                    .col(NotificationSettings::WorkspaceId)
                    .col(NotificationSettings::UserId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        if manager.get_database_backend() == DbBackend::Postgres {
            manager
                .get_connection()
                .execute_unprepared(&RlsPolicy::new("notification_settings").create_sql())
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(NotificationSettings::Table).to_owned())
            .await
    }
}

/// The table and column names.
#[derive(DeriveIden)]
enum NotificationSettings {
    Table,
    Id,
    WorkspaceId,
    UserId,
    ProjectActivity,
    WeeklySummary,
    ProductUpdates,
    CreatedAt,
    UpdatedAt,
}
