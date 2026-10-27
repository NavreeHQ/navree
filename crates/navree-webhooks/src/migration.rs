//! The webhooks module's migrations (`MigrationSet` `webhooks`). The
//! delivery log, `webhook_deliveries` and `webhook_attempts`, is time
//! series: TimescaleDB hypertables on `created_at`.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Hypertable, KeyStrategy, Migration, MigrationContext, Schema, SchemaError, SchemaMigration,
    SchemaResult,
};
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the webhooks module's migration set.
pub const WEBHOOKS_MIGRATIONS: &str = "webhooks";

/// The table of outgoing deliveries.
pub(crate) const DELIVERIES_TABLE: &str = "webhook_deliveries";

/// The table of delivery attempts.
pub(crate) const ATTEMPTS_TABLE: &str = "webhook_attempts";

/// The webhooks module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(WEBHOOKS_MIGRATIONS)
        .migration(CreateWebhookTables)
        .migration(SchemaMigration::new(
            "m20261027_000001_convert_webhook_deliveries_to_hypertables",
            ConvertDeliveriesToHypertables,
        ))
}

/// How the delivery log is partitioned on TimescaleDB: by `created_at`, a
/// day per chunk (deliveries are kept 30 days by default, so about thirty
/// chunks a table). `webhooks:prune` drops whole chunks.
pub(crate) fn hypertables() -> [(&'static str, Hypertable); 2] {
    let table = || {
        let mut hypertable = Hypertable::on("created_at");
        hypertable
            .chunk_interval("1 day")
            .keys(KeyStrategy::Extend)
            .migrate_data();
        hypertable
    };
    [(DELIVERIES_TABLE, table()), (ATTEMPTS_TABLE, table())]
}

/// Makes the delivery log time series.
///
/// A hypertable's primary key must include its time column, so no other
/// table can reference `webhook_deliveries (id)`: attempts now reference
/// their endpoint (`ON DELETE CASCADE`, as deliveries do) instead of their
/// delivery, and `webhooks:prune` deletes a delivery's attempts with it.
/// Then both tables become hypertables on TimescaleDB, their primary keys
/// `(id, created_at)`; elsewhere they stay regular tables with their
/// `created_at` indexes, and the migration reports a notice.
///
/// Reverting it keeps both: TimescaleDB cannot turn a hypertable back into
/// a plain table in place, and the tables work the same either way. Running
/// it again changes nothing.
struct ConvertDeliveriesToHypertables;

impl Migration for ConvertDeliveriesToHypertables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let foreign_keys = schema.foreign_keys(ATTEMPTS_TABLE).await?;
        let to_delivery = foreign_keys
            .iter()
            .find(|key| key.foreign_table == DELIVERIES_TABLE && key.columns == ["delivery_id"]);
        let to_endpoint = foreign_keys
            .iter()
            .any(|key| key.foreign_table == "webhook_endpoints" && key.columns == ["endpoint_id"]);
        if to_delivery.is_some() || !to_endpoint {
            schema
                .table(ATTEMPTS_TABLE, |t| {
                    match to_delivery.map(|key| key.name.as_deref()) {
                        // PostgreSQL and MySQL drop a foreign key by name...
                        Some(Some(name)) => t.drop_foreign(name),
                        // ...SQLite's have none: by its columns.
                        Some(None) => t.drop_foreign(["delivery_id"]),
                        None => {}
                    }
                    if !to_endpoint {
                        t.foreign(["endpoint_id"])
                            .references(["id"])
                            .on("webhook_endpoints")
                            .cascade_on_delete();
                    }
                })
                .await?;
        }
        for (table, hypertable) in hypertables() {
            if schema.is_hypertable(table).await? {
                continue;
            }
            match schema.convert_to_hypertable(table, hypertable).await {
                // Pretending (`migrate --pretend`), the foreign key dropped
                // above still points at the deliveries in the database the
                // plan is read from, so TimescaleDB's conversion of them
                // cannot be planned yet: say what the real run does.
                Err(SchemaError::Hypertable { .. })
                    if to_delivery.is_some()
                        && table == DELIVERIES_TABLE
                        && schema.is_pretending() =>
                {
                    if let Some(cx) = MigrationContext::current() {
                        cx.notice(format!(
                            "`{DELIVERIES_TABLE}` becomes a hypertable once the foreign key of `{ATTEMPTS_TABLE}` points at `webhook_endpoints`, which a pretended run only plans"
                        ));
                    }
                }
                converted => converted?,
            }
        }
        Ok(())
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
        Ok(())
    }
}

/// Creates `webhook_endpoints`, `webhook_deliveries`, `webhook_attempts`
/// and `webhook_receipts`.
struct CreateWebhookTables;

impl MigrationName for CreateWebhookTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_webhook_tables"
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
impl MigrationTrait for CreateWebhookTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("webhook_endpoints")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().null())
                    .col(ColumnDef::new("url").text().not_null())
                    .col(ColumnDef::new("description").string().null())
                    .col(ColumnDef::new("events").json().not_null())
                    .col(ColumnDef::new("headers").json().not_null())
                    .col(ColumnDef::new("secret").text().not_null())
                    .col(ColumnDef::new("previous_secret").text().null())
                    .col(nullable_timestamp("previous_secret_expires_at"))
                    .col(ColumnDef::new("enabled").boolean().not_null())
                    .col(nullable_timestamp("disabled_at"))
                    .col(ColumnDef::new("disabled_reason").string().null())
                    .col(nullable_timestamp("failing_since"))
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("webhook_endpoints_workspace_id_index")
                    .table("webhook_endpoints")
                    .col("workspace_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("webhook_deliveries")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("endpoint_id").uuid().not_null())
                    .col(ColumnDef::new("workspace_id").uuid().null())
                    .col(ColumnDef::new("message_id").string().not_null())
                    .col(ColumnDef::new("event_type").string().not_null())
                    .col(ColumnDef::new("payload").text().not_null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("attempts").integer().not_null())
                    .col(ColumnDef::new("max_attempts").integer().not_null())
                    .col(nullable_timestamp("next_attempt_at"))
                    .col(ColumnDef::new("last_status_code").integer().null())
                    .col(ColumnDef::new("last_error").text().null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("webhook_deliveries", "endpoint_id")
                            .to("webhook_endpoints", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        for (name, columns) in [
            (
                "webhook_deliveries_endpoint_created_index",
                &["endpoint_id", "created_at"][..],
            ),
            ("webhook_deliveries_status_index", &["status"][..]),
            ("webhook_deliveries_created_at_index", &["created_at"][..]),
        ] {
            let mut index = Index::create();
            index.name(name).table("webhook_deliveries");
            for column in columns {
                index.col(*column);
            }
            manager.create_index(index.to_owned()).await?;
        }
        manager
            .create_table(
                Table::create()
                    .table("webhook_attempts")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("delivery_id").uuid().not_null())
                    .col(ColumnDef::new("endpoint_id").uuid().not_null())
                    .col(ColumnDef::new("attempt").integer().not_null())
                    .col(ColumnDef::new("status_code").integer().null())
                    .col(ColumnDef::new("succeeded").boolean().not_null())
                    .col(ColumnDef::new("response_body").text().null())
                    .col(ColumnDef::new("error").text().null())
                    .col(ColumnDef::new("duration_ms").big_integer().not_null())
                    .col(timestamp("created_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("webhook_attempts", "delivery_id")
                            .to("webhook_deliveries", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("webhook_attempts_delivery_id_index")
                    .table("webhook_attempts")
                    .col("delivery_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("webhook_receipts")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("source").string().not_null())
                    .col(ColumnDef::new("message_id").string().not_null())
                    .col(timestamp("received_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("webhook_receipts_source_message_unique")
                    .table("webhook_receipts")
                    .col("source")
                    .col("message_id")
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("webhook_receipts_received_at_index")
                    .table("webhook_receipts")
                    .col("received_at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "webhook_receipts",
            "webhook_attempts",
            "webhook_deliveries",
            "webhook_endpoints",
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
