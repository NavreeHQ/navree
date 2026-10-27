//! The options the migrator's runs take and what they report.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

/// Options of [`Migrator::run`](crate::Migrator::run) (`navree migrate`).
///
/// ```
/// use navree_db::MigrateOptions;
///
/// let options = MigrateOptions { step: true, ..MigrateOptions::default() };
/// assert!(!options.pretend);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MigrateOptions {
    /// Record the SQL the pending migrations would run, and run nothing
    /// (`--pretend`). Schema migrations read the real database (so SQLite
    /// rebuilds and hypertable conversions plan against it); SeaORM
    /// migrations see no rows.
    pub pretend: bool,
    /// Record each migration as a batch of its own, so `migrate:rollback`
    /// reverts them one at a time (`--step`).
    pub step: bool,
    /// The schema dump to load first when the database is empty
    /// (`--schema-path`); by default `{schema.dump_path}/{driver}-schema.sql`
    /// when that file exists.
    pub schema_path: Option<PathBuf>,
}

/// Options of [`Migrator::rollback_with`](crate::Migrator::rollback_with)
/// (`navree migrate:rollback`). With neither `step` nor `batch`, the last
/// batch is reverted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RollbackOptions {
    /// Revert the last `n` migrations, across batches (`--step n`).
    pub step: Option<u32>,
    /// Revert batch `n` (`--batch n`).
    pub batch: Option<i64>,
    /// Record the SQL, run nothing (`--pretend`).
    pub pretend: bool,
}

/// Options of [`Migrator::fresh_with`](crate::Migrator::fresh_with)
/// (`navree migrate:fresh`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FreshOptions {
    /// Drop every view as well (`--drop-views`).
    pub drop_views: bool,
    /// Drop every user-defined type and domain as well, on PostgreSQL
    /// (`--drop-types`).
    pub drop_types: bool,
    /// How the migrations then run.
    pub migrate: MigrateOptions,
}

/// One migration run (or reverted) by the migrator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MigrationRun {
    /// The owning module (`MigrationSet::module`).
    pub module: String,
    /// The migration name.
    pub name: String,
    /// The batch it was recorded in, or removed from.
    pub batch: i64,
    /// The statements it ran (pretending: would run). Schema migrations
    /// record theirs on every run; SeaORM migrations when pretending.
    pub statements: Vec<String>,
    /// What the database could not do as asked (a hypertable on a
    /// database without TimescaleDB, ...).
    pub notices: Vec<String>,
    /// How long it took.
    pub duration: Duration,
}

impl MigrationRun {
    /// `module/name`.
    pub fn label(&self) -> String {
        format!("{}/{}", self.module, self.name)
    }
}

/// A schema dump loaded into an empty database before migrating.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LoadedDump {
    /// The dump file.
    pub path: PathBuf,
    /// How many statements it ran.
    pub statements: usize,
}

/// What a run of the migrator did (or, pretending, would do).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    /// The schema dump loaded first, if any.
    pub loaded_dump: Option<LoadedDump>,
    /// The migrations, in the order they ran.
    pub migrations: Vec<MigrationRun>,
}

impl MigrationReport {
    /// `module/name` of each migration, in order.
    pub fn labels(&self) -> Vec<String> {
        self.migrations.iter().map(MigrationRun::label).collect()
    }
}

/// What [`Migrator::refresh`](crate::Migrator::refresh) did: the
/// migrations it reverted, then the ones it ran.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Refreshed {
    /// Reverted, newest first.
    pub reverted: MigrationReport,
    /// Run again.
    pub migrated: MigrationReport,
}
