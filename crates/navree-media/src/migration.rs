//! The module's migrations (`MigrationSet` `media`), written with the
//! schema builder.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the media module's migration set.
pub const MEDIA_MIGRATIONS: &str = "media";

/// The media table.
pub const MEDIA_TABLE: &str = "media";

/// The media module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(MEDIA_MIGRATIONS).migration(SchemaMigration::new(
        "m20261027_000001_create_media_table",
        CreateMediaTable,
    ))
}

/// Creates `media`, indexed by owner and collection. Owners are not
/// foreign keys: any model can have media.
///
/// The id is a native `UUID`, except on MySQL and MariaDB, where the
/// model's `Uuid` field is written as 16 bytes: `BINARY(16)` there.
struct CreateMediaTable;

/// A UUID column `name`: `BINARY(16)` on MySQL, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, mysql: bool) -> ColumnBuilder<'a> {
    if mysql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

impl Migration for CreateMediaTable {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(MEDIA_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                t.string("model_type").length(100);
                t.string("model_id").length(64);
                t.string("collection").length(100);
                t.string("name");
                t.string("file_name");
                t.string("mime_type").length(127);
                t.string("disk").length(64);
                t.string("conversions_disk").length(64);
                t.string("path").length(1024);
                t.big_integer("size");
                t.integer("width").nullable();
                t.integer("height").nullable();
                t.jsonb("conversions");
                t.jsonb("metadata");
                t.jsonb("custom_properties");
                t.integer("order_column").default(0);
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index(["model_type", "model_id", "collection"])
                    .name("media_model_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(MEDIA_TABLE).await
    }
}
