//! The workspace module's migrations (`MigrationSet` `workspace`, after
//! `auth`): the tables, then `workspaces.kind`.

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the workspace module's migration set; tables that reference
/// `workspaces` run `.after([WORKSPACE_MIGRATIONS])`.
pub const WORKSPACE_MIGRATIONS: &str = "workspace";

/// The workspace module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(WORKSPACE_MIGRATIONS)
        .after(["auth"])
        .migration(CreateWorkspaceTables)
        .migration(AddWorkspaceKind)
}

/// Adds `workspaces.kind` (`live` or `demo`, default `live`) and its index.
struct AddWorkspaceKind;

impl MigrationName for AddWorkspaceKind {
    fn name(&self) -> &str {
        "m20261027_000001_add_workspace_kind"
    }
}

#[async_trait]
impl MigrationTrait for AddWorkspaceKind {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table("workspaces")
                    .add_column(
                        ColumnDef::new("kind")
                            .string_len(16)
                            .not_null()
                            .default("live"),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("workspaces_kind_index")
                    .table("workspaces")
                    .col("kind")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("workspaces_kind_index")
                    .table("workspaces")
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table("workspaces")
                    .drop_column("kind")
                    .to_owned(),
            )
            .await
    }
}

/// Creates `workspaces`, `workspace_members` and `workspace_invitations`.
struct CreateWorkspaceTables;

impl MigrationName for CreateWorkspaceTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_workspace_tables"
    }
}

fn timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

#[async_trait]
impl MigrationTrait for CreateWorkspaceTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("workspaces")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("slug").string().not_null().unique_key())
                    .col(ColumnDef::new("owner_id").uuid().not_null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("workspaces_owner_id_index")
                    .table("workspaces")
                    .col("owner_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("workspace_members")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("role").string().not_null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("workspace_members", "workspace_id")
                            .to("workspaces", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("workspace_members_workspace_user_unique")
                    .table("workspace_members")
                    .col("workspace_id")
                    .col("user_id")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("workspace_members_user_id_index")
                    .table("workspace_members")
                    .col("user_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("workspace_invitations")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("email").string().not_null())
                    .col(ColumnDef::new("role").string().not_null())
                    .col(
                        ColumnDef::new("token_hash")
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new("invited_by").uuid().null())
                    .col(timestamp("expires_at"))
                    .col(
                        ColumnDef::new("accepted_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("accepted_by").uuid().null())
                    .col(timestamp("created_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("workspace_invitations", "workspace_id")
                            .to("workspaces", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("workspace_invitations_workspace_email_index")
                    .table("workspace_invitations")
                    .col("workspace_id")
                    .col("email")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in ["workspace_invitations", "workspace_members", "workspaces"] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
