//! The migrations of the projects module, written with the schema builder.

use navree::db::MigrationSet;
use navree::schema::migration;

mod m20261028_000001_create_projects_table;

/// The module's migration set, run after the workspace tables.
pub fn migrations() -> MigrationSet {
    MigrationSet::new("projects")
        .after(["workspace"])
        .migration(migration!(m20261028_000001_create_projects_table))
}
