#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;

/// The benchmark's schema is created by `bench/db/init.sql` and must not be
/// migrated by the app (SPEC §2), so the generated users migration is gone and
/// the list is empty. `create_app` still needs a migrator type, and
/// `database.auto_migrate` is `false` in every config, so this never runs.
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            // inject-above (do not remove this comment)
        ]
    }
}
