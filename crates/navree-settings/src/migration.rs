//! The module's migrations (`MigrationSet` `settings`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the settings module's migration set.
pub const SETTINGS_MIGRATIONS: &str = "settings";

/// The settings module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SETTINGS_MIGRATIONS).migration(CreateSettingsTable)
}

/// Creates `settings`.
struct CreateSettingsTable;

impl MigrationName for CreateSettingsTable {
    fn name(&self) -> &str {
        "m20261027_000001_create_settings_table"
    }
}

#[async_trait]
impl MigrationTrait for CreateSettingsTable {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("settings")
                    .col(ColumnDef::new("key").string_len(100).not_null())
                    .col(ColumnDef::new("scope_type").string_len(16).not_null())
                    .col(ColumnDef::new("scope_id").uuid().not_null())
                    .col(ColumnDef::new("value").json_binary().not_null())
                    .col(ColumnDef::new("overridden_fields").json_binary().not_null())
                    .col(
                        ColumnDef::new("updated_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new("updated_by").uuid().null())
                    .primary_key(Index::create().col("key").col("scope_type").col("scope_id"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("settings_scope_index")
                    .table("settings")
                    .col("scope_type")
                    .col("scope_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("settings").to_owned())
            .await
    }
}
