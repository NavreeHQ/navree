//! The OAuth server's migrations (`MigrationSet` `oauth`, after `auth`),
//! written with the schema builder.
//!
//! The migration keeps the name it always had, so a database migrated by an
//! earlier version finds nothing pending. The columns are the types it
//! created: UUIDs as `BINARY(16)` on MySQL and MariaDB (where navree-db
//! writes UUIDs as 16 bytes), names, grants and digests as `VARCHAR`
//! without a length on PostgreSQL and SQLite and `VARCHAR(255)` on MySQL.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the OAuth server's migration set.
pub const OAUTH_MIGRATIONS: &str = "oauth";

/// The OAuth server's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(OAUTH_MIGRATIONS)
        .after(["auth"])
        .migration(SchemaMigration::new(
            "m20261026_000001_create_oauth_tables",
            CreateOAuthTables,
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

/// The tables, in the order they are created.
const TABLES: [&str; 8] = [
    "oauth_clients",
    "oauth_authorizations",
    "oauth_authorization_requests",
    "oauth_codes",
    "oauth_access_tokens",
    "oauth_refresh_tokens",
    "oauth_device_codes",
    "personal_access_tokens",
];

/// `client_id` referencing `oauth_clients(id)`, cascading deletes.
fn client_fk(t: &mut Blueprint) {
    t.foreign("client_id")
        .references("id")
        .on("oauth_clients")
        .cascade_on_delete();
}

/// Creates every OAuth table.
struct CreateOAuthTables;

impl Migration for CreateOAuthTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("oauth_clients", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "name", dialect);
                varchar(t, "secret_hash", dialect).nullable();
                t.text("redirect_uris");
                t.text("scopes");
                varchar(t, "grants", dialect);
                uuid(t, "owner_id", dialect).nullable();
                t.boolean("trusted").default(false);
                t.boolean("introspect").default(false);
                uuid(t, "workspace_id", dialect).nullable();
                t.text("logo_url").nullable();
                t.text("website").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.timestamp_tz("revoked_at").nullable();
            })
            .await?;
        schema
            .create("oauth_authorizations", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "user_id", dialect);
                uuid(t, "client_id", dialect);
                t.text("scopes");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                client_fk(t);
                t.unique(["user_id", "client_id"])
                    .name("oauth_authorizations_user_client_unique");
            })
            .await?;
        schema
            .create("oauth_authorization_requests", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "client_id", dialect);
                t.text("redirect_uri");
                t.text("scopes");
                t.text("state").nullable();
                varchar(t, "code_challenge", dialect);
                t.text("nonce").nullable();
                uuid(t, "user_id", dialect).nullable();
                varchar(t, "status", dialect);
                t.timestamp_tz("expires_at");
                t.timestamp_tz("created_at");
                client_fk(t);
            })
            .await?;
        schema
            .create("oauth_codes", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "code_hash", dialect).unique();
                uuid(t, "client_id", dialect);
                uuid(t, "user_id", dialect);
                t.text("redirect_uri");
                t.text("scopes");
                varchar(t, "code_challenge", dialect);
                t.text("nonce").nullable();
                t.timestamp_tz("expires_at");
                t.timestamp_tz("used_at").nullable();
                t.timestamp_tz("created_at");
                client_fk(t);
            })
            .await?;
        schema
            .create("oauth_access_tokens", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "token_hash", dialect).unique();
                uuid(t, "client_id", dialect);
                uuid(t, "user_id", dialect);
                t.text("scopes");
                varchar(t, "grant", dialect);
                uuid(t, "code_id", dialect).nullable();
                uuid(t, "family_id", dialect).nullable();
                uuid(t, "workspace_id", dialect).nullable();
                t.timestamp_tz("expires_at");
                t.timestamp_tz("revoked_at").nullable();
                t.timestamp_tz("created_at");
                client_fk(t);
                t.index(["user_id", "client_id"])
                    .name("oauth_access_tokens_user_client_index");
                t.index("family_id")
                    .name("oauth_access_tokens_family_index");
                t.index("code_id").name("oauth_access_tokens_code_index");
            })
            .await?;
        schema
            .create("oauth_refresh_tokens", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "token_hash", dialect).unique();
                uuid(t, "client_id", dialect);
                uuid(t, "user_id", dialect);
                t.text("scopes");
                uuid(t, "family_id", dialect);
                uuid(t, "code_id", dialect).nullable();
                uuid(t, "access_token_id", dialect).nullable();
                uuid(t, "workspace_id", dialect).nullable();
                t.timestamp_tz("expires_at");
                t.timestamp_tz("used_at").nullable();
                t.timestamp_tz("revoked_at").nullable();
                t.timestamp_tz("created_at");
                client_fk(t);
                t.index(["user_id", "client_id"])
                    .name("oauth_refresh_tokens_user_client_index");
                t.index("family_id")
                    .name("oauth_refresh_tokens_family_index");
            })
            .await?;
        schema
            .create("oauth_device_codes", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "device_code_hash", dialect).unique();
                varchar(t, "user_code", dialect).unique();
                uuid(t, "client_id", dialect);
                t.text("scopes");
                uuid(t, "user_id", dialect).nullable();
                varchar(t, "status", dialect);
                t.big_integer("interval_secs");
                t.timestamp_tz("last_polled_at").nullable();
                t.timestamp_tz("expires_at");
                t.timestamp_tz("used_at").nullable();
                t.timestamp_tz("created_at");
                client_fk(t);
            })
            .await?;
        schema
            .create("personal_access_tokens", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "user_id", dialect);
                varchar(t, "name", dialect);
                varchar(t, "token_hash", dialect).unique();
                t.text("scopes");
                t.timestamp_tz("expires_at").nullable();
                t.timestamp_tz("last_used_at").nullable();
                t.timestamp_tz("revoked_at").nullable();
                t.timestamp_tz("created_at");
                t.index("user_id").name("personal_access_tokens_user_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in TABLES.into_iter().rev() {
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
        assert_eq!(names, ["m20261026_000001_create_oauth_tables"]);
    }

    #[tokio::test]
    async fn uuids_are_sixteen_bytes_on_mysql() {
        let sql = statements(Target::mysql((8, 4))).await.join("\n");
        assert!(sql.contains("`id` BINARY(16) NOT NULL"), "{sql}");
        assert!(sql.contains("`token_hash` VARCHAR(255) NOT NULL"), "{sql}");
        let sql = statements(Target::postgres(16)).await.join("\n");
        assert!(sql.contains("\"id\" UUID NOT NULL"), "{sql}");
        assert!(sql.contains("\"token_hash\" VARCHAR NOT NULL"), "{sql}");
    }

    #[tokio::test]
    async fn every_table_is_created_and_dropped() {
        let sql = statements(Target::postgres(16)).await.join("\n");
        for table in TABLES {
            assert!(
                sql.contains(&format!("CREATE TABLE \"{table}\"")),
                "{table}"
            );
        }
    }
}
