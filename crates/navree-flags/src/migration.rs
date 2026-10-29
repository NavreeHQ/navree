//! The module's migrations (`MigrationSet` `flags`), on the schema builder.
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

/// The name of the flags module's migration set.
pub const FLAGS_MIGRATIONS: &str = "flags";

/// The flags module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(FLAGS_MIGRATIONS).migration(SchemaMigration::new(
        "m20261026_000001_create_feature_flag_tables",
        CreateFlagTables,
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

/// Creates `feature_flags` and `feature_flag_overrides`.
struct CreateFlagTables;

impl Migration for CreateFlagTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("feature_flags", |t| {
                t.string("name").length(100).primary();
                t.boolean("everyone").nullable();
                t.jsonb("rollout").nullable();
                t.boolean("killed").default(false);
                t.timestamp_tz("updated_at").default_precision();
                uuid(t, "updated_by", dialect).nullable();
            })
            .await?;
        schema
            .create("feature_flag_overrides", |t| {
                t.string("flag").length(100);
                t.string("scope_type").length(16);
                uuid(t, "scope_id", dialect);
                t.boolean("active");
                t.primary(["flag", "scope_type", "scope_id"]);
                t.index(["scope_type", "scope_id"])
                    .name("feature_flag_overrides_scope_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop("feature_flag_overrides").await?;
        schema.drop("feature_flags").await
    }
}
