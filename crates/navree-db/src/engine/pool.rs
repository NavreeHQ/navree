//! [`Pool`]: a pool of connections to one database server.

use std::sync::Arc;
use std::time::Instant;

use navree_config::ConfigSection;
use navree_kernel::BoxFuture;
use navree_telemetry::Telemetry;

use crate::engine::driver::{DriverPool, DriverRow, Executed, RowStream};
use crate::engine::executor::ReportingStream;
use crate::engine::report::Reporter;
use crate::engine::{Backend, DbError, Executor, Row, Statement, Transaction, TxOptions};
use crate::queries::Watchers;
use crate::{DbOptions, PoolStats};

/// A pool of connections to one database server, opened with the settings
/// of a [`DbOptions`] (pool sizes and timeouts, SQLite pragmas, PostgreSQL
/// session settings and TLS, MySQL's session).
///
/// A [`Db`](crate::Db) holds one for its writer, one per read replica and
/// one for its direct connection; [`Db::pool`](crate::Db::pool) hands out
/// the writer's. Statements run through [`Executor`] (each on a connection
/// taken for it), transactions begin with [`begin`](Pool::begin). Cloning
/// shares the pool.
///
/// ```no_run
/// # async fn demo() -> Result<(), navree_db::DbError> {
/// use navree_db::{DbOptions, Executor, ExecutorExt, Pool, Statement};
///
/// let pool = Pool::connect(&DbOptions {
///     url: "postgres://navree:navree@127.0.0.1:5432/app".into(),
///     ..DbOptions::default()
/// })
/// .await?;
/// let version: String = pool.fetch_scalar(&Statement::text("SELECT version()")).await?;
/// # let _ = version;
/// # Ok(()) }
/// ```
#[derive(Clone)]
pub struct Pool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    driver: DriverPool,
    reporter: Reporter,
}

impl Pool {
    /// Opens a pool for `options.url` (its scheme picks the backend) with
    /// the settings of `options`. Statements are reported to `tracing`
    /// only; a [`Db`](crate::Db) also reports them to its listeners and
    /// telemetry.
    pub async fn connect(options: &DbOptions) -> Result<Pool, DbError> {
        options.validate().map_err(|problems| {
            DbError::new(
                crate::ErrorKind::Connect,
                format!("invalid database options: {problems}"),
            )
        })?;
        let reporter = Reporter::new(
            Arc::new(Watchers::new(Arc::from(crate::DEFAULT_CONNECTION))),
            Telemetry::disabled(),
            &options.url,
            options,
        );
        Pool::open(&options.url, options, reporter).await
    }

    /// Opens a pool at `url` with the settings of `options`, reporting to
    /// `reporter`.
    pub(crate) async fn open(
        url: &str,
        options: &DbOptions,
        reporter: Reporter,
    ) -> Result<Pool, DbError> {
        let driver = DriverPool::open(url, options).await.map_err(|error| {
            let message = crate::db::connect_message(url, error.message());
            error.with_message(message)
        })?;
        Ok(Pool {
            inner: Arc::new(PoolInner { driver, reporter }),
        })
    }

    #[cfg(feature = "sqlite")]
    pub(crate) fn driver(&self) -> &DriverPool {
        &self.inner.driver
    }

    /// Whether `self` and `other` are the same pool.
    pub fn same_pool(&self, other: &Pool) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Connections open, idle, and the most the pool opens.
    pub fn stats(&self) -> PoolStats {
        let (size, idle, max) = self.inner.driver.stats();
        PoolStats { size, idle, max }
    }

    /// Checks a connection with a round trip to the server.
    pub async fn ping(&self) -> Result<(), DbError> {
        self.inner.driver.acquire().await?.ping().await
    }

    /// Closes the pool: statements fail from then on. (A pool replaces a
    /// connection it lost by itself; there is no reconnecting by hand.)
    pub async fn close(&self) {
        self.inner.driver.close().await;
    }

    /// Whether the pool is closed.
    pub fn is_closed(&self) -> bool {
        self.inner.driver.is_closed()
    }

    /// Begins a transaction on a connection of its own.
    pub async fn begin(&self) -> Result<Transaction, DbError> {
        self.begin_with(TxOptions::default()).await
    }

    /// Begins a transaction with `options`.
    pub async fn begin_with(&self, options: TxOptions) -> Result<Transaction, DbError> {
        let conn = self.inner.driver.acquire().await?;
        Transaction::begin(conn, self.inner.reporter.clone(), options).await
    }

    /// Runs `op` for `statement` (rewritten for this backend), timing and
    /// reporting it.
    async fn run<T>(
        &self,
        statement: &Statement,
        rows: impl FnOnce(&T) -> Option<u64>,
        op: impl for<'c> FnOnce(
            &'c DriverPool,
            &'c str,
            &'c [crate::Value],
        ) -> BoxFuture<'c, Result<T, DbError>>,
    ) -> Result<T, DbError> {
        let backend = self.backend();
        let statement = statement.for_backend(backend)?;
        let reporter = &self.inner.reporter;
        reporter.before(statement.sql());
        let started = Instant::now();
        let result = op(&self.inner.driver, statement.sql(), statement.values()).await;
        reporter.report(
            statement.sql(),
            statement.values(),
            backend,
            started.elapsed(),
            result.is_err(),
            result.as_ref().ok().and_then(rows),
        );
        result
    }
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool")
            .field("backend", &self.backend())
            .field("stats", &self.stats())
            .finish()
    }
}

impl Executor for Pool {
    fn backend(&self) -> Backend {
        self.inner.driver.backend()
    }

    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(self.run(
            statement,
            |done: &Executed| Some(done.rows_affected),
            |pool, sql, values| Box::pin(pool.execute(sql, values)),
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
                    |pool, sql, values| Box::pin(pool.fetch_all(sql, values)),
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
                    |pool, sql, values| Box::pin(pool.fetch_optional(sql, values)),
                )
                .await?;
            Ok(row.map(Row::from_driver))
        })
    }

    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
        Box::pin(async move {
            let backend = self.backend();
            let statement = statement.for_backend(backend)?;
            self.inner.reporter.before(statement.sql());
            let rows = self
                .inner
                .driver
                .fetch_stream(statement.sql().to_owned(), statement.values())?;
            Ok(
                ReportingStream::new(rows, self.inner.reporter.clone(), backend, &statement)
                    .boxed(),
            )
        })
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let statement = Statement::new(self.backend(), sql, []);
            self.run(
                &statement,
                |done: &Executed| Some(done.rows_affected),
                |pool, sql, _values| Box::pin(pool.execute_script(sql)),
            )
            .await
        })
    }
}
