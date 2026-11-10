//! The workspace module's migrations (`MigrationSet` `workspace`, after
//! `auth`): the tables, then `workspaces.timezone`, `country` and `kind`,
//! written with the schema builder.
//!
//! Their names are the ones the module's migrations always had, so a
//! database migrated by an earlier version finds nothing pending. The
//! columns are the types those migrations created: UUIDs as `BINARY(16)` on
//! MySQL and MariaDB (where navree-db writes UUIDs as 16 bytes), names,
//! slugs, emails and roles as `VARCHAR` without a length on PostgreSQL and
//! SQLite and `VARCHAR(255)` on MySQL.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the workspace module's migration set; tables that reference
/// `workspaces` run `.after([WORKSPACE_MIGRATIONS])`.
pub const WORKSPACE_MIGRATIONS: &str = "workspace";

/// The workspace module's migration set. The organization and partition
/// tables are created whether or not the features are on, so turning one on
/// later needs no migration.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(WORKSPACE_MIGRATIONS)
        .after(["auth"])
        .migration(SchemaMigration::new(
            "m20261026_000001_create_workspace_tables",
            CreateWorkspaceTables,
        ))
        .migration(SchemaMigration::new(
            "m20261026_000002_create_organization_tables",
            CreateOrganizationTables,
        ))
        .migration(SchemaMigration::new(
            "m20261026_000003_create_partition_tables",
            CreatePartitionTables,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000001_add_workspace_time_zone",
            AddWorkspaceTimeZone,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000002_add_workspace_kind",
            AddWorkspaceKind,
        ))
}

/// A UUID column: `BINARY(16)` on MySQL and MariaDB, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// A text column of no fixed length: `VARCHAR(255)` on MySQL and MariaDB,
/// `VARCHAR` elsewhere.
fn varchar<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.string(name).length(255)
    } else {
        t.raw_column(name, "VARCHAR")
    }
}

/// A scope column: `all` (the default) or `only`.
fn scope<'a>(t: &'a mut Blueprint, name: &str) -> ColumnBuilder<'a> {
    t.string(name).length(8).default("all")
}

/// The dialect of the database `schema` runs on.
async fn dialect(schema: &Schema<'_>) -> SchemaResult<Dialect> {
    Ok(schema.target().await?.dialect())
}

/// Creates `workspaces`, `workspace_members` and `workspace_invitations`.
struct CreateWorkspaceTables;

impl Migration for CreateWorkspaceTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = dialect(schema).await?;
        schema
            .create("workspaces", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "name", dialect);
                varchar(t, "slug", dialect).unique();
                uuid(t, "owner_id", dialect);
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index("owner_id").name("workspaces_owner_id_index");
            })
            .await?;
        schema
            .create("workspace_members", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "workspace_id", dialect);
                uuid(t, "user_id", dialect);
                varchar(t, "role", dialect);
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign("workspace_id")
                    .references("id")
                    .on("workspaces")
                    .cascade_on_delete();
                t.unique(["workspace_id", "user_id"])
                    .name("workspace_members_workspace_user_unique");
                t.index("user_id").name("workspace_members_user_id_index");
            })
            .await?;
        schema
            .create("workspace_invitations", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "workspace_id", dialect);
                varchar(t, "email", dialect);
                varchar(t, "role", dialect);
                varchar(t, "token_hash", dialect).unique();
                uuid(t, "invited_by", dialect).nullable();
                t.timestamp_tz("expires_at");
                t.timestamp_tz("accepted_at").nullable();
                uuid(t, "accepted_by", dialect).nullable();
                t.timestamp_tz("created_at");
                t.foreign("workspace_id")
                    .references("id")
                    .on("workspaces")
                    .cascade_on_delete();
                t.index(["workspace_id", "email"])
                    .name("workspace_invitations_workspace_email_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in ["workspace_invitations", "workspace_members", "workspaces"] {
            schema.drop(table).await?;
        }
        Ok(())
    }
}

/// Creates `organizations`, `organization_members`, `organization_grants`
/// and `access_requests`, and adds `workspaces.organization_id`.
struct CreateOrganizationTables;

impl Migration for CreateOrganizationTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = dialect(schema).await?;
        schema
            .create("organizations", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "name", dialect);
                varchar(t, "slug", dialect).unique();
                uuid(t, "owner_id", dialect);
                t.string("country").length(2).nullable();
                t.string("timezone").length(64).nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index("owner_id").name("organizations_owner_id_index");
            })
            .await?;
        schema
            .create("organization_members", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "organization_id", dialect);
                uuid(t, "user_id", dialect);
                varchar(t, "role", dialect);
                scope(t, "scope");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign("organization_id")
                    .references("id")
                    .on("organizations")
                    .cascade_on_delete();
                t.unique(["organization_id", "user_id"])
                    .name("organization_members_organization_user_unique");
                t.index("user_id")
                    .name("organization_members_user_id_index");
            })
            .await?;
        schema
            .create("organization_grants", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "organization_id", dialect);
                uuid(t, "member_id", dialect);
                uuid(t, "workspace_id", dialect);
                t.timestamp_tz("created_at");
                t.foreign("member_id")
                    .references("id")
                    .on("organization_members")
                    .cascade_on_delete();
                t.foreign("workspace_id")
                    .references("id")
                    .on("workspaces")
                    .cascade_on_delete();
                t.unique(["member_id", "workspace_id"])
                    .name("organization_grants_member_workspace_unique");
                t.index("workspace_id")
                    .name("organization_grants_workspace_id_index");
            })
            .await?;
        schema
            .create("access_requests", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "organization_id", dialect);
                uuid(t, "workspace_id", dialect).nullable();
                uuid(t, "user_id", dialect);
                t.text("message").nullable();
                t.string("status").length(16);
                varchar(t, "role", dialect).nullable();
                uuid(t, "decided_by", dialect).nullable();
                t.timestamp_tz("decided_at").nullable();
                t.text("reason").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign("organization_id")
                    .references("id")
                    .on("organizations")
                    .cascade_on_delete();
                t.foreign("workspace_id")
                    .references("id")
                    .on("workspaces")
                    .null_on_delete();
                t.index(["organization_id", "status"])
                    .name("access_requests_organization_status_index");
                t.index("user_id").name("access_requests_user_id_index");
            })
            .await?;
        // A plain nullable column: SQLite can't add a foreign key to an
        // existing table, so `Organizations::delete` detaches workspaces
        // itself.
        schema
            .table("workspaces", |t| {
                uuid(t, "organization_id", dialect).nullable();
                t.index("organization_id")
                    .name("workspaces_organization_id_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("workspaces", |t| {
                t.drop_index("workspaces_organization_id_index");
            })
            .await?;
        schema.drop_columns("workspaces", "organization_id").await?;
        for table in [
            "access_requests",
            "organization_grants",
            "organization_members",
            "organizations",
        ] {
            schema.drop(table).await?;
        }
        Ok(())
    }
}

/// Creates `workspace_partitions` and `partition_grants`, and adds
/// `workspace_members.partition_scope`.
struct CreatePartitionTables;

impl Migration for CreatePartitionTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = dialect(schema).await?;
        schema
            .create("workspace_partitions", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "workspace_id", dialect);
                varchar(t, "name", dialect);
                varchar(t, "slug", dialect);
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign("workspace_id")
                    .references("id")
                    .on("workspaces")
                    .cascade_on_delete();
                t.unique(["workspace_id", "slug"])
                    .name("workspace_partitions_workspace_slug_unique");
            })
            .await?;
        schema
            .create("partition_grants", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "workspace_id", dialect);
                uuid(t, "member_id", dialect);
                uuid(t, "partition_id", dialect);
                t.timestamp_tz("created_at");
                t.foreign("member_id")
                    .references("id")
                    .on("workspace_members")
                    .cascade_on_delete();
                t.foreign("partition_id")
                    .references("id")
                    .on("workspace_partitions")
                    .cascade_on_delete();
                t.unique(["member_id", "partition_id"])
                    .name("partition_grants_member_partition_unique");
                t.index("partition_id")
                    .name("partition_grants_partition_id_index");
            })
            .await?;
        schema
            .table("workspace_members", |t| {
                scope(t, "partition_scope");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .drop_columns("workspace_members", "partition_scope")
            .await?;
        for table in ["partition_grants", "workspace_partitions"] {
            schema.drop(table).await?;
        }
        Ok(())
    }
}

/// Adds `workspaces.timezone` and `workspaces.country` (nullable).
struct AddWorkspaceTimeZone;

impl Migration for AddWorkspaceTimeZone {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("workspaces", |t| {
                t.string("timezone").length(64).nullable();
                t.string("country").length(2).nullable();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .drop_columns("workspaces", ["country", "timezone"])
            .await
    }
}

/// Adds `workspaces.kind` (`live` or `demo`, default `live`) and its index.
struct AddWorkspaceKind;

impl Migration for AddWorkspaceKind {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("workspaces", |t| {
                t.string("kind").length(16).default("live");
                t.index("kind").name("workspaces_kind_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("workspaces", |t| {
                t.drop_index("workspaces_kind_index");
            })
            .await?;
        schema.drop_columns("workspaces", "kind").await
    }
}
