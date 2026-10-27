//! Registering ORM parts in modules: [`ModuleBuilderOrmExt`].

use std::sync::Arc;

use navree_kernel::{BoxFuture, Factory, Injectable, ModuleBuilder, Resolver};

use crate::error::Result;
use crate::events::{ClosureListener, Observer, ObserverHook};
use crate::exec::ExecContext;
use crate::hooks::{Flow, ModelEvent, ModelHook};
use crate::model::Model;
use crate::record::Record;

/// ORM registrations on a [`ModuleBuilder`]: observers and closure
/// listeners of model events.
///
/// Everything registered here belongs to the app being built, so each app
/// (each test) has its own listeners.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_orm::ModuleBuilderOrmExt;
///
/// struct DealsModule;
///
/// impl Module for DealsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.on_model::<Deal, _>(ModelEvent::Saving, |deal, _ctx| {
///             Box::pin(async move {
///                 deal.title = deal.title.trim().to_owned();
///                 Ok(())
///             })
///         });
///     }
/// }
/// ```
pub trait ModuleBuilderOrmExt {
    /// Registers `observer` for model `M`'s events (a `dyn ModelHook<M>`
    /// multi-binding).
    fn observe<M: Model>(&mut self, observer: impl Observer<M>) -> &mut Self;

    /// Registers an observer of model `M` that the container builds (with
    /// the visibility of this module), for observers with dependencies.
    fn observer<M: Model, O: Observer<M> + Injectable>(&mut self) -> &mut Self;

    /// Registers `listener` for `event` of model `M`. It may change the
    /// record; for a cancellable event it may return [`Flow::Cancel`]
    /// (`Ok(())` goes ahead). Inside a unit of work it hears "-ed" write
    /// events after the commit, on a snapshot of the record.
    ///
    /// The second type parameter is the listener's output (`()` or
    /// [`Flow`]): write `m.on_model::<Deal, _>(..)`.
    fn on_model<M, O>(
        &mut self,
        event: ModelEvent,
        listener: impl for<'a> Fn(&'a mut Record<M>, &'a ExecContext) -> BoxFuture<'a, Result<O>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self
    where
        M: Model,
        O: Into<Flow> + 'static;
}

impl ModuleBuilderOrmExt for ModuleBuilder {
    fn observe<M: Model>(&mut self, observer: impl Observer<M>) -> &mut Self {
        let hook: Arc<dyn ModelHook<M>> = Arc::new(ObserverHook::new(observer));
        self.multi(Factory::<dyn ModelHook<M>>::new(move |_| {
            let hook = hook.clone();
            async move { Ok(hook) }
        }))
    }

    fn observer<M: Model, O: Observer<M> + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn ModelHook<M>>::new(|r: Resolver| async move {
                let observer = O::construct(r).await?;
                Ok(Arc::new(ObserverHook::new(observer)) as Arc<dyn ModelHook<M>>)
            })
            .deps(O::dependencies()),
        )
    }

    fn on_model<M, O>(
        &mut self,
        event: ModelEvent,
        listener: impl for<'a> Fn(&'a mut Record<M>, &'a ExecContext) -> BoxFuture<'a, Result<O>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self
    where
        M: Model,
        O: Into<Flow> + 'static,
    {
        let hook: Arc<dyn ModelHook<M>> = Arc::new(ClosureListener::new(event, listener));
        self.multi(Factory::<dyn ModelHook<M>>::new(move |_| {
            let hook = hook.clone();
            async move { Ok(hook) }
        }))
    }
}
