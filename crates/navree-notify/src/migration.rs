//! The `notify` migration set: the `notifications` table, and the
//! `notification_deliveries`, `notification_opt_outs` and
//! `push_subscriptions` tables of the Slack, Teams, SMS, WhatsApp and web
//! push channels (schema-builder migrations).

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The table of the database channel.
pub const NOTIFICATIONS_TABLE: &str = "notifications";

/// The delivery log of the recorded channels.
pub const DELIVERIES_TABLE: &str = "notification_deliveries";

/// The SMS and WhatsApp opt-outs.
pub const OPT_OUTS_TABLE: &str = "notification_opt_outs";

/// The browser push subscriptions.
pub const PUSH_SUBSCRIPTIONS_TABLE: &str = "push_subscriptions";

/// The migrations of `NotifyModule` (module name `notify`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("notify")
        .migration(CreateNotifications)
        .migration(SchemaMigration::new(
            "m20261028_000001_create_notification_deliveries_table",
            CreateDeliveries,
        ))
        .migration(SchemaMigration::new(
            "m20261028_000002_create_notification_opt_outs_table",
            CreateOptOuts,
        ))
        .migration(SchemaMigration::new(
            "m20261028_000003_create_push_subscriptions_table",
            CreatePushSubscriptions,
        ))
}

/// Creates `notifications` with an index on the owner and read state. No
/// foreign key: the owner is polymorphic (`notifiable_type`, `notifiable_id`).
struct CreateNotifications;

impl MigrationName for CreateNotifications {
    fn name(&self) -> &str {
        "m20261025_000001_create_notifications"
    }
}

#[async_trait]
impl MigrationTrait for CreateNotifications {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(NOTIFICATIONS_TABLE)
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("notifiable_type").string().not_null())
                    .col(ColumnDef::new("notifiable_id").string().not_null())
                    .col(ColumnDef::new("type").string().not_null())
                    .col(ColumnDef::new("data").json_binary().not_null())
                    .col(ColumnDef::new("read_at").timestamp_with_time_zone().null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_notifications_notifiable")
                    .table(NOTIFICATIONS_TABLE)
                    .col("notifiable_type")
                    .col("notifiable_id")
                    .col("read_at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(NOTIFICATIONS_TABLE).to_owned())
            .await
    }
}

/// A UUID column of a model: `BINARY(16)` on MySQL (the bytes the ORM
/// writes there), `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, mysql: bool) -> ColumnBuilder<'a> {
    if mysql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// Creates `notification_deliveries`: one row per notification, recorded
/// channel and recipient, found by notification and by the provider's
/// message id (status callbacks).
struct CreateDeliveries;

impl Migration for CreateDeliveries {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(DELIVERIES_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "notification_id", mysql);
                t.string("notification_type");
                t.string("notifiable_type").length(64);
                t.string("notifiable_id");
                t.string("channel").length(16);
                t.string("provider").length(32).nullable();
                t.string("recipient").nullable();
                t.string("provider_message_id").nullable();
                t.string("status").length(16);
                t.text("error").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index(["notification_id"])
                    .name("notification_deliveries_notification_index");
                t.index(["provider", "provider_message_id"])
                    .name("notification_deliveries_provider_message_index");
                t.index(["notifiable_type", "notifiable_id"])
                    .name("notification_deliveries_notifiable_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(DELIVERIES_TABLE).await
    }
}

/// Creates `notification_opt_outs`: unique per channel and number.
struct CreateOptOuts;

impl Migration for CreateOptOuts {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(OPT_OUTS_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                t.string("channel").length(16);
                t.string("address").length(32);
                t.string("source").length(16);
                t.timestamp_tz("created_at");
                t.unique(["channel", "address"])
                    .name("notification_opt_outs_channel_address_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(OPT_OUTS_TABLE).await
    }
}

/// Creates `push_subscriptions`: unique per endpoint (by its hash, since
/// endpoints are long), found by notifiable.
struct CreatePushSubscriptions;

impl Migration for CreatePushSubscriptions {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(PUSH_SUBSCRIPTIONS_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                t.string("notifiable_type").length(64);
                t.string("notifiable_id");
                t.text("endpoint");
                t.char("endpoint_hash", 64);
                t.string("p256dh").length(128);
                t.string("auth").length(128);
                t.string("user_agent").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("last_used_at").nullable();
                t.unique(["endpoint_hash"])
                    .name("push_subscriptions_endpoint_hash_unique");
                t.index(["notifiable_type", "notifiable_id"])
                    .name("push_subscriptions_notifiable_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(PUSH_SUBSCRIPTIONS_TABLE).await
    }
}
