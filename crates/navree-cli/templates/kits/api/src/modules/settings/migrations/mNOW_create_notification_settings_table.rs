//! Creates the `notification_settings` table (one row per member and
//! workspace) and, on PostgreSQL, its row-level security policy.

use navree::schema::Dialect;
use navree::schema::prelude::*;
use navree::workspace::RlsPolicy;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("notification_settings", |t| {
                t.uuid_primary();
                t.uuid("workspace_id");
                t.uuid("user_id");
                t.boolean("project_activity");
                t.boolean("weekly_summary");
                t.boolean("product_updates");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.unique(["workspace_id", "user_id"])
                    .name("idx_notification_settings_workspace_user");
            })
            .await?;
        // On PostgreSQL the database enforces the workspace scope as well.
        if schema.target().await?.dialect() == Dialect::Postgres {
            schema
                .statement(&RlsPolicy::new("notification_settings").create_sql())
                .await?;
        }
        Ok(())
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("notification_settings").await
    }
}
