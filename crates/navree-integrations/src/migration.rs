//! The integrations module's migrations (`MigrationSet` `integrations`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Expr, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the integrations module's migration set.
pub const INTEGRATIONS_MIGRATIONS: &str = "integrations";

/// The integrations module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(INTEGRATIONS_MIGRATIONS).migration(CreateIntegrationConnections)
}

/// Creates `integration_connections`.
struct CreateIntegrationConnections;

impl MigrationName for CreateIntegrationConnections {
    fn name(&self) -> &str {
        "m20261027_000001_create_integration_connections_table"
    }
}

fn timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

#[async_trait]
impl MigrationTrait for CreateIntegrationConnections {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("integration_connections")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("owner_type").string_len(16).not_null())
                    .col(ColumnDef::new("owner_id").uuid().not_null())
                    .col(ColumnDef::new("provider").string_len(64).not_null())
                    .col(ColumnDef::new("scope_set").string_len(64).not_null())
                    .col(ColumnDef::new("kind").string_len(32).not_null())
                    .col(ColumnDef::new("external_id").string().null())
                    .col(ColumnDef::new("display_name").string().null())
                    .col(ColumnDef::new("credentials").text().not_null())
                    .col(ColumnDef::new("access_token").text().null())
                    .col(
                        ColumnDef::new("token_expires_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("scopes").text().null())
                    .col(
                        ColumnDef::new("status")
                            .string_len(16)
                            .not_null()
                            .default(Expr::val("healthy")),
                    )
                    .col(
                        ColumnDef::new("failures")
                            .integer()
                            .not_null()
                            .default(Expr::val(0)),
                    )
                    .col(ColumnDef::new("last_error").text().null())
                    .col(
                        ColumnDef::new("last_checked_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("connected_by").uuid().null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("integration_connections_owner_provider_unique")
                    .table("integration_connections")
                    .col("owner_type")
                    .col("owner_id")
                    .col("provider")
                    .col("scope_set")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("integration_connections_last_checked_index")
                    .table("integration_connections")
                    .col("last_checked_at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("integration_connections").to_owned())
            .await
    }
}
