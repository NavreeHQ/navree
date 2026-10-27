//! The `workflows` migration set: tables `workflow_runs`, `workflow_steps`,
//! `automations`, `automation_versions` and `automation_enrollments`.
//!
//! Ids are UUIDs stored as text, times are Unix milliseconds (`BIGINT`) and
//! JSON is stored as text, so every statement reads the same on SQLite,
//! PostgreSQL and MySQL.

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

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
        .migration(CreateWorkflowTables)
        .migration(CreateAutomationTables)
}

fn id(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).string().not_null().to_owned()
}

fn millis(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).big_integer().not_null().to_owned()
}

fn nullable_millis(name: &'static str) -> ColumnDef {
    ColumnDef::new(name).big_integer().null().to_owned()
}

async fn index(
    manager: &SchemaManager<'_>,
    table: &str,
    name: &str,
    columns: &[&str],
) -> Result<(), DbErr> {
    let mut index = Index::create();
    index.name(name).table(table.to_owned());
    for column in columns {
        index.col((*column).to_owned());
    }
    manager.create_index(index.to_owned()).await
}

/// Creates `workflow_runs` and `workflow_steps`.
struct CreateWorkflowTables;

impl MigrationName for CreateWorkflowTables {
    fn name(&self) -> &str {
        "m20261027_000001_create_workflow_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateWorkflowTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RUNS_TABLE)
                    .col(id("id").primary_key())
                    .col(ColumnDef::new("workflow").string().not_null())
                    .col(ColumnDef::new("version").big_integer().not_null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("input").text().not_null())
                    .col(ColumnDef::new("output").text().null())
                    .col(ColumnDef::new("error").text().null())
                    .col(ColumnDef::new("workspace_id").string().null())
                    .col(ColumnDef::new("dedup_key").string().null())
                    .col(nullable_millis("wake_at"))
                    .col(ColumnDef::new("lease_token").string().null())
                    .col(nullable_millis("lease_until"))
                    .col(
                        ColumnDef::new("wake_requested")
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(millis("created_at"))
                    .col(millis("updated_at"))
                    .col(nullable_millis("finished_at"))
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            RUNS_TABLE,
            "workflow_runs_status_wake_at_index",
            &["status", "wake_at"],
        )
        .await?;
        index(
            manager,
            RUNS_TABLE,
            "workflow_runs_workflow_dedup_key_index",
            &["workflow", "dedup_key"],
        )
        .await?;
        index(
            manager,
            RUNS_TABLE,
            "workflow_runs_workspace_id_created_at_index",
            &["workspace_id", "created_at"],
        )
        .await?;
        index(
            manager,
            RUNS_TABLE,
            "workflow_runs_finished_at_index",
            &["finished_at"],
        )
        .await?;
        manager
            .create_table(
                Table::create()
                    .table(STEPS_TABLE)
                    .col(id("run_id"))
                    .col(ColumnDef::new("step_key").string().not_null())
                    .col(ColumnDef::new("position").big_integer().not_null())
                    .col(ColumnDef::new("kind").string().not_null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("output").text().null())
                    .col(ColumnDef::new("error").text().null())
                    .col(ColumnDef::new("attempts").big_integer().not_null())
                    .col(nullable_millis("wake_at"))
                    .col(ColumnDef::new("event").string().null())
                    .col(ColumnDef::new("filter").text().null())
                    .col(millis("created_at"))
                    .col(millis("updated_at"))
                    .primary_key(Index::create().col("run_id").col("step_key"))
                    .foreign_key(
                        ForeignKey::create()
                            .from(STEPS_TABLE, "run_id")
                            .to(RUNS_TABLE, "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            STEPS_TABLE,
            "workflow_steps_event_status_index",
            &["event", "status"],
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(STEPS_TABLE).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(RUNS_TABLE).to_owned())
            .await
    }
}

/// Creates `automations`, `automation_versions` and
/// `automation_enrollments`.
struct CreateAutomationTables;

impl MigrationName for CreateAutomationTables {
    fn name(&self) -> &str {
        "m20261027_000002_create_automation_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateAutomationTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AUTOMATIONS_TABLE)
                    .col(id("id").primary_key())
                    .col(ColumnDef::new("workspace_id").string().null())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("description").text().null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("definition").text().not_null())
                    .col(ColumnDef::new("enrollment").string().not_null())
                    .col(ColumnDef::new("version").big_integer().null())
                    .col(ColumnDef::new("trigger_event").string().null())
                    .col(ColumnDef::new("template").string().null())
                    .col(millis("created_at"))
                    .col(millis("updated_at"))
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            AUTOMATIONS_TABLE,
            "automations_status_trigger_event_index",
            &["status", "trigger_event"],
        )
        .await?;
        index(
            manager,
            AUTOMATIONS_TABLE,
            "automations_workspace_id_created_at_index",
            &["workspace_id", "created_at"],
        )
        .await?;
        manager
            .create_table(
                Table::create()
                    .table(VERSIONS_TABLE)
                    .col(id("automation_id"))
                    .col(ColumnDef::new("version").big_integer().not_null())
                    .col(ColumnDef::new("definition").text().not_null())
                    .col(ColumnDef::new("published_by").string().null())
                    .col(millis("created_at"))
                    .primary_key(Index::create().col("automation_id").col("version"))
                    .foreign_key(
                        ForeignKey::create()
                            .from(VERSIONS_TABLE, "automation_id")
                            .to(AUTOMATIONS_TABLE, "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(ENROLLMENTS_TABLE)
                    .col(id("id").primary_key())
                    .col(id("automation_id"))
                    .col(ColumnDef::new("version").big_integer().not_null())
                    .col(ColumnDef::new("workspace_id").string().null())
                    .col(ColumnDef::new("subject").string().null())
                    .col(id("run_id"))
                    .col(millis("created_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .from(ENROLLMENTS_TABLE, "automation_id")
                            .to(AUTOMATIONS_TABLE, "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        index(
            manager,
            ENROLLMENTS_TABLE,
            "automation_enrollments_automation_subject_index",
            &["automation_id", "subject"],
        )
        .await?;
        index(
            manager,
            ENROLLMENTS_TABLE,
            "automation_enrollments_automation_created_at_index",
            &["automation_id", "created_at"],
        )
        .await?;
        index(
            manager,
            ENROLLMENTS_TABLE,
            "automation_enrollments_run_id_index",
            &["run_id"],
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [ENROLLMENTS_TABLE, VERSIONS_TABLE, AUTOMATIONS_TABLE] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}
