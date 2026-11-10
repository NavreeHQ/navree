//! [`Observer`]: one method per model event.

use std::future::Future;

use navree_kernel::BoxFuture;

use crate::error::Result;
use crate::exec::ExecContext;
use crate::hooks::{Flow, HookKind, ModelEvent, ModelHook};
use crate::model::Model;
use crate::record::Record;

/// Reacts to model `M`'s lifecycle, one async method per event; implement
/// the ones you need (the others do nothing).
///
/// The `-ing` methods run before the write, inside its transaction: they
/// may change the record or cancel the write by returning
/// [`Flow::Cancel`] (the operation then returns `Ok(false)`), and an error
/// aborts it. The "-ed" methods of writes run once the unit of work
/// commits (at once outside one), on the record as it was when the event
/// happened, so `was_changed`, `get_changes` and `get_original` describe
/// that write; their errors are logged and reported, and never undo the
/// commit. Set [`IN_TRANSACTION`](Observer::IN_TRANSACTION) for an
/// observer whose "-ed" methods must write inside the transaction.
///
/// Declare stateless observers on the model, or register any observer in
/// a module:
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_orm::{ExecContext, ModuleBuilderOrmExt, Observer};
///
/// /// Keeps titles tidy and tells the team about big deals.
/// struct DealObserver;
///
/// impl Observer<Deal> for DealObserver {
///     async fn saving(&self, deal: &mut Record<Deal>, _ctx: &ExecContext) -> Result<Flow> {
///         deal.title = deal.title.trim().to_owned();
///         Ok(if deal.title.is_empty() { Flow::Cancel } else { Flow::Continue })
///     }
///
///     async fn created(&self, deal: &Record<Deal>, _ctx: &ExecContext) -> Result<()> {
///         if deal.amount_cents >= 1_000_000 {
///             println!("a big deal: {}", deal.title); // after the commit
///         }
///         Ok(())
///     }
/// }
///
/// struct DealsModule;
///
/// impl Module for DealsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.observe::<Deal>(DealObserver);
///     }
/// }
/// ```
///
/// (`#[model(observers(DealObserver))]` on `Deal` declares it on the model
/// instead: it then also runs on a pool opened by hand, without an app.)
pub trait Observer<M: Model>: Send + Sync + 'static {
    /// Whether the "-ed" methods of writes run inside the transaction (at
    /// once, so what they write commits or rolls back with the record)
    /// instead of after the commit. `false` by default.
    const IN_TRANSACTION: bool = false;

    /// A record was read; may change it (extras, decrypted values).
    fn retrieved(
        &self,
        record: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (record, ctx);
        async { Ok(()) }
    }

    /// A record is about to be inserted or updated (also a clean one).
    fn saving(
        &self,
        record: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<Flow>> + Send {
        let _ = (record, ctx);
        async { Ok(Flow::Continue) }
    }

    /// A new record is about to be inserted.
    fn creating(
        &self,
        record: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<Flow>> + Send {
        let _ = (record, ctx);
        async { Ok(Flow::Continue) }
    }

    /// A new record was inserted.
    fn created(
        &self,
        record: &Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (record, ctx);
        async { Ok(()) }
    }

    /// A changed record is about to be updated.
    fn updating(
        &self,
        record: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<Flow>> + Send {
        let _ = (record, ctx);
        async { Ok(Flow::Continue) }
    }

    /// A record was updated.
    fn updated(
        &self,
        record: &Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (record, ctx);
        async { Ok(()) }
    }

    /// A record was inserted or updated (also a clean one).
    fn saved(
        &self,
        record: &Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (record, ctx);
        async { Ok(()) }
    }

    /// A record is about to be deleted (or soft-deleted).
    fn deleting(
        &self,
        record: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<Flow>> + Send {
        let _ = (record, ctx);
        async { Ok(Flow::Continue) }
    }

    /// A record was deleted (or soft-deleted).
    fn deleted(
        &self,
        record: &Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (record, ctx);
        async { Ok(()) }
    }

    /// A record was soft-deleted.
    fn trashed(
        &self,
        record: &Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (record, ctx);
        async { Ok(()) }
    }

    /// A soft-deleting model's record is about to be removed for good
    /// ([`Record::is_force_deleting`] is `true` from here to `deleted`).
    fn force_deleting(
        &self,
        record: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<Flow>> + Send {
        let _ = (record, ctx);
        async { Ok(Flow::Continue) }
    }

    /// A soft-deleting model's record was removed for good.
    fn force_deleted(
        &self,
        record: &Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (record, ctx);
        async { Ok(()) }
    }

    /// A soft-deleted record is about to be restored.
    fn restoring(
        &self,
        record: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<Flow>> + Send {
        let _ = (record, ctx);
        async { Ok(Flow::Continue) }
    }

    /// A soft-deleted record was restored.
    fn restored(
        &self,
        record: &Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (record, ctx);
        async { Ok(()) }
    }

    /// An unsaved copy was made with [`Record::replicate_on`]; may change
    /// the copy.
    fn replicating(
        &self,
        copy: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (copy, ctx);
        async { Ok(()) }
    }

    /// The application fired custom event `event` with
    /// [`Record::fire`]; may cancel it.
    fn custom(
        &self,
        event: &'static str,
        record: &mut Record<M>,
        ctx: &ExecContext,
    ) -> impl Future<Output = Result<Flow>> + Send {
        let _ = (event, record, ctx);
        async { Ok(Flow::Continue) }
    }
}

/// An [`Observer`] as a [`ModelHook`]: what `#[model(observers(..))]` and
/// [`ModuleBuilderOrmExt::observe`](crate::ModuleBuilderOrmExt::observe)
/// register. A [`HookKind::Listener`] (or a
/// [`HookKind::TransactionalListener`] with
/// [`IN_TRANSACTION`](Observer::IN_TRANSACTION)).
pub struct ObserverHook<O>(O);

impl<O> ObserverHook<O> {
    /// The hook running `observer`.
    pub fn new(observer: O) -> Self {
        ObserverHook(observer)
    }

    /// The observer.
    pub fn observer(&self) -> &O {
        &self.0
    }
}

impl<M: Model, O: Observer<M>> ModelHook<M> for ObserverHook<O> {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<Flow>> {
        let observer = &self.0;
        match event {
            ModelEvent::Saving => Box::pin(observer.saving(record, ctx)),
            ModelEvent::Creating => Box::pin(observer.creating(record, ctx)),
            ModelEvent::Updating => Box::pin(observer.updating(record, ctx)),
            ModelEvent::Deleting => Box::pin(observer.deleting(record, ctx)),
            ModelEvent::ForceDeleting => Box::pin(observer.force_deleting(record, ctx)),
            ModelEvent::Restoring => Box::pin(observer.restoring(record, ctx)),
            ModelEvent::Custom(name) => Box::pin(observer.custom(name, record, ctx)),
            _ => Box::pin(async { Ok(Flow::Continue) }),
        }
    }

    fn after<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<()>> {
        let observer = &self.0;
        match event {
            ModelEvent::Retrieved => Box::pin(observer.retrieved(record, ctx)),
            ModelEvent::Created => Box::pin(observer.created(record, ctx)),
            ModelEvent::Updated => Box::pin(observer.updated(record, ctx)),
            ModelEvent::Saved => Box::pin(observer.saved(record, ctx)),
            ModelEvent::Deleted => Box::pin(observer.deleted(record, ctx)),
            ModelEvent::Trashed => Box::pin(observer.trashed(record, ctx)),
            ModelEvent::ForceDeleted => Box::pin(observer.force_deleted(record, ctx)),
            ModelEvent::Restored => Box::pin(observer.restored(record, ctx)),
            ModelEvent::Replicating => Box::pin(observer.replicating(record, ctx)),
            _ => Box::pin(async { Ok(()) }),
        }
    }

    fn kind(&self) -> HookKind {
        if O::IN_TRANSACTION {
            HookKind::TransactionalListener
        } else {
            HookKind::Listener
        }
    }
}
