//! The `notify` migration set: the `notifications` table, the
//! `notification_deliveries`, `notification_opt_outs`,
//! `push_subscriptions` and `push_devices` tables of the Slack, Teams,
//! SMS, WhatsApp, web push and push channels, and the
//! `notification_preferences` and `notification_deferrals` tables of
//! preferences and held deliveries (schema-builder migrations).

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The table of the database channel.
pub const NOTIFICATIONS_TABLE: &str = "notifications";

/// The delivery log of the recorded channels.
pub const DELIVERIES_TABLE: &str = "notification_deliveries";

/// The SMS and WhatsApp opt-outs.
pub const OPT_OUTS_TABLE: &str = "notification_opt_outs";

/// The browser push subscriptions.
pub const PUSH_SUBSCRIPTIONS_TABLE: &str = "push_subscriptions";

/// The native app push devices.
pub const PUSH_DEVICES_TABLE: &str = "push_devices";

/// The preferences of people and workspaces.
pub const PREFERENCES_TABLE: &str = "notification_preferences";

/// The held deliveries (digests, quiet hours, send windows).
pub const DEFERRALS_TABLE: &str = "notification_deferrals";

/// The migrations of `NotifyModule` (module name `notify`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("notify")
        .migration(SchemaMigration::new(
            "m20261025_000001_create_notifications",
            CreateNotifications,
        ))
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
        .migration(SchemaMigration::new(
            "m20261028_000004_create_notification_preferences_table",
            CreatePreferences,
        ))
        .migration(SchemaMigration::new(
            "m20261028_000005_create_notification_deferrals_table",
            CreateDeferrals,
        ))
        .migration(SchemaMigration::new(
            "m20261112_000001_create_push_devices_table",
            CreatePushDevices,
        ))
}

/// Creates `push_devices`: unique per token (by its hash, since Firebase
/// tokens are long), found by notifiable and by the session that
/// registered them.
struct CreatePushDevices;

impl Migration for CreatePushDevices {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(PUSH_DEVICES_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                t.string("notifiable_type").length(64);
                t.string("notifiable_id");
                t.string("session_id").length(191);
                t.string("platform").length(16);
                t.text("token");
                t.char("token_hash", 64);
                t.string("app_id");
                t.string("environment").length(16);
                t.timestamp_tz("created_at");
                t.timestamp_tz("last_seen_at");
                t.timestamp_tz("last_used_at").nullable();
                t.unique(["token_hash"])
                    .name("push_devices_token_hash_unique");
                t.index(["notifiable_type", "notifiable_id"])
                    .name("push_devices_notifiable_index");
                t.index(["session_id"]).name("push_devices_session_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(PUSH_DEVICES_TABLE).await
    }
}

/// Creates `notification_preferences`: one row per owner (a notifiable or
/// a workspace), its choices as JSON.
struct CreatePreferences;

impl Migration for CreatePreferences {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(PREFERENCES_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                t.string("owner_type").length(64);
                t.string("owner_id").length(191);
                t.jsonb("preferences");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.unique(["owner_type", "owner_id"])
                    .name("notification_preferences_owner_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(PREFERENCES_TABLE).await
    }
}

/// Creates `notification_deferrals`: held deliveries, found by release
/// time (`notify:release`) and by notifiable.
struct CreateDeferrals;

impl Migration for CreateDeferrals {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(DEFERRALS_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "notification_id", mysql);
                t.string("notification_type");
                t.string("category").length(100).nullable();
                t.string("notifiable_type").length(64);
                t.string("notifiable_id");
                t.string("channel").length(16);
                t.string("reason").length(16);
                t.string("digest").length(16).nullable();
                t.jsonb("job");
                t.timestamp_tz("release_at");
                t.integer("attempts").default(0);
                t.timestamp_tz("created_at");
                t.index(["release_at"])
                    .name("notification_deferrals_release_index");
                t.index(["notifiable_type", "notifiable_id"])
                    .name("notification_deferrals_notifiable_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(DEFERRALS_TABLE).await
    }
}

/// Creates `notifications` with an index on the owner and read state. No
/// foreign key: the owner is polymorphic (`notifiable_type`, `notifiable_id`).
struct CreateNotifications;

impl Migration for CreateNotifications {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(NOTIFICATIONS_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                t.string("notifiable_type");
                t.string("notifiable_id");
                t.string("type");
                t.jsonb("data");
                t.timestamp_tz("read_at").nullable();
                t.timestamp_tz("created_at");
                t.index(["notifiable_type", "notifiable_id", "read_at"])
                    .name("idx_notifications_notifiable");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(NOTIFICATIONS_TABLE).await
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
