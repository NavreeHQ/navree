//! The module's migrations (`MigrationSet` `flags`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the flags module's migration set.
pub const FLAGS_MIGRATIONS: &str = "flags";

/// The flags module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(FLAGS_MIGRATIONS).migration(CreateFlagTables)
}

/// Creates `feature_flags` and `feature_flag_overrides`.
struct CreateFlagTables;

impl MigrationName for CreateFlagTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_feature_flag_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateFlagTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("feature_flags")
                    .col(
                        ColumnDef::new("name")
                            .string_len(100)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new("everyone").boolean().null())
                    .col(ColumnDef::new("rollout").json_binary().null())
                    .col(
                        ColumnDef::new("killed")
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new("updated_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new("updated_by").uuid().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("feature_flag_overrides")
                    .col(ColumnDef::new("flag").string_len(100).not_null())
                    .col(ColumnDef::new("scope_type").string_len(16).not_null())
                    .col(ColumnDef::new("scope_id").uuid().not_null())
                    .col(ColumnDef::new("active").boolean().not_null())
                    .primary_key(
                        Index::create()
                            .col("flag")
                            .col("scope_type")
                            .col("scope_id"),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("feature_flag_overrides_scope_index")
                    .table("feature_flag_overrides")
                    .col("scope_type")
                    .col("scope_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("feature_flag_overrides").to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table("feature_flags").to_owned())
            .await
    }
}
