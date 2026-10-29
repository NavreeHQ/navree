//! A savepoint of a unit of work's transaction, for work that may fail
//! without failing the unit of work.
//!
//! navree-db's `Uow::savepoint` nests on a unit of work that runs on
//! navree-db's own transaction only, and the ORM takes a `Uow`, not the
//! `Transaction` it returns. Plain `SAVEPOINT` statements run through the
//! unit of work on any of its transactions, on every database.

use navree_db::{DbError, Executor as _, Uow};

/// An open savepoint. End it with [`release`](Savepoint::release) (keep
/// the work) or [`rollback`](Savepoint::rollback) (undo it): dropped
/// unfinished, it stays in the transaction until the transaction ends.
pub(crate) struct Savepoint<'a> {
    uow: &'a Uow,
    name: &'static str,
}

impl<'a> Savepoint<'a> {
    /// Opens the savepoint `name` in the transaction of `uow` (begun if
    /// need be).
    pub(crate) async fn begin(uow: &'a Uow, name: &'static str) -> Result<Self, DbError> {
        uow.execute_script(&format!("SAVEPOINT {name}")).await?;
        Ok(Savepoint { uow, name })
    }

    /// Keeps what was done since the savepoint.
    pub(crate) async fn release(self) -> Result<(), DbError> {
        self.uow
            .execute_script(&format!("RELEASE SAVEPOINT {}", self.name))
            .await?;
        Ok(())
    }

    /// Undoes what was done since the savepoint, leaving the transaction
    /// usable (on PostgreSQL a failed statement aborts a transaction until
    /// it rolls back to a savepoint).
    pub(crate) async fn rollback(self) -> Result<(), DbError> {
        self.uow
            .execute_script(&format!("ROLLBACK TO SAVEPOINT {}", self.name))
            .await?;
        self.release().await
    }
}
