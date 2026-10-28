//! The `tasks` migration set, written with the schema builder.

use navree::db::MigrationSet;
use navree::schema::migration;

mod m20261028_000002_create_tasks_table;

/// The set, after the workspace tables and the projects.
pub fn migrations() -> MigrationSet {
    MigrationSet::new("tasks")
        .after(["workspace", "projects"])
        .migration(migration!(m20261028_000002_create_tasks_table))
}
