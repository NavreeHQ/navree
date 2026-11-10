//! The access module's migrations (`MigrationSet` `access`, after `auth`),
//! on the schema builder.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

use crate::entity::VERSION_KEY;

/// The access module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("access")
        .after(["auth"])
        .migration(SchemaMigration::new(
            "m20261025_000001_create_access_tables",
            CreateAccessTables,
        ))
}

/// Creates `roles`, `permissions`, `role_permissions`, `user_roles`,
/// `user_permissions` and `access_meta` (with the version row at 1).
///
/// The columns keep the types the first version of this migration gave
/// them, so databases created before and after it read alike: UUIDs are
/// `BINARY(16)` on MySQL (the bytes navree-db binds there), timestamps keep
/// the database's default precision.
struct CreateAccessTables;

/// A UUID column: `BINARY(16)` on MySQL, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, mysql: bool) -> ColumnBuilder<'a> {
    if mysql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

impl Migration for CreateAccessTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("roles", |t| {
                uuid(t, "id", mysql).primary();
                t.string("name").unique();
                t.string("description").nullable();
                t.timestamp_tz("created_at").default_precision();
            })
            .await?;
        schema
            .create("permissions", |t| {
                uuid(t, "id", mysql).primary();
                t.string("name").unique();
            })
            .await?;
        schema
            .create("role_permissions", |t| {
                uuid(t, "role_id", mysql);
                uuid(t, "permission_id", mysql);
                t.primary(["role_id", "permission_id"]);
                t.foreign(["role_id"])
                    .references(["id"])
                    .on("roles")
                    .cascade_on_delete();
                t.foreign(["permission_id"])
                    .references(["id"])
                    .on("permissions")
                    .cascade_on_delete();
            })
            .await?;
        schema
            .create("user_roles", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "user_id", mysql);
                uuid(t, "role_id", mysql);
                uuid(t, "team_id", mysql).nullable();
                t.foreign(["role_id"])
                    .references(["id"])
                    .on("roles")
                    .cascade_on_delete();
                t.unique(["user_id", "role_id", "team_id"])
                    .name("user_roles_user_role_team_unique");
            })
            .await?;
        schema
            .create("user_permissions", |t| {
                uuid(t, "user_id", mysql);
                uuid(t, "permission_id", mysql);
                t.primary(["user_id", "permission_id"]);
                t.foreign(["permission_id"])
                    .references(["id"])
                    .on("permissions")
                    .cascade_on_delete();
            })
            .await?;
        schema
            .create("access_meta", |t| {
                t.string("key").primary();
                t.big_integer("version");
            })
            .await?;
        let quote = if mysql { '`' } else { '"' };
        schema
            .statement(&format!(
                "INSERT INTO {quote}access_meta{quote} ({quote}key{quote}, {quote}version{quote}) \
                 VALUES ('{VERSION_KEY}', 1)"
            ))
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in [
            "access_meta",
            "user_permissions",
            "user_roles",
            "role_permissions",
            "permissions",
            "roles",
        ] {
            schema.drop(table).await?;
        }
        Ok(())
    }
}
