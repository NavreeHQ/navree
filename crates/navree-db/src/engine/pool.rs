//! [`Pool`]: a pool of connections to one database server.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use navree_config::ConfigSection;
use navree_kernel::BoxFuture;
use navree_telemetry::Telemetry;

use crate::engine::driver::{DriverConn, DriverPool, DriverRow, Executed, RowStream};
use crate::engine::executor::ReportingStream;
use crate::engine::report::{Reporter, Timing};
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
    /// Statements and transactions waiting for a connection.
    waiting: Arc<AtomicUsize>,
    /// The server's MariaDB version, once asked.
    mariadb: tokio::sync::OnceCell<Option<(u16, u16)>>,
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
        let pool = Pool {
            inner: Arc::new(PoolInner {
                driver,
                reporter,
                waiting: Arc::new(AtomicUsize::new(0)),
                mariadb: tokio::sync::OnceCell::new(),
            }),
        };
        // A MySQL or MariaDB server says which it is now, while the pool has
        // connections to spare. Asked later, inside a unit of work that holds
        // a connection (the ORM asks before its first statement), the question
        // would take a second one, and with every connection held by such a
        // unit of work none would come.
        pool.mariadb_version().await?;
        Ok(pool)
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
        PoolStats {
            size,
            idle,
            max,
            waiting: self.inner.waiting.load(Ordering::Relaxed),
        }
    }

    /// The server's version as `(major, minor)` when it is MariaDB, else
    /// `None` (MySQL, PostgreSQL, SQLite). The server is asked once, when the
    /// pool opens, without reporting the question to listeners or the query
    /// log, and every clone of the pool remembers the answer.
    pub async fn mariadb_version(&self) -> Result<Option<(u16, u16)>, DbError> {
        if self.backend() != Backend::MySql {
            return Ok(None);
        }
        self.inner
            .mariadb
            .get_or_try_init(|| async {
                let mut conn = self.take_connection().await?;
                let version = match conn.fetch_optional("SELECT VERSION()", &[]).await? {
                    Some(row) => row.value(0)?,
                    None => crate::Value::Null(crate::Kind::String),
                };
                Ok(match version {
                    crate::Value::String(text) => mariadb_version(&text),
                    _ => None,
                })
            })
            .await
            .copied()
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
        let reporter = &self.inner.reporter;
        let waited = Instant::now();
        let conn = match self.take_connection().await {
            Ok(conn) => conn,
            Err(error) => {
                let timing = Timing {
                    elapsed: Duration::ZERO,
                    wait: waited.elapsed(),
                };
                reporter.report_timed("BEGIN", &[], self.backend(), timing, true, None);
                return Err(error);
            }
        };
        Transaction::begin(conn, reporter.clone(), options, waited.elapsed()).await
    }

    /// A connection from the pool, counted in [`PoolStats::waiting`] until it
    /// comes.
    async fn take_connection(&self) -> Result<DriverConn, DbError> {
        let _waiting = Waiting::new(self.inner.waiting.clone());
        self.inner.driver.acquire().await
    }

    /// Runs `op` for `statement` (rewritten for this backend), timing and
    /// reporting it. The time is the statement's own; waiting for a free
    /// connection is reported beside it, so a saturated pool shows as what
    /// it is and does not make every statement look slow. A statement that
    /// gets no connection is reported as failed, having run for no time.
    async fn run<T>(
        &self,
        statement: &Statement,
        rows: impl FnOnce(&T) -> Option<u64>,
        op: impl for<'c> FnOnce(
            &'c mut DriverConn,
            &'c str,
            &'c [crate::Value],
        ) -> BoxFuture<'c, Result<T, DbError>>,
    ) -> Result<T, DbError> {
        let backend = self.backend();
        let statement = statement.for_backend(backend)?;
        let reporter = &self.inner.reporter;
        reporter.before(statement.sql());
        let waited = Instant::now();
        let mut conn = match self.take_connection().await {
            Ok(conn) => conn,
            Err(error) => {
                let timing = Timing {
                    elapsed: Duration::ZERO,
                    wait: waited.elapsed(),
                };
                reporter.report_timed(
                    statement.sql(),
                    statement.values(),
                    backend,
                    timing,
                    true,
                    None,
                );
                return Err(error);
            }
        };
        let wait = waited.elapsed();
        let started = Instant::now();
        let result = op(&mut conn, statement.sql(), statement.values()).await;
        let timing = Timing {
            elapsed: started.elapsed(),
            wait,
        };
        reporter.report_timed(
            statement.sql(),
            statement.values(),
            backend,
            timing,
            result.is_err(),
            result.as_ref().ok().and_then(rows),
        );
        result
    }
}

/// Counts one in [`PoolStats::waiting`] while it lives.
struct Waiting(Arc<AtomicUsize>);

impl Waiting {
    fn new(count: Arc<AtomicUsize>) -> Waiting {
        count.fetch_add(1, Ordering::Relaxed);
        Waiting(count)
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
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

/// `(major, minor)` of a MariaDB `VERSION()` (`11.4.2-MariaDB-log`), or
/// `None` for another server's.
fn mariadb_version(version: &str) -> Option<(u16, u16)> {
    if !version.to_ascii_lowercase().contains("mariadb") {
        return None;
    }
    let mut parts = version
        .split(|c: char| !c.is_ascii_digit())
        .map(|part| part.parse::<u16>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(major), Some(minor)) => Some((major, minor)),
        // A MariaDB whose version cannot be read is taken as the oldest
        // one supported.
        _ => Some((10, 6)),
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
            let backend = self.backend();
            let statement = statement.for_backend(backend)?;
            self.inner.reporter.before(statement.sql());
            let connected = Arc::new(OnceLock::new());
            let waiting = Waiting::new(self.inner.waiting.clone());
            let on_connected = {
                let connected = connected.clone();
                Box::new(move || {
                    let _ = connected.set(Instant::now());
                    drop(waiting);
                })
            };
            let rows = self.inner.driver.fetch_stream(
                statement.sql().to_owned(),
                statement.values(),
                on_connected,
            )?;
            Ok(ReportingStream::new(
                rows,
                self.inner.reporter.clone(),
                backend,
                &statement,
                Some(connected),
            )
            .boxed())
        })
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let statement = Statement::new(self.backend(), sql, []);
            self.run(
                &statement,
                |done: &Executed| Some(done.rows_affected),
                |conn, sql, _values| Box::pin(conn.execute_script(sql)),
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mariadb_versions() {
        assert_eq!(mariadb_version("11.4.13-MariaDB-ubu2404"), Some((11, 4)));
        assert_eq!(mariadb_version("10.6.28-MariaDB-log"), Some((10, 6)));
        assert_eq!(mariadb_version("8.4.11"), None);
        assert_eq!(mariadb_version("MariaDB"), Some((10, 6)));
    }
}
