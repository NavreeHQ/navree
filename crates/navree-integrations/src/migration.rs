//! The integrations module's migrations (`MigrationSet` `integrations`),
//! written with the schema builder.

use navree_db::MigrationSet;
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};

/// The name of the integrations module's migration set.
pub const INTEGRATIONS_MIGRATIONS: &str = "integrations";

/// The table of connections.
pub(crate) const CONNECTIONS_TABLE: &str = "integration_connections";

/// The integrations module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(INTEGRATIONS_MIGRATIONS).migration(SchemaMigration::new(
        "m20261027_000001_create_integration_connections_table",
        CreateIntegrationConnections,
    ))
}

/// Creates `integration_connections`: unique per owner, provider and scope
/// set, indexed by the last check (`integrations:check --due`). Owners are
/// not foreign keys, so the module works with or without navree-workspace;
/// the module forgets a deleted workspace's or organization's connections.
struct CreateIntegrationConnections;

impl Migration for CreateIntegrationConnections {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(CONNECTIONS_TABLE, |t| {
                t.uuid_primary();
                t.string("owner_type").length(16);
                t.uuid("owner_id");
                t.string("provider").length(64);
                t.string("scope_set").length(64);
                t.string("kind").length(32);
                t.string("external_id").nullable();
                t.string("display_name").nullable();
                t.text("credentials");
                t.text("access_token").nullable();
                t.timestamp_tz("token_expires_at").nullable();
                t.text("scopes").nullable();
                t.string("status").length(16).default("healthy");
                t.integer("failures").default(0);
                t.text("last_error").nullable();
                t.timestamp_tz("last_checked_at")
                    .nullable()
                    .index_named("integration_connections_last_checked_index");
                t.uuid("connected_by").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.unique(["owner_type", "owner_id", "provider", "scope_set"])
                    .name("integration_connections_owner_provider_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(CONNECTIONS_TABLE).await
    }
}
