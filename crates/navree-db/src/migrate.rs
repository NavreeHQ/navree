//! Module-owned migrations: [`MigrationSet`], [`Migrator`] and [`MigrationStatus`].

pub(crate) mod commands;
mod conn;
mod report;
mod runner;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_kernel::{AppContext, BoxError};
use navree_schema::{Dialect, NamedMigration, SchemaError};

use serde::Serialize;

pub use report::{
    FreshOptions, LoadedDump, MigrateOptions, MigrationReport, MigrationRun, Refreshed,
    RollbackOptions,
};
pub(crate) use runner::Runner;

use self::conn::Conn;
use crate::{Backend, Db, DbError, Executor, ExecutorExt, Statement, mysql};

/// The table recording applied migrations.
pub const MIGRATIONS_TABLE: &str = "navree_migrations";

/// The migrations a module owns, contributed with
/// [`ModuleBuilderDbExt::migrations`](crate::ModuleBuilderDbExt::migrations).
///
/// Sets run in dependency order: a set runs after every set named in
/// [`after`](MigrationSet::after) (names of modules not in the app are
/// ignored), otherwise in module-name order. Inside a set, migrations run
/// in name order, so name them `m20261025_000001_create_users` and so on.
///
/// A set holds migrations written with the schema builder
/// ([`navree_schema::Migration`], added with `navree_schema::migration!` or
/// [`SchemaMigration::new`](navree_schema::SchemaMigration::new)).
///
/// On SQLite and PostgreSQL each migration runs in its own transaction (DDL
/// included) unless it opts out (`within_transaction` returning `false`),
/// for statements such as `CREATE INDEX CONCURRENTLY`; the
/// `navree_migrations` row is written in the same transaction, so a failed
/// migration leaves nothing behind.
///
/// MySQL and MariaDB commit every DDL statement implicitly, so there a
/// migration runs outside a transaction unless it asks for one
/// (`within_transaction_on_mysql` returning `true`): worth it for data
/// migrations, whose `INSERT`s
/// and `UPDATE`s then roll back with the `navree_migrations` row. A MySQL
/// migration that fails part way keeps the statements that ran before the
/// failure and is not recorded: keep one DDL statement per migration, or
/// make its `up` re-runnable (`IF NOT EXISTS`).
///
/// ```
/// use navree_db::MigrationSet;
///
/// mod m20261025_000001_create_deals {
///     use navree_schema::{Schema, SchemaResult};
///
///     pub struct Migration;
///
///     impl navree_schema::Migration for Migration {
///         async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
///             schema.create("deals", |t| { t.id(); t.string("title"); }).await
///         }
///         async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
///             schema.drop_if_exists("deals").await
///         }
///     }
/// }
///
/// let set = MigrationSet::new("crm")
///     .after(["users"])
///     .migration(navree_schema::migration!(m20261025_000001_create_deals));
/// assert_eq!(set.module, "crm");
/// assert_eq!(set.names(), ["m20261025_000001_create_deals"]);
/// ```
pub struct MigrationSet {
    /// The name recorded in `navree_migrations.module` and used in `after`
    /// lists, e.g. `users`.
    pub module: &'static str,
    /// Modules whose migrations must run first (e.g. for foreign keys).
    pub after: Vec<&'static str>,
    /// The migrations written with the schema builder.
    pub migrations: Vec<Box<dyn NamedMigration>>,
}

impl MigrationSet {
    /// An empty set for `module`.
    pub fn new(module: &'static str) -> Self {
        Self {
            module,
            after: Vec::new(),
            migrations: Vec::new(),
        }
    }

    /// Runs after the migrations of `modules`.
    pub fn after(mut self, modules: impl IntoIterator<Item = &'static str>) -> Self {
        self.after.extend(modules);
        self
    }

    /// Adds a migration: `navree_schema::migration!(m20261025_000001_create_deals)`
    /// or `SchemaMigration::new(name, migration)`.
    pub fn migration(mut self, migration: impl NamedMigration + 'static) -> Self {
        self.migrations.push(Box::new(migration));
        self
    }

    /// The names of every migration of the set, in run order.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.migrations.iter().map(|m| m.name()).collect();
        names.sort_unstable();
        names
    }

    /// Every migration of the set, in run order (by name).
    pub fn entries(&self) -> Vec<SetMigration<'_>> {
        let mut entries: Vec<SetMigration<'_>> = self
            .migrations
            .iter()
            .map(|m| SetMigration(m.as_ref()))
            .collect();
        entries.sort_by(|a, b| a.name().cmp(b.name()));
        entries
    }

    /// Every migration of the set as a step, in run order.
    fn steps(&self) -> impl Iterator<Item = Step<'_>> {
        self.entries().into_iter().map(|body| Step {
            module: self.module,
            body,
        })
    }
}

impl fmt::Debug for MigrationSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MigrationSet")
            .field("module", &self.module)
            .field("after", &self.after)
            .field("migrations", &self.names())
            .finish()
    }
}

/// The state of one migration, as reported by [`Migrator::status`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MigrationStatus {
    /// The owning module (`MigrationSet::module`).
    pub module: String,
    /// The migration name.
    pub name: String,
    /// Whether it has been applied.
    pub applied: bool,
    /// The batch it was applied in.
    pub batch: Option<i64>,
    /// When it was applied.
    pub applied_at: Option<DateTime<Utc>>,
}

/// A migration problem.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MigrationError {
    /// Two sets use the same module name.
    #[error("two migration sets are named `{module}`; module names must be unique")]
    DuplicateModule {
        /// The module name.
        module: String,
    },
    /// A set contains two migrations with the same name.
    #[error("migration `{module}/{name}` is registered twice")]
    DuplicateMigration {
        /// The module name.
        module: String,
        /// The migration name.
        name: String,
    },
    /// The `after` lists form a cycle.
    #[error("migration order cycle between modules {}", modules.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", "))]
    Cycle {
        /// The modules on or behind the cycle.
        modules: Vec<String>,
    },
    /// A migration failed (and was rolled back when it ran in a transaction).
    #[error("migration `{migration}` failed: {source}")]
    Failed {
        /// `module/name`.
        migration: String,
        /// Why: the schema builder's error ([`SchemaError`]), the
        /// database's ([`DbError`]).
        #[source]
        source: BoxError,
    },
    /// `navree_migrations` records a migration no module registers any more.
    #[error("migration `{migration}` is recorded as applied but no module registers it")]
    Unknown {
        /// `module/name`.
        migration: String,
    },
    /// The database cannot do what was asked (`--drop-types` on MySQL).
    #[error("{message}")]
    Unsupported {
        /// What, and why not.
        message: String,
    },
    /// A schema dump could not be written or loaded.
    #[error("schema dump {}: {source}", path.display())]
    Dump {
        /// The dump file.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: Box<SchemaError>,
    },
    /// Reading or changing the schema outside a migration failed.
    #[error(transparent)]
    Schema(#[from] SchemaError),
    /// A database error outside a migration.
    #[error(transparent)]
    Db(#[from] DbError),
}

/// Runs the migrations every module contributed, against the app's [`Db`].
///
/// Applied migrations are recorded in `navree_migrations(module, name, batch,
/// applied_at)`; each [`up`](Migrator::up) run that applies anything is one
/// batch (one batch per migration with [`MigrateOptions::step`]), and
/// [`rollback`](Migrator::rollback) reverts the last batch.
///
/// Each migration runs on navree-db's own connections: in a transaction of
/// the [`Db`]'s direct connection (see [`MigrationSet`] for when), with its
/// `navree_migrations` row. While one runs, the `schema` configuration section
/// ([`SchemaOptions`](navree_schema::SchemaOptions)) configures its builder,
/// and its statements and notices are collected in the
/// [`MigrationReport`].
///
/// On SQLite, migrations run on a connection of their own opened with
/// foreign keys off, so rebuilding a table never runs the ON DELETE actions
/// of the tables referencing it; before each migration commits,
/// `PRAGMA foreign_key_check` must find no dangling reference.
///
/// ```no_run
/// # async fn deploy(ctx: &navree_kernel::AppContext) -> Result<(), navree_kernel::BoxError> {
/// use navree_db::{MigrateOptions, Migrator};
///
/// let report = Migrator::run(ctx, &MigrateOptions { pretend: true, ..MigrateOptions::default() }).await?;
/// for migration in &report.migrations {
///     println!("{}:", migration.label());
///     for statement in &migration.statements {
///         println!("  {statement};");
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Migrator;

impl Migrator {
    /// Applies every pending migration (one new batch); returns
    /// `module/name` of each, in the order they ran.
    pub async fn up(ctx: &AppContext) -> Result<Vec<String>, BoxError> {
        Ok(Migrator::run(ctx, &MigrateOptions::default())
            .await?
            .labels())
    }

    /// Applies every pending migration as `options` say: pretending,
    /// stepping, after loading the schema dump into an empty database
    /// (`navree migrate`).
    pub async fn run(
        ctx: &AppContext,
        options: &MigrateOptions,
    ) -> Result<MigrationReport, BoxError> {
        Ok(Runner::of(ctx).await?.up(options).await?)
    }

    /// Reverts the last batch, newest migration first; returns `module/name`
    /// of each.
    pub async fn rollback(ctx: &AppContext) -> Result<Vec<String>, BoxError> {
        Ok(Migrator::rollback_with(ctx, &RollbackOptions::default())
            .await?
            .labels())
    }

    /// Reverts the last batch, the last [`step`](RollbackOptions::step)
    /// migrations or batch [`batch`](RollbackOptions::batch), newest first
    /// (`navree migrate:rollback`).
    pub async fn rollback_with(
        ctx: &AppContext,
        options: &RollbackOptions,
    ) -> Result<MigrationReport, BoxError> {
        Ok(Runner::of(ctx).await?.rollback(options).await?)
    }

    /// Reverts every migration, newest first (`navree migrate:reset`).
    pub async fn reset(ctx: &AppContext, pretend: bool) -> Result<MigrationReport, BoxError> {
        Ok(Runner::of(ctx).await?.reset(pretend).await?)
    }

    /// Reverts every migration (or the last `step`), then migrates again
    /// (`navree migrate:refresh`).
    pub async fn refresh(ctx: &AppContext, step: Option<u32>) -> Result<Refreshed, BoxError> {
        let runner = Runner::of(ctx).await?;
        let reverted = match step {
            Some(step) => {
                runner
                    .rollback(&RollbackOptions {
                        step: Some(step),
                        ..RollbackOptions::default()
                    })
                    .await?
            }
            None => runner.reset(false).await?,
        };
        let migrated = runner.up(&MigrateOptions::default()).await?;
        Ok(Refreshed { reverted, migrated })
    }

    /// Drops every table, then applies every migration as batch 1.
    ///
    /// Every table goes, including tables no migration created: on SQLite
    /// on one connection with foreign key checks off, on PostgreSQL every
    /// table of the current schema with `CASCADE`, on MySQL and MariaDB
    /// every base table of the current database in one `DROP TABLE`, on
    /// one connection with `FOREIGN_KEY_CHECKS = 0` (turned back on before
    /// the connection returns to the pool, also when the drop fails).
    pub async fn fresh(ctx: &AppContext) -> Result<Vec<String>, BoxError> {
        Ok(Migrator::fresh_with(ctx, &FreshOptions::default())
            .await?
            .labels())
    }

    /// Drops every table (and views and PostgreSQL types when asked), then
    /// migrates (`navree migrate:fresh`).
    pub async fn fresh_with(
        ctx: &AppContext,
        options: &FreshOptions,
    ) -> Result<MigrationReport, BoxError> {
        Ok(Runner::of(ctx).await?.fresh(options).await?)
    }

    /// Every registered migration in run order, with its state.
    pub async fn status(ctx: &AppContext) -> Result<Vec<MigrationStatus>, BoxError> {
        Ok(Runner::of(ctx).await?.status().await?)
    }

    /// Creates the migrations table (`navree migrate:install`).
    pub async fn install(ctx: &AppContext) -> Result<(), BoxError> {
        Ok(Runner::of(ctx).await?.install().await?)
    }

    /// Writes the schema, with the migrations table's rows, as an SQL
    /// script to `path` (default `{schema.dump_path}/{driver}-schema.sql`,
    /// `database/schema/postgres-schema.sql`); returns the path
    /// (`navree schema:dump`). [`run`](Migrator::run) loads it into an
    /// empty database, then runs only the migrations it does not record.
    pub async fn dump(ctx: &AppContext, path: Option<&Path>) -> Result<PathBuf, BoxError> {
        Ok(Runner::of(ctx).await?.dump(path).await?)
    }
}

/// The migration sets contributed by the modules of `ctx`.
pub(crate) fn sets_of(ctx: &AppContext) -> Vec<Arc<MigrationSet>> {
    ctx.contributions::<MigrationSet>()
        .into_iter()
        .map(|c| c.value)
        .collect()
}

/// Runs the pending migrations of `app` on `db` while [`Db`] is being
/// constructed (`database.migrate_on_boot`); `url` and `sqlite` open SQLite's
/// migration connection.
pub(crate) async fn migrate_on_boot(
    db: &Db,
    app: &AppContext,
    url: &str,
    sqlite: &crate::SqliteOptions,
) -> Result<(), BoxError> {
    let options = runner::schema_options(app).await?;
    let runner = Runner::new(
        db.clone(),
        sets_of(app),
        options,
        Some(url.to_owned()),
        sqlite.clone(),
    );
    runner.up(&MigrateOptions::default()).await?;
    Ok(())
}

/// One migration in run order.
struct Step<'a> {
    module: &'static str,
    body: SetMigration<'a>,
}

/// One migration of a [`MigrationSet`] ([`MigrationSet::entries`]), for
/// runners of their own (navree-workspace's per-workspace migrations under
/// schema and database isolation).
#[derive(Clone, Copy)]
pub struct SetMigration<'a>(&'a dyn NamedMigration);

impl<'a> SetMigration<'a> {
    /// The name recorded in `navree_migrations`.
    pub fn name(&self) -> &'a str {
        self.0.name()
    }

    /// The migration itself.
    pub fn migration(&self) -> &'a dyn NamedMigration {
        self.0
    }

    /// Applies the migration on `conn` (a transaction, a unit of work).
    pub async fn up_on(&self, conn: &(impl Executor + ?Sized)) -> Result<(), DbError> {
        self.0
            .up(&navree_schema::Schema::new(&crate::schema::OnExecutor(
                conn,
            )))
            .await
            .map_err(|error| DbError::new(crate::ErrorKind::Other, error.to_string()))
    }
}

impl fmt::Debug for SetMigration<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SetMigration").field(&self.name()).finish()
    }
}

impl Step<'_> {
    fn name(&self) -> &str {
        self.body.name()
    }

    fn label(&self) -> String {
        format!("{}/{}", self.module, self.name())
    }

    /// Whether the step runs in a transaction on `dialect`: as the
    /// migration asks, else where DDL is transactional (PostgreSQL,
    /// SQLite; not MySQL).
    fn transactional(&self, dialect: Dialect) -> bool {
        self.body.0.transactional(dialect)
    }
}

/// Every migration in run order: sets topologically by `after` (ties by
/// module name), migrations by name.
fn plan(sets: &[Arc<MigrationSet>]) -> Result<Vec<Step<'_>>, MigrationError> {
    let mut by_module: BTreeMap<&'static str, &MigrationSet> = BTreeMap::new();
    for set in sets {
        if by_module.insert(set.module, set).is_some() {
            return Err(MigrationError::DuplicateModule {
                module: set.module.to_owned(),
            });
        }
        let mut names = HashSet::new();
        for step in set.steps() {
            if !names.insert(step.name().to_owned()) {
                return Err(MigrationError::DuplicateMigration {
                    module: set.module.to_owned(),
                    name: step.name().to_owned(),
                });
            }
        }
    }

    // Kahn's algorithm; `after` entries naming absent modules are ignored.
    let mut waiting_on: BTreeMap<&str, BTreeSet<&str>> = by_module
        .iter()
        .map(|(module, set)| {
            let deps = set
                .after
                .iter()
                .copied()
                .filter(|dep| by_module.contains_key(dep) && dep != module)
                .collect();
            (*module, deps)
        })
        .collect();
    let mut order = Vec::with_capacity(by_module.len());
    while let Some(next) = waiting_on
        .iter()
        .find(|(_, deps)| deps.is_empty())
        .map(|(module, _)| *module)
    {
        waiting_on.remove(next);
        for deps in waiting_on.values_mut() {
            deps.remove(next);
        }
        order.push(next);
    }
    if !waiting_on.is_empty() {
        return Err(MigrationError::Cycle {
            modules: waiting_on.keys().map(|m| (*m).to_owned()).collect(),
        });
    }

    let mut steps = Vec::new();
    for module in order {
        let mut set: Vec<Step<'_>> = by_module[module].steps().collect();
        set.sort_by(|a, b| a.name().cmp(b.name()));
        steps.extend(set);
    }
    Ok(steps)
}

/// A `navree_migrations` row.
struct Applied {
    batch: i64,
    applied_at: Option<DateTime<Utc>>,
}

/// The migrations table, as every version of the migrator has created it:
/// `module`, `name`, `batch` and `applied_at`, keyed by module and name.
/// MySQL's `applied_at` is a `DATETIME` (its `TIMESTAMP` ends in 2038 and
/// converts through the session time zone; a `DATETIME` holds the UTC time
/// as written).
fn create_table_sql(backend: Backend) -> &'static str {
    match backend {
        Backend::Postgres => {
            "CREATE TABLE IF NOT EXISTS \"navree_migrations\" ( \"module\" varchar NOT NULL, \
             \"name\" varchar NOT NULL, \"batch\" bigint NOT NULL, \
             \"applied_at\" timestamp with time zone NOT NULL, PRIMARY KEY (\"module\", \"name\") )"
        }
        Backend::MySql => {
            "CREATE TABLE IF NOT EXISTS `navree_migrations` ( `module` varchar(255) NOT NULL, \
             `name` varchar(255) NOT NULL, `batch` bigint NOT NULL, \
             `applied_at` datetime NOT NULL, PRIMARY KEY (`module`, `name`) )"
        }
        Backend::Sqlite => {
            "CREATE TABLE IF NOT EXISTS \"navree_migrations\" ( \"module\" varchar NOT NULL, \
             \"name\" varchar NOT NULL, \"batch\" integer NOT NULL, \
             \"applied_at\" timestamp_with_timezone_text NOT NULL, PRIMARY KEY (\"module\", \"name\") )"
        }
    }
}

/// The recorded migrations.
fn select_sql(backend: Backend) -> &'static str {
    match backend {
        Backend::MySql => "SELECT `module`, `name`, `batch`, `applied_at` FROM `navree_migrations`",
        _ => "SELECT \"module\", \"name\", \"batch\", \"applied_at\" FROM \"navree_migrations\"",
    }
}

/// Records a migration.
fn insert_sql(backend: Backend) -> &'static str {
    match backend {
        Backend::Postgres => {
            "INSERT INTO \"navree_migrations\" (\"module\", \"name\", \"batch\", \"applied_at\") \
             VALUES ($1, $2, $3, $4)"
        }
        Backend::MySql => {
            "INSERT INTO `navree_migrations` (`module`, `name`, `batch`, `applied_at`) \
             VALUES (?, ?, ?, ?)"
        }
        Backend::Sqlite => {
            "INSERT INTO \"navree_migrations\" (\"module\", \"name\", \"batch\", \"applied_at\") \
             VALUES (?, ?, ?, ?)"
        }
    }
}

/// Removes a migration's row.
fn delete_sql(backend: Backend) -> &'static str {
    match backend {
        Backend::Postgres => {
            "DELETE FROM \"navree_migrations\" WHERE \"module\" = $1 AND \"name\" = $2"
        }
        Backend::MySql => "DELETE FROM `navree_migrations` WHERE `module` = ? AND `name` = ?",
        Backend::Sqlite => {
            "DELETE FROM \"navree_migrations\" WHERE \"module\" = ? AND \"name\" = ?"
        }
    }
}

/// The migrations recorded in `navree_migrations` on `conn` (the table is
/// created when missing), by `(module, name)`, with their batches: for
/// runners of their own, such as navree-workspace's per-workspace
/// migrations, that keep the same table as the [`Migrator`].
pub async fn recorded_migrations(
    conn: &dyn Executor,
) -> Result<HashMap<(String, String), i64>, DbError> {
    Ok(applied_rows(conn)
        .await?
        .into_iter()
        .map(|(key, applied)| (key, applied.batch))
        .collect())
}

/// Records migration `name` of `module` as applied in batch `batch` on
/// `conn` (see [`recorded_migrations`]).
pub async fn record_migration(
    conn: &dyn Executor,
    module: &str,
    name: &str,
    batch: i64,
) -> Result<(), DbError> {
    let backend = conn.backend();
    let statement = Statement::new(
        backend,
        insert_sql(backend),
        [
            crate::Value::from(module),
            crate::Value::from(name),
            crate::Value::I64(batch),
            crate::Value::from(Utc::now()),
        ],
    );
    conn.execute(&statement).await.map(drop)
}

async fn ensure_table(conn: &dyn Executor) -> Result<(), DbError> {
    let backend = conn.backend();
    conn.execute(&Statement::new(backend, create_table_sql(backend), []))
        .await
        .map(drop)
}

async fn applied_rows(conn: &dyn Executor) -> Result<HashMap<(String, String), Applied>, DbError> {
    ensure_table(conn).await?;
    let backend = conn.backend();
    let rows = conn
        .fetch_all(&Statement::new(backend, select_sql(backend), []))
        .await?;
    let mut applied = HashMap::with_capacity(rows.len());
    for row in rows {
        let module: String = row.get("module")?;
        let name: String = row.get("name")?;
        let batch: i64 = row.get("batch")?;
        let applied_at = row
            .get::<Option<DateTime<Utc>>>("applied_at")
            .ok()
            .flatten();
        applied.insert((module, name), Applied { batch, applied_at });
    }
    Ok(applied)
}

/// `"name"` with embedded quotes doubled.
fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Drops every table of the database (`migrate:fresh`).
async fn drop_all_tables(conn: &Conn) -> Result<(), DbError> {
    match conn.backend() {
        Backend::Sqlite => conn.drop_all_sqlite_tables().await,
        Backend::Postgres => {
            let tables: Vec<String> = conn
                .fetch_column(&Statement::text(
                    "SELECT tablename AS name FROM pg_tables WHERE schemaname = current_schema()",
                ))
                .await?;
            let tx = conn.begin().await?;
            for table in &tables {
                tx.execute_script(&format!("DROP TABLE IF EXISTS {} CASCADE", quote(table)))
                    .await?;
            }
            tx.commit().await
        }
        Backend::MySql => drop_all_mysql_tables(conn).await,
    }
}

/// MySQL's half of [`drop_all_tables`]: every base table of the current
/// database in one `DROP TABLE` (all or none), with foreign key checks off
/// so the order does not matter.
///
/// `FOREIGN_KEY_CHECKS` is a session variable, so the statements run on
/// one connection, pinned by a transaction (`DROP TABLE` commits it
/// implicitly; the final `COMMIT` is then a no-op). Checks are turned back
/// on whatever the drop did, before the connection returns to the pool.
async fn drop_all_mysql_tables(conn: &Conn) -> Result<(), DbError> {
    let tables: Vec<String> = conn
        .fetch_column(&Statement::text(mysql::LIST_TABLES))
        .await?;
    if tables.is_empty() {
        return Ok(());
    }
    let tx = conn.begin().await?;
    tx.execute_script(mysql::FOREIGN_KEY_CHECKS_OFF).await?;
    let dropped = tx.execute_script(&mysql::drop_tables(&tables)).await;
    let restored = tx.execute_script(mysql::FOREIGN_KEY_CHECKS_ON).await;
    match (dropped, restored) {
        (Ok(_), Ok(_)) => tx.commit().await,
        (Err(error), _) | (Ok(_), Err(error)) => {
            if let Err(rollback) = tx.rollback().await {
                tracing::warn!(target: "navree_db::migrate", error = %rollback, "rollback after a failed migrate:fresh failed");
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers() {
        assert_eq!(quote("users"), "\"users\"");
        assert_eq!(quote("we\"ird"), "\"we\"\"ird\"");
    }
}
