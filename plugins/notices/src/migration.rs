//! Schema migrations for the `notices` plugin (`notices_notices`).

use navree::db::MigrationSet;
use navree::schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// Name of the migration set registered by `NoticesModule`.
pub const NOTICES_MIGRATIONS: &str = "notices";

/// Returns the migration set that creates `notices_notices`.
pub fn migrations() -> MigrationSet {
    MigrationSet::new(NOTICES_MIGRATIONS).migration(SchemaMigration::new(
        "m20261115_000001_create_notices_table",
        CreateNoticesTable,
    ))
}

/// Portable UUID column: `BINARY(16)` on MySQL and MariaDB, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// Creates the `notices_notices` table.
struct CreateNoticesTable;

impl Migration for CreateNoticesTable {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("notices_notices", |t| {
                t.big_integer("id").auto_increment();
                t.string("title").length(120);
                t.text("body");
                uuid(t, "posted_by", dialect);
                t.timestamp_tz("expires_at").default_precision();
                t.timestamp_tz("created_at").default_precision();
                t.index(["expires_at"])
                    .name("notices_notices_expires_at_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop("notices_notices").await
    }
}
