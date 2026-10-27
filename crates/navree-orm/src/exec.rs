//! Where queries run: [`Executor`], [`Target`] and [`ExecContext`].
//!
//! Terminal methods take an executor: `Deal::query().get(&db)`,
//! `deal.save(&uow)`. An executor resolves a [`Target`] per operation: an
//! SQL connection or transaction (with the unit of work whose change events
//! it records), or a document driver.
//!
//! | Executor | Reads | Writes |
//! |---|---|---|
//! | [`Db`] | the task's current unit of work when its transaction is open on this pool, else a read replica (the writer with none, after the unit of work wrote, or when asked) | the task's current unfinished unit of work on this pool, else the pool |
//! | [`Uow`] | its transaction | its transaction, change events recorded |
//! | `DatabaseConnection`, `DatabaseTransaction` | as given | as given, no change events |
//! | `Arc<E>` | as `E` | as `E` |
//!
//! `Db` joins the current unit of work (the request's, in a handler) the
//! way the rest of Navree does: on SQLite a write through a second pool
//! connection would wait for the request's write lock until
//! `busy_timeout`, and joining keeps the write in the request's
//! transaction and audit trail.
//!
//! **Connections.** An operation runs on the connection its query names
//! (`on_connection`), else on its model's (`#[model(connection = "..")]`)
//! when the executor is the default connection, else on the executor's
//! ([`Route`]). Given the default `Db`, it reaches the named connection
//! through the app's `DbConnections`; inside [`using_connection`] another
//! connection stands in for the default one. A unit of work spans one
//! connection: given a [`Uow`], an operation on another connection fails
//! with [`OrmError::Connection`]. SeaORM connections, transactions and
//! document stores run what they are given.
//!
//! A document store's executor resolves a [`DocumentTarget`]: the driver,
//! the app, and where the change events of its writes go (navree-db's unit
//! of work it joined, or a [`ChangeRecorder`] of the store's own).

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::stream::{BoxStream, Stream, StreamExt};
use navree_contracts::Clock;
use navree_db::{ChangeEvent, DEFAULT_CONNECTION, Db, Reader, Uow};
use navree_kernel::AppContext;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, ExecResult, QueryResult, StreamTrait,
    TransactionTrait,
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

/// Where an operation asks to run, beyond its [`Access`]: the connection
/// its query names (`on_connection`), its model's connection
/// (`#[model(connection = "..")]`), and whether its reads go to the writer
/// (`use_write`). Terminal methods hand it to
/// [`Executor::target_routed`].
///
/// ```
/// use navree_orm::Route;
///
/// let route = Route::new().for_model(Some("analytics")).writer(true);
/// assert_eq!(route.connection_for("default"), Some("analytics"));
/// // A named connection's executor runs a model's operations itself...
/// assert_eq!(route.connection_for("reports"), None);
/// // ...unless the query names a connection.
/// assert_eq!(route.on(Some("archive")).connection_for("reports"), Some("archive"));
/// assert!(route.reads_from_writer());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Route<'a> {
    connection: Option<&'a str>,
    model: Option<&'a str>,
    writer: bool,
}

impl<'a> Route<'a> {
    /// No preference: the executor's own connection, reads from its
    /// replicas.
    pub const fn new() -> Self {
        Route {
            connection: None,
            model: None,
            writer: false,
        }
    }

    /// On connection `name` (`on_connection`), whatever the executor's;
    /// `None` leaves the choice to the model and the executor.
    #[must_use]
    pub const fn on(self, name: Option<&'a str>) -> Self {
        Route {
            connection: name,
            ..self
        }
    }

    /// For a model living on connection `name`: followed when the
    /// executor is the default connection.
    #[must_use]
    pub const fn for_model(self, name: Option<&'a str>) -> Self {
        Route {
            model: name,
            ..self
        }
    }

    /// Reads go to the writer, even where read replicas exist, when
    /// `writer` (`use_write`).
    #[must_use]
    pub const fn writer(self, writer: bool) -> Self {
        Route { writer, ..self }
    }

    /// The connection the query names.
    pub const fn connection(&self) -> Option<&'a str> {
        self.connection
    }

    /// The connection of the query's model.
    pub const fn model_connection(&self) -> Option<&'a str> {
        self.model
    }

    /// Whether reads go to the writer.
    pub const fn reads_from_writer(&self) -> bool {
        self.writer
    }

    /// The connection an executor on connection `own` sends the operation
    /// to: the one the query names, else, when `own` is the default
    /// connection, the model's; `None` keeps it on `own`.
    pub fn connection_for(&self, own: &str) -> Option<&'a str> {
        let wanted = match self.connection {
            Some(name) => Some(name),
            None if own == DEFAULT_CONNECTION => self.model,
            None => None,
        };
        wanted.filter(|name| *name != own)
    }
}

tokio::task_local! {
    /// The connection standing in for the default one ([`using_connection`]).
    static USING_CONNECTION: Arc<str>;
}

/// Runs `future` with connection `name` standing in for the default one:
/// operations given the default [`Db`] run on `name`, unless their query
/// or model names another connection. Task-local, like
/// [`using_writer`]: tasks spawned inside start without it.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db) -> Result<()> {
/// let archived = navree_orm::using_connection("archive", Deal::query().count(&db)).await?;
/// # Ok(()) }
/// ```
pub async fn using_connection<F: Future>(name: &str, future: F) -> F::Output {
    USING_CONNECTION.scope(Arc::from(name), future).await
}

/// The connection [`using_connection`] set for the running task.
pub fn scoped_connection() -> Option<Arc<str>> {
    USING_CONNECTION.try_with(Arc::clone).ok()
}

/// Runs `future` with every read of the task going to the writer, even
/// where read replicas exist (see [`Db::using_writer`]): for reads that
/// must see a write made outside the task's unit of work.
pub async fn using_writer<F: Future>(future: F) -> F::Output {
    Db::using_writer(future).await
}

/// Something ORM operations run on: [`Db`], [`Uow`], a SeaORM connection or
/// transaction, a document store (see [`DocumentDriver`]), or an `Arc` of
/// one.
pub trait Executor: Send + Sync {
    /// Where the next operation runs.
    fn target(&self, access: Access) -> impl Future<Output = Result<Target<'_>, OrmError>> + Send;

    /// Where the next operation runs, following `route` (see [`Route`]).
    /// [`Db`] sends it to the connection the route asks for and reads from
    /// the writer when asked; [`Uow`] refuses another connection; other
    /// executors run it where they run everything.
    fn target_routed(
        &self,
        access: Access,
        route: Route<'_>,
    ) -> impl Future<Output = Result<Target<'_>, OrmError>> + Send {
        let _ = route;
        self.target(access)
    }
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
    /// The connection whose statement and transaction hooks run
    /// ([`Db::before_executing`], [`Db::before_starting_transaction`]).
    db: Option<Db>,
}

enum SqlConn<'a> {
    Pool(&'a DatabaseConnection),
    /// A pool handle of a [`Db`] (cheap to clone), which may be a named
    /// connection resolved for this operation.
    Owned(DatabaseConnection),
    Transaction(Arc<DatabaseTransaction>),
    TransactionRef(&'a DatabaseTransaction),
}

/// Where the writes through a [`DocumentTarget`] record their
/// [`ChangeEvent`]s when the target joins no navree-db unit of work: a
/// document store's own unit of work (MongoDB's) collects them for its
/// observers. See [`DocumentTarget::recording_changes`].
pub trait ChangeRecorder: Send + Sync + 'static {
    /// Records `event` (created, updated or deleted, with the stored
    /// values).
    fn record_change(&self, event: ChangeEvent);
}

/// A document store, the app it belongs to, and where the change events
/// of its writes go.
#[derive(Clone)]
pub struct DocumentTarget {
    driver: Arc<dyn DocumentDriver>,
    app: Option<AppContext>,
    uow: Option<Uow>,
    changes: Option<Arc<dyn ChangeRecorder>>,
}

impl DocumentTarget {
    /// The document store `driver`, in `app` (for clocks and other app
    /// services), if any. Its writes record no change events until
    /// [`with_uow`](Self::with_uow) or
    /// [`recording_changes`](Self::recording_changes) says where.
    pub fn new(driver: Arc<dyn DocumentDriver>, app: Option<AppContext>) -> Self {
        DocumentTarget {
            driver,
            app,
            uow: None,
            changes: None,
        }
    }

    /// Joins navree-db's unit of work `uow`: the writes' change events go
    /// to it (its observers, the audit trail, see them when it commits),
    /// and lifecycle hooks find it in their [`ExecContext`]. The writes
    /// themselves run where the driver runs them (a MongoDB transaction
    /// that commits after the unit of work, say).
    pub fn with_uow(mut self, uow: Uow) -> Self {
        self.uow = Some(uow);
        self
    }

    /// Records the writes' change events with `recorder` (when no unit of
    /// work is joined; a joined one takes them).
    pub fn recording_changes(mut self, recorder: Arc<dyn ChangeRecorder>) -> Self {
        self.changes = Some(recorder);
        self
    }

    /// The driver.
    pub fn driver(&self) -> &dyn DocumentDriver {
        &*self.driver
    }

    /// The app, if the store belongs to one.
    pub fn app(&self) -> Option<&AppContext> {
        self.app.as_ref()
    }

    /// The navree-db unit of work the target joined, if any.
    pub fn uow(&self) -> Option<&Uow> {
        self.uow.as_ref()
    }
}

/// What a running operation knows about where it runs: the app (for its
/// clock and other services), the unit of work, the dialect, the precision
/// the database keeps in date-times. Lifecycle hooks and casts receive it.
#[derive(Clone)]
pub struct ExecContext {
    app: Option<AppContext>,
    uow: Option<Uow>,
    dialect: Option<Dialect>,
    driver: Option<&'static str>,
    time_precision: u32,
}

/// The fractional-second digits SQL databases keep (and the ORM's
/// timestamps carry): microseconds.
const MICROSECONDS: u32 = 6;

impl Default for ExecContext {
    fn default() -> Self {
        ExecContext {
            app: None,
            uow: None,
            dialect: None,
            driver: None,
            time_precision: MICROSECONDS,
        }
    }
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

    /// The fractional-second digits the database keeps in date-times: 6
    /// (microseconds) on the SQL databases, the driver's
    /// [`time_precision`](DocumentDriver::time_precision) on a document
    /// store (3 on MongoDB).
    pub fn time_precision(&self) -> u32 {
        self.time_precision
    }

    /// The current time: the app's `dyn Clock` when it registers one
    /// (tests freeze time with a fixed clock), else the system clock;
    /// truncated to the [precision](Self::time_precision) the database
    /// keeps (microseconds on PostgreSQL, MySQL and SQLite, milliseconds on
    /// MongoDB), so a saved record and its stored row agree.
    pub async fn now(&self) -> DateTime<Utc> {
        let now = match &self.app {
            Some(app) => match app.try_get::<dyn Clock>().await {
                Some(clock) => clock.now(),
                None => Utc::now(),
            },
            None => Utc::now(),
        };
        truncate_to(now, self.time_precision)
    }
}

impl fmt::Debug for ExecContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecContext")
            .field("database", &self.database())
            .field("uow", &self.uow)
            .field("app", &self.app.is_some())
            .field("time_precision", &self.time_precision)
            .finish()
    }
}

/// `at` without its sub-microsecond part (the precision SQL databases
/// keep).
pub(crate) fn truncate_to_micros(at: DateTime<Utc>) -> DateTime<Utc> {
    truncate_to(at, MICROSECONDS)
}

/// `at` without the fractional-second digits after the first `digits` (at
/// most 9, nanoseconds).
pub(crate) fn truncate_to(at: DateTime<Utc>, digits: u32) -> DateTime<Utc> {
    let unit = 10_u32.pow(9 - digits.min(9));
    let nanos = at.timestamp_subsec_nanos();
    DateTime::from_timestamp(at.timestamp(), nanos - nanos % unit).unwrap_or(at)
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
                time_precision: MICROSECONDS,
            },
            Target::Document(document) => ExecContext {
                app: document.app.clone(),
                uow: document.uow.clone(),
                dialect: None,
                driver: Some(document.driver.name()),
                time_precision: document.driver.time_precision().min(9),
            },
        }
    }

    /// The unit of work writes are recorded in: the SQL target's, or the
    /// one a document target joined.
    pub fn uow(&self) -> Option<&Uow> {
        match self {
            Target::Sql(sql) => sql.uow.as_ref(),
            Target::Document(document) => document.uow.as_ref(),
        }
    }

    /// Records the change event of a write that ran on this target: in its
    /// unit of work, or with a document target's [`ChangeRecorder`];
    /// nowhere when it has neither (a write on the pool).
    pub fn record_change(&self, event: ChangeEvent) {
        match self {
            Target::Sql(sql) => {
                if let Some(uow) = &sql.uow {
                    uow.record_change(event);
                }
            }
            Target::Document(document) => match (&document.uow, &document.changes) {
                (Some(uow), _) => uow.record_change(event),
                (None, Some(recorder)) => recorder.record_change(event),
                (None, None) => {}
            },
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
                    SqlConn::Pool(_) | SqlConn::Owned(_) => Ok(None),
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
                SqlConn::Pool(conn) => {
                    sql.announce_transaction();
                    Ok(Some(conn.begin().await?))
                }
                SqlConn::Owned(conn) => {
                    sql.announce_transaction();
                    Ok(Some(conn.begin().await?))
                }
                SqlConn::Transaction(_) | SqlConn::TransactionRef(_) => {
                    self.begin_savepoint().await
                }
            },
            Target::Document(_) => Ok(None),
        }
    }

    /// A transaction of its own for a target on a pool connection (several
    /// statements that must succeed together); `None` when the target is
    /// already a transaction (the statements join it) or a document store.
    pub(crate) async fn begin_transaction(&self) -> Result<Option<DatabaseTransaction>, OrmError> {
        match self {
            Target::Sql(sql) => match &sql.conn {
                SqlConn::Pool(conn) => {
                    sql.announce_transaction();
                    Ok(Some(conn.begin().await?))
                }
                SqlConn::Owned(conn) => {
                    sql.announce_transaction();
                    Ok(Some(conn.begin().await?))
                }
                SqlConn::Transaction(_) | SqlConn::TransactionRef(_) => Ok(None),
            },
            Target::Document(_) => Ok(None),
        }
    }

    /// The same target running on `tx` (a savepoint of its transaction).
    pub(crate) fn on_transaction<'b>(&self, tx: &'b DatabaseTransaction) -> Target<'b> {
        match self {
            Target::Sql(sql) => Target::Sql(SqlTarget {
                db: sql.db.clone(),
                ..SqlTarget::new(
                    SqlConn::TransactionRef(tx),
                    sql.dialect,
                    sql.uow.clone(),
                    sql.app.clone(),
                )
            }),
            Target::Document(document) => Target::Document(document.clone()),
        }
    }

    /// The rows of `query`.
    pub async fn fetch(&self, query: &QueryIr) -> Result<Vec<Row>, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_select(query, sql.dialect)?;
                Ok(sql
                    .query_all_timed(&statement, query.timeout)
                    .await?
                    .into_iter()
                    .map(Row::from_sql)
                    .collect())
            }
            Target::Document(document) => {
                crate::testing::note_statement();
                document.driver.find(query).await
            }
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
                let Some(timeout) = query.timeout else {
                    return sql.stream(&statement).await;
                };
                match sql.dialect {
                    // The hint is in the statement.
                    Dialect::MySql => Ok(sql
                        .stream(&statement)
                        .await
                        .map_err(|error| error.timed_out(timeout))?
                        .map(move |row| row.map_err(|error| error.timed_out(timeout)))
                        .boxed()),
                    dialect => Err(OrmError::unsupported(
                        "a statement timeout on a cursor",
                        dialect.name(),
                    )),
                }
            }
            Target::Document(document) => {
                crate::testing::note_statement();
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
                match sql.query_one_timed(&statement, query.timeout).await? {
                    Some(row) => Row::from_sql(row).value("aggregate"),
                    None => Ok(Value::BigInt(None)),
                }
            }
            Target::Document(document) => {
                crate::testing::note_statement();
                document.driver.aggregate(query, aggregate).await
            }
        }
    }

    /// Whether `query` has any row.
    pub async fn exists(&self, query: &QueryIr) -> Result<bool, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_exists(query, sql.dialect)?;
                match sql.query_one_timed(&statement, query.timeout).await? {
                    Some(row) => {
                        let value = Row::from_sql(row).value("exists")?;
                        bool::from_value(value).map_err(|error| {
                            crate::row::decode_error("EXISTS", "exists", "bool".into(), &error)
                        })
                    }
                    None => Ok(false),
                }
            }
            Target::Document(document) => {
                crate::testing::note_statement();
                document.driver.exists(query).await
            }
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
            Target::Document(document) => {
                crate::testing::note_statement();
                document.driver.insert(insert).await
            }
        }
    }

    /// Runs an update; returns the number of rows changed.
    pub async fn update(&self, update: &UpdateIr) -> Result<u64, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_update(update, sql.dialect)?;
                Ok(sql.execute(&statement).await?.rows_affected())
            }
            Target::Document(document) => {
                crate::testing::note_statement();
                document.driver.update(update).await
            }
        }
    }

    /// Runs a delete; returns the number of rows deleted.
    pub async fn delete(&self, delete: &DeleteIr) -> Result<u64, OrmError> {
        match self {
            Target::Sql(sql) => {
                let statement = sql::compile_delete(delete, sql.dialect)?;
                Ok(sql.execute(&statement).await?.rows_affected())
            }
            Target::Document(document) => {
                crate::testing::note_statement();
                document.driver.delete(delete).await
            }
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
            db: None,
        }
    }

    /// The target, running the statement and transaction hooks of `db`.
    fn watched_by(mut self, db: &Db) -> Self {
        self.db = Some(db.clone());
        self
    }

    /// Runs the connection's before-executing hooks for `sql`.
    fn announce(&self, sql: &str) {
        if let Some(db) = &self.db {
            db.run_before_executing(sql);
        }
    }

    /// Runs the connection's before-transaction hooks.
    fn announce_transaction(&self) {
        if let Some(db) = &self.db {
            db.run_before_starting_transaction();
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

    /// Runs `statement` and returns its rows. Outside a transaction, a read
    /// that loses its connection runs once more on another pooled one (see
    /// [`OrmError::is_lost_connection`]).
    pub async fn query_all(&self, statement: &Statement) -> Result<Vec<QueryResult>, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        self.announce(&statement.sql);
        crate::testing::note_statement();
        let statement = statement.to_sea_orm();
        let rows = match &self.conn {
            SqlConn::Pool(conn) => {
                read_again_if_lost(conn, statement, |conn, s| conn.query_all_raw(s)).await
            }
            SqlConn::Owned(conn) => {
                read_again_if_lost(conn, statement, |conn, s| conn.query_all_raw(s)).await
            }
            SqlConn::Transaction(tx) => tx.query_all_raw(statement).await,
            SqlConn::TransactionRef(tx) => tx.query_all_raw(statement).await,
        };
        rows.map_err(|error| self.error(error))
    }

    /// Runs `statement` and returns its first row (read again once outside
    /// a transaction when the connection is lost, like
    /// [`query_all`](Self::query_all)).
    pub async fn query_one(&self, statement: &Statement) -> Result<Option<QueryResult>, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        self.announce(&statement.sql);
        crate::testing::note_statement();
        let statement = statement.to_sea_orm();
        let row = match &self.conn {
            SqlConn::Pool(conn) => {
                read_again_if_lost(conn, statement, |conn, s| conn.query_one_raw(s)).await
            }
            SqlConn::Owned(conn) => {
                read_again_if_lost(conn, statement, |conn, s| conn.query_one_raw(s)).await
            }
            SqlConn::Transaction(tx) => tx.query_one_raw(statement).await,
            SqlConn::TransactionRef(tx) => tx.query_one_raw(statement).await,
        };
        row.map_err(|error| self.error(error))
    }

    /// Runs `statement` for its effect.
    pub async fn execute(&self, statement: &Statement) -> Result<ExecResult, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        self.announce(&statement.sql);
        crate::testing::note_statement();
        let statement = statement.to_sea_orm();
        let result = match &self.conn {
            SqlConn::Pool(conn) => conn.execute_raw(statement).await,
            SqlConn::Owned(conn) => conn.execute_raw(statement).await,
            SqlConn::Transaction(tx) => tx.execute_raw(statement).await,
            SqlConn::TransactionRef(tx) => tx.execute_raw(statement).await,
        };
        result.map_err(|error| self.error(error))
    }

    /// The ORM error for a database error: SQLite without a registered
    /// `regexp` function cannot run regular expressions (`Unsupported`);
    /// anything else is the database's error.
    fn error(&self, error: sea_orm::DbErr) -> OrmError {
        if self.dialect == Dialect::Sqlite
            && error
                .to_string()
                .to_ascii_lowercase()
                .contains("no such function: regexp")
        {
            return OrmError::unsupported(
                "a regular expression without a `regexp` function registered on the connection",
                "SQLite",
            );
        }
        OrmError::Db(error)
    }

    /// Runs `sql` as written, without values (several statements may be
    /// separated by `;`).
    pub async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        self.announce(sql);
        Ok(match &self.conn {
            SqlConn::Pool(conn) => conn.execute_unprepared(sql).await?,
            SqlConn::Owned(conn) => conn.execute_unprepared(sql).await?,
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
        self.announce(&statement.sql);
        crate::testing::note_statement();
        let statement = statement.to_sea_orm();
        let rows: RowsStream<'_> = match &self.conn {
            SqlConn::Pool(conn) => conn
                .stream_raw(statement)
                .await?
                .map(|row| row.map(Row::from_sql).map_err(OrmError::from))
                .boxed(),
            SqlConn::Owned(conn) => conn
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

    /// [`query_all`](Self::query_all), stopped by the database after
    /// `timeout` when there is one (see `timeout` on the builders).
    pub(crate) async fn query_all_timed(
        &self,
        statement: &Statement,
        timeout: Option<Duration>,
    ) -> Result<Vec<QueryResult>, OrmError> {
        match self.timed(TimedRead::All(statement), timeout).await? {
            TimedRows::All(rows) => Ok(rows),
            TimedRows::One(row) => Ok(row.into_iter().collect()),
        }
    }

    /// [`query_one`](Self::query_one), stopped by the database after
    /// `timeout` when there is one.
    pub(crate) async fn query_one_timed(
        &self,
        statement: &Statement,
        timeout: Option<Duration>,
    ) -> Result<Option<QueryResult>, OrmError> {
        match self.timed(TimedRead::One(statement), timeout).await? {
            TimedRows::One(row) => Ok(row),
            TimedRows::All(rows) => Ok(rows.into_iter().next()),
        }
    }

    async fn read(&self, read: TimedRead<'_>) -> Result<TimedRows, OrmError> {
        match read {
            TimedRead::All(statement) => self.query_all(statement).await.map(TimedRows::All),
            TimedRead::One(statement) => self.query_one(statement).await.map(TimedRows::One),
        }
    }

    /// Runs `read`, stopped by the database after `timeout`: MySQL's hint
    /// is in the statement; PostgreSQL's `statement_timeout` is set for a
    /// transaction of its own around it; SQLite has no statement timeout.
    async fn timed(
        &self,
        read: TimedRead<'_>,
        timeout: Option<Duration>,
    ) -> Result<TimedRows, OrmError> {
        let Some(timeout) = timeout else {
            return self.read(read).await;
        };
        match self.dialect {
            Dialect::MySql => self
                .read(read)
                .await
                .map_err(|error| error.timed_out(timeout)),
            Dialect::Postgres => self.postgres_timed(read, timeout).await,
            Dialect::Sqlite => Err(OrmError::unsupported("a statement timeout", "SQLite")),
        }
    }

    /// `SET LOCAL statement_timeout` around `read`, in a transaction of its
    /// own on the pool, or a savepoint inside a transaction (which restores
    /// the previous timeout before it is released, and rolls back when the
    /// statement fails, so the transaction stays usable).
    async fn postgres_timed(
        &self,
        read: TimedRead<'_>,
        timeout: Duration,
    ) -> Result<TimedRows, OrmError> {
        ensure_free(self.conn.transaction_id())?;
        let in_transaction = self.in_transaction();
        let tx = match &self.conn {
            SqlConn::Pool(conn) => {
                self.announce_transaction();
                conn.begin().await?
            }
            SqlConn::Owned(conn) => {
                self.announce_transaction();
                conn.begin().await?
            }
            SqlConn::Transaction(tx) => tx.begin().await?,
            SqlConn::TransactionRef(tx) => tx.begin().await?,
        };
        let nested = SqlTarget {
            db: self.db.clone(),
            ..SqlTarget::new(
                SqlConn::TransactionRef(&tx),
                self.dialect,
                self.uow.clone(),
                self.app.clone(),
            )
        };
        let result = async {
            let previous = if in_transaction {
                let current = Statement {
                    sql: "SELECT current_setting('statement_timeout') AS previous".to_owned(),
                    values: Vec::new(),
                    dialect: Dialect::Postgres,
                };
                match nested.query_one(&current).await? {
                    Some(row) => Some(row.try_get::<String>("", "previous")?),
                    None => None,
                }
            } else {
                None
            };
            nested
                .execute_unprepared(&format!(
                    "SET LOCAL statement_timeout = {}",
                    sql::timeout_millis(timeout)
                ))
                .await?;
            let rows = nested
                .read(read)
                .await
                .map_err(|error| error.timed_out(timeout))?;
            if let Some(previous) = previous {
                nested
                    .execute(&Statement {
                        sql: "SELECT set_config('statement_timeout', $1, true)".to_owned(),
                        values: vec![Value::from(previous)],
                        dialect: Dialect::Postgres,
                    })
                    .await?;
            }
            Ok::<_, OrmError>(rows)
        }
        .await;
        drop(nested);
        match result {
            Ok(rows) => {
                tx.commit().await?;
                Ok(rows)
            }
            Err(error) => {
                if let Err(rollback) = tx.rollback().await {
                    tracing::warn!(target: "navree_orm", error = %rollback, "rolling back a timed-out statement failed");
                }
                Err(error)
            }
        }
    }
}

/// Runs `read` with `statement` on the pool `conn`, and once more (on
/// another pooled connection) when the connection was lost under it.
async fn read_again_if_lost<'c, T, F, Fut>(
    conn: &'c DatabaseConnection,
    statement: sea_orm::Statement,
    read: F,
) -> Result<T, sea_orm::DbErr>
where
    F: Fn(&'c DatabaseConnection, sea_orm::Statement) -> Fut,
    Fut: Future<Output = Result<T, sea_orm::DbErr>>,
{
    match read(conn, statement.clone()).await {
        Err(error) if crate::error::lost_connection(&error) => {
            tracing::warn!(target: "navree_orm", %error, "the connection was lost under a read; reading again");
            read(conn, statement).await
        }
        result => result,
    }
}

/// A read that may run under a statement timeout.
enum TimedRead<'s> {
    All(&'s Statement),
    One(&'s Statement),
}

/// What a [`TimedRead`] returns.
enum TimedRows {
    All(Vec<QueryResult>),
    One(Option<QueryResult>),
}

impl SqlConn<'_> {
    /// The identity of the transaction statements run in (its address),
    /// `None` on the pool.
    fn transaction_id(&self) -> Option<usize> {
        match self {
            SqlConn::Pool(_) | SqlConn::Owned(_) => None,
            SqlConn::Transaction(tx) => Some(Arc::as_ptr(tx) as usize),
            SqlConn::TransactionRef(tx) => {
                Some(std::ptr::from_ref::<DatabaseTransaction>(tx) as usize)
            }
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

/// Where an operation of `access` runs on `db`: the task's unit of work on
/// its pool (writes join it while it is unfinished, reads use its open
/// transaction), else the writer for writes, and for reads a replica or
/// the writer (`writer`, [`Db::using_writer`], or a unit of work that
/// wrote). A write marks the task's unit of work as having written, so
/// its later reads stay on the writer.
async fn db_target(db: &Db, access: Access, writer: bool) -> Result<Target<'static>, OrmError> {
    let dialect = Dialect::from_backend(db.backend())?;
    let app = db.app();
    let current = Uow::current().filter(|uow| same_pool(db, uow));
    let target = match access {
        Access::Read => match db.reader().await {
            Reader::Transaction(tx) => {
                SqlTarget::new(SqlConn::Transaction(tx), dialect, current, app)
            }
            Reader::Pool(conn) => {
                let conn = if writer { db.conn() } else { conn };
                SqlTarget::new(SqlConn::Owned(conn.clone()), dialect, None, app)
            }
        },
        Access::Write => match current {
            Some(uow) => {
                uow.mark_modified();
                if uow.is_finished().await {
                    SqlTarget::new(SqlConn::Owned(db.conn().clone()), dialect, None, app)
                } else {
                    let tx = uow.tx().await?;
                    SqlTarget::new(SqlConn::Transaction(tx), dialect, Some(uow), app)
                }
            }
            None => SqlTarget::new(SqlConn::Owned(db.conn().clone()), dialect, None, app),
        },
    };
    Ok(Target::Sql(target.watched_by(db)))
}

/// An [`OrmError::Connection`] for connection `name`.
fn connection_error(name: &str, message: impl Into<String>) -> OrmError {
    OrmError::Connection {
        connection: name.to_owned().into(),
        message: message.into(),
    }
}

impl Executor for Db {
    async fn target(&self, access: Access) -> Result<Target<'_>, OrmError> {
        self.target_routed(access, Route::new()).await
    }

    async fn target_routed(
        &self,
        access: Access,
        route: Route<'_>,
    ) -> Result<Target<'_>, OrmError> {
        let scoped = if route.connection.is_none() && self.is_default() {
            scoped_connection()
        } else {
            None
        };
        let route = match &scoped {
            // The scope stands in for the default connection, below the
            // model's own.
            Some(name) if route.model.is_none() => route.for_model(Some(&**name)),
            _ => route,
        };
        match route.connection_for(self.name()) {
            Some(name) => {
                let db = self.connection(name).await.map_err(|error| {
                    let message = match error {
                        sea_orm::DbErr::Custom(message) => message,
                        other => other.to_string(),
                    };
                    connection_error(name, message)
                })?;
                db_target(&db, access, route.writer).await
            }
            None => db_target(self, access, route.writer).await,
        }
    }
}

impl Executor for Uow {
    async fn target(&self, access: Access) -> Result<Target<'_>, OrmError> {
        if access == Access::Write {
            self.mark_modified();
        }
        let tx = self.tx().await?;
        Ok(Target::Sql(
            SqlTarget::new(
                SqlConn::Transaction(tx),
                Dialect::from_backend(self.db().backend())?,
                Some(self.clone()),
                self.db().app(),
            )
            .watched_by(self.db()),
        ))
    }

    async fn target_routed(
        &self,
        access: Access,
        route: Route<'_>,
    ) -> Result<Target<'_>, OrmError> {
        let own = self.db().name();
        if let Some(name) = route.connection_for(own) {
            return Err(connection_error(
                name,
                format!(
                    "the unit of work runs on connection `{own}`, and a unit of work spans one connection; pass a `Db` (it runs the operation on `{name}`) or a unit of work of `{name}`"
                ),
            ));
        }
        self.target(access).await
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

/// A target runs operations where it is (its connection or transaction,
/// its unit of work), so code holding one can call any method taking an
/// executor.
impl Executor for Target<'_> {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        Ok(self.reborrow())
    }
}

impl Target<'_> {
    /// The same target, borrowed: the same connection or transaction, unit
    /// of work and app. (A target is itself an [`Executor`] through this,
    /// so code handed one, such as a factory's after-creating hook, keeps
    /// writing in the same transaction.)
    pub fn reborrow(&self) -> Target<'_> {
        match self {
            Target::Sql(sql) => Target::Sql(SqlTarget {
                db: sql.db.clone(),
                ..SqlTarget::new(
                    match &sql.conn {
                        SqlConn::Pool(conn) => SqlConn::Pool(conn),
                        SqlConn::Owned(conn) => SqlConn::Pool(conn),
                        SqlConn::Transaction(tx) => SqlConn::Transaction(Arc::clone(tx)),
                        SqlConn::TransactionRef(tx) => SqlConn::TransactionRef(tx),
                    },
                    sql.dialect,
                    sql.uow.clone(),
                    sql.app.clone(),
                )
            }),
            Target::Document(document) => Target::Document(document.clone()),
        }
    }
}

impl<E: Executor + ?Sized> Executor for Arc<E> {
    fn target(&self, access: Access) -> impl Future<Output = Result<Target<'_>, OrmError>> + Send {
        (**self).target(access)
    }

    fn target_routed(
        &self,
        access: Access,
        route: Route<'_>,
    ) -> impl Future<Output = Result<Target<'_>, OrmError>> + Send {
        (**self).target_routed(access, route)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_keep_the_precision_asked_for() {
        let at = DateTime::from_timestamp(1_700_000_000, 123_456_789).unwrap();
        assert_eq!(truncate_to(at, 6).timestamp_subsec_nanos(), 123_456_000);
        assert_eq!(truncate_to(at, 3).timestamp_subsec_nanos(), 123_000_000);
        assert_eq!(truncate_to(at, 0).timestamp_subsec_nanos(), 0);
        assert_eq!(truncate_to(at, 9), at);
        assert_eq!(truncate_to(at, 12), at, "more than nanoseconds keeps all");
        assert_eq!(ExecContext::default().time_precision(), 6);
    }
}
