//! The module's migrations (`MigrationSet` `media`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the media module's migration set.
pub const MEDIA_MIGRATIONS: &str = "media";

/// The media table.
pub const MEDIA_TABLE: &str = "media";

/// The media module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(MEDIA_MIGRATIONS).migration(CreateMediaTable)
}

/// Creates `media`.
struct CreateMediaTable;

impl MigrationName for CreateMediaTable {
    fn name(&self) -> &str {
        "m20261027_000001_create_media_table"
    }
}

#[async_trait]
impl MigrationTrait for CreateMediaTable {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MEDIA_TABLE)
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("model_type").string_len(100).not_null())
                    .col(ColumnDef::new("model_id").string_len(64).not_null())
                    .col(ColumnDef::new("collection").string_len(100).not_null())
                    .col(ColumnDef::new("name").string_len(255).not_null())
                    .col(ColumnDef::new("file_name").string_len(255).not_null())
                    .col(ColumnDef::new("mime_type").string_len(127).not_null())
                    .col(ColumnDef::new("disk").string_len(64).not_null())
                    .col(ColumnDef::new("conversions_disk").string_len(64).not_null())
                    .col(ColumnDef::new("path").string_len(1024).not_null())
                    .col(ColumnDef::new("size").big_integer().not_null())
                    .col(ColumnDef::new("width").integer().null())
                    .col(ColumnDef::new("height").integer().null())
                    .col(ColumnDef::new("conversions").json_binary().not_null())
                    .col(ColumnDef::new("metadata").json_binary().not_null())
                    .col(ColumnDef::new("custom_properties").json_binary().not_null())
                    .col(ColumnDef::new("order_column").integer().not_null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new("updated_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("media_model_index")
                    .table(MEDIA_TABLE)
                    .col("model_type")
                    .col("model_id")
                    .col("collection")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MEDIA_TABLE).to_owned())
            .await
    }
}
