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
    /// ```no_run
    /// # use navree_orm::__doc_models::{Record, Result, Uow};
    /// use navree_orm::Model;
    /// use navree_orm::cast::{Cast, CastError};
    ///
    /// /// An amount in cents that adds up through its cast.
    /// #[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
    /// pub struct Cents(pub i64);
    ///
    /// pub struct AsCents;
    ///
    /// impl Cast<Cents> for AsCents {
    ///     type Stored = i64;
    ///
    ///     fn get(stored: i64) -> Result<Cents, CastError> {
    ///         Ok(Cents(stored))
    ///     }
    ///
    ///     fn set(value: &Cents) -> Result<i64, CastError> {
    ///         Ok(value.0)
    ///     }
    ///
    ///     fn increment(value: &Cents, amount: &Cents) -> Result<Cents, CastError> {
    ///         value.0.checked_add(amount.0).map(Cents).ok_or_else(|| CastError::new("the balance overflows"))
    ///     }
    /// }
    ///
    /// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
    /// #[model(table = "accounts")]
    /// pub struct Account {
    ///     pub id: i64,
    ///     #[model(cast = AsCents)]
    ///     pub balance: Cents,
    /// }
    ///
    /// # async fn demo(uow: Uow, mut account: Record<Account>) -> navree_orm::Result<()> {
    /// account.increment_cast(&uow, Account::BALANCE, Cents(500)).await?;
    /// # Ok(()) }
    /// ```
    pub async fn increment_cast<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Send + Sync,
    {
        self.increment_cast_with(exec, column, amount, std::iter::empty())
            .await
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
    pub async fn decrement_cast<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Send + Sync,
    {
        self.decrement_cast_with(exec, column, amount, std::iter::empty())
            .await
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
