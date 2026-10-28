//! [`Transaction`]: a database transaction or a savepoint inside one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use navree_kernel::BoxFuture;

use crate::IsolationLevel;
use crate::engine::driver::{DriverConn, DriverRow, Executed, RowStream};
use crate::engine::executor::ReportingStream;
use crate::engine::report::Reporter;
use crate::engine::{Backend, DbError, Executor, Row, Statement};

/// How a transaction begins: its isolation level, whether it only reads,
/// and (SQLite) whether it takes the write lock at once.
///
/// ```
/// use navree_db::{IsolationLevel, TxOptions};
///
/// let options = TxOptions::new().isolation(IsolationLevel::Serializable).read_only();
/// assert!(options.is_read_only());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TxOptions {
    isolation: Option<IsolationLevel>,
    read_only: bool,
    immediate: bool,
}

impl TxOptions {
    /// The server's defaults.
    pub fn new() -> TxOptions {
        TxOptions::default()
    }

    /// Begins at `level` (PostgreSQL and MySQL; SQLite's transactions are
    /// always serializable).
    #[must_use]
    pub fn isolation(mut self, level: IsolationLevel) -> TxOptions {
        self.isolation = Some(level);
        self
    }

    /// Begins a transaction that only reads (PostgreSQL and MySQL).
    #[must_use]
    pub fn read_only(mut self) -> TxOptions {
        self.read_only = true;
        self
    }

    /// On SQLite, takes the write lock when the transaction begins (`BEGIN
    /// IMMEDIATE`), so a transaction that reads then writes queues on the
    /// lock instead of failing with `database is locked`. Units of work
    /// begin this way.
    #[must_use]
    pub fn immediate(mut self) -> TxOptions {
        self.immediate = true;
        self
    }

    /// The isolation level asked for.
    pub fn isolation_level(&self) -> Option<IsolationLevel> {
        self.isolation
    }

    /// Whether the transaction only reads.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Whether SQLite takes the write lock at once.
    pub fn is_immediate(&self) -> bool {
        self.immediate
    }

    /// The statement to run before `BEGIN` (MySQL sets the next
    /// transaction's isolation level outside it).
    fn before_begin(&self, backend: Backend) -> Option<String> {
        match (backend, self.isolation) {
            (Backend::MySql, Some(level)) => Some(format!(
                "SET TRANSACTION ISOLATION LEVEL {}",
                level.as_sql()
            )),
            _ => None,
        }
    }

    /// The statement that begins the transaction, when it is not plain
    /// `BEGIN`.
    fn begin(&self, backend: Backend) -> Option<String> {
        match backend {
            Backend::Postgres => {
                let mut sql = String::from("BEGIN");
                if let Some(level) = self.isolation {
                    sql.push_str(" ISOLATION LEVEL ");
                    sql.push_str(level.as_sql());
                }
                if self.read_only {
                    sql.push_str(" READ ONLY");
                }
                (sql.len() > "BEGIN".len()).then_some(sql)
            }
            Backend::MySql => self
                .read_only
                .then(|| "START TRANSACTION READ ONLY".to_owned()),
            Backend::Sqlite => self.immediate.then(|| "BEGIN IMMEDIATE".to_owned()),
        }
    }
}

/// The connection of a transaction: rolled back down to no transaction
/// when the last handle on it is dropped without finishing (the rollback
/// is queued, and sent when the connection goes back to the pool).
pub(crate) struct TxConn {
    conn: DriverConn,
}

impl TxConn {
    pub(crate) fn conn(&self) -> &DriverConn {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut DriverConn {
        &mut self.conn
    }
}

impl Drop for TxConn {
    fn drop(&mut self) {
        // One queued rollback per open level. (SQLite's worker lowers the
        // depth when it runs each one, so the depth is read once.)
        for _ in 0..self.conn.depth() {
            self.conn.start_rollback();
        }
    }
}

/// What every handle on one transaction shares.
struct Shared {
    conn: Arc<tokio::sync::Mutex<TxConn>>,
    backend: Backend,
    reporter: Reporter,
    /// The shallowest level a dropped handle could not roll back itself
    /// (its connection was busy): rolled back before the next statement.
    pending_rollback: Mutex<Option<usize>>,
}

/// A database transaction, or a savepoint inside one.
///
/// Begun with [`Pool::begin`](crate::Pool::begin) (or
/// [`Db::begin`](crate::Db::begin)); [`savepoint`](Transaction::savepoint)
/// nests a savepoint that commits (`RELEASE`) or rolls back (`ROLLBACK TO`)
/// on its own. Statements run through [`Executor`]; the handle is shared
/// (`&self`), and statements on it run one at a time. Dropping a
/// transaction that was neither committed nor rolled back rolls it back.
///
/// ```no_run
/// # async fn demo(pool: navree_db::Pool) -> Result<(), navree_db::DbError> {
/// use navree_db::{Executor, Statement};
///
/// let tx = pool.begin().await?;
/// tx.execute(&Statement::portable("UPDATE deals SET stage = ? WHERE id = ?", ["won".into(), 7_i64.into()]))
///     .await?;
/// let savepoint = tx.savepoint().await?;
/// savepoint.execute(&Statement::text("DELETE FROM deal_notes")).await?;
/// savepoint.rollback().await?; // the notes stay
/// tx.commit().await?;
/// # Ok(()) }
/// ```
pub struct Transaction {
    shared: Arc<Shared>,
    /// The level this handle owns: 1 for the transaction, 2 and up for
    /// savepoints.
    depth: usize,
    finished: AtomicBool,
}

impl Transaction {
    /// Begins a transaction on `conn`.
    pub(crate) async fn begin(
        mut conn: DriverConn,
        reporter: Reporter,
        options: TxOptions,
    ) -> Result<Transaction, DbError> {
        let backend = conn.backend();
        reporter.before_transaction();
        if let Some(sql) = options.before_begin(backend) {
            let started = Instant::now();
            let result = conn.execute_script(&sql).await;
            reporter.report(&sql, &[], backend, started.elapsed(), result.is_err(), None);
            result?;
        }
        let begin = options.begin(backend);
        let shown = begin.clone().unwrap_or_else(|| "BEGIN".to_owned());
        let started = Instant::now();
        let result = conn.begin(begin).await;
        reporter.report(
            &shown,
            &[],
            backend,
            started.elapsed(),
            result.is_err(),
            None,
        );
        result?;
        Ok(Transaction {
            shared: Arc::new(Shared {
                conn: Arc::new(tokio::sync::Mutex::new(TxConn { conn })),
                backend,
                reporter,
                pending_rollback: Mutex::new(None),
            }),
            depth: 1,
            finished: AtomicBool::new(false),
        })
    }

    /// Whether this is a savepoint inside a transaction.
    pub fn is_savepoint(&self) -> bool {
        self.depth > 1
    }

    /// How deep it is: 1 for a transaction, 2 for its first savepoint, ...
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Whether it was committed or rolled back.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Whether `self` and `other` run on the same transaction (savepoints
    /// included).
    pub fn same_transaction(&self, other: &Transaction) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// The connection, locked, with the rollbacks dropped handles left
    /// behind done first. Fails when this handle's level has ended.
    async fn lock(&self) -> Result<tokio::sync::OwnedMutexGuard<TxConn>, DbError> {
        if self.is_finished() {
            return Err(DbError::done(
                "the transaction is already committed or rolled back",
            ));
        }
        let mut guard = Arc::clone(&self.shared.conn).lock_owned().await;
        let pending = lock_std(&self.shared.pending_rollback).take();
        if let Some(depth) = pending {
            while guard.conn.depth() >= depth {
                guard.conn.rollback().await?;
            }
        }
        if guard.conn.depth() < self.depth {
            return Err(DbError::done(
                "the transaction (or savepoint) was rolled back by an enclosing one",
            ));
        }
        Ok(guard)
    }

    /// A savepoint inside this transaction (or savepoint): a nested level
    /// that commits or rolls back on its own. This level must be the
    /// innermost open one.
    pub async fn savepoint(&self) -> Result<Transaction, DbError> {
        let mut guard = self.lock().await?;
        if guard.conn.depth() != self.depth {
            return Err(DbError::unsupported(
                "a savepoint of this transaction is still open: finish it first",
            ));
        }
        let sql = format!("SAVEPOINT _sqlx_savepoint_{}", self.depth);
        let started = Instant::now();
        let result = guard.conn.begin(None).await;
        self.shared.reporter.report(
            &sql,
            &[],
            self.shared.backend,
            started.elapsed(),
            result.is_err(),
            None,
        );
        result?;
        Ok(Transaction {
            shared: Arc::clone(&self.shared),
            depth: self.depth + 1,
            finished: AtomicBool::new(false),
        })
    }

    /// Commits the transaction, or releases the savepoint. Fails while a
    /// savepoint inside it is still open.
    pub async fn commit(self) -> Result<(), DbError> {
        let mut guard = self.lock().await?;
        if guard.conn.depth() != self.depth {
            return Err(DbError::unsupported(
                "a savepoint of this transaction is still open: finish it before committing",
            ));
        }
        let sql = if self.depth == 1 {
            "COMMIT".to_owned()
        } else {
            format!("RELEASE SAVEPOINT _sqlx_savepoint_{}", self.depth - 1)
        };
        let started = Instant::now();
        let result = guard.conn.commit().await;
        self.shared.reporter.report(
            &sql,
            &[],
            self.shared.backend,
            started.elapsed(),
            result.is_err(),
            None,
        );
        // A failed COMMIT ended the transaction all the same.
        self.finished.store(true, Ordering::Release);
        result
    }

    /// Rolls the transaction back, or back to the savepoint (with any
    /// savepoint still open inside it).
    pub async fn rollback(self) -> Result<(), DbError> {
        self.rollback_ref().await
    }

    /// [`rollback`](Transaction::rollback) through a shared handle.
    pub(crate) async fn rollback_ref(&self) -> Result<(), DbError> {
        let mut guard = self.lock().await?;
        self.finished.store(true, Ordering::Release);
        while guard.conn.depth() >= self.depth {
            let sql = if guard.conn.depth() == 1 {
                "ROLLBACK".to_owned()
            } else {
                format!(
                    "ROLLBACK TO SAVEPOINT _sqlx_savepoint_{}",
                    guard.conn.depth() - 1
                )
            };
            let started = Instant::now();
            let result = guard.conn.rollback().await;
            self.shared.reporter.report(
                &sql,
                &[],
                self.shared.backend,
                started.elapsed(),
                result.is_err(),
                None,
            );
            result?;
        }
        Ok(())
    }

    /// Commits through a shared handle (the unit of work owns its
    /// transaction behind an `Arc`).
    pub(crate) async fn commit_ref(&self) -> Result<(), DbError> {
        let mut guard = self.lock().await?;
        if guard.conn.depth() != self.depth {
            return Err(DbError::unsupported(
                "a savepoint of this transaction is still open: finish it before committing",
            ));
        }
        let started = Instant::now();
        let result = guard.conn.commit().await;
        self.shared.reporter.report(
            "COMMIT",
            &[],
            self.shared.backend,
            started.elapsed(),
            result.is_err(),
            None,
        );
        self.finished.store(true, Ordering::Release);
        result
    }

    /// Runs `f` on the locked connection, reporting the statement.
    async fn run<T>(
        &self,
        statement: &Statement,
        rows: impl FnOnce(&T) -> Option<u64>,
        f: impl for<'c> FnOnce(
            &'c mut DriverConn,
            &'c str,
            &'c [crate::Value],
        ) -> BoxFuture<'c, Result<T, DbError>>,
    ) -> Result<T, DbError> {
        let statement = statement.for_backend(self.shared.backend)?;
        let mut guard = self.lock().await?;
        self.shared.reporter.before(statement.sql());
        let started = Instant::now();
        let result = f(&mut guard.conn, statement.sql(), statement.values()).await;
        self.shared.reporter.report(
            statement.sql(),
            statement.values(),
            self.shared.backend,
            started.elapsed(),
            result.is_err(),
            result.as_ref().ok().and_then(rows),
        );
        result
    }
}

impl Drop for Transaction {
    /// A handle dropped unfinished rolls its level back: before the next
    /// statement of another handle on the transaction, or, when it was the
    /// last handle, when its connection goes back to the pool (see
    /// [`TxConn`]).
    fn drop(&mut self) {
        if self.is_finished() {
            return;
        }
        let mut pending = lock_std(&self.shared.pending_rollback);
        *pending = Some(pending.map_or(self.depth, |depth| depth.min(self.depth)));
    }
}

impl std::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("backend", &self.shared.backend)
            .field("depth", &self.depth)
            .field("finished", &self.is_finished())
            .finish()
    }
}

impl Executor for Transaction {
    fn backend(&self) -> Backend {
        self.shared.backend
    }

    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(self.run(
            statement,
            |done: &Executed| Some(done.rows_affected),
            |conn, sql, values| Box::pin(conn.execute(sql, values)),
        ))
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        Box::pin(async move {
            let rows = self
                .run(
                    statement,
                    |rows: &Vec<DriverRow>| u64::try_from(rows.len()).ok(),
                    |conn, sql, values| Box::pin(conn.fetch_all(sql, values)),
                )
                .await?;
            Ok(rows.into_iter().map(Row::from_driver).collect())
        })
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        Box::pin(async move {
            let row = self
                .run(
                    statement,
                    |row: &Option<DriverRow>| Some(u64::from(row.is_some())),
                    |conn, sql, values| Box::pin(conn.fetch_optional(sql, values)),
                )
                .await?;
            Ok(row.map(Row::from_driver))
        })
    }

    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
        Box::pin(async move {
            let statement = statement.for_backend(self.shared.backend)?;
            let guard = self.lock().await?;
            self.shared.reporter.before(statement.sql());
            let rows = crate::engine::driver::fetch_stream_on(
                guard,
                statement.sql().to_owned(),
                statement.values(),
            )?;
            Ok(ReportingStream::new(
                rows,
                self.shared.reporter.clone(),
                self.shared.backend,
                &statement,
            )
            .boxed())
        })
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let statement = Statement::new(self.shared.backend, sql, []);
            self.run(
                &statement,
                |done: &Executed| Some(done.rows_affected),
                |conn, sql, _values| Box::pin(conn.execute_script(sql)),
            )
            .await
        })
    }
}

fn lock_std<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
