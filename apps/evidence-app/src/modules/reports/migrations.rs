//! The `reports` migration set.

use navree::db::MigrationSet;
use navree::schema::migration;

/// The set, after the workspace tables.
pub fn migrations() -> MigrationSet {
    MigrationSet::new("reports")
        .after(["workspace"])
        .migration(migration!(m20261028_000003_create_reports_table))
}

/// Creates `reports`.
#[allow(non_camel_case_types)]
mod m20261028_000003_create_reports_table {
    use navree::schema::prelude::*;

    /// `m20261028_000003_create_reports_table`.
    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("reports", |t| {
                    t.uuid_primary();
                    t.uuid("workspace_id").index();
                    t.string("kind").length(32);
                    t.string("status").length(16).default("queued");
                    t.json("result").nullable();
                    t.integer("attempts").default(0);
                    t.timestamp_tz("finished_at").nullable();
                    t.timestamp_tz("created_at");
                    t.timestamp_tz("updated_at");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("reports").await
        }
    }
}
