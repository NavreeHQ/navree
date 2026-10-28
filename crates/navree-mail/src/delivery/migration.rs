//! The mail module's migrations (`MigrationSet` `mail`), on the schema
//! builder.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the mail module's migration set.
pub const MAIL_MIGRATIONS: &str = "mail";

/// The mail module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(MAIL_MIGRATIONS).migration(SchemaMigration::new(
        "m20261027_000001_create_mail_tables",
        CreateMailTables,
    ))
}

/// Creates `mail_transports`, `mail_deliveries`, `mail_events`,
/// `mail_suppressions` and `mail_outbox`.
///
/// The columns keep the types the first version of this migration gave
/// them, so databases created before and after it read alike: UUIDs are
/// `BINARY(16)` on MySQL (the bytes navree-db binds there), timestamps
/// keep the database's default precision (whole seconds on MySQL).
struct CreateMailTables;

/// A UUID column: `BINARY(16)` on MySQL, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, mysql: bool) -> ColumnBuilder<'a> {
    if mysql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// An instant: `TIMESTAMP WITH TIME ZONE` (`TIMESTAMP` on MySQL).
fn timestamp<'a>(t: &'a mut Blueprint, name: &str) -> ColumnBuilder<'a> {
    t.timestamp_tz(name).default_precision()
}

impl Migration for CreateMailTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("mail_transports", |t| {
                uuid(t, "id", mysql).primary();
                t.string("tier");
                uuid(t, "workspace_id", mysql).nullable();
                uuid(t, "organization_id", mysql).nullable();
                t.string("domain").nullable();
                t.string("kind");
                t.text("config");
                t.boolean("enabled");
                timestamp(t, "checked_at").nullable();
                timestamp(t, "created_at");
                timestamp(t, "updated_at");
                t.index(["workspace_id"])
                    .name("mail_transports_workspace_index");
                t.index(["organization_id"])
                    .name("mail_transports_organization_index");
                t.index(["domain"]).name("mail_transports_domain_index");
            })
            .await?;
        schema
            .create("mail_deliveries", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "workspace_id", mysql).nullable();
                uuid(t, "organization_id", mysql).nullable();
                t.string("status");
                t.string("transport");
                t.string("tier");
                t.string("provider_id").nullable();
                t.string("template").nullable();
                t.text("subject");
                t.string("sender");
                t.string("recipient_hash");
                t.integer("recipients");
                t.text("error").nullable();
                timestamp(t, "opened_at").nullable();
                timestamp(t, "clicked_at").nullable();
                timestamp(t, "created_at");
                timestamp(t, "updated_at");
                t.index(["provider_id"])
                    .name("mail_deliveries_provider_id_index");
                t.index(["workspace_id", "created_at"])
                    .name("mail_deliveries_workspace_created_index");
                t.index(["recipient_hash"])
                    .name("mail_deliveries_recipient_index");
                t.index(["created_at"])
                    .name("mail_deliveries_created_at_index");
            })
            .await?;
        schema
            .create("mail_events", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "delivery_id", mysql).nullable();
                uuid(t, "workspace_id", mysql).nullable();
                t.string("provider");
                t.string("kind");
                t.string("recipient_hash").nullable();
                t.string("bounce_type").nullable();
                t.text("url").nullable();
                t.text("detail").nullable();
                timestamp(t, "occurred_at");
                timestamp(t, "created_at");
                t.foreign(["delivery_id"])
                    .references(["id"])
                    .on("mail_deliveries")
                    .cascade_on_delete();
                t.index(["delivery_id"]).name("mail_events_delivery_index");
            })
            .await?;
        schema
            .create("mail_suppressions", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "workspace_id", mysql).nullable();
                t.string("email");
                t.string("reason");
                t.text("detail").nullable();
                uuid(t, "delivery_id", mysql).nullable();
                timestamp(t, "created_at");
                t.index(["email", "workspace_id"])
                    .name("mail_suppressions_email_workspace_index");
            })
            .await?;
        schema
            .create("mail_outbox", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "workspace_id", mysql).nullable();
                uuid(t, "organization_id", mysql).nullable();
                t.text("reason");
                t.string("status");
                t.text("subject");
                t.json("recipients");
                t.text("message");
                t.string("via").nullable();
                t.string("template").nullable();
                uuid(t, "delivery_id", mysql).nullable();
                uuid(t, "decided_by", mysql).nullable();
                timestamp(t, "decided_at").nullable();
                timestamp(t, "created_at");
                timestamp(t, "updated_at");
                t.index(["status", "created_at"])
                    .name("mail_outbox_status_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in [
            "mail_outbox",
            "mail_suppressions",
            "mail_events",
            "mail_deliveries",
            "mail_transports",
        ] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}
