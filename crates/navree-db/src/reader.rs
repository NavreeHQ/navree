//! [`Reader`]: the connection [`Db::reader`](crate::Db::reader) reads
//! with.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, ExecResult,
    QueryResult, Statement,
};

/// A future as `#[async_trait]` methods of [`ConnectionTrait`] return it.
type BoxedFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where [`Db::reader`](crate::Db::reader) reads: the open transaction of
/// the task's unit of work, or the pool. A [`ConnectionTrait`], so it goes
/// wherever SeaORM takes a connection (`.one(&db.reader().await)`).
pub enum Reader<'a> {
    /// The pool.
    Pool(&'a DatabaseConnection),
    /// The current unit of work's open transaction.
    Transaction(Arc<DatabaseTransaction>),
}

impl Reader<'_> {
    /// Whether reads go through the current unit of work's transaction.
    pub fn in_transaction(&self) -> bool {
        matches!(self, Reader::Transaction(_))
    }
}

impl std::fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Reader::Pool(_) => "Reader::Pool",
            Reader::Transaction(_) => "Reader::Transaction",
        })
    }
}

// `ConnectionTrait` is an `#[async_trait]` trait; these are its methods in
// the macro's expanded form, each delegating to the inner connection.
impl ConnectionTrait for Reader<'_> {
    fn get_database_backend(&self) -> DbBackend {
        match self {
            Reader::Pool(conn) => conn.get_database_backend(),
            Reader::Transaction(tx) => tx.get_database_backend(),
        }
    }

    fn execute_raw<'life0, 'async_trait>(
        &'life0 self,
        stmt: Statement,
    ) -> BoxedFuture<'async_trait, Result<ExecResult, DbErr>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        match self {
            Reader::Pool(conn) => conn.execute_raw(stmt),
            Reader::Transaction(tx) => tx.execute_raw(stmt),
        }
    }

    fn execute_unprepared<'life0, 'life1, 'async_trait>(
        &'life0 self,
        sql: &'life1 str,
    ) -> BoxedFuture<'async_trait, Result<ExecResult, DbErr>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        match self {
            Reader::Pool(conn) => conn.execute_unprepared(sql),
            Reader::Transaction(tx) => tx.execute_unprepared(sql),
        }
    }

    fn query_one_raw<'life0, 'async_trait>(
        &'life0 self,
        stmt: Statement,
    ) -> BoxedFuture<'async_trait, Result<Option<QueryResult>, DbErr>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        match self {
            Reader::Pool(conn) => conn.query_one_raw(stmt),
            Reader::Transaction(tx) => tx.query_one_raw(stmt),
        }
    }

    fn query_all_raw<'life0, 'async_trait>(
        &'life0 self,
        stmt: Statement,
    ) -> BoxedFuture<'async_trait, Result<Vec<QueryResult>, DbErr>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        match self {
            Reader::Pool(conn) => conn.query_all_raw(stmt),
            Reader::Transaction(tx) => tx.query_all_raw(stmt),
        }
    }

    fn support_returning(&self) -> bool {
        match self {
            Reader::Pool(conn) => conn.support_returning(),
            Reader::Transaction(tx) => tx.support_returning(),
        }
    }

    fn is_mock_connection(&self) -> bool {
        match self {
            Reader::Pool(conn) => conn.is_mock_connection(),
            Reader::Transaction(tx) => tx.is_mock_connection(),
        }
    }
}
