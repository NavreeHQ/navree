//! The directory module's migrations (`MigrationSet` `directory`), written
//! with the schema builder.

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the directory module's migration set.
pub const DIRECTORY_MIGRATIONS: &str = "directory";

/// The directory module's migration set: connections, users, groups,
/// memberships, runs and run changes. Owners, connections and rows are
/// not tied by foreign keys (the module deletes a connection's rows
/// itself), so it works with or without navree-workspace.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(DIRECTORY_MIGRATIONS)
        .migration(SchemaMigration::new(
            "m20261028_000001_create_directory_connections_table",
            Create("directory_connections", connections),
        ))
        .migration(SchemaMigration::new(
            "m20261028_000002_create_directory_users_table",
            Create("directory_users", users),
        ))
        .migration(SchemaMigration::new(
            "m20261028_000003_create_directory_groups_table",
            Create("directory_groups", groups),
        ))
        .migration(SchemaMigration::new(
            "m20261028_000004_create_directory_memberships_table",
            Create("directory_memberships", memberships),
        ))
        .migration(SchemaMigration::new(
            "m20261028_000005_create_directory_runs_table",
            Create("directory_runs", runs),
        ))
        .migration(SchemaMigration::new(
            "m20261028_000006_create_directory_run_changes_table",
            Create("directory_run_changes", run_changes),
        ))
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

fn connections(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    t.string("owner_type").length(16);
    uuid(t, "owner_id", mysql);
    t.string("name");
    t.string("source").length(64);
    t.text("settings");
    t.text("secret").nullable();
    t.string("scim_token_hash")
        .length(64)
        .nullable()
        .unique();
    t.string("scim_token_hint").length(32).nullable();
    t.text("cursor").nullable();
    t.string("status").length(16).default("active");
    t.integer("sync_interval_mins").default(60);
    uuid(t, "last_run_id", mysql).nullable();
    t.timestamp_tz("last_synced_at").nullable();
    t.timestamp_tz("next_sync_at")
        .nullable()
        .index_named("directory_connections_next_sync_index");
    uuid(t, "created_by", mysql).nullable();
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at");
    t.index(["owner_type", "owner_id"])
        .name("directory_connections_owner_index");
}

fn users(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    uuid(t, "connection_id", mysql);
    t.string("external_id");
    t.string("user_name");
    t.string("email").nullable();
    t.string("display_name").nullable();
    t.boolean("active").default(true);
    t.long_text("data");
    t.long_text("resource").nullable();
    t.string("fingerprint").length(64);
    t.string("local_id").nullable();
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at");
    t.timestamp_tz("deactivated_at").nullable();
    t.unique(["connection_id", "external_id"])
        .name("directory_users_connection_external_unique");
    t.index(["connection_id", "user_name"])
        .name("directory_users_connection_user_name_index");
}

fn groups(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    uuid(t, "connection_id", mysql);
    t.string("external_id");
    t.string("display_name");
    t.long_text("data");
    t.long_text("resource").nullable();
    t.string("fingerprint").length(64);
    t.string("local_id").nullable();
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at");
    t.unique(["connection_id", "external_id"])
        .name("directory_groups_connection_external_unique");
    t.index(["connection_id", "display_name"])
        .name("directory_groups_connection_name_index");
}

fn memberships(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    uuid(t, "connection_id", mysql).index_named("directory_memberships_connection_index");
    uuid(t, "group_id", mysql);
    uuid(t, "user_id", mysql).index_named("directory_memberships_user_index");
    t.unique(["group_id", "user_id"])
        .name("directory_memberships_group_user_unique");
}

fn runs(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    uuid(t, "connection_id", mysql);
    t.string("status").length(16);
    t.string("triggered_by").length(16);
    t.boolean("dry_run").default(false);
    t.boolean("force").default(false);
    t.boolean("full_sync").default(false);
    t.boolean("complete").nullable();
    t.text("summary");
    t.big_integer("done").default(0);
    t.big_integer("total").default(0);
    t.string("note").nullable();
    t.text("error").nullable();
    uuid(t, "operation_id", mysql).nullable();
    uuid(t, "created_by", mysql).nullable();
    t.timestamp_tz("created_at")
        .index_named("directory_runs_created_index");
    t.timestamp_tz("started_at").nullable();
    t.timestamp_tz("finished_at").nullable();
    t.index(["connection_id", "status"])
        .name("directory_runs_connection_status_index");
}

fn run_changes(t: &mut Blueprint, mysql: bool) {
    uuid(t, "id", mysql).primary();
    uuid(t, "run_id", mysql).index_named("directory_run_changes_run_index");
    t.string("subject").length(8);
    t.string("action").length(16);
    t.string("external_id");
    t.string("label");
    t.text("fields");
    t.integer("members_added").default(0);
    t.integer("members_removed").default(0);
    t.string("status").length(16);
    t.text("error").nullable();
    t.timestamp_tz("created_at");
}
