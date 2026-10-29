//! The module's migrations (`MigrationSet` `sso`, after `auth`), written
//! with the schema builder.
//!
//! The migration keeps the name it always had, so a database migrated by an
//! earlier version finds nothing pending. The columns are the types it
//! created: UUIDs as `BINARY(16)` on MySQL and MariaDB (where navree-db
//! writes UUIDs as 16 bytes, and where `users.id` has that type), names,
//! slugs and domains as `VARCHAR` without a length on PostgreSQL and SQLite
//! and `VARCHAR(255)` on MySQL.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the module's migration set.
pub const SSO_MIGRATIONS: &str = "sso";

/// The module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SSO_MIGRATIONS)
        .after(["auth"])
        .migration(SchemaMigration::new(
            "m20261027_000001_create_sso_tables",
            CreateSsoTables,
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

/// Creates `sso_connections`, `sso_domains` and `sso_identities`.
struct CreateSsoTables;

impl Migration for CreateSsoTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("sso_connections", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "owner_type", dialect);
                uuid(t, "owner_id", dialect).nullable();
                varchar(t, "slug", dialect);
                varchar(t, "name", dialect);
                varchar(t, "protocol", dialect);
                t.boolean("enabled");
                t.boolean("enforce").nullable();
                t.text("settings");
                t.text("provisioning");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.unique("slug").name("sso_connections_slug_unique");
                t.index(["owner_type", "owner_id"])
                    .name("sso_connections_owner_index");
            })
            .await?;
        schema
            .create("sso_domains", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "connection_id", dialect);
                varchar(t, "domain", dialect);
                varchar(t, "token", dialect);
                t.timestamp_tz("verified_at").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign("connection_id")
                    .references("id")
                    .on("sso_connections")
                    .cascade_on_delete();
                t.unique(["connection_id", "domain"])
                    .name("sso_domains_connection_domain_unique");
                t.index("domain").name("sso_domains_domain_index");
            })
            .await?;
        schema
            .create("sso_identities", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "connection_id", dialect);
                uuid(t, "user_id", dialect);
                varchar(t, "subject", dialect);
                varchar(t, "email", dialect).nullable();
                t.text("attributes");
                t.timestamp_tz("last_login_at");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign("connection_id")
                    .references("id")
                    .on("sso_connections")
                    .cascade_on_delete();
                t.foreign("user_id")
                    .references("id")
                    .on("users")
                    .cascade_on_delete();
                t.unique(["connection_id", "subject"])
                    .name("sso_identities_connection_subject_unique");
                t.index("user_id").name("sso_identities_user_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in ["sso_identities", "sso_domains", "sso_connections"] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use navree_db::navree_schema::Target;

    use super::*;

    /// The SQL `up` writes for `target`, offline.
    async fn statements(target: Target) -> Vec<String> {
        let schema = Schema::offline(target);
        for entry in migrations().entries() {
            entry.migration().up(&schema).await.unwrap();
        }
        schema.statements()
    }

    #[tokio::test]
    async fn the_name_is_the_one_databases_recorded() {
        let names: Vec<String> = migrations()
            .entries()
            .iter()
            .map(|entry| entry.name().to_owned())
            .collect();
        assert_eq!(names, ["m20261027_000001_create_sso_tables"]);
    }

    #[tokio::test]
    async fn uuids_are_sixteen_bytes_on_mysql() {
        let sql = statements(Target::mysql((8, 4))).await.join("\n");
        assert!(sql.contains("`id` BINARY(16) NOT NULL"), "{sql}");
        assert!(sql.contains("`slug` VARCHAR(255) NOT NULL"), "{sql}");
        let sql = statements(Target::postgres(16)).await.join("\n");
        assert!(sql.contains("\"id\" UUID NOT NULL"), "{sql}");
        assert!(sql.contains("\"slug\" VARCHAR NOT NULL"), "{sql}");
    }
}
