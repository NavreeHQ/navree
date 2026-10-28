//! The domains module's migrations (`MigrationSet` `domains`), written with
//! the schema builder.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the domains module's migration set.
pub const DOMAINS_MIGRATIONS: &str = "domains";

/// The domains module's migration set: domains, certificates, challenges
/// and ACME accounts, and the change that lets several workspaces claim
/// one host (only a verified claim owns it). Domains are not tied to
/// `workspaces` by a foreign key (the module removes a deleted workspace's
/// domains itself), so the set runs before or after the workspace set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(DOMAINS_MIGRATIONS)
        .migration(SchemaMigration::new(
            "m20261028_100001_create_custom_domains_table",
            Create("custom_domains", domains),
        ))
        .migration(SchemaMigration::new(
            "m20261028_100002_create_custom_domain_certificates_table",
            Create("custom_domain_certificates", certificates),
        ))
        .migration(SchemaMigration::new(
            "m20261028_100003_create_custom_domain_challenges_table",
            Create("custom_domain_challenges", challenges),
        ))
        .migration(SchemaMigration::new(
            "m20261028_100004_create_custom_domain_acme_accounts_table",
            Create("custom_domain_acme_accounts", accounts),
        ))
        .migration(SchemaMigration::new(
            "m20261028_100005_claim_domains_by_proof",
            ClaimByProof,
        ))
}

/// Lets several workspaces claim one host: `host` stops being unique, and
/// `verified_host` (the host while the claim is verified, `NULL`
/// otherwise) is, so the database lets exactly one claim own a host. A
/// workspace still has one row per host. `transfer` marks the claims made
/// by `Domains::transfer`.
struct ClaimByProof;

impl Migration for ClaimByProof {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("custom_domains", |t| {
                t.string("verified_host").length(253).nullable();
                t.boolean("transfer").default(false);
            })
            .await?;
        // Every host was unique, so each verified row owns its host.
        schema
            .statement("UPDATE custom_domains SET verified_host = host WHERE status = 'verified'")
            .await?;
        schema
            .table("custom_domains", |t| {
                t.drop_unique(["host"]);
                t.index(["host"]).name("custom_domains_host_index");
                t.unique(["verified_host"])
                    .name("custom_domains_verified_host_unique");
                t.unique(["workspace_id", "host"])
                    .name("custom_domains_workspace_host_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        // A host had one row: claims that are not verified go, verified
        // domains stay (each owns its host, so `host` can be unique again).
        schema
            .statement("DELETE FROM custom_domains WHERE status <> 'verified'")
            .await?;
        schema
            .table("custom_domains", |t| {
                t.drop_unique("custom_domains_workspace_host_unique");
                t.drop_unique("custom_domains_verified_host_unique");
                t.drop_index("custom_domains_host_index");
                t.unique(["host"]);
            })
            .await?;
        schema
            .table("custom_domains", |t| {
                t.drop_column(["verified_host", "transfer"]);
            })
            .await
    }
}

/// Creates table `.0` with the columns `.1` adds (given whether the
/// database is MySQL or MariaDB); `down` drops it.
struct Create(&'static str, fn(&mut Blueprint, bool));

impl Migration for Create {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        let columns = self.1;
        schema.create(self.0, |t| columns(t, mysql)).await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(self.0).await
    }
}

/// A UUID column of the models: `BINARY(16)` on MySQL and MariaDB (where
/// the ORM writes UUIDs as 16 bytes), `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, mysql: bool) -> ColumnBuilder<'a> {
    if mysql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

fn domains(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    uuid(t, "workspace_id", mysql).index_named("custom_domains_workspace_index");
    t.string("host").length(253).unique();
    t.string("purpose").length(32).default("app");
    t.string("status").length(16).default("pending");
    t.string("verification_token").length(64);
    t.boolean("managed").default(false);
    t.boolean("is_primary").default(false);
    t.boolean("points_to_app").nullable();
    t.integer("check_attempts").default(0);
    t.timestamp_tz("last_checked_at").nullable();
    t.timestamp_tz("next_check_at")
        .nullable()
        .index_named("custom_domains_next_check_index");
    t.timestamp_tz("verified_at").nullable();
    t.text("last_error").nullable();
    t.string("tls_status").length(16).default("pending");
    t.timestamp_tz("certificate_expires_at").nullable();
    t.timestamp_tz("certificate_checked_at").nullable();
    t.text("tls_error").nullable();
    uuid(t, "created_by", mysql).nullable();
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at");
}

fn certificates(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    t.string("host").length(253).unique();
    uuid(t, "domain_id", mysql).nullable();
    t.long_text("chain_pem");
    t.text("private_key");
    t.timestamp_tz("not_before");
    t.timestamp_tz("not_after")
        .index_named("custom_domain_certificates_not_after_index");
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at")
        .index_named("custom_domain_certificates_updated_index");
}

fn challenges(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    t.string("token").length(255).unique();
    t.string("host").length(253);
    t.text("key_authorization");
    t.timestamp_tz("expires_at");
    t.timestamp_tz("created_at");
}

fn accounts(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    t.string("directory_url").length(255).unique();
    t.text("credentials");
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at");
}
