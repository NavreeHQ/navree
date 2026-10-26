//! The billing migrations (`MigrationSet` `billing`, after `auth` and
//! `workspace`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the billing migration set; tables that reference the billing
/// tables run `.after([BILLING_MIGRATIONS])`.
pub const BILLING_MIGRATIONS: &str = "billing";

/// The billing migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(BILLING_MIGRATIONS)
        .after(["auth", "workspace"])
        .migration(CreateBillingTables)
}

/// Creates `billing_customers`, `subscriptions`, `subscription_items`,
/// `billing_usage` and `billing_events`.
struct CreateBillingTables;

impl MigrationName for CreateBillingTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_billing_tables"
    }
}

fn timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

fn nullable_timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .null()
        .to_owned()
}

#[async_trait]
impl MigrationTrait for CreateBillingTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("billing_customers")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("billable_type").string().not_null())
                    .col(ColumnDef::new("billable_id").uuid().not_null())
                    .col(ColumnDef::new("provider").string().null())
                    .col(ColumnDef::new("provider_customer_id").string().null())
                    .col(ColumnDef::new("email").string().null())
                    .col(nullable_timestamp("trial_ends_at"))
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("billing_customers_billable_unique")
                    .table("billing_customers")
                    .col("billable_type")
                    .col("billable_id")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("billing_customers_provider_customer_id_index")
                    .table("billing_customers")
                    .col("provider_customer_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("subscriptions")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("billable_type").string().not_null())
                    .col(ColumnDef::new("billable_id").uuid().not_null())
                    .col(ColumnDef::new("provider").string().not_null())
                    .col(
                        ColumnDef::new("provider_id")
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new("provider_customer_id").string().not_null())
                    .col(ColumnDef::new("plan").string().not_null())
                    .col(ColumnDef::new("interval").string().not_null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("quantity").big_integer().not_null())
                    .col(nullable_timestamp("trial_ends_at"))
                    .col(nullable_timestamp("ends_at"))
                    .col(nullable_timestamp("current_period_start"))
                    .col(nullable_timestamp("current_period_end"))
                    .col(nullable_timestamp("past_due_since"))
                    .col(nullable_timestamp("provider_updated_at"))
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("subscriptions_billable_index")
                    .table("subscriptions")
                    .col("billable_type")
                    .col("billable_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("subscription_items")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("subscription_id").uuid().not_null())
                    .col(ColumnDef::new("provider_item_id").string().null())
                    .col(ColumnDef::new("price_id").string().not_null())
                    .col(ColumnDef::new("meter").string().null())
                    .col(ColumnDef::new("quantity").big_integer().null())
                    .col(timestamp("created_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("subscription_items", "subscription_id")
                            .to("subscriptions", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("subscription_items_subscription_id_index")
                    .table("subscription_items")
                    .col("subscription_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("billing_usage")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("billable_type").string().not_null())
                    .col(ColumnDef::new("billable_id").uuid().not_null())
                    .col(ColumnDef::new("meter").string().not_null())
                    .col(ColumnDef::new("quantity").big_integer().not_null())
                    .col(timestamp("recorded_at"))
                    .col(nullable_timestamp("reported_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("billing_usage_billable_meter_index")
                    .table("billing_usage")
                    .col("billable_type")
                    .col("billable_id")
                    .col("meter")
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("billing_usage_reported_at_index")
                    .table("billing_usage")
                    .col("reported_at")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("billing_events")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("provider").string().not_null())
                    .col(ColumnDef::new("provider_event_id").string().not_null())
                    .col(ColumnDef::new("event_type").string().not_null())
                    .col(ColumnDef::new("payload").text().not_null())
                    .col(timestamp("received_at"))
                    .col(nullable_timestamp("processed_at"))
                    .col(ColumnDef::new("attempts").integer().not_null().default(0))
                    .col(ColumnDef::new("error").text().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("billing_events_provider_event_unique")
                    .table("billing_events")
                    .col("provider")
                    .col("provider_event_id")
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "billing_events",
            "billing_usage",
            "subscription_items",
            "subscriptions",
            "billing_customers",
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
