//! Creates the `customers` table.

use navree::schema::prelude::*;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("customers", |t| {
                t.uuid_primary();
                t.string("name");
                t.string("email").unique();
                t.string("phone");
                t.string("city");
                t.string("plan");
                t.string("status");
                t.big_integer("monthly_paise");
                t.date("since");
                t.boolean("autopay");
                t.text("notes").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index("city");
                t.index("status");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("customers").await
    }
}
