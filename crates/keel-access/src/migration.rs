//! The access module's migrations (`MigrationSet` `access`, after `auth`).

use keel_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Query, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

use crate::entity::access_meta::VERSION_KEY;

/// The access module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("access")
        .after(["auth"])
        .migration(CreateAccessTables)
}

/// Creates `roles`, `permissions`, `role_permissions`, `user_roles`,
/// `user_permissions` and `access_meta` (with the version row at 1).
struct CreateAccessTables;

impl MigrationName for CreateAccessTables {
    fn name(&self) -> &str {
        "m20261025_000001_create_access_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateAccessTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("roles")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null().unique_key())
                    .col(ColumnDef::new("description").string().null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("permissions")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null().unique_key())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("role_permissions")
                    .col(ColumnDef::new("role_id").uuid().not_null())
                    .col(ColumnDef::new("permission_id").uuid().not_null())
                    .primary_key(Index::create().col("role_id").col("permission_id"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("role_permissions", "role_id")
                            .to("roles", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from("role_permissions", "permission_id")
                            .to("permissions", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("user_roles")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("role_id").uuid().not_null())
                    .col(ColumnDef::new("team_id").uuid().null())
                    .foreign_key(
                        ForeignKey::create()
                            .from("user_roles", "role_id")
                            .to("roles", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("user_roles_user_role_team_unique")
                    .table("user_roles")
                    .col("user_id")
                    .col("role_id")
                    .col("team_id")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("user_permissions")
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("permission_id").uuid().not_null())
                    .primary_key(Index::create().col("user_id").col("permission_id"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("user_permissions", "permission_id")
                            .to("permissions", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("access_meta")
                    .col(ColumnDef::new("key").string().not_null().primary_key())
                    .col(ColumnDef::new("version").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .execute(
                Query::insert()
                    .into_table("access_meta")
                    .columns(["key", "version"])
                    .values_panic([VERSION_KEY.into(), 1_i64.into()])
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "access_meta",
            "user_permissions",
            "user_roles",
            "role_permissions",
            "permissions",
            "roles",
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
