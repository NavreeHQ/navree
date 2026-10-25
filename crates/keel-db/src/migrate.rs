//! Module-owned migrations: [`MigrationSet`], [`Migrator`] and [`MigrationStatus`].

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use keel_kernel::{AppContext, BoxError};
use sea_orm::sea_query::{ColumnDef, Expr, ExprTrait, Index, Query, Table};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, TransactionTrait};
use sea_orm_migration::{MigrationTrait, SchemaManager};
use serde::Serialize;

use crate::Db;

/// The table recording applied migrations.
pub const MIGRATIONS_TABLE: &str = "keel_migrations";

/// The migrations a module owns, contributed with
/// [`ModuleBuilderDbExt::migrations`](crate::ModuleBuilderDbExt::migrations).
///
/// Sets run in dependency order: a set runs after every set named in
/// [`after`](MigrationSet::after) (names of modules not in the app are
/// ignored), otherwise in module-name order. Inside a set, migrations run
/// in name order, so name them `m20261025_000001_create_users` and so on.
///
/// Each migration runs in its own transaction (DDL included) unless it
/// returns `Some(false)` from `use_transaction` (for statements such as
/// `CREATE INDEX CONCURRENTLY`, or SQLite table rebuilds that switch
/// `PRAGMA foreign_keys`, which has no effect inside a transaction); the
/// `keel_migrations` row is written in the same transaction.
///
/// ```
/// use keel_db::MigrationSet;
///
/// let set = MigrationSet::new("crm").after(["users"]);
/// assert_eq!(set.module, "crm");
/// ```
pub struct MigrationSet {
    /// The name recorded in `keel_migrations.module` and used in `after`
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
    /// `keel_migrations` records a migration no module registers any more.
    #[error("migration `{migration}` is recorded as applied but no module registers it")]
    Unknown {
        /// `module/name`.
        migration: String,
    },
    /// A database error outside a migration.
    #[error(transparent)]
    Db(#[from] DbErr),
}

/// Runs the migrations every module contributed, against the app's [`Db`].
///
/// Applied migrations are recorded in `keel_migrations(module, name, batch,
/// applied_at)`; each [`up`](Migrator::up) run that applies anything is one
/// batch, and [`rollback`](Migrator::rollback) reverts the last batch.
#[derive(Debug)]
pub struct Migrator;

impl Migrator {
    /// Applies every pending migration (one new batch); returns
    /// `module/name` of each, in the order they ran.
    pub async fn up(ctx: &AppContext) -> Result<Vec<String>, BoxError> {
        let db = ctx.get::<Db>().await?;
        Ok(run_up(&db, &sets_of(ctx)).await?)
    }

    /// Reverts the last batch, newest migration first; returns `module/name`
    /// of each.
    pub async fn rollback(ctx: &AppContext) -> Result<Vec<String>, BoxError> {
        let db = ctx.get::<Db>().await?;
        Ok(run_rollback(&db, &sets_of(ctx)).await?)
    }

    /// Drops every table, then applies every migration as batch 1.
    pub async fn fresh(ctx: &AppContext) -> Result<Vec<String>, BoxError> {
        let db = ctx.get::<Db>().await?;
        let sets = sets_of(ctx);
        plan(&sets)?;
        drop_all_tables(db.conn()).await?;
        Ok(run_up(&db, &sets).await?)
    }

    /// Every registered migration in run order, with its state.
    pub async fn status(ctx: &AppContext) -> Result<Vec<MigrationStatus>, BoxError> {
        let db = ctx.get::<Db>().await?;
        Ok(run_status(&db, &sets_of(ctx)).await?)
    }
}

/// The migration sets contributed by the modules of `ctx`.
pub(crate) fn sets_of(ctx: &AppContext) -> Vec<Arc<MigrationSet>> {
    ctx.contributions::<MigrationSet>()
        .into_iter()
        .map(|c| c.value)
        .collect()
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

/// A `keel_migrations` row.
struct Applied {
    batch: i64,
    applied_at: Option<DateTime<Utc>>,
}

async fn ensure_table(conn: &DatabaseConnection) -> Result<(), DbErr> {
    conn.execute(
        Table::create()
            .table(MIGRATIONS_TABLE)
            .if_not_exists()
            .col(ColumnDef::new("module").string().not_null())
            .col(ColumnDef::new("name").string().not_null())
            .col(ColumnDef::new("batch").big_integer().not_null())
            .col(
                ColumnDef::new("applied_at")
                    .timestamp_with_time_zone()
                    .not_null(),
            )
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

/// Whether `migration` runs in a transaction on `backend`: as it asks, else
/// everywhere DDL is transactional (PostgreSQL, SQLite; not MySQL).
fn transactional(backend: DbBackend, migration: &dyn MigrationTrait) -> bool {
    migration
        .use_transaction()
        .unwrap_or(backend != DbBackend::MySql)
}

#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
}

/// Runs one migration step and records (or removes) its row, atomically
/// when the step is transactional.
async fn run_step(
    conn: &DatabaseConnection,
    step: &Step<'_>,
    direction: Direction,
    batch: i64,
) -> Result<(), MigrationError> {
    let failed = |source| MigrationError::Failed {
        migration: step.label(),
        source,
    };
    if transactional(conn.get_database_backend(), step.migration) {
        let tx = conn.begin().await?;
        let result = async {
            let manager = SchemaManager::new(&tx);
            match direction {
                Direction::Up => step.migration.up(&manager).await?,
                Direction::Down => step.migration.down(&manager).await?,
            }
            bookkeeping(&tx, step, direction, batch).await
        }
        .await;
        match result {
            Ok(()) => tx.commit().await.map_err(failed),
            Err(error) => {
                if let Err(rollback) = tx.rollback().await {
                    tracing::warn!(target: "keel_db::migrate", migration = %step.label(), error = %rollback, "rollback failed");
                }
                Err(failed(error))
            }
        }
    } else {
        let manager = SchemaManager::new(conn);
        match direction {
            Direction::Up => step.migration.up(&manager).await,
            Direction::Down => step.migration.down(&manager).await,
        }
        .map_err(failed)?;
        bookkeeping(conn, step, direction, batch)
            .await
            .map_err(failed)
    }
}

async fn bookkeeping<C: ConnectionTrait>(
    conn: &C,
    step: &Step<'_>,
    direction: Direction,
    batch: i64,
) -> Result<(), DbErr> {
    match direction {
        Direction::Up => {
            conn.execute(
                Query::insert()
                    .into_table(MIGRATIONS_TABLE)
                    .columns(["module", "name", "batch", "applied_at"])
                    .values([
                        step.module.into(),
                        step.name().into(),
                        batch.into(),
                        Utc::now().into(),
                    ])
                    .map_err(|e| DbErr::Custom(e.to_string()))?,
            )
            .await?;
        }
        Direction::Down => {
            conn.execute(
                Query::delete()
                    .from_table(MIGRATIONS_TABLE)
                    .and_where(Expr::col("module").eq(step.module))
                    .and_where(Expr::col("name").eq(step.name())),
            )
            .await?;
        }
    }
    Ok(())
}

pub(crate) async fn run_up(
    db: &Db,
    sets: &[Arc<MigrationSet>],
) -> Result<Vec<String>, MigrationError> {
    let steps = plan(sets)?;
    let conn = db.conn();
    let applied = applied_rows(conn).await?;
    let batch = applied.values().map(|a| a.batch).max().unwrap_or(0) + 1;
    let mut ran = Vec::new();
    for step in &steps {
        if applied.contains_key(&(step.module.to_owned(), step.name().to_owned())) {
            continue;
        }
        tracing::info!(target: "keel_db::migrate", migration = %step.label(), batch, "migrating");
        run_step(conn, step, Direction::Up, batch).await?;
        ran.push(step.label());
    }
    Ok(ran)
}

async fn run_rollback(db: &Db, sets: &[Arc<MigrationSet>]) -> Result<Vec<String>, MigrationError> {
    let steps = plan(sets)?;
    let conn = db.conn();
    let applied = applied_rows(conn).await?;
    let Some(last) = applied.values().map(|a| a.batch).max() else {
        return Ok(Vec::new());
    };
    let mut in_batch: HashSet<(String, String)> = applied
        .iter()
        .filter(|(_, a)| a.batch == last)
        .map(|(key, _)| key.clone())
        .collect();
    let mut reverted = Vec::new();
    for step in steps.iter().rev() {
        if !in_batch.remove(&(step.module.to_owned(), step.name().to_owned())) {
            continue;
        }
        tracing::info!(target: "keel_db::migrate", migration = %step.label(), batch = last, "rolling back");
        run_step(conn, step, Direction::Down, last).await?;
        reverted.push(step.label());
    }
    if let Some((module, name)) = in_batch.into_iter().next() {
        return Err(MigrationError::Unknown {
            migration: format!("{module}/{name}"),
        });
    }
    Ok(reverted)
}

async fn run_status(
    db: &Db,
    sets: &[Arc<MigrationSet>],
) -> Result<Vec<MigrationStatus>, MigrationError> {
    let steps = plan(sets)?;
    let applied = applied_rows(db.conn()).await?;
    Ok(steps
        .iter()
        .map(|step| {
            let row = applied.get(&(step.module.to_owned(), step.name().to_owned()));
            MigrationStatus {
                module: step.module.to_owned(),
                name: step.name().to_owned(),
                applied: row.is_some(),
                batch: row.map(|r| r.batch),
                applied_at: row.and_then(|r| r.applied_at),
            }
        })
        .collect())
}

/// `"name"` with embedded quotes doubled.
fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Drops every table of the database (`migrate:fresh`).
async fn drop_all_tables(conn: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = conn.get_database_backend();
    let (list, drop_suffix) = match backend {
        DbBackend::Sqlite => (
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            "",
        ),
        DbBackend::Postgres => (
            "SELECT tablename AS name FROM pg_tables WHERE schemaname = current_schema()",
            " CASCADE",
        ),
        _ => {
            return Err(DbErr::BackendNotSupported {
                db: "MySQL",
                ctx: "migrate:fresh",
            });
        }
    };
    let tables: Vec<String> = conn
        .query_all_raw(Statement::from_string(backend, list))
        .await?
        .iter()
        .map(|row| row.try_get::<String>("", "name"))
        .collect::<Result<_, _>>()?;
    let tx = conn.begin().await?;
    if backend == DbBackend::Sqlite {
        // Foreign keys are checked at commit, when every table is gone.
        tx.execute_unprepared("PRAGMA defer_foreign_keys = ON")
            .await?;
    }
    for table in &tables {
        tx.execute_unprepared(&format!(
            "DROP TABLE IF EXISTS {}{drop_suffix}",
            quote(table)
        ))
        .await?;
    }
    tx.commit().await
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
