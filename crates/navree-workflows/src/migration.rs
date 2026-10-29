//! The `workflows` migration set: tables `workflow_runs`, `workflow_steps`,
//! `automations`, `automation_versions` and `automation_enrollments`, written
//! with the schema builder.
//!
//! Ids are UUIDs stored as text, times are Unix milliseconds (`BIGINT`) and
//! JSON is stored as text, so every statement reads the same on SQLite,
//! PostgreSQL and MySQL. The names are the ones the module's migrations
//! always had, so a database migrated by an earlier version finds nothing
//! pending; the columns are the types those migrations created (text of no
//! fixed length is `VARCHAR` on PostgreSQL and SQLite, `VARCHAR(255)` on
//! MySQL and MariaDB).

use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};

/// The name of the workflows module's migration set.
pub const WORKFLOWS_MIGRATIONS: &str = "workflows";

/// The table of workflow runs.
pub const RUNS_TABLE: &str = "workflow_runs";

/// The table of the steps runs took.
pub const STEPS_TABLE: &str = "workflow_steps";

/// The table of automations.
pub const AUTOMATIONS_TABLE: &str = "automations";

/// The table of published automation versions.
pub const VERSIONS_TABLE: &str = "automation_versions";

/// The table of automation enrollments.
pub const ENROLLMENTS_TABLE: &str = "automation_enrollments";

/// The workflows module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(WORKFLOWS_MIGRATIONS)
        .migration(SchemaMigration::new(
            "m20261027_000001_create_workflow_tables",
            CreateWorkflowTables,
        ))
        .migration(SchemaMigration::new(
            "m20261027_000002_create_automation_tables",
            CreateAutomationTables,
        ))
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

/// Creates `workflow_runs` and `workflow_steps`.
struct CreateWorkflowTables;

impl Migration for CreateWorkflowTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let d = schema.target().await?.dialect();
        schema
            .create(RUNS_TABLE, |t| {
                varchar(t, "id", d).primary();
                varchar(t, "workflow", d);
                t.big_integer("version");
                varchar(t, "status", d);
                t.text("input");
                t.text("output").nullable();
                t.text("error").nullable();
                varchar(t, "workspace_id", d).nullable();
                varchar(t, "dedup_key", d).nullable();
                t.big_integer("wake_at").nullable();
                varchar(t, "lease_token", d).nullable();
                t.big_integer("lease_until").nullable();
                t.boolean("wake_requested").default(false);
                t.big_integer("created_at");
                t.big_integer("updated_at");
                t.big_integer("finished_at").nullable();
                t.index(["status", "wake_at"])
                    .name("workflow_runs_status_wake_at_index");
                t.index(["workflow", "dedup_key"])
                    .name("workflow_runs_workflow_dedup_key_index");
                t.index(["workspace_id", "created_at"])
                    .name("workflow_runs_workspace_id_created_at_index");
                t.index(["finished_at"])
                    .name("workflow_runs_finished_at_index");
            })
            .await?;
        schema
            .create(STEPS_TABLE, |t| {
                varchar(t, "run_id", d);
                varchar(t, "step_key", d);
                t.big_integer("position");
                varchar(t, "kind", d);
                varchar(t, "status", d);
                t.text("output").nullable();
                t.text("error").nullable();
                t.big_integer("attempts");
                t.big_integer("wake_at").nullable();
                varchar(t, "event", d).nullable();
                t.text("filter").nullable();
                t.big_integer("created_at");
                t.big_integer("updated_at");
                t.primary(["run_id", "step_key"]);
                t.foreign(["run_id"])
                    .references(["id"])
                    .on(RUNS_TABLE)
                    .cascade_on_delete();
                t.index(["event", "status"])
                    .name("workflow_steps_event_status_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(STEPS_TABLE).await?;
        schema.drop_if_exists(RUNS_TABLE).await
    }
}

/// Creates `automations`, `automation_versions` and
/// `automation_enrollments`.
struct CreateAutomationTables;

impl Migration for CreateAutomationTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let d = schema.target().await?.dialect();
        schema
            .create(AUTOMATIONS_TABLE, |t| {
                varchar(t, "id", d).primary();
                varchar(t, "workspace_id", d).nullable();
                varchar(t, "name", d);
                t.text("description").nullable();
                varchar(t, "status", d);
                t.text("definition");
                varchar(t, "enrollment", d);
                t.big_integer("version").nullable();
                varchar(t, "trigger_event", d).nullable();
                varchar(t, "template", d).nullable();
                t.big_integer("created_at");
                t.big_integer("updated_at");
                t.index(["status", "trigger_event"])
                    .name("automations_status_trigger_event_index");
                t.index(["workspace_id", "created_at"])
                    .name("automations_workspace_id_created_at_index");
            })
            .await?;
        schema
            .create(VERSIONS_TABLE, |t| {
                varchar(t, "automation_id", d);
                t.big_integer("version");
                t.text("definition");
                varchar(t, "published_by", d).nullable();
                t.big_integer("created_at");
                t.primary(["automation_id", "version"]);
                t.foreign(["automation_id"])
                    .references(["id"])
                    .on(AUTOMATIONS_TABLE)
                    .cascade_on_delete();
            })
            .await?;
        schema
            .create(ENROLLMENTS_TABLE, |t| {
                varchar(t, "id", d).primary();
                varchar(t, "automation_id", d);
                t.big_integer("version");
                varchar(t, "workspace_id", d).nullable();
                varchar(t, "subject", d).nullable();
                varchar(t, "run_id", d);
                t.big_integer("created_at");
                t.foreign(["automation_id"])
                    .references(["id"])
                    .on(AUTOMATIONS_TABLE)
                    .cascade_on_delete();
                t.index(["automation_id", "subject"])
                    .name("automation_enrollments_automation_subject_index");
                t.index(["automation_id", "created_at"])
                    .name("automation_enrollments_automation_created_at_index");
                t.index(["run_id"])
                    .name("automation_enrollments_run_id_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in [ENROLLMENTS_TABLE, VERSIONS_TABLE, AUTOMATIONS_TABLE] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}
