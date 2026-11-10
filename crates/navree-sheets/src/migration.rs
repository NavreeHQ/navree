//! The sheets module's migrations (`MigrationSet` `sheets`), written with
//! the schema builder. The name is the one the module's migration always
//! had, so a database migrated by an earlier version finds nothing
//! pending.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Migration, Schema, SchemaMigration, SchemaResult,
};

use crate::import::store::{IMPORT_MAPPINGS_TABLE, IMPORT_ROWS_TABLE, IMPORTS_TABLE};

/// The name of the sheets module's migration set.
pub const SHEETS_MIGRATIONS: &str = "sheets";

/// The sheets module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SHEETS_MIGRATIONS).migration(SchemaMigration::new(
        "m20261027_000001_create_import_tables",
        CreateImportTables,
    ))
}

/// Creates `imports`, `import_rows` and `import_mappings`. Ids are text,
/// times are Unix milliseconds and JSON is text, the same on every
/// database.
struct CreateImportTables;

/// An id: text of 36 characters.
fn id<'a>(t: &'a mut Blueprint, name: &str) -> ColumnBuilder<'a> {
    t.string(name).length(36)
}

/// A time in Unix milliseconds.
fn millis<'a>(t: &'a mut Blueprint, name: &str) -> ColumnBuilder<'a> {
    t.big_integer(name)
}

impl Migration for CreateImportTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(IMPORTS_TABLE, |t| {
                id(t, "id").primary();
                t.string("importer").length(100);
                t.string("status").length(20);
                t.text("filename");
                t.string("disk").length(100);
                t.text("path");
                t.string("format").length(10).nullable();
                t.string("encoding").length(50).nullable();
                t.text("sheet").nullable();
                t.string("locale").length(35).nullable();
                t.text("headers");
                t.text("mapping");
                t.boolean("mapping_chosen");
                t.boolean("save_mapping");
                t.text("missing");
                t.string("duplicates").length(10).nullable();
                t.text("counts");
                id(t, "operation_id").nullable();
                t.text("error_path").nullable();
                t.text("error").nullable();
                id(t, "owner_id").nullable();
                id(t, "workspace_id").nullable();
                millis(t, "created_at");
                millis(t, "updated_at");
                t.big_integer("finished_at").nullable();
                t.index(["owner_id", "created_at"])
                    .name("imports_owner_created_index");
                t.index(["updated_at"]).name("imports_updated_at_index");
            })
            .await?;
        schema
            .create(IMPORT_ROWS_TABLE, |t| {
                id(t, "import_id");
                t.big_integer("row_no");
                t.string("status").length(20);
                t.text("row_key").nullable();
                t.text("cells");
                t.text("errors");
                t.primary(["import_id", "row_no"]);
                t.index(["import_id", "status", "row_no"])
                    .name("import_rows_status_index");
            })
            .await?;
        schema
            .create(IMPORT_MAPPINGS_TABLE, |t| {
                id(t, "id").primary();
                t.string("importer").length(100);
                id(t, "workspace_id").nullable();
                t.string("fingerprint").length(64);
                t.text("mapping");
                millis(t, "created_at");
                millis(t, "updated_at");
                t.index(["importer", "fingerprint"])
                    .name("import_mappings_lookup_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in [IMPORT_MAPPINGS_TABLE, IMPORT_ROWS_TABLE, IMPORTS_TABLE] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}
