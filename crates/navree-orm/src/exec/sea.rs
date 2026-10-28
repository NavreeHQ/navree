//! SeaORM's connections and transactions under the ORM, while crates move
//! off SeaORM (docs/superpowers/specs/2026-10-28-navree-data-layer-on-sqlx.md):
//! a `DatabaseConnection` or `DatabaseTransaction` given as the executor, a
//! `Db` made from a SeaORM connection, and the savepoints of a unit of work
//! on SeaORM's transaction. Statements go through navree-db's bridge
//! ([`navree_db::compat::SeaExecutor`]), so values, rows and errors are
//! navree-db's; streams read through SeaORM's own cursor. The module goes
//! when units of work run on navree-db's transactions only.

use futures::StreamExt;
use navree_db::compat::{self, SeaExecutor as Bridge};
use navree_db::{Db, Executor as DbExecutor, Row as DbRow, Uow};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, StreamTrait, TransactionTrait,
};

use super::{Access, Executor, Guarded, RowsStream, SqlConn, SqlTarget, Target};
use crate::error::OrmError;
use crate::sql::{Dialect, Statement};

/// A SeaORM connection or transaction statements run on.
pub(super) enum SeaConn<'a> {
    /// A connection given as the executor.
    Pool(&'a DatabaseConnection),
    /// The connection of a `Db` made from a SeaORM connection.
    Owned(DatabaseConnection),
    /// A transaction (or savepoint).
    Tx(&'a DatabaseTransaction),
}

impl SeaConn<'_> {
    /// The SeaORM connection of `db` (a `Db` without a pool of navree-db's
    /// own).
    pub(super) fn from_db(db: &Db) -> SeaConn<'static> {
        SeaConn::Owned(db.conn().clone())
    }

    /// The transaction's address; `None` on a connection.
    pub(super) fn transaction_id(&self) -> Option<usize> {
        match self {
            SeaConn::Pool(_) | SeaConn::Owned(_) => None,
            SeaConn::Tx(tx) => Some(std::ptr::from_ref::<DatabaseTransaction>(tx) as usize),
        }
    }

    /// Whether statements run in a transaction.
    pub(super) fn in_transaction(&self) -> bool {
        self.transaction_id().is_some()
    }

    /// navree-db's statements on this connection.
    pub(super) fn executor(&self) -> SeaExecutor<'_> {
        match self {
            SeaConn::Pool(conn) => SeaExecutor::Conn(Bridge::new(*conn)),
            SeaConn::Owned(conn) => SeaExecutor::Conn(Bridge::new(conn)),
            SeaConn::Tx(tx) => SeaExecutor::Tx(Bridge::new(*tx)),
        }
    }

    /// The same connection, borrowed.
    pub(super) fn reborrow(&self) -> SeaConn<'_> {
        match self {
            SeaConn::Pool(conn) => SeaConn::Pool(conn),
            SeaConn::Owned(conn) => SeaConn::Pool(conn),
            SeaConn::Tx(tx) => SeaConn::Tx(tx),
        }
    }

    /// A transaction on the connection, or a savepoint of the transaction.
    pub(super) async fn begin(&self) -> Result<SeaTransaction, OrmError> {
        let tx = match self {
            SeaConn::Pool(conn) => conn.begin().await?,
            SeaConn::Owned(conn) => conn.begin().await?,
            SeaConn::Tx(tx) => tx.begin().await?,
        };
        Ok(SeaTransaction(tx))
    }

    /// The rows of `statement` as the database sends them, through SeaORM's
    /// cursor; a transaction's stream marks it busy (`transaction`).
    pub(super) async fn stream(
        &self,
        statement: &Statement,
        transaction: Option<usize>,
    ) -> Result<RowsStream<'_>, OrmError> {
        let backend = statement.dialect.backend();
        let statement = sea_orm::Statement::from_sql_and_values(
            compat::db_backend(backend),
            statement.sql.clone(),
            statement
                .values
                .iter()
                .map(|value| compat::to_sea_value_on(backend, value)),
        );
        Ok(match self {
            SeaConn::Pool(conn) => Guarded::new(
                conn.stream_raw(statement).await?.map(|row| row.map(DbRow::from)),
                None,
            )
            .boxed(),
            SeaConn::Owned(conn) => Guarded::new(
                conn.stream_raw(statement).await?.map(|row| row.map(DbRow::from)),
                None,
            )
            .boxed(),
            SeaConn::Tx(tx) => Guarded::new(
                tx.stream_raw(statement).await?.map(|row| row.map(DbRow::from)),
                transaction,
            )
            .boxed(),
        })
    }
}

/// navree-db's executor on a SeaORM connection or transaction.
pub(super) enum SeaExecutor<'x> {
    Conn(Bridge<'x, DatabaseConnection>),
    Tx(Bridge<'x, DatabaseTransaction>),
}

impl SeaExecutor<'_> {
    pub(super) fn get(&self) -> &dyn DbExecutor {
        match self {
            SeaExecutor::Conn(bridge) => bridge,
            SeaExecutor::Tx(bridge) => bridge,
        }
    }
}

/// A SeaORM transaction or savepoint the ORM began.
pub(super) struct SeaTransaction(DatabaseTransaction);

impl SeaTransaction {
    /// A savepoint of the transaction of `uow`, a unit of work on SeaORM's
    /// transaction (begun if need be).
    pub(super) async fn savepoint_of(uow: &Uow) -> Result<SeaTransaction, OrmError> {
        Ok(SeaTransaction(uow.tx().await?.begin().await?))
    }

    pub(super) fn conn(&self) -> SeaConn<'_> {
        SeaConn::Tx(&self.0)
    }

    pub(super) async fn commit(self) -> Result<(), OrmError> {
        Ok(self.0.commit().await?)
    }

    pub(super) async fn rollback(self) -> Result<(), OrmError> {
        Ok(self.0.rollback().await?)
    }
}

/// The dialect of a SeaORM connection.
fn dialect_of(conn: &impl ConnectionTrait) -> Dialect {
    Dialect::from(compat::backend_of(conn.get_database_backend()))
}

/// A SeaORM connection runs every operation on itself, with no unit of
/// work (no change events).
impl Executor for DatabaseConnection {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        Ok(Target::Sql(SqlTarget::new(
            SqlConn::Sea(SeaConn::Pool(self)),
            dialect_of(self),
            None,
            None,
        )))
    }
}

/// A SeaORM transaction runs every operation in itself, with no unit of
/// work (no change events).
impl Executor for DatabaseTransaction {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        Ok(Target::Sql(SqlTarget::new(
            SqlConn::Sea(SeaConn::Tx(self)),
            dialect_of(self),
            None,
            None,
        )))
    }
}
