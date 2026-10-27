//! [`SchemaConnection`]: what the schema builder needs from a database
//! connection.

use std::future::Future;
use std::pin::Pin;

use sea_orm::{ConnectionTrait, DbBackend, DbErr, QueryResult, Statement};

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What the schema builder needs from a connection: its backend, a way to
/// run a statement and a way to read rows.
///
/// Every SeaORM connection implements it: a `DatabaseConnection` (the
/// pool), a `DatabaseTransaction`, and the connection of a migration's
/// `SchemaManager`, so a [`Schema`](crate::Schema) works on any of them.
pub trait SchemaConnection: Send + Sync {
    /// The database backend.
    fn backend(&self) -> DbBackend;

    /// Runs `sql` (unprepared: DDL takes no parameters).
    fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), DbErr>>;

    /// Runs a query and returns its rows.
    fn query<'a>(&'a self, statement: Statement) -> BoxFuture<'a, Result<Vec<QueryResult>, DbErr>>;
}

impl<C: ConnectionTrait + Send + Sync> SchemaConnection for C {
    fn backend(&self) -> DbBackend {
        self.get_database_backend()
    }

    fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move { self.execute_unprepared(sql).await.map(drop) })
    }

    fn query<'a>(&'a self, statement: Statement) -> BoxFuture<'a, Result<Vec<QueryResult>, DbErr>> {
        Box::pin(async move { self.query_all_raw(statement).await })
    }
}
