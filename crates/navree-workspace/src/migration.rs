//! The workspace module's migrations (`MigrationSet` `workspace`, after
//! `auth`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the workspace module's migration set; tables that reference
/// `workspaces` run `.after([WORKSPACE_MIGRATIONS])`.
pub const WORKSPACE_MIGRATIONS: &str = "workspace";

/// The workspace module's migration set. The organization and partition
/// tables are created whether or not the features are on, so turning one on
/// later needs no migration.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(WORKSPACE_MIGRATIONS)
        .after(["auth"])
        .migration(CreateWorkspaceTables)
        .migration(CreateOrganizationTables)
        .migration(CreatePartitionTables)
        .migration(AddWorkspaceTimeZone)
}

/// Adds `workspaces.timezone` and `workspaces.country` (nullable).
struct AddWorkspaceTimeZone;

impl MigrationName for AddWorkspaceTimeZone {
    fn name(&self) -> &str {
        "m20261027_000001_add_workspace_time_zone"
    }
}

#[async_trait]
impl MigrationTrait for AddWorkspaceTimeZone {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One column per statement: SQLite alters one at a time.
        manager
            .alter_table(
                Table::alter()
                    .table("workspaces")
                    .add_column(ColumnDef::new("timezone").string_len(64).null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table("workspaces")
                    .add_column(ColumnDef::new("country").string_len(2).null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in ["country", "timezone"] {
            manager
                .alter_table(
                    Table::alter()
                        .table("workspaces")
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
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

/// Creates `organizations`, `organization_members`, `organization_grants`
/// and `access_requests`, and adds `workspaces.organization_id`.
struct CreateOrganizationTables;

impl MigrationName for CreateOrganizationTables {
    fn name(&self) -> &str {
        "m20261026_000002_create_organization_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateOrganizationTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("organizations")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("slug").string().not_null().unique_key())
                    .col(ColumnDef::new("owner_id").uuid().not_null())
                    .col(ColumnDef::new("country").string_len(2).null())
                    .col(ColumnDef::new("timezone").string_len(64).null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "organizations_owner_id_index",
            "organizations",
            &["owner_id"],
            false,
        )
        .await?;
        manager
            .create_table(
                Table::create()
                    .table("organization_members")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("organization_id").uuid().not_null())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("role").string().not_null())
                    .col(scope("scope"))
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("organization_members", "organization_id")
                            .to("organizations", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "organization_members_organization_user_unique",
            "organization_members",
            &["organization_id", "user_id"],
            true,
        )
        .await?;
        index(
            manager,
            "organization_members_user_id_index",
            "organization_members",
            &["user_id"],
            false,
        )
        .await?;
        manager
            .create_table(
                Table::create()
                    .table("organization_grants")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("organization_id").uuid().not_null())
                    .col(ColumnDef::new("member_id").uuid().not_null())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(timestamp("created_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("organization_grants", "member_id")
                            .to("organization_members", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from("organization_grants", "workspace_id")
                            .to("workspaces", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "organization_grants_member_workspace_unique",
            "organization_grants",
            &["member_id", "workspace_id"],
            true,
        )
        .await?;
        index(
            manager,
            "organization_grants_workspace_id_index",
            "organization_grants",
            &["workspace_id"],
            false,
        )
        .await?;
        manager
            .create_table(
                Table::create()
                    .table("access_requests")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("organization_id").uuid().not_null())
                    .col(ColumnDef::new("workspace_id").uuid().null())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("message").text().null())
                    .col(ColumnDef::new("status").string_len(16).not_null())
                    .col(ColumnDef::new("role").string().null())
                    .col(ColumnDef::new("decided_by").uuid().null())
                    .col(
                        ColumnDef::new("decided_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("reason").text().null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("access_requests", "organization_id")
                            .to("organizations", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from("access_requests", "workspace_id")
                            .to("workspaces", "id")
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "access_requests_organization_status_index",
            "access_requests",
            &["organization_id", "status"],
            false,
        )
        .await?;
        index(
            manager,
            "access_requests_user_id_index",
            "access_requests",
            &["user_id"],
            false,
        )
        .await?;
        // A plain nullable column: SQLite can't add a foreign key to an
        // existing table, so `Organizations::delete` detaches workspaces
        // itself.
        manager
            .alter_table(
                Table::alter()
                    .table("workspaces")
                    .add_column(ColumnDef::new("organization_id").uuid().null())
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "workspaces_organization_id_index",
            "workspaces",
            &["organization_id"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("workspaces_organization_id_index")
                    .table("workspaces")
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table("workspaces")
                    .drop_column("organization_id")
                    .to_owned(),
            )
            .await?;
        for table in [
            "access_requests",
            "organization_grants",
            "organization_members",
            "organizations",
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}

/// Creates `workspace_partitions` and `partition_grants`, and adds
/// `workspace_members.partition_scope`.
struct CreatePartitionTables;

impl MigrationName for CreatePartitionTables {
    fn name(&self) -> &str {
        "m20261026_000003_create_partition_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreatePartitionTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("workspace_partitions")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("slug").string().not_null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("workspace_partitions", "workspace_id")
                            .to("workspaces", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "workspace_partitions_workspace_slug_unique",
            "workspace_partitions",
            &["workspace_id", "slug"],
            true,
        )
        .await?;
        manager
            .create_table(
                Table::create()
                    .table("partition_grants")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("member_id").uuid().not_null())
                    .col(ColumnDef::new("partition_id").uuid().not_null())
                    .col(timestamp("created_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("partition_grants", "member_id")
                            .to("workspace_members", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from("partition_grants", "partition_id")
                            .to("workspace_partitions", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "partition_grants_member_partition_unique",
            "partition_grants",
            &["member_id", "partition_id"],
            true,
        )
        .await?;
        index(
            manager,
            "partition_grants_partition_id_index",
            "partition_grants",
            &["partition_id"],
            false,
        )
        .await?;
        manager
            .alter_table(
                Table::alter()
                    .table("workspace_members")
                    .add_column(scope("partition_scope"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table("workspace_members")
                    .drop_column("partition_scope")
                    .to_owned(),
            )
            .await?;
        for table in ["partition_grants", "workspace_partitions"] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}

/// A scope column: `all` (the default) or `only`.
fn scope(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .string_len(8)
        .not_null()
        .default("all")
        .to_owned()
}

/// Creates index `name` on `table` (`columns`), unique or not.
async fn index(
    manager: &SchemaManager<'_>,
    name: &str,
    table: &'static str,
    columns: &[&'static str],
    unique: bool,
) -> Result<(), DbErr> {
    let mut index = Index::create();
    index.name(name).table(table);
    for column in columns {
        index.col(*column);
    }
    if unique {
        index.unique();
    }
    manager.create_index(index.to_owned()).await
}
