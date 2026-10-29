//! Creates the `projects` table and, on PostgreSQL, its row-level security
//! policy: a transaction sees only the rows of the workspace it is in
//! (`navree.workspace`, set for every workspace request).

use navree::schema::Dialect;
use navree::schema::prelude::*;
use navree::workspace::RlsPolicy;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("projects", |t| {
                t.uuid_primary();
                t.uuid("workspace_id");
                t.string("name");
                t.text("description").nullable();
                t.string("status");
                t.uuid("created_by");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index("workspace_id").name("idx_projects_workspace_id");
            })
            .await?;
        // On PostgreSQL the database enforces the workspace scope as well.
        if schema.target().await?.dialect() == Dialect::Postgres {
            schema
                .statement(&RlsPolicy::new("projects").create_sql())
                .await?;
        }
        Ok(())
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("projects").await
    }
}
