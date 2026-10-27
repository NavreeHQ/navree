//! Module-owned migrations: [`MigrationSet`], [`Migrator`] and [`MigrationStatus`].

pub(crate) mod commands;
mod pretend;
mod report;
mod runner;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_kernel::{AppContext, BoxError};
use sea_orm::sea_query::{ColumnDef, Index, Query, Table};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, TransactionTrait};
use sea_orm_migration::MigrationTrait;
use serde::Serialize;

pub use report::{
    FreshOptions, LoadedDump, MigrateOptions, MigrationReport, MigrationRun, Refreshed,
    RollbackOptions,
};
pub(crate) use runner::Runner;

use crate::{Db, mysql};

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
/// On SQLite and PostgreSQL each migration runs in its own transaction (DDL
/// included) unless it returns `Some(false)` from `use_transaction` (for
/// statements such as `CREATE INDEX CONCURRENTLY`, or SQLite table rebuilds
/// that switch `PRAGMA foreign_keys`, which has no effect inside a
/// transaction); the `navree_migrations` row is written in the same
/// transaction, so a failed migration leaves nothing behind.
///
/// MySQL and MariaDB commit every DDL statement implicitly, so there a
/// migration runs outside a transaction unless it returns `Some(true)`
/// (worth it for data migrations, whose `INSERT`s and `UPDATE`s then roll
/// back with the `navree_migrations` row). A MySQL migration that fails
/// part way keeps the statements that ran before the failure and is not
/// recorded: keep one DDL statement per migration, or make its `up`
/// re-runnable (`IF NOT EXISTS`).
///
/// ```
/// use navree_db::MigrationSet;
///
/// let set = MigrationSet::new("crm").after(["users"]);
/// assert_eq!(set.module, "crm");
/// ```
pub struct MigrationSet {
    /// The name recorded in `navree_migrations.module` and used in `after`
    /// lists, e.g. `users`.
    pub module: &'static str,
    /// Modules whose migrations must run first (e.g. for foreign keys).
    pub after: Vec<&'static str>,
    /// The migrations.
    pub migrations: Vec<Box<dyn MigrationTrait>>,
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

    /// Adds a migration.
    pub fn migration(mut self, migration: impl MigrationTrait + 'static) -> Self {
        self.migrations.push(Box::new(migration));
        self
    }
}

impl fmt::Debug for MigrationSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MigrationSet")
            .field("module", &self.module)
            .field("after", &self.after)
            .field(
                "migrations",
                &self.migrations.iter().map(|m| m.name()).collect::<Vec<_>>(),
            )
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
        /// The database error.
        #[source]
        source: DbErr,
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
        source: Box<navree_schema::SchemaError>,
    },
    /// A database error outside a migration.
    #[error(transparent)]
    Db(#[from] DbErr),
}

/// Runs the migrations every module contributed, against the app's [`Db`].
///
/// Applied migrations are recorded in `navree_migrations(module, name, batch,
/// applied_at)`; each [`up`](Migrator::up) run that applies anything is one
/// batch (one batch per migration with [`MigrateOptions::step`]), and
/// [`rollback`](Migrator::rollback) reverts the last batch.
///
/// Migrations written with the schema builder (`navree_schema::Migration`,
/// added with `navree_schema::migration!`) and SeaORM migrations share a
/// [`MigrationSet`]. While one runs, the `schema` configuration section
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
/// constructed (`database.migrate_on_boot`); `url` opens SQLite's migration
/// connection.
pub(crate) async fn migrate_on_boot(db: &Db, app: &AppContext, url: &str) -> Result<(), BoxError> {
    let options = runner::schema_options(app).await?;
    let runner = Runner::new(db.clone(), sets_of(app), options, Some(url.to_owned()));
    runner.up(&MigrateOptions::default()).await?;
    Ok(())
}

/// One migration in run order.
struct Step<'a> {
    module: &'static str,
    migration: &'a dyn MigrationTrait,
}

impl Step<'_> {
    fn name(&self) -> &str {
        self.migration.name()
    }

    fn label(&self) -> String {
        format!("{}/{}", self.module, self.name())
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
        for migration in &set.migrations {
            if !names.insert(migration.name()) {
                return Err(MigrationError::DuplicateMigration {
                    module: set.module.to_owned(),
                    name: migration.name().to_owned(),
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
        let set = by_module[module];
        let mut migrations: Vec<&dyn MigrationTrait> =
            set.migrations.iter().map(|m| m.as_ref()).collect();
        migrations.sort_by(|a, b| a.name().cmp(b.name()));
        steps.extend(migrations.into_iter().map(|migration| Step {
            module: set.module,
            migration,
        }));
    }
    Ok(steps)
}

/// A `navree_migrations` row.
struct Applied {
    batch: i64,
    applied_at: Option<DateTime<Utc>>,
}

async fn ensure_table(conn: &DatabaseConnection) -> Result<(), DbErr> {
    let mut applied_at = ColumnDef::new("applied_at");
    match conn.get_database_backend() {
        // MySQL's `TIMESTAMP` ends in 2038 and converts through the session
        // time zone; a `DATETIME` holds the UTC time as written.
        DbBackend::MySql => applied_at.date_time(),
        _ => applied_at.timestamp_with_time_zone(),
    }
    .not_null();
    conn.execute(
        Table::create()
            .table(MIGRATIONS_TABLE)
            .if_not_exists()
            .col(ColumnDef::new("module").string().not_null())
            .col(ColumnDef::new("name").string().not_null())
            .col(ColumnDef::new("batch").big_integer().not_null())
            .col(applied_at)
            .primary_key(Index::create().col("module").col("name")),
    )
    .await
    .map(drop)
}

async fn applied_rows(
    conn: &DatabaseConnection,
) -> Result<HashMap<(String, String), Applied>, DbErr> {
    ensure_table(conn).await?;
    let rows = conn
        .query_all(
            Query::select()
                .columns(["module", "name", "batch", "applied_at"])
                .from(MIGRATIONS_TABLE),
        )
        .await?;
    let mut applied = HashMap::with_capacity(rows.len());
    for row in rows {
        let module: String = row.try_get("", "module")?;
        let name: String = row.try_get("", "name")?;
        let batch: i64 = row.try_get("", "batch")?;
        let applied_at: Option<DateTime<Utc>> = row.try_get("", "applied_at").ok();
        applied.insert((module, name), Applied { batch, applied_at });
    }
    Ok(applied)
}

/// `"name"` with embedded quotes doubled.
fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Drops every table of the database (`migrate:fresh`).
async fn drop_all_tables(conn: &DatabaseConnection) -> Result<(), DbErr> {
    match conn.get_database_backend() {
        DbBackend::Sqlite => drop_all_sqlite_tables(conn).await,
        DbBackend::Postgres => {
            let tables: Vec<String> = conn
                .query_all_raw(Statement::from_string(
                    DbBackend::Postgres,
                    "SELECT tablename AS name FROM pg_tables WHERE schemaname = current_schema()",
                ))
                .await?
                .iter()
                .map(|row| row.try_get::<String>("", "name"))
                .collect::<Result<_, _>>()?;
            let tx = conn.begin().await?;
            for table in &tables {
                tx.execute_unprepared(&format!("DROP TABLE IF EXISTS {} CASCADE", quote(table)))
                    .await?;
            }
            tx.commit().await
        }
        DbBackend::MySql => drop_all_mysql_tables(conn).await,
        other => Err(DbErr::BackendNotSupported {
            db: other.as_str(),
            ctx: "migrate:fresh",
        }),
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
async fn drop_all_mysql_tables(conn: &DatabaseConnection) -> Result<(), DbErr> {
    let tables: Vec<String> = conn
        .query_all_raw(Statement::from_string(DbBackend::MySql, mysql::LIST_TABLES))
        .await?
        .iter()
        .map(|row| row.try_get::<String>("", "name"))
        .collect::<Result<_, _>>()?;
    if tables.is_empty() {
        return Ok(());
    }
    let tx = conn.begin().await?;
    tx.execute_unprepared(mysql::FOREIGN_KEY_CHECKS_OFF).await?;
    let dropped = tx.execute_unprepared(&mysql::drop_tables(&tables)).await;
    let restored = tx.execute_unprepared(mysql::FOREIGN_KEY_CHECKS_ON).await;
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

/// SQLite's half of [`drop_all_tables`], on one connection taken out of the
/// pool with `PRAGMA foreign_keys = OFF`.
///
/// With foreign keys on, `DROP TABLE` first runs an implicit `DELETE`, and
/// its `ON DELETE CASCADE` actions fail once a table they touch references
/// a table already dropped ("no such table: main.roles" for navree-access's
/// `role_permissions`), whatever `defer_foreign_keys` says. The pragma has
/// no effect inside a transaction, so it is set before `BEGIN`, and the
/// connection is closed afterwards (also on error) rather than going back
/// to the pool with foreign keys off.
async fn drop_all_sqlite_tables(conn: &DatabaseConnection) -> Result<(), DbErr> {
    use sea_orm::sqlx::{self, AssertSqlSafe, Connection as _};
    use sea_orm::{DatabaseConnectionType, RuntimeErr};

    let DatabaseConnectionType::SqlxSqlitePoolConnection(_) = &conn.inner else {
        return Err(DbErr::BackendNotSupported {
            db: "SQLite without an SQLx pool",
            ctx: "migrate:fresh",
        });
    };
    let exec = |error: sqlx::Error| DbErr::Exec(RuntimeErr::SqlxError(Arc::new(error)));
    let mut dedicated = conn
        .get_sqlite_connection_pool()
        .acquire()
        .await
        .map_err(|error| DbErr::Conn(RuntimeErr::SqlxError(Arc::new(error))))?;
    dedicated.close_on_drop();
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *dedicated)
        .await
        .map_err(exec)?;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&mut *dedicated)
    .await
    .map_err(|error| DbErr::Query(RuntimeErr::SqlxError(Arc::new(error))))?;
    let mut tx = dedicated.begin().await.map_err(exec)?;
    for table in &tables {
        sqlx::query(AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {}",
            quote(table)
        )))
        .execute(&mut *tx)
        .await
        .map_err(exec)?;
    }
    tx.commit().await.map_err(exec)?;
    dedicated.close().await.map_err(exec)
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
