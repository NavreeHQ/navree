//! Creates the `user_profiles` table.

use navree::schema::prelude::*;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("user_profiles", |t| {
                t.uuid("user_id").primary();
                t.text("bio").nullable();
                t.string("timezone");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("user_profiles").await
    }
}
