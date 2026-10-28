//! The migrations of the projects module.

use navree::db::MigrationSet;

mod m20261028_000001_create_projects_table;

/// The module's migration set, run after the workspace tables.
pub fn migrations() -> MigrationSet {
    MigrationSet::new("projects")
        .after(["workspace"])
        .migration(m20261028_000001_create_projects_table::Migration)
}
