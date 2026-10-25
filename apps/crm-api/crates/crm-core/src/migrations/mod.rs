//! The CRM's one migration set, `crm`: every CRM table and index. It runs
//! after keel-auth's `auth` (owners are users) and keel-access's `access`.
//! Feature crates add no migrations of their own; a schema change is a new
//! migration here (`m<date>_<seq>_<what>`, run in name order).

use keel::db::MigrationSet;

mod m20261025_000001_create_crm_tables;

/// The name of the set in `keel_migrations.module` and `after` lists.
pub const CRM_MIGRATIONS: &str = "crm";

/// The `crm` migration set.
pub fn migrations() -> MigrationSet {
    MigrationSet::new(CRM_MIGRATIONS)
        .after(["auth", "access"])
        .migration(m20261025_000001_create_crm_tables::Migration)
}
