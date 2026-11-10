//! Creates the `invoices` table.

use navree::schema::prelude::*;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("invoices", |t| {
                t.uuid_primary();
                // One invoice per deal: a repeated `deal.won` finds it.
                t.uuid("deal_id").unique();
                t.big_integer("amount_cents");
                t.string("currency");
                t.string("status");
                t.timestamps_tz();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("invoices").await
    }
}
