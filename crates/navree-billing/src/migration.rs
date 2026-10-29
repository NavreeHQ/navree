//! The billing migrations (`MigrationSet` `billing`, after `auth` and
//! `workspace`), on the schema builder.
//!
//! The name is the one the migration always had, so a database migrated by
//! an earlier version finds nothing pending. The columns keep the types it
//! created: UUIDs as `BINARY(16)` on MySQL and MariaDB (where navree-db
//! writes UUIDs as 16 bytes), text as `VARCHAR` without a length on
//! PostgreSQL and SQLite and `VARCHAR(255)` on MySQL, timestamps at the
//! database's default precision.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the billing migration set; tables that reference the billing
/// tables run `.after([BILLING_MIGRATIONS])`.
pub const BILLING_MIGRATIONS: &str = "billing";

/// The billing migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(BILLING_MIGRATIONS)
        .after(["auth", "workspace"])
        .migration(SchemaMigration::new(
            "m20261026_000001_create_billing_tables",
            CreateBillingTables,
        ))
}

/// A UUID column: `BINARY(16)` on MySQL and MariaDB, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// A text column of no fixed length: `VARCHAR(255)` on MySQL and MariaDB,
/// `VARCHAR` elsewhere.
fn varchar<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.string(name).length(255)
    } else {
        t.raw_column(name, "VARCHAR")
    }
}

/// A timestamp at the database's default precision.
fn timestamp<'a>(t: &'a mut Blueprint, name: &str) -> ColumnBuilder<'a> {
    t.timestamp_tz(name).default_precision()
}

/// Creates `billing_customers`, `subscriptions`, `subscription_items`,
/// `billing_usage` and `billing_events`.
struct CreateBillingTables;

impl Migration for CreateBillingTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("billing_customers", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "billable_type", dialect);
                uuid(t, "billable_id", dialect);
                varchar(t, "provider", dialect).nullable();
                varchar(t, "provider_customer_id", dialect)
                    .nullable()
                    .index_named("billing_customers_provider_customer_id_index");
                varchar(t, "email", dialect).nullable();
                timestamp(t, "trial_ends_at").nullable();
                timestamp(t, "created_at");
                timestamp(t, "updated_at");
                t.unique(["billable_type", "billable_id"])
                    .name("billing_customers_billable_unique");
            })
            .await?;
        schema
            .create("subscriptions", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "billable_type", dialect);
                uuid(t, "billable_id", dialect);
                varchar(t, "provider", dialect);
                varchar(t, "provider_id", dialect).unique();
                varchar(t, "provider_customer_id", dialect);
                varchar(t, "plan", dialect);
                varchar(t, "interval", dialect);
                varchar(t, "status", dialect);
                t.big_integer("quantity");
                timestamp(t, "trial_ends_at").nullable();
                timestamp(t, "ends_at").nullable();
                timestamp(t, "current_period_start").nullable();
                timestamp(t, "current_period_end").nullable();
                timestamp(t, "past_due_since").nullable();
                timestamp(t, "provider_updated_at").nullable();
                timestamp(t, "created_at");
                timestamp(t, "updated_at");
                t.index(["billable_type", "billable_id"])
                    .name("subscriptions_billable_index");
            })
            .await?;
        schema
            .create("subscription_items", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "subscription_id", dialect)
                    .index_named("subscription_items_subscription_id_index");
                varchar(t, "provider_item_id", dialect).nullable();
                varchar(t, "price_id", dialect);
                varchar(t, "meter", dialect).nullable();
                t.big_integer("quantity").nullable();
                timestamp(t, "created_at");
                t.foreign("subscription_id")
                    .references("id")
                    .on("subscriptions")
                    .cascade_on_delete();
            })
            .await?;
        schema
            .create("billing_usage", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "billable_type", dialect);
                uuid(t, "billable_id", dialect);
                varchar(t, "meter", dialect);
                t.big_integer("quantity");
                timestamp(t, "recorded_at");
                timestamp(t, "reported_at")
                    .nullable()
                    .index_named("billing_usage_reported_at_index");
                t.index(["billable_type", "billable_id", "meter"])
                    .name("billing_usage_billable_meter_index");
            })
            .await?;
        schema
            .create("billing_events", |t| {
                uuid(t, "id", dialect).primary();
                varchar(t, "provider", dialect);
                varchar(t, "provider_event_id", dialect);
                varchar(t, "event_type", dialect);
                t.text("payload");
                timestamp(t, "received_at");
                timestamp(t, "processed_at").nullable();
                t.integer("attempts").default(0);
                t.text("error").nullable();
                t.unique(["provider", "provider_event_id"])
                    .name("billing_events_provider_event_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in [
            "billing_events",
            "billing_usage",
            "subscription_items",
            "subscriptions",
            "billing_customers",
        ] {
            schema.drop(table).await?;
        }
        Ok(())
    }
}
