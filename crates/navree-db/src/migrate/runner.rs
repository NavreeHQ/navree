//! The runner behind [`Migrator`](crate::Migrator): the connection
//! migrations run on, batches and steps, rollbacks, pretending, and schema
//! dumps.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use navree_config::Config;
use navree_kernel::{AppContext, BoxError};
use navree_schema::{
    Dialect, DumpOptions, MigrationContext, Schema, SchemaConnection, SchemaError, SchemaOptions,
};
use sea_orm::sea_query::{Expr, ExprTrait, Query};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, DbErr, Statement,
    TransactionTrait,
};
use sea_orm_migration::{MigrationTrait, SchemaManager};

use super::pretend::{STATEMENT_LIMIT, record_into, recording_connection};
use super::report::{
    FreshOptions, LoadedDump, MigrateOptions, MigrationReport, MigrationRun, RollbackOptions,
};
use super::{
    Applied, MIGRATIONS_TABLE, MigrationError, MigrationSet, MigrationStatus, Step, applied_rows,
    drop_all_tables, ensure_table, plan, sets_of,
};
use crate::{Db, DbOptions};

/// How long a migration connection waits for SQLite's write lock.
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

/// Whether `migration` runs in a transaction on `backend`: as it asks, else
/// everywhere DDL is transactional (PostgreSQL, SQLite; not MySQL).
pub(super) fn transactional(backend: DbBackend, migration: &dyn MigrationTrait) -> bool {
    migration
        .use_transaction()
        .unwrap_or(backend != DbBackend::MySql)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
}

/// The connection migrations run on.
struct Session {
    conn: DatabaseConnection,
    /// A SQLite connection of its own, opened with foreign keys off and
    /// closed afterwards: rebuilding a table drops it, and with foreign
    /// keys on that would run the ON DELETE actions of the tables that
    /// reference it. Each migration's rows are checked
    /// (`PRAGMA foreign_key_check`) before it commits instead.
    dedicated: bool,
}

impl Session {
    fn dialect(&self) -> Dialect {
        Dialect::of(self.conn.get_database_backend()).unwrap_or(Dialect::Sqlite)
    }

    async fn close(self) {
        if self.dedicated
            && let Err(error) = self.conn.close().await
        {
            tracing::warn!(target: "navree_db::migrate", %error, "closing the migration connection failed");
        }
    }
}

/// Runs the migrations of a set of modules against one database.
pub(crate) struct Runner {
    db: Db,
    sets: Vec<Arc<MigrationSet>>,
    options: SchemaOptions,
    url: Option<String>,
}

/// The `schema` configuration section, or its defaults when the app has
/// no configuration.
pub(crate) async fn schema_options(app: &AppContext) -> Result<SchemaOptions, BoxError> {
    match app.try_get::<Config>().await {
        Some(config) => Ok(config.typed::<SchemaOptions>()?),
        None => Ok(SchemaOptions::default()),
    }
}

impl Runner {
    /// The runner of `app`'s migrations on its [`Db`].
    pub(crate) async fn of(app: &AppContext) -> Result<Runner, BoxError> {
        let db = (*app.get::<Db>().await?).clone();
        let url = app.try_get::<DbOptions>().await.map(|o| o.url.clone());
        Ok(Runner::new(
            db,
            sets_of(app),
            schema_options(app).await?,
            url,
        ))
    }

    /// A runner for `sets` on `db`; `url` opens SQLite's migration
    /// connection.
    pub(crate) fn new(
        db: Db,
        sets: Vec<Arc<MigrationSet>>,
        options: SchemaOptions,
        url: Option<String>,
    ) -> Runner {
        Runner {
            db,
            sets,
            options,
            url,
        }
    }

    /// The database dialect.
    pub(crate) fn dialect(&self) -> Dialect {
        Dialect::of(self.db.backend()).unwrap_or(Dialect::Sqlite)
    }

    async fn session(&self) -> Result<Session, DbErr> {
        let sqlite = self.db.backend() == DbBackend::Sqlite;
        match self.url.as_deref() {
            Some(url) if sqlite && !url.contains("memory") => {
                let mut options = ConnectOptions::new(url.to_owned());
                options
                    .max_connections(1)
                    .min_connections(1)
                    .acquire_timeout(SQLITE_BUSY_TIMEOUT * 2)
                    .sqlx_logging(false)
                    .map_sqlx_sqlite_opts(|o| {
                        o.journal_mode(sea_orm::sqlx::sqlite::SqliteJournalMode::Wal)
                            .busy_timeout(SQLITE_BUSY_TIMEOUT)
                            .foreign_keys(false)
                    });
                Ok(Session {
                    conn: Database::connect(options).await?,
                    dedicated: true,
                })
            }
            _ => Ok(Session {
                conn: self.db.conn().clone(),
                dedicated: false,
            }),
        }
    }

    /// The recorded migrations; with `create`, the migrations table is
    /// created first, otherwise a missing table means none.
    async fn recorded(
        &self,
        conn: &DatabaseConnection,
        create: bool,
    ) -> Result<HashMap<(String, String), Applied>, MigrationError> {
        if !create
            && !Schema::new(conn)
                .has_table(MIGRATIONS_TABLE)
                .await
                .map_err(schema_error)?
        {
            return Ok(HashMap::new());
        }
        Ok(applied_rows(conn).await?)
    }

    /// Creates the migrations table (`migrate:install`).
    pub(crate) async fn install(&self) -> Result<(), MigrationError> {
        Ok(ensure_table(self.db.conn()).await?)
    }

    /// Runs the pending migrations as one batch (or one batch each with
    /// `step`), after loading the schema dump into an empty database.
    pub(crate) async fn up(
        &self,
        options: &MigrateOptions,
    ) -> Result<MigrationReport, MigrationError> {
        let steps = plan(&self.sets)?;
        let session = self.session().await?;
        let result = self.up_on(&session, &steps, options).await;
        session.close().await;
        result
    }

    async fn up_on(
        &self,
        session: &Session,
        steps: &[Step<'_>],
        options: &MigrateOptions,
    ) -> Result<MigrationReport, MigrationError> {
        let mut report = MigrationReport::default();
        if !options.pretend {
            report.loaded_dump = self
                .load_dump_if_empty(session, options.schema_path.as_deref())
                .await?;
        }
        let applied = self.recorded(&session.conn, !options.pretend).await?;
        let mut batch = applied.values().map(|a| a.batch).max().unwrap_or(0) + 1;
        for step in steps {
            if applied.contains_key(&(step.module.to_owned(), step.name().to_owned())) {
                continue;
            }
            tracing::info!(target: "navree_db::migrate", migration = %step.label(), batch, pretend = options.pretend, "migrating");
            let run = if options.pretend {
                self.pretend(session, step, Direction::Up, batch).await?
            } else {
                self.apply(session, step, Direction::Up, batch).await?
            };
            report.migrations.push(run);
            if options.step && !options.pretend {
                batch += 1;
            }
        }
        Ok(report)
    }

    /// Reverts the last batch, the last `step` migrations, or batch
    /// `batch`, newest first.
    pub(crate) async fn rollback(
        &self,
        options: &RollbackOptions,
    ) -> Result<MigrationReport, MigrationError> {
        let steps = plan(&self.sets)?;
        let session = self.session().await?;
        let result = async {
            let applied = self.recorded(&session.conn, false).await?;
            let order = newest_first(&steps, &applied);
            let chosen: Vec<&((String, String), i64)> = match (options.step, options.batch) {
                (Some(n), _) => order.iter().take(n as usize).collect(),
                (None, Some(batch)) => order.iter().filter(|(_, b)| *b == batch).collect(),
                (None, None) => {
                    let last = order.first().map(|(_, b)| *b);
                    order.iter().filter(|(_, b)| Some(*b) == last).collect()
                }
            };
            self.revert(&session, &steps, &chosen, options.pretend)
                .await
        }
        .await;
        session.close().await;
        result
    }

    /// Reverts every recorded migration, newest first.
    pub(crate) async fn reset(&self, pretend: bool) -> Result<MigrationReport, MigrationError> {
        let steps = plan(&self.sets)?;
        let session = self.session().await?;
        let result = async {
            let applied = self.recorded(&session.conn, false).await?;
            let order = newest_first(&steps, &applied);
            let chosen: Vec<&((String, String), i64)> = order.iter().collect();
            self.revert(&session, &steps, &chosen, pretend).await
        }
        .await;
        session.close().await;
        result
    }

    async fn revert(
        &self,
        session: &Session,
        steps: &[Step<'_>],
        chosen: &[&((String, String), i64)],
        pretend: bool,
    ) -> Result<MigrationReport, MigrationError> {
        let by_key: HashMap<(&str, &str), &Step<'_>> = steps
            .iter()
            .map(|step| ((step.module, step.name()), step))
            .collect();
        let mut report = MigrationReport::default();
        let mut unknown = None;
        for ((module, name), batch) in chosen {
            let Some(step) = by_key.get(&(module.as_str(), name.as_str())) else {
                unknown.get_or_insert_with(|| format!("{module}/{name}"));
                continue;
            };
            tracing::info!(target: "navree_db::migrate", migration = %step.label(), batch, pretend, "rolling back");
            let run = if pretend {
                self.pretend(session, step, Direction::Down, *batch).await?
            } else {
                self.apply(session, step, Direction::Down, *batch).await?
            };
            report.migrations.push(run);
        }
        match unknown {
            Some(migration) => Err(MigrationError::Unknown { migration }),
            None => Ok(report),
        }
    }

    /// Drops every table (and views and types when asked), then migrates.
    pub(crate) async fn fresh(
        &self,
        options: &FreshOptions,
    ) -> Result<MigrationReport, MigrationError> {
        plan(&self.sets)?;
        let dialect = self.dialect();
        if options.drop_types && dialect != Dialect::Postgres {
            return Err(MigrationError::Unsupported {
                message: format!("--drop-types drops PostgreSQL types; this database is {dialect}"),
            });
        }
        drop_all_tables(self.db.conn()).await?;
        let schema = Schema::new(self.db.conn());
        if options.drop_views {
            schema.drop_all_views().await.map_err(schema_error)?;
        }
        if options.drop_types {
            schema.drop_all_types().await.map_err(schema_error)?;
        }
        self.up(&options.migrate).await
    }

    /// Every registered migration in run order, with its state.
    pub(crate) async fn status(&self) -> Result<Vec<MigrationStatus>, MigrationError> {
        let steps = plan(&self.sets)?;
        let applied = self.recorded(self.db.conn(), true).await?;
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

    /// Writes the schema dump, with the migrations table's rows, to `path`
    /// (or `{schema.dump_path}/{driver}-schema.sql`); returns where.
    pub(crate) async fn dump(&self, path: Option<&Path>) -> Result<PathBuf, MigrationError> {
        let path = path.map_or_else(|| self.options.dump_file(self.dialect()), Path::to_path_buf);
        let script = Schema::new(self.db.conn())
            .with_options(self.options.clone())
            .dump(&DumpOptions::default().with_data(MIGRATIONS_TABLE))
            .await
            .map_err(|source| MigrationError::Dump {
                path: path.clone(),
                source: Box::new(source),
            })?;
        let io = |error: std::io::Error| MigrationError::Dump {
            path: path.clone(),
            source: Box::new(SchemaError::Dump {
                message: error.to_string(),
            }),
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&path, script).map_err(io)?;
        Ok(path)
    }

    /// Loads the schema dump when the database is empty (no tables, or only
    /// an empty migrations table) and the dump exists; an explicit `path`
    /// must exist.
    async fn load_dump_if_empty(
        &self,
        session: &Session,
        path: Option<&Path>,
    ) -> Result<Option<LoadedDump>, MigrationError> {
        let explicit = path.is_some();
        let path = path.map_or_else(
            || self.options.dump_file(session.dialect()),
            Path::to_path_buf,
        );
        let dump_error = |source: SchemaError| MigrationError::Dump {
            path: path.clone(),
            source: Box::new(source),
        };
        if !path.is_file() {
            if explicit {
                return Err(dump_error(SchemaError::Dump {
                    message: "there is no file at this path".to_owned(),
                }));
            }
            return Ok(None);
        }
        let schema = Schema::new(&session.conn);
        let tables = schema.table_listing().await.map_err(schema_error)?;
        let only_migrations = tables.len() == 1 && tables[0] == MIGRATIONS_TABLE;
        if !(tables.is_empty() || only_migrations) {
            return Ok(None);
        }
        if only_migrations {
            if !self.recorded(&session.conn, false).await?.is_empty() {
                return Ok(None);
            }
            schema.drop(MIGRATIONS_TABLE).await.map_err(schema_error)?;
        }
        let script = std::fs::read_to_string(&path).map_err(|error| {
            dump_error(SchemaError::Dump {
                message: error.to_string(),
            })
        })?;
        tracing::info!(target: "navree_db::migrate", path = %path.display(), "loading the schema dump");
        let tx = session.conn.begin().await?;
        match Schema::new(&tx).load(&script).await {
            Ok(statements) => {
                tx.commit().await?;
                Ok(Some(LoadedDump { path, statements }))
            }
            Err(error) => {
                if let Err(rollback) = tx.rollback().await {
                    tracing::warn!(target: "navree_db::migrate", error = %rollback, "rollback after a failed dump load failed");
                }
                Err(dump_error(error))
            }
        }
    }

    /// Runs one migration step for real and records (or removes) its row,
    /// atomically when the step is transactional.
    async fn apply(
        &self,
        session: &Session,
        step: &Step<'_>,
        direction: Direction,
        batch: i64,
    ) -> Result<MigrationRun, MigrationError> {
        let failed = |source| MigrationError::Failed {
            migration: step.label(),
            source,
        };
        let context = MigrationContext::new(step.label(), self.options.clone());
        let started = Instant::now();
        let check = session.dedicated;
        if transactional(session.conn.get_database_backend(), step.migration) {
            let tx = session.conn.begin().await?;
            let result = context
                .clone()
                .scope(async {
                    let manager = SchemaManager::new(&tx);
                    match direction {
                        Direction::Up => step.migration.up(&manager).await?,
                        Direction::Down => step.migration.down(&manager).await?,
                    }
                    if check {
                        foreign_key_check(&tx).await?;
                    }
                    bookkeeping(&tx, step, direction, batch).await
                })
                .await;
            match result {
                Ok(()) => tx.commit().await.map_err(failed)?,
                Err(error) => {
                    if let Err(rollback) = tx.rollback().await {
                        tracing::warn!(target: "navree_db::migrate", migration = %step.label(), error = %rollback, "rollback failed");
                    }
                    return Err(failed(error));
                }
            }
        } else {
            context
                .clone()
                .scope(async {
                    let manager = SchemaManager::new(&session.conn);
                    match direction {
                        Direction::Up => step.migration.up(&manager).await,
                        Direction::Down => step.migration.down(&manager).await,
                    }
                })
                .await
                .map_err(failed)?;
            if check {
                foreign_key_check(&session.conn).await.map_err(failed)?;
            }
            bookkeeping(&session.conn, step, direction, batch)
                .await
                .map_err(failed)?;
        }
        Ok(run_of(step, batch, &context, started.elapsed()))
    }

    /// Runs one migration step against a connection that records instead
    /// of running; schema migrations read the database through the
    /// session.
    async fn pretend(
        &self,
        session: &Session,
        step: &Step<'_>,
        direction: Direction,
        batch: i64,
    ) -> Result<MigrationRun, MigrationError> {
        let failed = |source| MigrationError::Failed {
            migration: step.label(),
            source,
        };
        let reader: Arc<dyn SchemaConnection> = Arc::new(session.conn.clone());
        let context = MigrationContext::pretending(step.label(), self.options.clone(), reader);
        let recording = recording_connection(session.conn.get_database_backend());
        let started = Instant::now();
        let result = context
            .clone()
            .scope(async {
                let manager = SchemaManager::new(&recording);
                match direction {
                    Direction::Up => step.migration.up(&manager).await,
                    Direction::Down => step.migration.down(&manager).await,
                }
            })
            .await;
        record_into(recording, &context);
        result.map_err(|error| {
            let message = error.to_string();
            failed(if message.contains("`exec_results` buffer is empty") {
                DbErr::Migration(format!(
                    "it runs more than {STATEMENT_LIMIT} statements, more than pretending keeps"
                ))
            } else if message.contains("`query_results` buffer is empty") {
                DbErr::Migration(
                    "it reads the database, which a SeaORM migration cannot do while the migrator pretends (write it with the schema builder, which reads the real database when pretending)"
                        .to_owned(),
                )
            } else {
                error
            })
        })?;
        Ok(run_of(step, batch, &context, started.elapsed()))
    }
}

fn run_of(
    step: &Step<'_>,
    batch: i64,
    context: &MigrationContext,
    duration: Duration,
) -> MigrationRun {
    MigrationRun {
        module: step.module.to_owned(),
        name: step.name().to_owned(),
        batch,
        statements: context.statements(),
        notices: context.notices(),
        duration,
    }
}

/// The recorded migrations, newest first: by batch, then by run order (a
/// migration no module registers sorts first in its batch).
fn newest_first(
    steps: &[Step<'_>],
    applied: &HashMap<(String, String), Applied>,
) -> Vec<((String, String), i64)> {
    let position: HashMap<(&str, &str), usize> = steps
        .iter()
        .enumerate()
        .map(|(i, step)| ((step.module, step.name()), i))
        .collect();
    let mut order: Vec<((String, String), i64)> = applied
        .iter()
        .map(|(key, applied)| (key.clone(), applied.batch))
        .collect();
    order.sort_by(|(a, batch_a), (b, batch_b)| {
        let at = |key: &(String, String)| {
            position
                .get(&(key.0.as_str(), key.1.as_str()))
                .copied()
                .unwrap_or(usize::MAX)
        };
        batch_b
            .cmp(batch_a)
            .then_with(|| at(b).cmp(&at(a)))
            .then_with(|| b.cmp(a))
    });
    order
}

/// Records (up) or removes (down) the migration's row.
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
                        chrono::Utc::now().into(),
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

/// Fails when rows reference rows that do not exist (SQLite migrations run
/// with foreign keys off).
async fn foreign_key_check<C: ConnectionTrait>(conn: &C) -> Result<(), DbErr> {
    let rows = conn
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_key_check",
        ))
        .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let mut seen = HashSet::new();
    let mut problems = Vec::new();
    for row in &rows {
        let table: String = row.try_get_by_index(0).unwrap_or_default();
        let rowid: Option<i64> = row.try_get_by_index(1).ok().flatten();
        let parent: String = row.try_get_by_index(2).unwrap_or_default();
        let problem = match rowid {
            Some(rowid) => format!("`{table}` row {rowid} references `{parent}`"),
            None => format!("a `{table}` row references `{parent}`"),
        };
        if seen.insert(problem.clone()) && problems.len() < 5 {
            problems.push(problem);
        }
    }
    let more = rows.len().saturating_sub(problems.len());
    Err(DbErr::Migration(format!(
        "the migration leaves {} row{} whose foreign key points nowhere: {}{}",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        problems.join("; "),
        if more > 0 {
            format!("; and {more} more")
        } else {
            String::new()
        }
    )))
}

fn schema_error(error: SchemaError) -> MigrationError {
    MigrationError::Db(DbErr::from(error))
}
