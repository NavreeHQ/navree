//! The module's migrations (`MigrationSet` `settings`), on the schema
//! builder.
//!
//! The name is the one the migration always had, so a database migrated by
//! an earlier version finds nothing pending. The columns keep the types it
//! created: UUIDs as `BINARY(16)` on MySQL and MariaDB (where navree-db
//! writes UUIDs as 16 bytes), JSON as `jsonb` on PostgreSQL, timestamps at
//! the database's default precision.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the settings module's migration set.
pub const SETTINGS_MIGRATIONS: &str = "settings";

/// The settings module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SETTINGS_MIGRATIONS).migration(SchemaMigration::new(
        "m20261027_000001_create_settings_table",
        CreateSettingsTable,
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

/// Creates `settings`.
struct CreateSettingsTable;

impl Migration for CreateSettingsTable {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("settings", |t| {
                t.string("key").length(100);
                t.string("scope_type").length(16);
                uuid(t, "scope_id", dialect);
                t.jsonb("value");
                t.jsonb("overridden_fields");
                t.timestamp_tz("updated_at").default_precision();
                uuid(t, "updated_by", dialect).nullable();
                t.primary(["key", "scope_type", "scope_id"]);
                t.index(["scope_type", "scope_id"])
                    .name("settings_scope_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop("settings").await
    }
}
