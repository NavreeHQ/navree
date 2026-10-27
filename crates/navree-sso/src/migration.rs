//! The module's migrations (`MigrationSet` `sso`, after `auth`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the module's migration set.
pub const SSO_MIGRATIONS: &str = "sso";

/// The module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SSO_MIGRATIONS)
        .after(["auth"])
        .migration(CreateSsoTables)
}

/// Creates `sso_connections`, `sso_domains` and `sso_identities`.
struct CreateSsoTables;

impl MigrationName for CreateSsoTables {
    fn name(&self) -> &str {
        "m20261027_000001_create_sso_tables"
    }
}

fn timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

#[async_trait]
impl MigrationTrait for CreateSsoTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("sso_connections")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("owner_type").string().not_null())
                    .col(ColumnDef::new("owner_id").uuid().null())
                    .col(ColumnDef::new("slug").string().not_null())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("protocol").string().not_null())
                    .col(ColumnDef::new("enabled").boolean().not_null())
                    .col(ColumnDef::new("enforce").boolean().null())
                    .col(ColumnDef::new("settings").text().not_null())
                    .col(ColumnDef::new("provisioning").text().not_null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("sso_connections_slug_unique")
                    .table("sso_connections")
                    .col("slug")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("sso_connections_owner_index")
                    .table("sso_connections")
                    .col("owner_type")
                    .col("owner_id")
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table("sso_domains")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("connection_id").uuid().not_null())
                    .col(ColumnDef::new("domain").string().not_null())
                    .col(ColumnDef::new("token").string().not_null())
                    .col(
                        ColumnDef::new("verified_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("sso_domains", "connection_id")
                            .to("sso_connections", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("sso_domains_connection_domain_unique")
                    .table("sso_domains")
                    .col("connection_id")
                    .col("domain")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("sso_domains_domain_index")
                    .table("sso_domains")
                    .col("domain")
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table("sso_identities")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("connection_id").uuid().not_null())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("subject").string().not_null())
                    .col(ColumnDef::new("email").string().null())
                    .col(ColumnDef::new("attributes").text().not_null())
                    .col(timestamp("last_login_at"))
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("sso_identities", "connection_id")
                            .to("sso_connections", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from("sso_identities", "user_id")
                            .to("users", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("sso_identities_connection_subject_unique")
                    .table("sso_identities")
                    .col("connection_id")
                    .col("subject")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("sso_identities_user_index")
                    .table("sso_identities")
                    .col("user_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in ["sso_identities", "sso_domains", "sso_connections"] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
