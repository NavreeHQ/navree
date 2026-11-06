//! Creates the `customers` table and, on PostgreSQL, its row-level security
//! policy: a transaction sees only the rows of the workspace it is in
//! (`navree.workspace`, set for every workspace request).

use navree::schema::Dialect;
use navree::schema::prelude::*;
use navree::workspace::RlsPolicy;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("customers", |t| {
                t.uuid_primary();
                t.uuid("workspace_id");
                t.string("name");
                t.string("email");
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
                t.unique(["workspace_id", "email"])
                    .name("idx_customers_workspace_email");
                t.index("city");
                t.index("status");
            })
            .await?;
        // On PostgreSQL the database enforces the workspace scope as well.
        if schema.target().await?.dialect() == Dialect::Postgres {
            schema
                .statement(&RlsPolicy::new("customers").create_sql())
                .await?;
        }
        Ok(())
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("customers").await
    }
}
