//! The drivers: one SQLx pool or pooled connection per backend, and the
//! statements, transactions and rows on them. Everything backend-specific
//! about binding, reading and connecting lives in `postgres`, `mysql` and
//! `sqlite`; this module dispatches.

use std::convert::Infallible;
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use sqlx::pool::{PoolConnection, PoolOptions};
use sqlx::{AssertSqlSafe, Row as _, SqlSafeStr};

use crate::DbOptions;
use crate::engine::{Backend, DbError, Value};

#[cfg(feature = "mysql")]
pub(crate) mod mysql;
#[cfg(feature = "postgres")]
pub(crate) mod postgres;
#[cfg(feature = "sqlite")]
pub(crate) mod sqlite;

/// Runs `$body` with the pool, connection or row of whichever backend
/// `$value` (an enum of this module) holds, `$driver` naming the backend's
/// module and `$backend` its [`Backend`].
macro_rules! on_driver {
    ($value:expr, $enum:ident, |$inner:ident, $driver:ident, $backend:ident| $body:expr) => {
        match $value {
            #[cfg(feature = "postgres")]
            $enum::Postgres($inner) => {
                #[allow(unused_imports)]
                use $crate::engine::driver::postgres as $driver;
                #[allow(dead_code)]
                const $backend: Backend = Backend::Postgres;
                $body
            }
            #[cfg(feature = "mysql")]
            $enum::MySql($inner) => {
                #[allow(unused_imports)]
                use $crate::engine::driver::mysql as $driver;
                #[allow(dead_code)]
                const $backend: Backend = Backend::MySql;
                $body
            }
            #[cfg(feature = "sqlite")]
            $enum::Sqlite($inner) => {
                #[allow(unused_imports)]
                use $crate::engine::driver::sqlite as $driver;
                #[allow(dead_code)]
                const $backend: Backend = Backend::Sqlite;
                $body
            }
            $enum::Never(never) => match *never {},
        }
    };
}

/// A SQLx pool of one backend.
#[derive(Clone, Debug)]
pub(crate) enum DriverPool {
    #[cfg(feature = "postgres")]
    Postgres(sqlx::PgPool),
    #[cfg(feature = "mysql")]
    MySql(sqlx::MySqlPool),
    #[cfg(feature = "sqlite")]
    Sqlite(sqlx::SqlitePool),
    /// Keeps the enum inhabited-or-not without any driver compiled in.
    #[allow(dead_code)]
    Never(Infallible),
}

/// A connection taken from a [`DriverPool`].
#[derive(Debug)]
pub(crate) enum DriverConn {
    #[cfg(feature = "postgres")]
    Postgres(PoolConnection<sqlx::Postgres>),
    #[cfg(feature = "mysql")]
    MySql(PoolConnection<sqlx::MySql>),
    #[cfg(feature = "sqlite")]
    Sqlite(PoolConnection<sqlx::Sqlite>),
    #[allow(dead_code)]
    Never(Infallible),
}

/// A row as the driver returns it.
pub(crate) enum DriverRow {
    #[cfg(feature = "postgres")]
    Postgres(sqlx::postgres::PgRow),
    #[cfg(feature = "mysql")]
    MySql(sqlx::mysql::MySqlRow),
    #[cfg(feature = "sqlite")]
    Sqlite(sqlx::sqlite::SqliteRow),
    #[allow(dead_code)]
    Never(Infallible),
}

/// What a statement that returns no rows did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Executed {
    /// The rows it inserted, updated or deleted.
    pub rows_affected: u64,
    /// The key the database assigned to the last inserted row: MySQL's
    /// `LAST_INSERT_ID()`, SQLite's `last_insert_rowid()`; `None` on
    /// PostgreSQL (use `RETURNING`).
    pub last_insert_id: Option<u64>,
}

/// What a statement's text ends with when its values' types key it: the
/// SQL as written, then this, the type each placeholder binds (`INT8`,
/// `TEXT`, ...), separated by `, `, and [`PARAMS_END`].
pub(crate) const PARAMS_START: &str = " /* params: ";

/// The end of the comment [`PARAMS_START`] begins.
pub(crate) const PARAMS_END: &str = " */";

/// A statement as a driver whose prepared statements keep the types of
/// their first run (PostgreSQL, MySQL) prepares it: the SQL as written
/// followed by a comment naming the type each value binds, and the values.
///
/// SQLx caches a connection's prepared statements by their text. With the
/// types in the text, the same SQL run with a value of another type (an
/// `I64` then an `F64`, text then an integer, a typed `NULL` then a value
/// of another kind) is another statement, prepared for its own types,
/// instead of the first one reused with bytes the database reads as its
/// first types. Statements whose types stay the same keep one text and
/// one prepared statement per connection. See DESIGN.md, "Prepared
/// statements and parameter types".
#[cfg_attr(not(any(feature = "postgres", feature = "mysql")), allow(dead_code))]
pub(crate) struct Keyed<'s, A> {
    /// The SQL as written.
    written: &'s str,
    /// Where the comment goes in it: before any trailing `;` and
    /// whitespace (after a `;`, MySQL would read the comment as a
    /// statement of its own).
    end: usize,
    /// The text being built: `written[..end]`, then the comment.
    sql: String,
    args: A,
    bound: usize,
}

#[cfg_attr(not(any(feature = "postgres", feature = "mysql")), allow(dead_code))]
impl<'s, A: sqlx::Arguments> Keyed<'s, A> {
    /// `sql`, about to bind `values` values.
    pub(crate) fn new(sql: &'s str, values: usize) -> Keyed<'s, A> {
        let end = sql
            .trim_end_matches(|c: char| c == ';' || c.is_whitespace())
            .len();
        // Type names are mostly at most 11 bytes (`TIMESTAMPTZ`), and `, `.
        let comment = match values {
            0 => 0,
            n => PARAMS_START.len() + PARAMS_END.len() + n * 13,
        };
        let mut text = String::with_capacity(sql.len() + comment);
        text.push_str(&sql[..end]);
        Keyed {
            written: sql,
            end,
            sql: text,
            args: A::default(),
            bound: 0,
        }
    }

    /// Binds `value`, naming the type it binds as in the statement's text.
    pub(crate) fn add<'t, T>(&mut self, value: T) -> Result<(), sqlx::error::BoxDynError>
    where
        T: sqlx::Encode<'t, A::Database> + sqlx::Type<A::Database>,
    {
        let type_info = value.produces().unwrap_or_else(T::type_info);
        let separator = if self.bound == 0 { PARAMS_START } else { ", " };
        self.sql.push_str(separator);
        self.sql.push_str(sqlx::TypeInfo::name(&type_info));
        self.bound += 1;
        self.args.add(value)
    }

    /// The text to prepare (the SQL as written when nothing is bound) and
    /// the values.
    pub(crate) fn finish(mut self) -> (String, A) {
        if self.bound > 0 {
            self.sql.push_str(PARAMS_END);
        }
        self.sql.push_str(&self.written[self.end..]);
        (self.sql, self.args)
    }
}

/// A vector as little-endian `f32` bytes: MySQL 9's and MariaDB's binary
/// `VECTOR` form, and the blobs SQLite extensions such as sqlite-vec read.
#[cfg_attr(not(any(feature = "mysql", feature = "sqlite")), allow(dead_code))]
pub(crate) fn vector_bytes(items: &[f32]) -> Vec<u8> {
    items.iter().flat_map(|item| item.to_le_bytes()).collect()
}

/// Rows read one by one as the database sends them.
pub type RowStream = BoxStream<'static, Result<crate::engine::Row, DbError>>;

/// How many rows a stream buffers ahead of its reader.
const STREAM_BUFFER: usize = 64;

/// The pool settings every backend shares: sizes, timeouts, and pinging a
/// connection that sat idle (or every connection, with
/// `test_before_acquire`) before handing it out.
pub(crate) fn pool_options<DB: sqlx::Database>(options: &DbOptions) -> PoolOptions<DB> {
    let pool = PoolOptions::<DB>::new()
        .max_connections(options.max_connections)
        .min_connections(options.min_connections)
        .acquire_timeout(Duration::from_secs(options.connect_timeout_secs.max(1) * 4))
        .idle_timeout(seconds_or_never(options.idle_timeout_secs))
        .max_lifetime(seconds_or_never(options.max_lifetime_secs));
    if options.test_before_acquire {
        return pool.test_before_acquire(true);
    }
    // Hot connections go straight out; one that sat idle long enough to
    // have been dropped on the way is pinged first.
    let threshold = Duration::from_secs(DbOptions::IDLE_PING_SECS);
    pool.test_before_acquire(false)
        .before_acquire(move |conn, meta| {
            Box::pin(async move {
                if meta.idle_for >= threshold {
                    sqlx::Connection::ping(conn).await?;
                }
                Ok(true)
            })
        })
}

/// `secs` as a duration; 0 means none.
fn seconds_or_never(secs: u64) -> Option<Duration> {
    (secs > 0).then(|| Duration::from_secs(secs))
}

impl DriverPool {
    /// Opens a pool for `url` (whose scheme picks the backend) with the
    /// settings of `options`.
    pub(crate) async fn open(url: &str, options: &DbOptions) -> Result<DriverPool, DbError> {
        let backend = Backend::from_url(url).ok_or_else(|| {
            DbError::new(
                crate::ErrorKind::Connect,
                format!(
                    "unsupported database URL scheme in `{}` (use postgres://, mysql://, mariadb:// or sqlite:)",
                    crate::options::redact_url(url)
                ),
            )
        })?;
        match backend {
            #[cfg(feature = "postgres")]
            Backend::Postgres => {
                let connect = postgres::connect_options(url, &options.postgres)?;
                let pool = postgres::pool_options(options)
                    .connect_with(connect)
                    .await
                    .map_err(|error| connect_error(error, backend))?;
                Ok(DriverPool::Postgres(pool))
            }
            #[cfg(feature = "mysql")]
            Backend::MySql => {
                let connect = mysql::connect_options(url)?;
                let pool = mysql::pool_options(options)
                    .connect_with(connect)
                    .await
                    .map_err(|error| connect_error(error, backend))?;
                Ok(DriverPool::MySql(pool))
            }
            #[cfg(feature = "sqlite")]
            Backend::Sqlite => {
                sqlite::create_parent_dir(url).await?;
                let connect = sqlite::connect_options(url, &options.sqlite)?;
                let pool = sqlite::pool_options(options)
                    .connect_with(connect)
                    .await
                    .map_err(|error| connect_error(error, backend))?;
                Ok(DriverPool::Sqlite(pool))
            }
            #[allow(unreachable_patterns)]
            backend => Err(DbError::driver_missing(backend)),
        }
    }

    /// The backend.
    pub(crate) fn backend(&self) -> Backend {
        on_driver!(self, DriverPool, |_pool, _driver, BACKEND| BACKEND)
    }

    /// Connections open, idle, and the most the pool opens.
    pub(crate) fn stats(&self) -> (u32, usize, u32) {
        on_driver!(self, DriverPool, |pool, _driver, _BACKEND| (
            pool.size(),
            pool.num_idle(),
            pool.options().get_max_connections()
        ))
    }

    /// Closes the pool: statements fail from then on.
    pub(crate) async fn close(&self) {
        on_driver!(self, DriverPool, |pool, _driver, _BACKEND| pool
            .close()
            .await);
    }

    /// Whether the pool is closed.
    pub(crate) fn is_closed(&self) -> bool {
        on_driver!(self, DriverPool, |pool, _driver, _BACKEND| pool.is_closed())
    }

    /// A connection of its own.
    pub(crate) async fn acquire(&self) -> Result<DriverConn, DbError> {
        on_driver!(self, DriverPool, |pool, _driver, BACKEND| pool
            .acquire()
            .await
            .map(Into::into)
            .map_err(|error| DbError::from_sqlx(error, Some(BACKEND))))
    }

    /// Runs a statement that returns no rows.
    pub(crate) async fn execute(&self, sql: &str, values: &[Value]) -> Result<Executed, DbError> {
        on_driver!(self, DriverPool, |pool, driver, BACKEND| {
            let (sql, args) = driver::statement(&sql, values)?;
            let done = sqlx::query_with(AssertSqlSafe(sql), args)
                .execute(pool)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))?;
            Ok(executed(&done))
        })
    }

    /// Runs a query and returns its rows.
    pub(crate) async fn fetch_all(
        &self,
        sql: &str,
        values: &[Value],
    ) -> Result<Vec<DriverRow>, DbError> {
        on_driver!(self, DriverPool, |pool, driver, BACKEND| {
            let (sql, args) = driver::statement(&sql, values)?;
            let rows = sqlx::query_with(AssertSqlSafe(sql), args)
                .fetch_all(pool)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
    }

    /// Runs a query and returns its first row, if any.
    pub(crate) async fn fetch_optional(
        &self,
        sql: &str,
        values: &[Value],
    ) -> Result<Option<DriverRow>, DbError> {
        on_driver!(self, DriverPool, |pool, driver, BACKEND| {
            let (sql, args) = driver::statement(&sql, values)?;
            let row = sqlx::query_with(AssertSqlSafe(sql), args)
                .fetch_optional(pool)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))?;
            Ok(row.map(Into::into))
        })
    }

    /// Runs SQL that binds nothing and may hold several statements.
    pub(crate) async fn execute_script(&self, sql: &str) -> Result<Executed, DbError> {
        on_driver!(self, DriverPool, |pool, _driver, BACKEND| {
            let done = sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
                .execute(pool)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))?;
            Ok(executed(&done))
        })
    }

    /// Runs a query whose rows arrive one by one, on a connection of the
    /// stream's own (taken from the pool until the stream ends or is
    /// dropped).
    pub(crate) fn fetch_stream(
        &self,
        sql: String,
        values: &[Value],
    ) -> Result<BoxStream<'static, Result<DriverRow, DbError>>, DbError> {
        on_driver!(self, DriverPool, |pool, driver, BACKEND| {
            let (sql, args) = driver::statement(&sql, values)?;
            let pool = pool.clone();
            let (sender, receiver) = tokio::sync::mpsc::channel(STREAM_BUFFER);
            tokio::spawn(async move {
                let mut rows = sqlx::query_with(AssertSqlSafe(sql), args).fetch(&pool);
                while let Some(row) = rows.next().await {
                    let row = row
                        .map(DriverRow::from)
                        .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)));
                    if sender.send(row).await.is_err() {
                        break;
                    }
                }
            });
            Ok(tokio_stream_from(receiver))
        })
    }
}

impl DriverConn {
    /// The backend.
    pub(crate) fn backend(&self) -> Backend {
        on_driver!(self, DriverConn, |_conn, _driver, BACKEND| BACKEND)
    }

    /// Runs a statement that returns no rows.
    pub(crate) async fn execute(
        &mut self,
        sql: &str,
        values: &[Value],
    ) -> Result<Executed, DbError> {
        on_driver!(self, DriverConn, |conn, driver, BACKEND| {
            let (sql, args) = driver::statement(&sql, values)?;
            let done = sqlx::query_with(AssertSqlSafe(sql), args)
                .execute(&mut **conn)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))?;
            Ok(executed(&done))
        })
    }

    /// Runs a query and returns its rows.
    pub(crate) async fn fetch_all(
        &mut self,
        sql: &str,
        values: &[Value],
    ) -> Result<Vec<DriverRow>, DbError> {
        on_driver!(self, DriverConn, |conn, driver, BACKEND| {
            let (sql, args) = driver::statement(&sql, values)?;
            let rows = sqlx::query_with(AssertSqlSafe(sql), args)
                .fetch_all(&mut **conn)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
    }

    /// Runs a query and returns its first row, if any.
    pub(crate) async fn fetch_optional(
        &mut self,
        sql: &str,
        values: &[Value],
    ) -> Result<Option<DriverRow>, DbError> {
        on_driver!(self, DriverConn, |conn, driver, BACKEND| {
            let (sql, args) = driver::statement(&sql, values)?;
            let row = sqlx::query_with(AssertSqlSafe(sql), args)
                .fetch_optional(&mut **conn)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))?;
            Ok(row.map(Into::into))
        })
    }

    /// Runs SQL that binds nothing and may hold several statements.
    pub(crate) async fn execute_script(&mut self, sql: &str) -> Result<Executed, DbError> {
        on_driver!(self, DriverConn, |conn, _driver, BACKEND| {
            let done = sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
                .execute(&mut **conn)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))?;
            Ok(executed(&done))
        })
    }

    /// Checks the connection with a round trip.
    pub(crate) async fn ping(&mut self) -> Result<(), DbError> {
        on_driver!(self, DriverConn, |conn, _driver, BACKEND| {
            sqlx::Connection::ping(&mut **conn)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))
        })
    }

    /// Begins a transaction, or a savepoint inside the open one. `begin`
    /// replaces `BEGIN` for a transaction (`BEGIN IMMEDIATE`).
    pub(crate) async fn begin(&mut self, begin: Option<String>) -> Result<(), DbError> {
        on_driver!(self, DriverConn, |conn, _driver, BACKEND| {
            let statement = begin.map(|sql| AssertSqlSafe(sql).into_sql_str());
            transaction_manager::begin(conn, statement)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))
        })
    }

    /// Commits the transaction, or releases the newest savepoint.
    pub(crate) async fn commit(&mut self) -> Result<(), DbError> {
        on_driver!(self, DriverConn, |conn, _driver, BACKEND| {
            transaction_manager::commit(conn)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))
        })
    }

    /// Rolls the transaction back, or back to the newest savepoint.
    pub(crate) async fn rollback(&mut self) -> Result<(), DbError> {
        on_driver!(self, DriverConn, |conn, _driver, BACKEND| {
            transaction_manager::rollback(conn)
                .await
                .map_err(|error| DbError::from_sqlx(error, Some(BACKEND)))
        })
    }

    /// Queues a rollback of the newest transaction level, sent with the
    /// next statement or when the connection goes back to the pool (for
    /// drops, which cannot wait).
    pub(crate) fn start_rollback(&mut self) {
        on_driver!(self, DriverConn, |conn, _driver, _BACKEND| {
            transaction_manager::start_rollback(conn)
        });
    }

    /// How deep the connection's transactions are: 0 outside one, 1 in a
    /// transaction, more with savepoints.
    pub(crate) fn depth(&self) -> usize {
        on_driver!(self, DriverConn, |conn, _driver, _BACKEND| {
            transaction_manager::depth(conn)
        })
    }
}

/// The SQLx transaction manager of whichever backend a connection is.
mod transaction_manager {
    use sqlx::pool::PoolConnection;
    use sqlx::{Database, SqlStr};
    use sqlx_core::transaction::TransactionManager;

    pub(super) async fn begin<DB: Database>(
        conn: &mut PoolConnection<DB>,
        statement: Option<SqlStr>,
    ) -> Result<(), sqlx::Error> {
        DB::TransactionManager::begin(&mut **conn, statement).await
    }

    pub(super) async fn commit<DB: Database>(
        conn: &mut PoolConnection<DB>,
    ) -> Result<(), sqlx::Error> {
        DB::TransactionManager::commit(&mut **conn).await
    }

    pub(super) async fn rollback<DB: Database>(
        conn: &mut PoolConnection<DB>,
    ) -> Result<(), sqlx::Error> {
        DB::TransactionManager::rollback(&mut **conn).await
    }

    pub(super) fn start_rollback<DB: Database>(conn: &mut PoolConnection<DB>) {
        DB::TransactionManager::start_rollback(&mut **conn);
    }

    pub(super) fn depth<DB: Database>(conn: &PoolConnection<DB>) -> usize {
        DB::TransactionManager::get_transaction_depth(&**conn)
    }
}

/// Runs a query whose rows arrive one by one on the connection `guard`
/// holds (a transaction's), keeping it until the stream ends or is
/// dropped.
pub(crate) fn fetch_stream_on(
    mut guard: tokio::sync::OwnedMutexGuard<crate::engine::transaction::TxConn>,
    sql: String,
    values: &[Value],
) -> Result<BoxStream<'static, Result<DriverRow, DbError>>, DbError> {
    let backend = guard.conn().backend();
    let (sender, receiver) = tokio::sync::mpsc::channel(STREAM_BUFFER);
    macro_rules! stream_on {
        ($driver:ident, $variant:ident) => {{
            let (sql, args) = $driver::statement(&sql, values)?;
            tokio::spawn(async move {
                let DriverConn::$variant(conn) = guard.conn_mut() else {
                    return;
                };
                let mut rows = sqlx::query_with(AssertSqlSafe(sql), args).fetch(&mut **conn);
                while let Some(row) = rows.next().await {
                    let row = row
                        .map(DriverRow::from)
                        .map_err(|error| DbError::from_sqlx(error, Some(backend)));
                    if sender.send(row).await.is_err() {
                        break;
                    }
                }
            });
        }};
    }
    match backend {
        #[cfg(feature = "postgres")]
        Backend::Postgres => stream_on!(postgres, Postgres),
        #[cfg(feature = "mysql")]
        Backend::MySql => stream_on!(mysql, MySql),
        #[cfg(feature = "sqlite")]
        Backend::Sqlite => stream_on!(sqlite, Sqlite),
        #[allow(unreachable_patterns)]
        backend => return Err(DbError::driver_missing(backend)),
    }
    Ok(tokio_stream_from(receiver))
}

/// The rows a channel receives, as a stream.
fn tokio_stream_from(
    mut receiver: tokio::sync::mpsc::Receiver<Result<DriverRow, DbError>>,
) -> BoxStream<'static, Result<DriverRow, DbError>> {
    futures_util::stream::poll_fn(move |cx| receiver.poll_recv(cx)).boxed()
}

/// What a statement did, from the driver's result.
fn executed<R: QueryResultExt>(done: &R) -> Executed {
    Executed {
        rows_affected: done.rows_affected(),
        last_insert_id: done.last_insert_id(),
    }
}

/// Rows affected and the assigned key, per driver.
trait QueryResultExt {
    fn rows_affected(&self) -> u64;
    fn last_insert_id(&self) -> Option<u64>;
}

#[cfg(feature = "postgres")]
impl QueryResultExt for sqlx::postgres::PgQueryResult {
    fn rows_affected(&self) -> u64 {
        sqlx::postgres::PgQueryResult::rows_affected(self)
    }

    fn last_insert_id(&self) -> Option<u64> {
        None
    }
}

#[cfg(feature = "mysql")]
impl QueryResultExt for sqlx::mysql::MySqlQueryResult {
    fn rows_affected(&self) -> u64 {
        sqlx::mysql::MySqlQueryResult::rows_affected(self)
    }

    fn last_insert_id(&self) -> Option<u64> {
        Some(sqlx::mysql::MySqlQueryResult::last_insert_id(self))
    }
}

#[cfg(feature = "sqlite")]
impl QueryResultExt for sqlx::sqlite::SqliteQueryResult {
    fn rows_affected(&self) -> u64 {
        sqlx::sqlite::SqliteQueryResult::rows_affected(self)
    }

    fn last_insert_id(&self) -> Option<u64> {
        u64::try_from(self.last_insert_rowid()).ok()
    }
}

macro_rules! conversions {
    ($($feature:literal, $variant:ident, $db:ty, $row:ty;)*) => {
        $(
            #[cfg(feature = $feature)]
            impl From<PoolConnection<$db>> for DriverConn {
                fn from(conn: PoolConnection<$db>) -> Self {
                    DriverConn::$variant(conn)
                }
            }

            #[cfg(feature = $feature)]
            impl From<$row> for DriverRow {
                fn from(row: $row) -> Self {
                    DriverRow::$variant(row)
                }
            }
        )*
    };
}

conversions! {
    "postgres", Postgres, sqlx::Postgres, sqlx::postgres::PgRow;
    "mysql", MySql, sqlx::MySql, sqlx::mysql::MySqlRow;
    "sqlite", Sqlite, sqlx::Sqlite, sqlx::sqlite::SqliteRow;
}

impl DriverRow {
    /// The column names, in order.
    pub(crate) fn columns(&self) -> Vec<&str> {
        on_driver!(self, DriverRow, |row, _driver, _BACKEND| row
            .columns()
            .iter()
            .map(sqlx::Column::name)
            .collect())
    }

    /// How many columns.
    pub(crate) fn len(&self) -> usize {
        on_driver!(self, DriverRow, |row, _driver, _BACKEND| row.len())
    }

    /// Column `index` as a [`Value`].
    pub(crate) fn value(&self, index: usize) -> Result<Value, DbError> {
        on_driver!(self, DriverRow, |row, driver, _BACKEND| driver::value(
            row, index
        ))
    }

    /// The backend the row came from.
    pub(crate) fn backend(&self) -> Backend {
        on_driver!(self, DriverRow, |_row, _driver, BACKEND| BACKEND)
    }
}

/// A connection error that names the database without its password (the
/// driver's own message may echo the URL).
fn connect_error(error: sqlx::Error, backend: Backend) -> DbError {
    DbError::from_sqlx(error, Some(backend)).with_kind(crate::ErrorKind::Connect)
}
