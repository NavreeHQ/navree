//! The social module's migrations (`MigrationSet` `social`, after `auth`),
//! written with the schema builder.
//!
//! The migration keeps the name it always had, so a database migrated by an
//! earlier version finds nothing pending. The columns are the types it
//! created: UUIDs as `BINARY(16)` on MySQL and MariaDB (where navree-db
//! writes UUIDs as 16 bytes, and where `users.id` is that type), names and
//! identifiers as `VARCHAR` without a length on PostgreSQL and SQLite and
//! `VARCHAR(255)` on MySQL.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the social module's migration set.
pub const SOCIAL_MIGRATIONS: &str = "social";

/// The social module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SOCIAL_MIGRATIONS)
        .after(["auth"])
        .migration(SchemaMigration::new(
            "m20261026_000001_create_social_accounts_table",
            CreateSocialAccounts,
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

/// Creates `social_accounts`, unique per (provider, provider user id) and
/// per (user, provider); rows go with their user.
struct CreateSocialAccounts;

impl Migration for CreateSocialAccounts {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("social_accounts", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "user_id", dialect);
                varchar(t, "provider", dialect);
                varchar(t, "provider_user_id", dialect);
                varchar(t, "email", dialect).nullable();
                varchar(t, "name", dialect).nullable();
                t.text("avatar_url").nullable();
                varchar(t, "nickname", dialect).nullable();
                t.text("access_token").nullable();
                t.text("refresh_token").nullable();
                t.timestamp_tz("token_expires_at").nullable();
                t.text("scopes").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign("user_id")
                    .references("id")
                    .on("users")
                    .cascade_on_delete();
                t.unique(["provider", "provider_user_id"])
                    .name("social_accounts_provider_user_unique");
                t.unique(["user_id", "provider"])
                    .name("social_accounts_user_provider_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("social_accounts").await
    }
}
