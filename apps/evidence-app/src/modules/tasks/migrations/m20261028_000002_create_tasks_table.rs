//! Creates `tasks`: a project's work items, owned by a workspace.

use navree::schema::prelude::*;

/// `m20261028_000002_create_tasks_table`.
pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("tasks", |t| {
                t.uuid_primary();
                t.uuid("workspace_id").index();
                t.foreign_uuid("project_id")
                    .index()
                    .constrained_on("projects", "id")
                    .cascade_on_delete();
                t.string("title");
                t.string("status").length(16).default("todo");
                t.integer("priority").default(3);
                t.date("due_on").nullable();
                t.uuid("assignee_id").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index(["workspace_id", "status"]);
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("tasks").await
    }
}
