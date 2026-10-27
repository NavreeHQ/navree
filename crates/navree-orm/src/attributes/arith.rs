//! Increments and decrements through a cast: value objects (money,
//! counters) that know how to add up.

use crate::column::{Assignment, Column};
use crate::error::Result;
use crate::exec::{Access, Executor};
use crate::model::Model;
use crate::persist;
use crate::record::Record;

impl<M: Model> Record<M> {
    /// Adds `amount` to cast field `column`, atomically: the record's field
    /// becomes the cast's [`increment`](crate::cast::Cast::increment) of
    /// it, and the database runs `SET column = column + <amount's stored
    /// form>` (plus the update timestamp), so concurrent increments add
    /// up. The stored form must be a number (money stored as cents, a
    /// counter stored as an integer).
    ///
    /// Returns `false` when a hook cancelled it; a record that is not
    /// stored is [`OrmError::NotPersisted`](crate::OrmError::NotPersisted),
    /// never a table-wide increment.
    ///
    /// ```ignore
    /// // `balance: Money` with `#[model(cast = MoneyCents)]`, where MoneyCents
    /// // implements Cast::increment and Cast::decrement.
    /// account.increment_cast(&uow, Account::BALANCE, Money::eur(5)).await?;
    /// ```
    pub async fn increment_cast<E, T>(&mut self, exec: &E, column: Column<M, T>, amount: T) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Send + Sync,
    {
        self.increment_cast_with(exec, column, amount, std::iter::empty()).await
    }

    /// Like [`increment_cast`](Record::increment_cast), also setting
    /// `extra` columns.
    pub async fn increment_cast_with<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
        extra: impl IntoIterator<Item = Assignment>,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Send + Sync,
    {
        let extra: Vec<Assignment> = extra.into_iter().collect();
        let target = exec.target(Access::Write).await?;
        persist::increment_cast(&target, self, &column, &amount, false, extra).await
    }

    /// Subtracts `amount` from cast field `column` (the cast's
    /// [`decrement`](crate::cast::Cast::decrement)), atomically.
    pub async fn decrement_cast<E, T>(&mut self, exec: &E, column: Column<M, T>, amount: T) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Send + Sync,
    {
        self.decrement_cast_with(exec, column, amount, std::iter::empty()).await
    }

    /// Like [`decrement_cast`](Record::decrement_cast), also setting
    /// `extra` columns.
    pub async fn decrement_cast_with<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
        extra: impl IntoIterator<Item = Assignment>,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Send + Sync,
    {
        let extra: Vec<Assignment> = extra.into_iter().collect();
        let target = exec.target(Access::Write).await?;
        persist::increment_cast(&target, self, &column, &amount, true, extra).await
    }
}
