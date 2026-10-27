//! The sheets module's migrations (`MigrationSet` `sheets`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

use crate::import::store::{IMPORT_MAPPINGS_TABLE, IMPORT_ROWS_TABLE, IMPORTS_TABLE};

/// The name of the sheets module's migration set.
pub const SHEETS_MIGRATIONS: &str = "sheets";

/// The sheets module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SHEETS_MIGRATIONS).migration(CreateImportTables)
}

/// Creates `imports`, `import_rows` and `import_mappings`. Ids are text,
/// times are Unix milliseconds and JSON is text, the same on every
/// database.
struct CreateImportTables;

impl MigrationName for CreateImportTables {
    fn name(&self) -> &str {
        "m20261027_000001_create_import_tables"
    }
}

fn id(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).string_len(36).not_null().to_owned()
}

fn nullable_id(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).string_len(36).null().to_owned()
}

fn millis(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).big_integer().not_null().to_owned()
}

#[async_trait]
impl MigrationTrait for CreateImportTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(IMPORTS_TABLE)
                    .col(id("id").primary_key())
                    .col(ColumnDef::new("importer").string_len(100).not_null())
                    .col(ColumnDef::new("status").string_len(20).not_null())
                    .col(ColumnDef::new("filename").text().not_null())
                    .col(ColumnDef::new("disk").string_len(100).not_null())
                    .col(ColumnDef::new("path").text().not_null())
                    .col(ColumnDef::new("format").string_len(10).null())
                    .col(ColumnDef::new("encoding").string_len(50).null())
                    .col(ColumnDef::new("sheet").text().null())
                    .col(ColumnDef::new("locale").string_len(35).null())
                    .col(ColumnDef::new("headers").text().not_null())
                    .col(ColumnDef::new("mapping").text().not_null())
                    .col(ColumnDef::new("mapping_chosen").boolean().not_null())
                    .col(ColumnDef::new("save_mapping").boolean().not_null())
                    .col(ColumnDef::new("missing").text().not_null())
                    .col(ColumnDef::new("duplicates").string_len(10).null())
                    .col(ColumnDef::new("counts").text().not_null())
                    .col(nullable_id("operation_id"))
                    .col(ColumnDef::new("error_path").text().null())
                    .col(ColumnDef::new("error").text().null())
                    .col(nullable_id("owner_id"))
                    .col(nullable_id("workspace_id"))
                    .col(millis("created_at"))
                    .col(millis("updated_at"))
                    .col(ColumnDef::new("finished_at").big_integer().null())
                    .to_owned(),
            )
            .await?;
        for (name, columns) in [
            (
                "imports_owner_created_index",
                &["owner_id", "created_at"][..],
            ),
            ("imports_updated_at_index", &["updated_at"][..]),
        ] {
            let mut index = Index::create();
            index.name(name).table(IMPORTS_TABLE);
            for column in columns {
                index.col(*column);
            }
            manager.create_index(index.to_owned()).await?;
        }
        manager
            .create_table(
                Table::create()
                    .table(IMPORT_ROWS_TABLE)
                    .col(id("import_id"))
                    .col(ColumnDef::new("row_no").big_integer().not_null())
                    .col(ColumnDef::new("status").string_len(20).not_null())
                    .col(ColumnDef::new("row_key").text().null())
                    .col(ColumnDef::new("cells").text().not_null())
                    .col(ColumnDef::new("errors").text().not_null())
                    .primary_key(Index::create().col("import_id").col("row_no"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("import_rows_status_index")
                    .table(IMPORT_ROWS_TABLE)
                    .col("import_id")
                    .col("status")
                    .col("row_no")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(IMPORT_MAPPINGS_TABLE)
                    .col(id("id").primary_key())
                    .col(ColumnDef::new("importer").string_len(100).not_null())
                    .col(nullable_id("workspace_id"))
                    .col(ColumnDef::new("fingerprint").string_len(64).not_null())
                    .col(ColumnDef::new("mapping").text().not_null())
                    .col(millis("created_at"))
                    .col(millis("updated_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("import_mappings_lookup_index")
                    .table(IMPORT_MAPPINGS_TABLE)
                    .col("importer")
                    .col("fingerprint")
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [IMPORT_MAPPINGS_TABLE, IMPORT_ROWS_TABLE, IMPORTS_TABLE] {
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }
        Ok(())
    }
}
