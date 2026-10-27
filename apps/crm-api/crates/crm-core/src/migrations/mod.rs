//! The CRM's one migration set, `crm`, written with the schema builder:
//! every CRM table and index, and the activity timeline as a time series.
//! It runs after navree-auth's `auth` (owners are users) and
//! navree-access's `access`. Feature crates add no migrations of their
//! own; a schema change is a new migration here
//! (`m<date>_<seq>_<what>`, run in name order; `navree make:migration`
//! writes one).

use navree::db::MigrationSet;
use navree::schema::migration;

mod m20261025_000001_create_crm_tables;
mod m20261027_000001_make_activities_a_time_series;

pub use m20261027_000001_make_activities_a_time_series::TIME_COLUMN as ACTIVITIES_TIME_COLUMN;

/// The name of the set in `navree_migrations.module` and `after` lists.
pub const CRM_MIGRATIONS: &str = "crm";

/// The `crm` migration set.
pub fn migrations() -> MigrationSet {
    MigrationSet::new(CRM_MIGRATIONS)
        .after(["auth", "access"])
        .migration(migration!(m20261025_000001_create_crm_tables))
        .migration(migration!(m20261027_000001_make_activities_a_time_series))
}
