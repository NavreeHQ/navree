//! Events a record fires on demand: custom events and `replicating`.

use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor};
use crate::hooks::{Flow, Hooks, ModelEvent};
use crate::model::Model;
use crate::record::Record;

impl<M: Model> Record<M> {
    /// Fires custom event `event` (`"archived"`, `"escalated"`, ...) to the
    /// model's listeners at once: observers' [`custom`](crate::Observer::custom)
    /// method, and closure listeners of `ModelEvent::Custom(event)`.
    /// Returns [`Flow::Cancel`] when one of them cancels it, so the caller
    /// can skip the step it announces.
    ///
    /// Custom events run where they are fired (inside the transaction when
    /// `exec` is a unit of work); fire the "-ed" ones after the write they
    /// announce. Built-in event names are refused.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, mut deal: Record<Deal>) -> Result<()> {
    /// if deal.fire(&uow, "archiving").await? == Flow::Continue {
    ///     deal.update(&uow, |d| d.stage = Stage::Lost).await?;
    ///     deal.fire(&uow, "archived").await?;
    /// }
    /// # Ok(()) }
    /// ```
    pub async fn fire<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        event: &'static str,
    ) -> Result<Flow> {
        if ModelEvent::from_name(event).is_some() {
            return Err(OrmError::invalid(format!(
                "`{event}` is a built-in model event: fire events of your own (`archived`, `escalated`, ...)"
            )));
        }
        if event.trim().is_empty() {
            return Err(OrmError::invalid("a custom model event needs a name"));
        }
        let target = exec.target(Access::Write).await?;
        let ctx = target.context();
        let hooks = Hooks::<M>::resolve(&ctx).await?;
        hooks.before(ModelEvent::Custom(event), &ctx, self).await
    }

    /// An unsaved copy of the record, like [`replicate`](Record::replicate),
    /// after the model's listeners heard `replicating` on it (they may
    /// change the copy). `exec` decides which app's listeners hear it.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, deal: Record<Deal>) -> Result<()> {
    /// let mut copy = deal.replicate_on(&uow).await?;
    /// copy.title = format!("{} (renewal)", deal.title);
    /// copy.save(&uow).await?;
    /// # Ok(()) }
    /// ```
    pub async fn replicate_on<E: Executor + ?Sized>(&self, exec: &E) -> Result<Record<M>> {
        let mut copy = self.replicate();
        let target = exec.target(Access::Read).await?;
        let ctx = target.context();
        let hooks = Hooks::<M>::resolve(&ctx).await?;
        hooks
            .after(ModelEvent::Replicating, &ctx, &mut copy)
            .await?;
        Ok(copy)
    }

    /// An unsaved copy of the record without any listener hearing about it
    /// (what [`replicate`](Record::replicate) does; named for symmetry with
    /// the other quiet operations).
    pub fn replicate_quietly(&self) -> Record<M> {
        self.replicate()
    }
}
