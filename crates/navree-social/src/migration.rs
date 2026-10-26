//! The social module's migrations (`MigrationSet` `social`, after `auth`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the social module's migration set.
pub const SOCIAL_MIGRATIONS: &str = "social";

/// The social module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SOCIAL_MIGRATIONS)
        .after(["auth"])
        .migration(CreateSocialAccounts)
}

/// Creates `social_accounts`.
struct CreateSocialAccounts;

impl MigrationName for CreateSocialAccounts {
    fn name(&self) -> &str {
        "m20261026_000001_create_social_accounts_table"
    }
}

fn timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

#[async_trait]
impl MigrationTrait for CreateSocialAccounts {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("social_accounts")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("provider").string().not_null())
                    .col(ColumnDef::new("provider_user_id").string().not_null())
                    .col(ColumnDef::new("email").string().null())
                    .col(ColumnDef::new("name").string().null())
                    .col(ColumnDef::new("avatar_url").text().null())
                    .col(ColumnDef::new("nickname").string().null())
                    .col(ColumnDef::new("access_token").text().null())
                    .col(ColumnDef::new("refresh_token").text().null())
                    .col(
                        ColumnDef::new("token_expires_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("scopes").text().null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("social_accounts", "user_id")
                            .to("users", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("social_accounts_provider_user_unique")
                    .table("social_accounts")
                    .col("provider")
                    .col("provider_user_id")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("social_accounts_user_provider_unique")
                    .table("social_accounts")
                    .col("user_id")
                    .col("provider")
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("social_accounts").to_owned())
            .await
    }
}
