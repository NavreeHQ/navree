//! The `notify` migration set: the `notifications` table.

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The table of the database channel.
pub const NOTIFICATIONS_TABLE: &str = "notifications";

/// The migrations of `NotifyModule` (module name `notify`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("notify").migration(CreateNotifications)
}

/// Creates `notifications` with an index on the owner and read state. No
/// foreign key: the owner is polymorphic (`notifiable_type`, `notifiable_id`).
struct CreateNotifications;

impl MigrationName for CreateNotifications {
    fn name(&self) -> &str {
        "m20261025_000001_create_notifications"
    }
}

#[async_trait]
impl MigrationTrait for CreateNotifications {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(NOTIFICATIONS_TABLE)
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("notifiable_type").string().not_null())
                    .col(ColumnDef::new("notifiable_id").string().not_null())
                    .col(ColumnDef::new("type").string().not_null())
                    .col(ColumnDef::new("data").json_binary().not_null())
                    .col(ColumnDef::new("read_at").timestamp_with_time_zone().null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_notifications_notifiable")
                    .table(NOTIFICATIONS_TABLE)
                    .col("notifiable_type")
                    .col("notifiable_id")
                    .col("read_at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(NOTIFICATIONS_TABLE).to_owned())
            .await
    }
}
