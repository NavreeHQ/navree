//! The OAuth server's migrations (`MigrationSet` `oauth`, after `auth`).

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{
    ColumnDef, ForeignKey, ForeignKeyAction, ForeignKeyCreateStatement, Index, Table,
    TableCreateStatement,
};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the OAuth server's migration set.
pub const OAUTH_MIGRATIONS: &str = "oauth";

/// The OAuth server's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(OAUTH_MIGRATIONS)
        .after(["auth"])
        .migration(CreateOAuthTables)
}

/// Creates every OAuth table.
struct CreateOAuthTables;

impl MigrationName for CreateOAuthTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_oauth_tables"
    }
}

fn id() -> ColumnDef {
    ColumnDef::new("id")
        .uuid()
        .not_null()
        .primary_key()
        .to_owned()
}

fn uuid(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).uuid().not_null().to_owned()
}

fn uuid_null(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).uuid().null().to_owned()
}

fn text(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).text().not_null().to_owned()
}

fn text_null(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).text().null().to_owned()
}

fn string(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).string().not_null().to_owned()
}

fn unique_string(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .string()
        .not_null()
        .unique_key()
        .to_owned()
}

fn timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

fn timestamp_null(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .null()
        .to_owned()
}

/// `client_id` referencing `oauth_clients(id)`, cascading deletes.
fn client_fk(table: &'static str) -> ForeignKeyCreateStatement {
    ForeignKey::create()
        .from(table, "client_id")
        .to("oauth_clients", "id")
        .on_delete(ForeignKeyAction::Cascade)
        .to_owned()
}

fn index(
    name: &'static str,
    table: &'static str,
    columns: &[&'static str],
) -> sea_orm::sea_query::IndexCreateStatement {
    let mut index = Index::create();
    index.name(name).table(table);
    for column in columns {
        index.col(*column);
    }
    index.to_owned()
}

fn tables() -> Vec<TableCreateStatement> {
    vec![
        Table::create()
            .table("oauth_clients")
            .col(id())
            .col(string("name"))
            .col(ColumnDef::new("secret_hash").string().null())
            .col(text("redirect_uris"))
            .col(text("scopes"))
            .col(string("grants"))
            .col(uuid_null("owner_id"))
            .col(
                ColumnDef::new("trusted")
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .col(
                ColumnDef::new("introspect")
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .col(uuid_null("workspace_id"))
            .col(text_null("logo_url"))
            .col(text_null("website"))
            .col(timestamp("created_at"))
            .col(timestamp("updated_at"))
            .col(timestamp_null("revoked_at"))
            .to_owned(),
        Table::create()
            .table("oauth_authorizations")
            .col(id())
            .col(uuid("user_id"))
            .col(uuid("client_id"))
            .col(text("scopes"))
            .col(timestamp("created_at"))
            .col(timestamp("updated_at"))
            .foreign_key(&mut client_fk("oauth_authorizations"))
            .to_owned(),
        Table::create()
            .table("oauth_authorization_requests")
            .col(id())
            .col(uuid("client_id"))
            .col(text("redirect_uri"))
            .col(text("scopes"))
            .col(text_null("state"))
            .col(string("code_challenge"))
            .col(text_null("nonce"))
            .col(uuid_null("user_id"))
            .col(string("status"))
            .col(timestamp("expires_at"))
            .col(timestamp("created_at"))
            .foreign_key(&mut client_fk("oauth_authorization_requests"))
            .to_owned(),
        Table::create()
            .table("oauth_codes")
            .col(id())
            .col(unique_string("code_hash"))
            .col(uuid("client_id"))
            .col(uuid("user_id"))
            .col(text("redirect_uri"))
            .col(text("scopes"))
            .col(string("code_challenge"))
            .col(text_null("nonce"))
            .col(timestamp("expires_at"))
            .col(timestamp_null("used_at"))
            .col(timestamp("created_at"))
            .foreign_key(&mut client_fk("oauth_codes"))
            .to_owned(),
        Table::create()
            .table("oauth_access_tokens")
            .col(id())
            .col(unique_string("token_hash"))
            .col(uuid("client_id"))
            .col(uuid("user_id"))
            .col(text("scopes"))
            .col(string("grant"))
            .col(uuid_null("code_id"))
            .col(uuid_null("family_id"))
            .col(uuid_null("workspace_id"))
            .col(timestamp("expires_at"))
            .col(timestamp_null("revoked_at"))
            .col(timestamp("created_at"))
            .foreign_key(&mut client_fk("oauth_access_tokens"))
            .to_owned(),
        Table::create()
            .table("oauth_refresh_tokens")
            .col(id())
            .col(unique_string("token_hash"))
            .col(uuid("client_id"))
            .col(uuid("user_id"))
            .col(text("scopes"))
            .col(uuid("family_id"))
            .col(uuid_null("code_id"))
            .col(uuid_null("access_token_id"))
            .col(uuid_null("workspace_id"))
            .col(timestamp("expires_at"))
            .col(timestamp_null("used_at"))
            .col(timestamp_null("revoked_at"))
            .col(timestamp("created_at"))
            .foreign_key(&mut client_fk("oauth_refresh_tokens"))
            .to_owned(),
        Table::create()
            .table("oauth_device_codes")
            .col(id())
            .col(unique_string("device_code_hash"))
            .col(unique_string("user_code"))
            .col(uuid("client_id"))
            .col(text("scopes"))
            .col(uuid_null("user_id"))
            .col(string("status"))
            .col(ColumnDef::new("interval_secs").big_integer().not_null())
            .col(timestamp_null("last_polled_at"))
            .col(timestamp("expires_at"))
            .col(timestamp_null("used_at"))
            .col(timestamp("created_at"))
            .foreign_key(&mut client_fk("oauth_device_codes"))
            .to_owned(),
        Table::create()
            .table("personal_access_tokens")
            .col(id())
            .col(uuid("user_id"))
            .col(string("name"))
            .col(unique_string("token_hash"))
            .col(text("scopes"))
            .col(timestamp_null("expires_at"))
            .col(timestamp_null("last_used_at"))
            .col(timestamp_null("revoked_at"))
            .col(timestamp("created_at"))
            .to_owned(),
    ]
}

#[async_trait]
impl MigrationTrait for CreateOAuthTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in tables() {
            manager.create_table(table).await?;
        }
        let mut unique_authorization = index(
            "oauth_authorizations_user_client_unique",
            "oauth_authorizations",
            &["user_id", "client_id"],
        );
        unique_authorization.unique();
        for statement in [
            unique_authorization,
            index(
                "oauth_access_tokens_user_client_index",
                "oauth_access_tokens",
                &["user_id", "client_id"],
            ),
            index(
                "oauth_access_tokens_family_index",
                "oauth_access_tokens",
                &["family_id"],
            ),
            index(
                "oauth_access_tokens_code_index",
                "oauth_access_tokens",
                &["code_id"],
            ),
            index(
                "oauth_refresh_tokens_user_client_index",
                "oauth_refresh_tokens",
                &["user_id", "client_id"],
            ),
            index(
                "oauth_refresh_tokens_family_index",
                "oauth_refresh_tokens",
                &["family_id"],
            ),
            index(
                "personal_access_tokens_user_index",
                "personal_access_tokens",
                &["user_id"],
            ),
        ] {
            manager.create_index(statement).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "personal_access_tokens",
            "oauth_device_codes",
            "oauth_refresh_tokens",
            "oauth_access_tokens",
            "oauth_codes",
            "oauth_authorization_requests",
            "oauth_authorizations",
            "oauth_clients",
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
