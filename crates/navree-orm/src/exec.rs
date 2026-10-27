//! Where queries run: [`Executor`], [`Target`] and [`ExecContext`].
//!
//! Terminal methods take an executor: `Deal::query().get(&db)`,
//! `deal.save(&uow)`. An executor resolves a [`Target`] per operation: an
//! SQL connection or transaction (with the unit of work whose change events
//! it records), or a document driver.
//!
//! | Executor | Reads | Writes |
//! |---|---|---|
//! | [`Db`] | the task's current unit of work when its transaction is open on this pool, else the pool | the task's current unfinished unit of work on this pool, else the pool |
//! | [`Uow`] | its transaction | its transaction, change events recorded |
//! | `DatabaseConnection`, `DatabaseTransaction` | as given | as given, no change events |
//! | `Arc<E>` | as `E` | as `E` |
//!
//! `Db` joins the current unit of work (the request's, in a handler) the
//! way the rest of Navree does: on SQLite a write through a second pool
//! connection would wait for the request's write lock until
//! `busy_timeout`, and joining keeps the write in the request's
//! transaction and audit trail.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use chrono::{DateTime, Utc};
use futures::stream::{BoxStream, Stream, StreamExt};
use navree_contracts::Clock;
use navree_db::{Db, Reader, Uow};
use navree_kernel::AppContext;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, ExecResult, QueryResult,
    StreamTrait, TransactionTrait,
};

use crate::driver::{DocumentDriver, InsertOutcome};
use crate::error::OrmError;
use crate::ir::{Aggregate, DeleteIr, InsertIr, QueryIr, UpdateIr};
use crate::row::Row;
use crate::sql::{self, Dialect, Statement};
use crate::streams::{StreamingGuard, ensure_free};
use crate::value::{Field, Value};

/// Rows read one by one as the database sends them (see
/// [`Target::stream`]).
pub type RowsStream<'a> = BoxStream<'a, Result<Row, OrmError>>;

/// What an operation does, which decides where `&Db` runs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Reads.
    Read,
    /// Writes.
    Write,
}

/// Something ORM operations run on: [`Db`], [`Uow`], a SeaORM connection or
/// transaction, a document store (see [`DocumentDriver`]), or an `Arc` of
/// one.
pub trait Executor: Send + Sync {
    /// Where the next operation runs.
    fn target(&self, access: Access) -> impl Future<Output = Result<Target<'_>, OrmError>> + Send;
}

/// Where one operation runs.
pub enum Target<'a> {
    /// An SQL database.
    Sql(SqlTarget<'a>),
    /// A document store.
    Document(DocumentTarget),
}

/// An SQL connection or transaction, with the unit of work it belongs to.
pub struct SqlTarget<'a> {
    conn: SqlConn<'a>,
    dialect: Dialect,
    uow: Option<Uow>,
    app: Option<AppContext>,
}

enum SqlConn<'a> {
    Pool(&'a DatabaseConnection),
    Transaction(Arc<DatabaseTransaction>),
    TransactionRef(&'a DatabaseTransaction),
}

/// A document store and the app it belongs to.
#[derive(Clone)]
pub struct DocumentTarget {
    driver: Arc<dyn DocumentDriver>,
    app: Option<AppContext>,
}

impl DocumentTarget {
    /// The document store `driver`, in `app` (for clocks and other app
    /// services), if any.
    pub fn new(driver: Arc<dyn DocumentDriver>, app: Option<AppContext>) -> Self {
        DocumentTarget { driver, app }
    }

    /// The driver.
    pub fn driver(&self) -> &dyn DocumentDriver {
        &*self.driver
    }
}

/// What a running operation knows about where it runs: the app (for its
/// clock and other services), the unit of work, the dialect. Lifecycle
/// hooks and casts receive it.
#[derive(Clone, Default)]
pub struct ExecContext {
    app: Option<AppContext>,
    uow: Option<Uow>,
    dialect: Option<Dialect>,
    driver: Option<&'static str>,
}

impl ExecContext {
    /// The app, when the executor's pool belongs to one.
    pub fn app(&self) -> Option<&AppContext> {
        self.app.as_ref()
    }

    /// The unit of work the operation runs in, if any.
    pub fn uow(&self) -> Option<&Uow> {
        self.uow.as_ref()
    }

    /// The SQL dialect; `None` on a document store.
    pub fn dialect(&self) -> Option<Dialect> {
        self.dialect
    }

    /// The database's name in messages (`PostgreSQL`, `mongodb`, ...).
    pub fn database(&self) -> &'static str {
        match (self.dialect, self.driver) {
            (Some(dialect), _) => dialect.name(),
            (None, Some(driver)) => driver,
            (None, None) => "the database",
        }
    }

    /// The current time: the app's `dyn Clock` when it registers one
    /// (tests freeze time with a fixed clock), else the system clock;
    /// truncated to microseconds, the precision PostgreSQL and MySQL store.
    pub async fn now(&self) -> DateTime<Utc> {
        let now = match &self.app {
            Some(app) => match app.try_get::<dyn Clock>().await {
                Some(clock) => clock.now(),
                None => Utc::now(),
            },
            None => Utc::now(),
        };
        truncate_to_micros(now)
    }
}

impl fmt::Debug for ExecContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecContext")
            .field("database", &self.database())
            .field("uow", &self.uow)
            .field("app", &self.app.is_some())
            .finish()
    }
}

/// `at` without its sub-microsecond part.
pub(crate) fn truncate_to_micros(at: DateTime<Utc>) -> DateTime<Utc> {
    let micros = at.timestamp_micros();
    DateTime::from_timestamp_micros(micros).unwrap_or(at)
}

impl Target<'_> {
    /// The context of this operation.
    pub fn context(&self) -> ExecContext {
        match self {
            Target::Sql(sql) => ExecContext {
                app: sql.app.clone(),
                uow: sql.uow.clone(),
                dialect: Some(sql.dialect),
                driver: None,
            },
            Target::Document(document) => ExecContext {
                app: document.app.clone(),
                uow: None,
                dialect: None,
                driver: Some(document.driver.name()),
            },
        }
    }

    /// The unit of work writes are recorded in.
    pub fn uow(&self) -> Option<&Uow> {
        match self {
            Target::Sql(sql) => sql.uow.as_ref(),
            Target::Document(_) => None,
        }
    }

    /// The SQL dialect; `None` on a document store.
    pub fn dialect(&self) -> Option<Dialect> {
        match self {
            Target::Sql(sql) => Some(sql.dialect),
            Target::Document(_) => None,
        }
    }

    /// A savepoint (a nested transaction) of the transaction this target
    /// runs in; `None` outside a transaction and on document stores.
    pub(crate) async fn begin_savepoint(&self) -> Result<Option<DatabaseTransaction>, OrmError> {
        match self {
            Target::Sql(sql) => {
                ensure_free(sql.conn.transaction_id())?;
                match &sql.conn {
                    SqlConn::Transaction(tx) => Ok(Some(tx.begin().await?)),
                    SqlConn::TransactionRef(tx) => Ok(Some(tx.begin().await?)),
                    SqlConn::Pool(_) => Ok(None),
                }
            }
            Target::Document(_) => Ok(None),
        }
    }

    /// A transaction for work that must happen as a whole: a savepoint of
    /// the transaction this target runs in, or a new transaction on the
    /// pool. `None` on document stores (each operation stands alone).
    pub(crate) async fn begin_atomic(&self) -> Result<Option<DatabaseTransaction>, OrmError> {
        match self {
            Target::Sql(sql) => match &sql.conn {
                SqlConn::Pool(conn) => Ok(Some(conn.begin().await?)),
                SqlConn::Transaction(_) | SqlConn::TransactionRef(_) => self.begin_savepoint().await,
            },
            Target::Document(_) => Ok(None),
        }
    }

    /// The same target running on `tx` (a savepoint of its transaction).
    pub(crate) fn on_transaction<'b>(&self, tx: &'b DatabaseTransaction) -> Target<'b> {
        match self {
            Target::Sql(sql) => Target::Sql(SqlTarget::new(
                SqlConn::TransactionRef(tx),
                sql.dialect,
                sql.uow.clone(),
                sql.app.clone(),
            )),
            Target::Document(document) => Target::Document(document.clone()),
        }
    }

    /// The rows of `query`.
    pub async fn fetch(&self, query: &QueryIr) -> Result<Vec<Row>, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_select(query, sql.dialect)?;
                Ok(sql
                    .query_all(&statement)
                    .await?
                    .into_iter()
                    .map(Row::from_sql)
                    .collect())
            }
            Target::Document(document) => document.driver.find(query).await,
        }
    }

    /// The rows of `query` one by one, as the database sends them: an SQL
    /// cursor (the driver reads the next rows from the socket as the stream
    /// is polled), or a document driver's
    /// [`stream`](DocumentDriver::stream).
    ///
    /// Inside a transaction the stream holds the transaction's connection
    /// until it is dropped; other statements on that transaction fail with
    /// an error meanwhile (they would wait for the stream forever).
    pub async fn stream<'s>(&'s self, query: &'s QueryIr) -> Result<RowsStream<'s>, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_select(query, sql.dialect)?;
                sql.stream(&statement).await
            }
            Target::Document(document) => {
                let rows = document.driver.stream(query).await?;
                Ok(rows)
            }
        }
    }

    /// An aggregate over the rows of `query` (`NULL` when there are none,
    /// except for counts).
    pub async fn aggregate(
        &self,
        query: &QueryIr,
        aggregate: &Aggregate,
    ) -> Result<Value, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_aggregate(query, aggregate, sql.dialect)?;
                match sql.query_one(&statement).await? {
                    Some(row) => Row::from_sql(row).value("aggregate"),
                    None => Ok(Value::BigInt(None)),
                }
            }
            Target::Document(document) => document.driver.aggregate(query, aggregate).await,
        }
    }

    /// Whether `query` has any row.
    pub async fn exists(&self, query: &QueryIr) -> Result<bool, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_exists(query, sql.dialect)?;
                match sql.query_one(&statement).await? {
                    Some(row) => {
                        let value = Row::from_sql(row).value("exists")?;
                        bool::from_value(value).map_err(|error| {
                            crate::row::decode_error("EXISTS", "exists", "bool".into(), &error)
                        })
                    }
                    None => Ok(false),
                }
            }
            Target::Document(document) => document.driver.exists(query).await,
        }
    }

    /// Inserts rows; generated keys come back in the outcome.
    pub async fn insert(&self, insert: &InsertIr) -> Result<InsertOutcome, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_insert(insert, sql.dialect)?;
                if !insert.returning.is_empty() && sql.dialect.supports_returning() {
                    let rows: Vec<Row> = sql
                        .query_all(&statement)
                        .await?
                        .into_iter()
                        .map(Row::from_sql)
                        .collect();
                    Ok(InsertOutcome {
                        rows_affected: rows.len() as u64,
                        returned: rows,
                        last_insert_id: None,
                    })
                } else {
                    let result = sql.execute(&statement).await?;
                    Ok(InsertOutcome {
                        rows_affected: result.rows_affected(),
                        returned: Vec::new(),
                        last_insert_id: (sql.dialect == Dialect::MySql)
                            .then(|| result.last_insert_id()),
                    })
                }
            }
            Target::Document(document) => document.driver.insert(insert).await,
        }
    }

    /// Runs an update; returns the number of rows changed.
    pub async fn update(&self, update: &UpdateIr) -> Result<u64, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_update(update, sql.dialect)?;
                Ok(sql.execute(&statement).await?.rows_affected())
            }
            Target::Document(document) => document.driver.update(update).await,
        }
    }

    /// Runs a delete; returns the number of rows deleted.
    pub async fn delete(&self, delete: &DeleteIr) -> Result<u64, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_delete(delete, sql.dialect)?;
                Ok(sql.execute(&statement).await?.rows_affected())
            }
            Target::Document(document) => document.driver.delete(delete).await,
        }
    }
}

impl<'a> SqlTarget<'a> {
    fn new(conn: SqlConn<'a>, dialect: Dialect, uow: Option<Uow>, app: Option<AppContext>) -> Self {
        SqlTarget {
            conn,
            dialect,
            uow,
            app,
        }
    }

    /// The dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Whether statements run inside a transaction (a unit of work's, or
    /// one passed as the executor).
    pub fn in_transaction(&self) -> bool {
        self.conn.transaction_id().is_some()
    }

    /// Runs `statement` and returns its rows.
    pub async fn query_all(&self, statement: &Statement) -> Result<Vec<QueryResult>, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        let statement = statement.to_sea_orm();
        Ok(match &self.conn {
            SqlConn::Pool(conn) => conn.query_all_raw(statement).await?,
            SqlConn::Transaction(tx) => tx.query_all_raw(statement).await?,
            SqlConn::TransactionRef(tx) => tx.query_all_raw(statement).await?,
        })
    }

    /// Runs `statement` and returns its first row.
    pub async fn query_one(&self, statement: &Statement) -> Result<Option<QueryResult>, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        let statement = statement.to_sea_orm();
        Ok(match &self.conn {
            SqlConn::Pool(conn) => conn.query_one_raw(statement).await?,
            SqlConn::Transaction(tx) => tx.query_one_raw(statement).await?,
            SqlConn::TransactionRef(tx) => tx.query_one_raw(statement).await?,
        })
    }

    /// Runs `statement` for its effect.
    pub async fn execute(&self, statement: &Statement) -> Result<ExecResult, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        let statement = statement.to_sea_orm();
        Ok(match &self.conn {
            SqlConn::Pool(conn) => conn.execute_raw(statement).await?,
            SqlConn::Transaction(tx) => tx.execute_raw(statement).await?,
            SqlConn::TransactionRef(tx) => tx.execute_raw(statement).await?,
        })
    }

    /// Runs `sql` as written, without values (several statements may be
    /// separated by `;`).
    pub async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        Ok(match &self.conn {
            SqlConn::Pool(conn) => conn.execute_unprepared(sql).await?,
            SqlConn::Transaction(tx) => tx.execute_unprepared(sql).await?,
            SqlConn::TransactionRef(tx) => tx.execute_unprepared(sql).await?,
        })
    }

    /// Runs `statement` and hands its rows out one by one as the database
    /// sends them. On the pool the stream holds one pooled connection; in a
    /// transaction it holds the transaction's connection, and other
    /// statements on that transaction fail until the stream is dropped.
    pub async fn stream(&self, statement: &Statement) -> Result<RowsStream<'_>, OrmError> {
        let id = self.conn.transaction_id();
        ensure_free(id)?;
        let statement = statement.to_sea_orm();
        let rows: RowsStream<'_> = match &self.conn {
            SqlConn::Pool(conn) => conn
                .stream_raw(statement)
                .await?
                .map(|row| row.map(Row::from_sql).map_err(OrmError::from))
                .boxed(),
            SqlConn::Transaction(tx) => Guarded::new(tx.stream_raw(statement).await?, id).boxed(),
            SqlConn::TransactionRef(tx) => {
                Guarded::new(tx.stream_raw(statement).await?, id).boxed()
            }
        };
        Ok(rows)
    }
}

impl SqlConn<'_> {
    /// The identity of the transaction statements run in (its address),
    /// `None` on the pool.
    fn transaction_id(&self) -> Option<usize> {
        match self {
            SqlConn::Pool(_) => None,
            SqlConn::Transaction(tx) => Some(Arc::as_ptr(tx) as usize),
            SqlConn::TransactionRef(tx) => Some(std::ptr::from_ref::<DatabaseTransaction>(tx) as usize),
        }
    }
}

/// A transaction's row stream, marking the transaction busy while it lives
/// (see [`ensure_free`]).
struct Guarded<S> {
    rows: Pin<Box<S>>,
    _busy: Option<StreamingGuard>,
}

impl<S> Guarded<S> {
    fn new(rows: S, transaction: Option<usize>) -> Self {
        Guarded {
            rows: Box::pin(rows),
            _busy: transaction.map(StreamingGuard::new),
        }
    }
}

impl<S> Stream for Guarded<S>
where
    S: Stream<Item = Result<QueryResult, sea_orm::DbErr>>,
{
    type Item = Result<Row, OrmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rows
            .as_mut()
            .poll_next(cx)
            .map(|row| row.map(|row| row.map(Row::from_sql).map_err(OrmError::from)))
    }
}

/// Whether `uow` runs on the same pool as `db`.
fn same_pool(db: &Db, uow: &Uow) -> bool {
    std::ptr::eq(db.conn(), uow.db().conn())
}

impl Executor for Db {
    async fn target(&self, access: Access) -> Result<Target<'_>, OrmError> {
        let dialect = Dialect::from_backend(self.backend())?;
        let app = self.app();
        match access {
            Access::Read => {
                let uow = Uow::current().filter(|uow| same_pool(self, uow));
                let conn = match self.reader().await {
                    Reader::Pool(conn) => SqlConn::Pool(conn),
                    Reader::Transaction(tx) => SqlConn::Transaction(tx),
                };
                let uow = match &conn {
                    SqlConn::Transaction(_) => uow,
                    _ => None,
                };
                Ok(Target::Sql(SqlTarget::new(conn, dialect, uow, app)))
            }
            Access::Write => {
                if let Some(uow) = Uow::current().filter(|uow| same_pool(self, uow))
                    && !uow.is_finished().await
                {
                    let tx = uow.tx().await?;
                    return Ok(Target::Sql(SqlTarget::new(
                        SqlConn::Transaction(tx),
                        dialect,
                        Some(uow),
                        app,
                    )));
                }
                Ok(Target::Sql(SqlTarget::new(
                    SqlConn::Pool(self.conn()),
                    dialect,
                    None,
                    app,
                )))
            }
        }
    }
}

impl Executor for Uow {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        let tx = self.tx().await?;
        Ok(Target::Sql(SqlTarget::new(
            SqlConn::Transaction(tx),
            Dialect::from_backend(self.db().backend())?,
            Some(self.clone()),
            self.db().app(),
        )))
    }
}

impl Executor for DatabaseConnection {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        Ok(Target::Sql(SqlTarget::new(
            SqlConn::Pool(self),
            Dialect::from_backend(ConnectionTrait::get_database_backend(self))?,
            None,
            None,
        )))
    }
}

impl Executor for DatabaseTransaction {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        Ok(Target::Sql(SqlTarget::new(
            SqlConn::TransactionRef(self),
            Dialect::from_backend(ConnectionTrait::get_database_backend(self))?,
            None,
            None,
        )))
    }
}

impl<E: Executor + ?Sized> Executor for Arc<E> {
    fn target(&self, access: Access) -> impl Future<Output = Result<Target<'_>, OrmError>> + Send {
        (**self).target(access)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_keep_microseconds() {
        let at = DateTime::from_timestamp(1_700_000_000, 123_456_789).unwrap();
        assert_eq!(truncate_to_micros(at).timestamp_subsec_nanos(), 123_456_000);
    }
}
