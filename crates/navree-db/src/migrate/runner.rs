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
use sea_orm_migration::SchemaManager;

use super::conn::{Conn, Tx};
use super::pretend::{STATEMENT_LIMIT, record_into, recording_connection};
use super::report::{
    FreshOptions, LoadedDump, MigrateOptions, MigrationReport, MigrationRun, RollbackOptions,
};
use super::{
    Applied, MIGRATIONS_TABLE, MigrationError, MigrationSet, MigrationStatus, SetMigration, Step,
    applied_rows, delete_sql, drop_all_tables, ensure_table, insert_sql, plan, sets_of,
};
use crate::{Backend, Db, DbError, DbOptions, Executor, SqliteOptions, Statement, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
}

/// Runs the migrations of a set of modules against one database.
pub(crate) struct Runner {
    db: Db,
    sets: Vec<Arc<MigrationSet>>,
    options: SchemaOptions,
    url: Option<String>,
    /// `database.sqlite`, for SQLite's migration connection.
    sqlite: SqliteOptions,
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
    /// The runner of `app`'s migrations on its [`Db`]'s direct connection
    /// (the `Db` itself without `direct_url`).
    pub(crate) async fn of(app: &AppContext) -> Result<Runner, BoxError> {
        let db = app.get::<Db>().await?.direct().await?;
        let options = app.try_get::<DbOptions>().await;
        let url = options
            .as_ref()
            .map(|o| o.direct_url.clone().unwrap_or_else(|| o.url.clone()));
        let sqlite = options.map(|o| o.sqlite.clone()).unwrap_or_default();
        Ok(Runner::new(
            db,
            sets_of(app),
            schema_options(app).await?,
            url,
            sqlite,
        ))
    }

    /// A runner for `sets` on `db`; `url` and `sqlite` open SQLite's
    /// migration connection.
    pub(crate) fn new(
        db: Db,
        sets: Vec<Arc<MigrationSet>>,
        options: SchemaOptions,
        url: Option<String>,
        sqlite: SqliteOptions,
    ) -> Runner {
        Runner {
            db,
            sets,
            options,
            url,
            sqlite,
        }
    }

    /// The database's connections.
    fn conn(&self) -> Conn {
        Conn::of(&self.db)
    }

    /// The connection migrations run on: on SQLite (a file), one of their
    /// own with foreign keys off; elsewhere the database's.
    async fn session(&self) -> Result<Conn, DbError> {
        let conn = self.conn();
        match self.url.as_deref() {
            Some(url) if conn.dialect() == Dialect::Sqlite && !url.contains("memory") => {
                Conn::sqlite(url, &self.sqlite).await
            }
            _ => Ok(conn),
        }
    }

    /// The recorded migrations; with `create`, the migrations table is
    /// created first, otherwise a missing table means none.
    async fn recorded(
        &self,
        conn: &Conn,
        create: bool,
    ) -> Result<HashMap<(String, String), Applied>, MigrationError> {
        if !create && !Schema::new(conn).has_table(MIGRATIONS_TABLE).await? {
            return Ok(HashMap::new());
        }
        Ok(applied_rows(conn).await?)
    }

    /// Creates the migrations table (`migrate:install`).
    pub(crate) async fn install(&self) -> Result<(), MigrationError> {
        Ok(ensure_table(&self.conn()).await?)
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
        session: &Conn,
        steps: &[Step<'_>],
        options: &MigrateOptions,
    ) -> Result<MigrationReport, MigrationError> {
        let mut report = MigrationReport::default();
        if !options.pretend {
            report.loaded_dump = self
                .load_dump_if_empty(session, options.schema_path.as_deref())
                .await?;
        }
        let applied = self.recorded(session, !options.pretend).await?;
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
            let applied = self.recorded(&session, false).await?;
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
            let applied = self.recorded(&session, false).await?;
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
        session: &Conn,
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
        let conn = self.conn();
        let dialect = conn.dialect();
        if options.drop_types && dialect != Dialect::Postgres {
            return Err(MigrationError::Unsupported {
                message: format!("--drop-types drops PostgreSQL types; this database is {dialect}"),
            });
        }
        drop_all_tables(&conn).await?;
        let schema = Schema::new(&conn);
        if options.drop_views {
            schema.drop_all_views().await?;
        }
        if options.drop_types {
            schema.drop_all_types().await?;
        }
        self.up(&options.migrate).await
    }

    /// Every registered migration in run order, with its state.
    pub(crate) async fn status(&self) -> Result<Vec<MigrationStatus>, MigrationError> {
        let steps = plan(&self.sets)?;
        let applied = self.recorded(&self.conn(), true).await?;
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
        let conn = self.conn();
        let path = path.map_or_else(|| self.options.dump_file(conn.dialect()), Path::to_path_buf);
        let script = Schema::new(&conn)
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
        session: &Conn,
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
        let schema = Schema::new(session);
        let tables = schema.table_listing().await?;
        let only_migrations = tables.len() == 1 && tables[0] == MIGRATIONS_TABLE;
        if !(tables.is_empty() || only_migrations) {
            return Ok(None);
        }
        if only_migrations {
            if !self.recorded(session, false).await?.is_empty() {
                return Ok(None);
            }
            schema.drop(MIGRATIONS_TABLE).await?;
        }
        let script = std::fs::read_to_string(&path).map_err(|error| {
            dump_error(SchemaError::Dump {
                message: error.to_string(),
            })
        })?;
        tracing::info!(target: "navree_db::migrate", path = %path.display(), "loading the schema dump");
        let tx = session.begin().await?;
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
        session: &Conn,
        step: &Step<'_>,
        direction: Direction,
        batch: i64,
    ) -> Result<MigrationRun, MigrationError> {
        let failed = |source: BoxError| MigrationError::Failed {
            migration: step.label(),
            source,
        };
        let context = MigrationContext::new(step.label(), self.options.clone());
        let started = Instant::now();
        let check = session.is_dedicated();
        if step.transactional(session.dialect()) {
            let tx = match step.body {
                SetMigration::Schema(_) => session.begin().await,
                SetMigration::SeaOrm(_) => session.begin_sea().await,
            }
            .map_err(|error| failed(error.into()))?;
            let result = context
                .clone()
                .scope(async {
                    run_in_transaction(step, direction, &tx).await?;
                    if check {
                        foreign_key_check(&tx).await?;
                    }
                    bookkeeping(&tx, step, direction, batch).await?;
                    Ok::<(), BoxError>(())
                })
                .await;
            match result {
                Ok(()) => tx.commit().await.map_err(|error| failed(error.into()))?,
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
                .scope(run_on_connection(step, direction, session))
                .await
                .map_err(failed)?;
            if check {
                foreign_key_check(session).await.map_err(failed)?;
            }
            bookkeeping(session, step, direction, batch)
                .await
                .map_err(|error| failed(error.into()))?;
        }
        Ok(run_of(step, batch, &context, started.elapsed()))
    }

    /// Runs one migration step without changing anything and records what
    /// it would run: a schema-builder migration reads the real database
    /// through the session and records its statements
    /// ([`MigrationContext::pretending`]); a SeaORM migration runs against
    /// a connection that keeps its statements and answers no reads.
    async fn pretend(
        &self,
        session: &Conn,
        step: &Step<'_>,
        direction: Direction,
        batch: i64,
    ) -> Result<MigrationRun, MigrationError> {
        let failed = |source: BoxError| MigrationError::Failed {
            migration: step.label(),
            source,
        };
        let reader: Arc<dyn SchemaConnection> = Arc::new(session.clone());
        let context = MigrationContext::pretending(step.label(), self.options.clone(), reader);
        let started = Instant::now();
        match step.body {
            SetMigration::Schema(migration) => {
                context
                    .clone()
                    .scope(async {
                        let schema = Schema::new(session);
                        match direction {
                            Direction::Up => migration.up(&schema).await,
                            Direction::Down => migration.down(&schema).await,
                        }
                    })
                    .await
                    .map_err(|error| failed(error.into()))?;
            }
            SetMigration::SeaOrm(migration) => {
                let recording = recording_connection(session.sea().get_database_backend());
                let result = context
                    .clone()
                    .scope(async {
                        let manager = SchemaManager::new(&recording);
                        match direction {
                            Direction::Up => migration.up(&manager).await,
                            Direction::Down => migration.down(&manager).await,
                        }
                    })
                    .await;
                record_into(recording, &context);
                result.map_err(|error| {
                    let message = error.to_string();
                    failed(if message.contains("`exec_results` buffer is empty") {
                        format!(
                            "it runs more than {STATEMENT_LIMIT} statements, more than pretending keeps"
                        )
                        .into()
                    } else if message.contains("`query_results` buffer is empty") {
                        "it reads the database, which a SeaORM migration cannot do while the migrator pretends (write it with the schema builder, which reads the real database when pretending)"
                            .into()
                    } else {
                        error.into()
                    })
                })?;
            }
        }
        Ok(run_of(step, batch, &context, started.elapsed()))
    }
}

/// Runs a transactional step in `tx`: a schema-builder migration on
/// navree-db's transaction, a SeaORM migration on SeaORM's.
async fn run_in_transaction(
    step: &Step<'_>,
    direction: Direction,
    tx: &Tx,
) -> Result<(), BoxError> {
    match step.body {
        SetMigration::Schema(migration) => {
            let schema = Schema::new(tx);
            match direction {
                Direction::Up => migration.up(&schema).await?,
                Direction::Down => migration.down(&schema).await?,
            }
        }
        SetMigration::SeaOrm(migration) => {
            let manager = match tx.sea() {
                Some(sea) => SchemaManager::new(sea),
                None => return Err("a SeaORM migration needs a SeaORM transaction".into()),
            };
            match direction {
                Direction::Up => migration.up(&manager).await?,
                Direction::Down => migration.down(&manager).await?,
            }
        }
    }
    Ok(())
}

/// Runs a step outside a transaction, on the session's connection.
async fn run_on_connection(
    step: &Step<'_>,
    direction: Direction,
    session: &Conn,
) -> Result<(), BoxError> {
    match step.body {
        SetMigration::Schema(migration) => {
            let schema = Schema::new(session);
            match direction {
                Direction::Up => migration.up(&schema).await?,
                Direction::Down => migration.down(&schema).await?,
            }
        }
        SetMigration::SeaOrm(migration) => {
            let manager = SchemaManager::new(session.sea());
            match direction {
                Direction::Up => migration.up(&manager).await?,
                Direction::Down => migration.down(&manager).await?,
            }
        }
    }
    Ok(())
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
async fn bookkeeping(
    conn: &dyn Executor,
    step: &Step<'_>,
    direction: Direction,
    batch: i64,
) -> Result<(), DbError> {
    let backend = conn.backend();
    let statement = match direction {
        Direction::Up => Statement::new(
            backend,
            insert_sql(backend),
            [
                Value::from(step.module),
                Value::from(step.name()),
                Value::I64(batch),
                Value::from(chrono::Utc::now()),
            ],
        ),
        Direction::Down => Statement::new(
            backend,
            delete_sql(backend),
            [Value::from(step.module), Value::from(step.name())],
        ),
    };
    conn.execute(&statement).await.map(drop)
}

/// Fails when rows reference rows that do not exist (SQLite migrations run
/// with foreign keys off).
async fn foreign_key_check(conn: &dyn Executor) -> Result<(), BoxError> {
    let rows = conn
        .fetch_all(&Statement::new(
            Backend::Sqlite,
            "PRAGMA foreign_key_check",
            [],
        ))
        .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let mut seen = HashSet::new();
    let mut problems = Vec::new();
    for row in &rows {
        let table: String = row.get(0).unwrap_or_default();
        let rowid: Option<i64> = row.get(1).ok().flatten();
        let parent: String = row.get(2).unwrap_or_default();
        let problem = match rowid {
            Some(rowid) => format!("`{table}` row {rowid} references `{parent}`"),
            None => format!("a `{table}` row references `{parent}`"),
        };
        if seen.insert(problem.clone()) && problems.len() < 5 {
            problems.push(problem);
        }
    }
    let more = rows.len().saturating_sub(problems.len());
    Err(format!(
        "the migration leaves {} row{} whose foreign key points nowhere: {}{}",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        problems.join("; "),
        if more > 0 {
            format!("; and {more} more")
        } else {
            String::new()
        }
    )
    .into())
}

#[cfg(test)]
mod tests {
    use navree_schema::{NamedMigration, SchemaMigration, SchemaResult};

    use super::*;

    struct Empty;

    impl navree_schema::Migration for Empty {
        async fn up(&self, _schema: &Schema<'_>) -> SchemaResult {
            Ok(())
        }

        async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
            Ok(())
        }
    }

    struct OnMySql;

    impl navree_schema::Migration for OnMySql {
        async fn up(&self, _schema: &Schema<'_>) -> SchemaResult {
            Ok(())
        }

        async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
            Ok(())
        }

        fn within_transaction_on_mysql(&self) -> bool {
            true
        }
    }

    fn applied(rows: &[(&str, i64)]) -> HashMap<(String, String), Applied> {
        rows.iter()
            .map(|(name, batch)| {
                (
                    ("crm".to_owned(), (*name).to_owned()),
                    Applied {
                        batch: *batch,
                        applied_at: None,
                    },
                )
            })
            .collect()
    }

    fn step(migration: &dyn NamedMigration) -> Step<'_> {
        Step {
            module: "crm",
            body: SetMigration::Schema(migration),
        }
    }

    #[test]
    fn rollbacks_go_newest_first_by_batch_then_run_order() {
        let (a, b, c) = (
            SchemaMigration::new("m1_a", Empty),
            SchemaMigration::new("m2_b", Empty),
            SchemaMigration::new("m3_c", Empty),
        );
        let steps = [step(&a), step(&b), step(&c)];
        let order = newest_first(
            &steps,
            &applied(&[("m1_a", 1), ("m2_b", 2), ("m3_c", 2), ("m9_gone", 2)]),
        );
        let names: Vec<(&str, i64)> = order
            .iter()
            .map(|((_, name), batch)| (name.as_str(), *batch))
            .collect();
        assert_eq!(
            names,
            [("m9_gone", 2), ("m3_c", 2), ("m2_b", 2), ("m1_a", 1)],
            "a migration no module registers sorts first in its batch, so a rollback reaches it and says so"
        );
    }

    #[test]
    fn transactions_follow_the_database_unless_the_migration_says() {
        let plain = SchemaMigration::new("m1", Empty);
        assert!(step(&plain).transactional(Dialect::Postgres));
        assert!(step(&plain).transactional(Dialect::Sqlite));
        assert!(!step(&plain).transactional(Dialect::MySql));
        let asks = SchemaMigration::new("m2", OnMySql);
        assert!(step(&asks).transactional(Dialect::MySql));
    }
}
