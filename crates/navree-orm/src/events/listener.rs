//! Closure listeners: one closure per model and event.

use std::marker::PhantomData;

use navree_kernel::BoxFuture;

use crate::error::Result;
use crate::exec::ExecContext;
use crate::hooks::{Flow, HookKind, ModelEvent, ModelHook};
use crate::model::Model;
use crate::record::Record;

/// A closure listening to one event of model `M`: what
/// [`ModuleBuilderOrmExt::on_model`](crate::ModuleBuilderOrmExt::on_model)
/// registers. A [`HookKind::Listener`]: it hears "-ed" write events after
/// the commit.
pub(crate) struct ClosureListener<F, O> {
    event: ModelEvent,
    listener: F,
    _output: PhantomData<fn() -> O>,
}

impl<F, O> ClosureListener<F, O> {
    /// `listener` for `event`.
    pub(crate) fn new(event: ModelEvent, listener: F) -> Self {
        ClosureListener {
            event,
            listener,
            _output: PhantomData,
        }
    }
}

impl<M, F, O> ModelHook<M> for ClosureListener<F, O>
where
    M: Model,
    F: for<'a> Fn(&'a mut Record<M>, &'a ExecContext) -> BoxFuture<'a, Result<O>>
        + Send
        + Sync
        + 'static,
    O: Into<Flow> + 'static,
{
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<Flow>> {
        if event != self.event {
            return Box::pin(async { Ok(Flow::Continue) });
        }
        let heard = (self.listener)(record, ctx);
        Box::pin(async move { heard.await.map(Into::into) })
    }

    fn after<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<()>> {
        if event != self.event {
            return Box::pin(async { Ok(()) });
        }
        let heard = (self.listener)(record, ctx);
        // A step that happened cannot be cancelled: the flow is moot.
        Box::pin(async move { heard.await.map(|_| ()) })
    }

    fn kind(&self) -> HookKind {
        HookKind::Listener
    }
}
