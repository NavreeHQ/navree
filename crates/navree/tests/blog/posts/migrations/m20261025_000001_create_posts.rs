//! Creates the `posts` table.

use navree::schema::prelude::*;

/// `m20261025_000001_create_posts`.
pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("posts", |t| {
                t.uuid_primary();
                t.uuid("author_id")
                    .constrained_on("users", "id")
                    .cascade_on_delete();
                t.string("title");
                t.text("body");
                t.boolean("published").default(false);
                t.timestamp_tz("published_at").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("posts").await
    }
}
