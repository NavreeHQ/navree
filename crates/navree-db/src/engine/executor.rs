//! [`Executor`]: what statements run on — a pool, a transaction, a unit of
//! work, a [`Db`](crate::Db).

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use futures_util::Stream;
use futures_util::stream::BoxStream;
use navree_kernel::BoxFuture;

use crate::engine::driver::{DriverRow, Executed, RowStream};
use crate::engine::report::Reporter;
use crate::engine::{Backend, DbError, FromValue, Row, Statement, Value};

/// Something statements run on: a [`Pool`](crate::Pool), a
/// [`Transaction`](crate::Transaction), a [`Uow`](crate::Uow) (its
/// transaction) or a [`Db`](crate::Db) (the task's unit of work when it is
/// open, else the pool).
///
/// Every statement is timed and reported to the connection's query log,
/// listeners and hooks (`Db::on_query`, `QueryLog::capture`,
/// `Db::before_executing`), its telemetry and its slow-query log. The trait
/// is object safe (`&dyn Executor`); [`ExecutorExt`] adds typed reads.
///
/// ```no_run
/// # async fn demo(db: navree_db::Db) -> Result<(), navree_db::DbError> {
/// use navree_db::{Executor, ExecutorExt, Statement};
///
/// let open: i64 = db
///     .fetch_scalar(&Statement::portable("SELECT COUNT(*) FROM deals WHERE stage = ?", ["open".into()]))
///     .await?;
/// for row in db.fetch_all(&Statement::text("SELECT id, title FROM deals")).await? {
///     let title: String = row.get("title")?;
///     # let _ = title;
/// }
/// # let _ = open;
/// # Ok(()) }
/// ```
pub trait Executor: Send + Sync {
    /// The backend statements run on.
    fn backend(&self) -> Backend;

    /// Runs a statement that returns no rows: how many rows it affected,
    /// and the key the database assigned (MySQL, SQLite).
    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>>;

    /// Runs a query and returns its rows.
    fn fetch_all<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>>;

    /// Runs a query and returns its first row, if any.
    fn fetch_optional<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>>;

    /// Runs a query whose rows arrive one by one, as the database sends
    /// them. The stream holds a connection (a transaction's, which waits
    /// for it) until it ends or is dropped.
    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>>;

    /// Runs SQL that binds nothing and may hold several statements
    /// separated by `;` (DDL, scripts), without preparing it.
    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>>;

    /// Runs a query that must return a row; [`ErrorKind::NotFound`](crate::ErrorKind::NotFound)
    /// otherwise.
    fn fetch_one<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Row, DbError>> {
        Box::pin(async move {
            self.fetch_optional(statement).await?.ok_or_else(|| {
                DbError::not_found(format!("the query returned no row: {}", statement.sql()))
            })
        })
    }
}

/// Typed reads on any [`Executor`].
pub trait ExecutorExt: Executor {
    /// The first column of the one row a query returns, as `T`.
    fn fetch_scalar<'a, T: FromValue + Send + 'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<T, DbError>> {
        Box::pin(async move { self.fetch_one(statement).await?.get::<T>(0) })
    }

    /// The first column of every row, as `T`.
    fn fetch_column<'a, T: FromValue + Send + 'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<T>, DbError>> {
        Box::pin(async move {
            self.fetch_all(statement)
                .await?
                .iter()
                .map(|row| row.get::<T>(0))
                .collect()
        })
    }
}

impl<E: Executor + ?Sized> ExecutorExt for E {}

macro_rules! forward {
    ($($ty:ty),*) => {
        $(
            impl<E: Executor + ?Sized> Executor for $ty {
                fn backend(&self) -> Backend {
                    (**self).backend()
                }

                fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
                    (**self).execute(statement)
                }

                fn fetch_all<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
                    (**self).fetch_all(statement)
                }

                fn fetch_optional<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
                    (**self).fetch_optional(statement)
                }

                fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
                    (**self).fetch(statement)
                }

                fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
                    (**self).execute_script(sql)
                }

                fn fetch_one<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Row, DbError>> {
                    (**self).fetch_one(statement)
                }
            }
        )*
    };
}

forward!(&E, Arc<E>, Box<E>);

/// Rows of a driver stream as [`Row`]s, reported to the connection once the
/// stream ends or is dropped (with the rows read and the time since it
/// opened).
pub(crate) struct ReportingStream {
    rows: BoxStream<'static, Result<DriverRow, DbError>>,
    report: Option<PendingReport>,
}

struct PendingReport {
    reporter: Reporter,
    backend: Backend,
    sql: String,
    values: Vec<Value>,
    started: Instant,
    rows: u64,
    failed: bool,
}

impl PendingReport {
    fn send(self) {
        self.reporter.report(
            &self.sql,
            &self.values,
            self.backend,
            self.started.elapsed(),
            self.failed,
            Some(self.rows),
        );
    }
}

impl ReportingStream {
    pub(crate) fn new(
        rows: BoxStream<'static, Result<DriverRow, DbError>>,
        reporter: Reporter,
        backend: Backend,
        statement: &Statement,
    ) -> ReportingStream {
        ReportingStream {
            rows,
            report: Some(PendingReport {
                reporter,
                backend,
                sql: statement.sql().to_owned(),
                values: statement.values().to_vec(),
                started: Instant::now(),
                rows: 0,
                failed: false,
            }),
        }
    }

    pub(crate) fn boxed(self) -> RowStream {
        Box::pin(self)
    }
}

impl Stream for ReportingStream {
    type Item = Result<Row, DbError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = self.rows.as_mut().poll_next(cx);
        match &next {
            Poll::Ready(Some(Ok(_))) => {
                if let Some(report) = self.report.as_mut() {
                    report.rows += 1;
                }
            }
            Poll::Ready(Some(Err(_))) => {
                if let Some(report) = self.report.as_mut() {
                    report.failed = true;
                }
            }
            Poll::Ready(None) => {
                if let Some(report) = self.report.take() {
                    report.send();
                }
            }
            Poll::Pending => {}
        }
        next.map(|row| row.map(|row| row.map(Row::from_driver)))
    }
}

impl Drop for ReportingStream {
    fn drop(&mut self) {
        if let Some(report) = self.report.take() {
            report.send();
        }
    }
}
