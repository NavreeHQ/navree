//! The auth module's migrations (`MigrationSet` `auth`): `users`,
//! `refresh_tokens`, `password_reset_tokens`, the session columns of
//! `refresh_tokens`, `login_tokens` and `passkeys`, written with the schema
//! builder.
//!
//! Their names are the ones the module's migrations always had, so a
//! database migrated by an earlier version finds nothing pending. The
//! columns are the types those migrations created: UUIDs as `BINARY(16)` on
//! MySQL and MariaDB (where navree-db writes UUIDs as 16 bytes, and where
//! other modules' foreign keys to `users.id` expect that type), names and
//! emails as `VARCHAR` without a length on PostgreSQL and SQLite and
//! `VARCHAR(255)` on MySQL.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the auth module's migration set; feature modules whose
/// tables reference `users` run `.after([AUTH_MIGRATIONS])`.
pub const AUTH_MIGRATIONS: &str = "auth";

/// The auth module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(AUTH_MIGRATIONS)
        .migration(SchemaMigration::new(
            "m20261025_000001_create_users_table",
            Create("users", users),
        ))
        .migration(SchemaMigration::new(
            "m20261025_000002_create_refresh_tokens_table",
            Create("refresh_tokens", refresh_tokens),
        ))
        .migration(SchemaMigration::new(
            "m20261025_000003_create_password_reset_tokens_table",
            Create("password_reset_tokens", password_reset_tokens),
        ))
        .migration(SchemaMigration::new(
            "m20261027_000001_add_session_columns_to_refresh_tokens",
            AddSessionColumns,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000002_create_login_tokens_table",
            Create("login_tokens", login_tokens),
        ))
        .migration(SchemaMigration::new(
            "m20261027_000003_create_passkeys_table",
            Create("passkeys", passkeys),
        ))
}

/// Creates table `.0` with the columns `.1` adds (given the database's
/// dialect); `down` drops it.
struct Create(&'static str, fn(&mut Blueprint, Dialect));

impl Migration for Create {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        let columns = self.1;
        schema.create(self.0, |t| columns(t, dialect)).await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(self.0).await
    }
}

/// A UUID column: `BINARY(16)` on MySQL and MariaDB, `uuid` elsewhere.
pub(crate) fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// A text column of no fixed length: `VARCHAR(255)` on MySQL and MariaDB,
/// `VARCHAR` elsewhere.
pub(crate) fn varchar<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.string(name).length(255)
    } else {
        t.raw_column(name, "VARCHAR")
    }
}

/// `users`.
fn users(t: &mut Blueprint, dialect: Dialect) {
    uuid(t, "id", dialect).primary();
    varchar(t, "name", dialect);
    varchar(t, "email", dialect).unique();
    varchar(t, "password_hash", dialect);
    t.timestamp_tz("email_verified_at").nullable();
    t.text("two_factor_secret").nullable();
    t.text("two_factor_recovery_codes").nullable();
    t.timestamp_tz("two_factor_confirmed_at").nullable();
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at");
}

/// `refresh_tokens` (rows go with their user).
fn refresh_tokens(t: &mut Blueprint, dialect: Dialect) {
    uuid(t, "id", dialect).primary();
    uuid(t, "user_id", dialect).index_named("refresh_tokens_user_id_index");
    uuid(t, "family_id", dialect).index_named("refresh_tokens_family_id_index");
    t.string("token_hash").length(64).unique();
    t.timestamp_tz("expires_at");
    t.timestamp_tz("revoked_at").nullable();
    uuid(t, "replaced_by", dialect).nullable();
    t.string("ip").length(45).nullable();
    t.string("user_agent").length(512).nullable();
    t.timestamp_tz("created_at");
    t.foreign("user_id")
        .references("id")
        .on("users")
        .cascade_on_delete();
}

/// `password_reset_tokens`.
fn password_reset_tokens(t: &mut Blueprint, dialect: Dialect) {
    varchar(t, "email", dialect).primary();
    t.string("token_hash").length(64);
    t.timestamp_tz("created_at");
}

/// Adds the session columns to `refresh_tokens`: `authenticated_at`,
/// `impersonator_id` and `audience`.
struct AddSessionColumns;

impl Migration for AddSessionColumns {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .table("refresh_tokens", |t| {
                t.timestamp_tz("authenticated_at").nullable();
                uuid(t, "impersonator_id", dialect).nullable();
                t.string("audience").length(32).nullable();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .drop_columns(
                "refresh_tokens",
                ["audience", "impersonator_id", "authenticated_at"],
            )
            .await
    }
}

/// `login_tokens` (rows go with their user).
fn login_tokens(t: &mut Blueprint, dialect: Dialect) {
    uuid(t, "id", dialect).primary();
    uuid(t, "user_id", dialect);
    t.string("kind").length(16);
    t.string("token_hash").length(64);
    t.integer("attempts").default(0);
    t.timestamp_tz("expires_at");
    t.timestamp_tz("used_at").nullable();
    t.timestamp_tz("created_at");
    t.foreign("user_id")
        .references("id")
        .on("users")
        .cascade_on_delete();
    t.index(["user_id", "kind"])
        .name("login_tokens_user_id_kind_index");
}

/// `passkeys` (rows go with their user).
fn passkeys(t: &mut Blueprint, dialect: Dialect) {
    uuid(t, "id", dialect).primary();
    uuid(t, "user_id", dialect).index_named("passkeys_user_id_index");
    t.text("credential_id");
    t.string("credential_hash").length(64).unique();
    t.text("public_key");
    t.integer("algorithm");
    t.big_integer("sign_count");
    t.string("transports").length(255).nullable();
    varchar(t, "name", dialect);
    t.timestamp_tz("created_at");
    t.timestamp_tz("last_used_at").nullable();
    t.foreign("user_id")
        .references("id")
        .on("users")
        .cascade_on_delete();
}

#[cfg(test)]
mod tests {
    use navree_db::navree_schema::Target;

    use super::*;

    /// The SQL `up` of every migration writes for `target`, offline.
    async fn statements(target: Target) -> Vec<String> {
        let schema = Schema::offline(target);
        for entry in migrations().entries() {
            entry.migration().up(&schema).await.unwrap();
        }
        schema.statements()
    }

    #[tokio::test]
    async fn names_are_the_ones_databases_recorded() {
        let names: Vec<String> = migrations()
            .entries()
            .iter()
            .map(|entry| entry.name().to_owned())
            .collect();
        assert_eq!(
            names,
            [
                "m20261025_000001_create_users_table",
                "m20261025_000002_create_refresh_tokens_table",
                "m20261025_000003_create_password_reset_tokens_table",
                "m20261027_000001_add_session_columns_to_refresh_tokens",
                "m20261027_000002_create_login_tokens_table",
                "m20261027_000003_create_passkeys_table",
            ]
        );
    }

    #[tokio::test]
    async fn uuids_are_sixteen_bytes_on_mysql() {
        let sql = statements(Target::mysql((8, 4))).await.join("\n");
        assert!(sql.contains("`id` BINARY(16) NOT NULL"), "{sql}");
        assert!(sql.contains("`email` VARCHAR(255) NOT NULL"), "{sql}");
        let sql = statements(Target::postgres(16)).await.join("\n");
        assert!(sql.contains("\"id\" UUID NOT NULL"), "{sql}");
        assert!(sql.contains("\"email\" VARCHAR NOT NULL"), "{sql}");
    }
}
