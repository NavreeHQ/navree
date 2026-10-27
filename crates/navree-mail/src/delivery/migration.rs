//! The mail module's migrations (`MigrationSet` `mail`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the mail module's migration set.
pub const MAIL_MIGRATIONS: &str = "mail";

/// The mail module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(MAIL_MIGRATIONS).migration(CreateMailTables)
}

/// Creates `mail_transports`, `mail_deliveries`, `mail_events`,
/// `mail_suppressions` and `mail_outbox`.
struct CreateMailTables;

impl MigrationName for CreateMailTables {
    fn name(&self) -> &str {
        "m20261027_000001_create_mail_tables"
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

async fn index(
    manager: &SchemaManager<'_>,
    table: &'static str,
    name: &'static str,
    columns: &[&'static str],
) -> Result<(), DbErr> {
    let mut index = Index::create();
    index.name(name).table(table);
    for column in columns {
        index.col(*column);
    }
    manager.create_index(index.to_owned()).await
}

#[async_trait]
impl MigrationTrait for CreateMailTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("mail_transports")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("tier").string().not_null())
                    .col(ColumnDef::new("workspace_id").uuid().null())
                    .col(ColumnDef::new("organization_id").uuid().null())
                    .col(ColumnDef::new("domain").string().null())
                    .col(ColumnDef::new("kind").string().not_null())
                    .col(ColumnDef::new("config").text().not_null())
                    .col(ColumnDef::new("enabled").boolean().not_null())
                    .col(nullable_timestamp("checked_at"))
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "mail_transports",
            "mail_transports_workspace_index",
            &["workspace_id"],
        )
        .await?;
        index(
            manager,
            "mail_transports",
            "mail_transports_organization_index",
            &["organization_id"],
        )
        .await?;
        index(
            manager,
            "mail_transports",
            "mail_transports_domain_index",
            &["domain"],
        )
        .await?;

        manager
            .create_table(
                Table::create()
                    .table("mail_deliveries")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().null())
                    .col(ColumnDef::new("organization_id").uuid().null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("transport").string().not_null())
                    .col(ColumnDef::new("tier").string().not_null())
                    .col(ColumnDef::new("provider_id").string().null())
                    .col(ColumnDef::new("template").string().null())
                    .col(ColumnDef::new("subject").text().not_null())
                    .col(ColumnDef::new("sender").string().not_null())
                    .col(ColumnDef::new("recipient_hash").string().not_null())
                    .col(ColumnDef::new("recipients").integer().not_null())
                    .col(ColumnDef::new("error").text().null())
                    .col(nullable_timestamp("opened_at"))
                    .col(nullable_timestamp("clicked_at"))
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "mail_deliveries",
            "mail_deliveries_provider_id_index",
            &["provider_id"],
        )
        .await?;
        index(
            manager,
            "mail_deliveries",
            "mail_deliveries_workspace_created_index",
            &["workspace_id", "created_at"],
        )
        .await?;
        index(
            manager,
            "mail_deliveries",
            "mail_deliveries_recipient_index",
            &["recipient_hash"],
        )
        .await?;
        index(
            manager,
            "mail_deliveries",
            "mail_deliveries_created_at_index",
            &["created_at"],
        )
        .await?;

        manager
            .create_table(
                Table::create()
                    .table("mail_events")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("delivery_id").uuid().null())
                    .col(ColumnDef::new("workspace_id").uuid().null())
                    .col(ColumnDef::new("provider").string().not_null())
                    .col(ColumnDef::new("kind").string().not_null())
                    .col(ColumnDef::new("recipient_hash").string().null())
                    .col(ColumnDef::new("bounce_type").string().null())
                    .col(ColumnDef::new("url").text().null())
                    .col(ColumnDef::new("detail").text().null())
                    .col(timestamp("occurred_at"))
                    .col(timestamp("created_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from("mail_events", "delivery_id")
                            .to("mail_deliveries", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "mail_events",
            "mail_events_delivery_index",
            &["delivery_id"],
        )
        .await?;

        manager
            .create_table(
                Table::create()
                    .table("mail_suppressions")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().null())
                    .col(ColumnDef::new("email").string().not_null())
                    .col(ColumnDef::new("reason").string().not_null())
                    .col(ColumnDef::new("detail").text().null())
                    .col(ColumnDef::new("delivery_id").uuid().null())
                    .col(timestamp("created_at"))
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "mail_suppressions",
            "mail_suppressions_email_workspace_index",
            &["email", "workspace_id"],
        )
        .await?;

        manager
            .create_table(
                Table::create()
                    .table("mail_outbox")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().null())
                    .col(ColumnDef::new("organization_id").uuid().null())
                    .col(ColumnDef::new("reason").text().not_null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("subject").text().not_null())
                    .col(ColumnDef::new("recipients").json().not_null())
                    .col(ColumnDef::new("message").text().not_null())
                    .col(ColumnDef::new("via").string().null())
                    .col(ColumnDef::new("template").string().null())
                    .col(ColumnDef::new("delivery_id").uuid().null())
                    .col(ColumnDef::new("decided_by").uuid().null())
                    .col(nullable_timestamp("decided_at"))
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            "mail_outbox",
            "mail_outbox_status_index",
            &["status", "created_at"],
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "mail_outbox",
            "mail_suppressions",
            "mail_events",
            "mail_deliveries",
            "mail_transports",
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
