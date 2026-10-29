//! The webhooks module's migrations (`MigrationSet` `webhooks`). The
//! delivery log, `webhook_deliveries` and `webhook_attempts`, is time
//! series: TimescaleDB hypertables on `created_at`. Written with the schema
//! builder.
//!
//! The names are the ones the module's migrations always had, so a database
//! migrated by an earlier version finds nothing pending. The columns are the
//! types those migrations created: UUIDs as `BINARY(16)` on MySQL and
//! MariaDB (where navree-db writes UUIDs as 16 bytes), text of no fixed
//! length as `VARCHAR` without a length on PostgreSQL and SQLite and
//! `VARCHAR(255)` on MySQL, JSON as `json`.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Hypertable, KeyStrategy, Migration, MigrationContext,
    Schema, SchemaError, SchemaMigration, SchemaResult,
};

/// The name of the webhooks module's migration set.
pub const WEBHOOKS_MIGRATIONS: &str = "webhooks";

/// The table of outgoing deliveries.
pub(crate) const DELIVERIES_TABLE: &str = "webhook_deliveries";

/// The table of delivery attempts.
pub(crate) const ATTEMPTS_TABLE: &str = "webhook_attempts";

/// The webhooks module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(WEBHOOKS_MIGRATIONS)
        .migration(SchemaMigration::new(
            "m20261026_000001_create_webhook_tables",
            CreateWebhookTables,
        ))
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

/// Creates `webhook_endpoints`, `webhook_deliveries`, `webhook_attempts`
/// and `webhook_receipts`.
struct CreateWebhookTables;

impl Migration for CreateWebhookTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let d = schema.target().await?.dialect();
        schema
            .create("webhook_endpoints", |t| {
                uuid(t, "id", d).primary();
                uuid(t, "workspace_id", d)
                    .nullable()
                    .index_named("webhook_endpoints_workspace_id_index");
                t.text("url");
                varchar(t, "description", d).nullable();
                t.json("events");
                t.json("headers");
                t.text("secret");
                t.text("previous_secret").nullable();
                t.timestamp_tz("previous_secret_expires_at").nullable();
                t.boolean("enabled");
                t.timestamp_tz("disabled_at").nullable();
                varchar(t, "disabled_reason", d).nullable();
                t.timestamp_tz("failing_since").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
            })
            .await?;
        schema
            .create("webhook_deliveries", |t| {
                uuid(t, "id", d).primary();
                uuid(t, "endpoint_id", d);
                uuid(t, "workspace_id", d).nullable();
                varchar(t, "message_id", d);
                varchar(t, "event_type", d);
                t.text("payload");
                varchar(t, "status", d);
                t.integer("attempts");
                t.integer("max_attempts");
                t.timestamp_tz("next_attempt_at").nullable();
                t.integer("last_status_code").nullable();
                t.text("last_error").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign(["endpoint_id"])
                    .references(["id"])
                    .on("webhook_endpoints")
                    .cascade_on_delete();
                t.index(["endpoint_id", "created_at"])
                    .name("webhook_deliveries_endpoint_created_index");
                t.index(["status"]).name("webhook_deliveries_status_index");
                t.index(["created_at"])
                    .name("webhook_deliveries_created_at_index");
            })
            .await?;
        schema
            .create("webhook_attempts", |t| {
                uuid(t, "id", d).primary();
                uuid(t, "delivery_id", d).index_named("webhook_attempts_delivery_id_index");
                uuid(t, "endpoint_id", d);
                t.integer("attempt");
                t.integer("status_code").nullable();
                t.boolean("succeeded");
                t.text("response_body").nullable();
                t.text("error").nullable();
                t.big_integer("duration_ms");
                t.timestamp_tz("created_at");
                t.foreign(["delivery_id"])
                    .references(["id"])
                    .on("webhook_deliveries")
                    .cascade_on_delete();
            })
            .await?;
        schema
            .create("webhook_receipts", |t| {
                uuid(t, "id", d).primary();
                varchar(t, "source", d);
                varchar(t, "message_id", d);
                t.timestamp_tz("received_at")
                    .index_named("webhook_receipts_received_at_index");
                t.unique(["source", "message_id"])
                    .name("webhook_receipts_source_message_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in [
            "webhook_receipts",
            "webhook_attempts",
            "webhook_deliveries",
            "webhook_endpoints",
        ] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}
