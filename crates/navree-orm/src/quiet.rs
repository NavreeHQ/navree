//! Quiet operations: writes no listener hears.
//!
//! Each runs its loud counterpart inside
//! [`without_events`](crate::without_events): observers, closure
//! listeners, typed application events and broadcasts are silenced,
//! behaviours (touching parents, filling a workspace column) still run, and
//! inside a unit of work the change is still recorded for the audit trail.

use crate::column::Column;
use crate::error::Result;
use crate::events::without_events;
use crate::exec::Executor;
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::value::Numeric;

impl<M: Model> Record<M> {
    /// [`save`](Record::save) without model events.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, mut deal: Record<Deal>) -> Result<()> {
    /// deal.views += 1;
    /// deal.save_quietly(&uow).await?; // no observer hears a view count
    /// # Ok(()) }
    /// ```
    pub async fn save_quietly<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        without_events(self.save(exec)).await
    }

    /// [`update`](Record::update) without model events.
    pub async fn update_quietly<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        change: impl FnOnce(&mut M) + Send,
    ) -> Result<bool> {
        without_events(self.update(exec, change)).await
    }

    /// [`delete`](Record::delete) without model events.
    pub async fn delete_quietly<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        without_events(self.delete(exec)).await
    }

    /// [`touch`](Record::touch) without model events.
    pub async fn touch_quietly<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        without_events(self.touch(exec)).await
    }

    /// [`increment`](Record::increment) without model events.
    pub async fn increment_quietly<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        without_events(self.increment(exec, column, amount)).await
    }

    /// [`decrement`](Record::decrement) without model events.
    pub async fn decrement_quietly<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        without_events(self.decrement(exec, column, amount)).await
    }

    /// [`increment_each`](Record::increment_each) without model events.
    pub async fn increment_each_quietly<E, T>(
        &mut self,
        exec: &E,
        steps: impl IntoIterator<Item = (Column<M, T>, T)>,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        let steps: Vec<(Column<M, T>, T)> = steps.into_iter().collect();
        without_events(self.increment_each(exec, steps)).await
    }

    /// [`decrement_each`](Record::decrement_each) without model events.
    pub async fn decrement_each_quietly<E, T>(
        &mut self,
        exec: &E,
        steps: impl IntoIterator<Item = (Column<M, T>, T)>,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        let steps: Vec<(Column<M, T>, T)> = steps.into_iter().collect();
        without_events(self.decrement_each(exec, steps)).await
    }
}

impl<M: Model> Builder<M> {
    /// Inserts `model` without model events and returns its record (the
    /// builder's conditions do not apply to an insert).
    pub async fn create_quietly<E: Executor + ?Sized>(self, exec: &E, model: M) -> Result<Record<M>> {
        without_events(M::create(exec, model)).await
    }
}
