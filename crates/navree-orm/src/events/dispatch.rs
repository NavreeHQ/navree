//! [`Dispatch`]: model events as typed application events, through
//! navree-events.

use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_events::{Event, EventBus};
use navree_kernel::BoxFuture;

use crate::error::{OrmError, Result};
use crate::exec::ExecContext;
use crate::hooks::{Flow, HookKind, ModelEvent, ModelHook};
use crate::model::Model;
use crate::record::Record;

/// Emits application event `E`, built from the record
/// (`E: From<&Record<M>>`), through the app's navree-events
/// [`EventBus`] when model `M` reaches `event`: what
/// `#[model(dispatches(created = DealCreated, ..))]` declares.
///
/// A [`HookKind::Listener`]: an "-ed" write event is emitted after the
/// unit of work commits (never for a write that was rolled back), an `-ing`
/// event at once (a failing listener then refuses the write).
/// [`without_events`](crate::without_events) silences it. An app without
/// `EventsModule` dispatches nothing (a warning says so once).
///
/// ```
/// use navree_orm::prelude::*;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Model, Clone, Debug, Default, Serialize)]
/// #[model(dispatches(created = DealCreated))]
/// struct Deal {
///     id: i64,
///     title: String,
/// }
///
/// #[derive(Clone, Debug, Serialize, Deserialize)]
/// struct DealCreated {
///     id: i64,
///     title: String,
/// }
///
/// impl navree_events::Event for DealCreated {
///     const NAME: &'static str = "deal.created";
/// }
///
/// impl From<&Record<Deal>> for DealCreated {
///     fn from(deal: &Record<Deal>) -> Self {
///         DealCreated { id: deal.id, title: deal.title.clone() }
///     }
/// }
/// ```
pub struct Dispatch<M, E> {
    event: ModelEvent,
    _types: PhantomData<fn() -> (M, E)>,
}

impl<M, E> Dispatch<M, E> {
    /// Emits `E` when the model reaches `event`.
    pub const fn new(event: ModelEvent) -> Self {
        Dispatch {
            event,
            _types: PhantomData,
        }
    }
}

impl<M, E> fmt::Debug for Dispatch<M, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dispatch")
            .field("event", &self.event)
            .field("as", &std::any::type_name::<E>())
            .finish()
    }
}

impl<M, E> Dispatch<M, E>
where
    M: Model,
    E: Event + for<'r> From<&'r Record<M>>,
{
    /// Emits the event for `record` through the app's bus.
    async fn emit(&self, ctx: &ExecContext, record: &Record<M>) -> Result<()> {
        let bus = match ctx.app() {
            Some(app) => app.try_get::<EventBus>().await,
            None => None,
        };
        let Some(bus) = bus else {
            static WARNED: AtomicBool = AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "navree_orm::events",
                    model = M::meta().name(),
                    event = E::NAME,
                    "a model dispatches application events, but its executor's app has no EventsModule: nothing is dispatched"
                );
            }
            return Ok(());
        };
        bus.emit(E::from(record))
            .await
            .map_err(|error| OrmError::Hook {
                model: M::meta().name().into(),
                message: format!("event `{}`: {error}", E::NAME),
            })
    }
}

impl<M, E> ModelHook<M> for Dispatch<M, E>
where
    M: Model,
    E: Event + for<'r> From<&'r Record<M>>,
{
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<Flow>> {
        Box::pin(async move {
            if event == self.event {
                self.emit(ctx, record).await?;
            }
            Ok(Flow::Continue)
        })
    }

    fn after<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if event == self.event {
                self.emit(ctx, record).await?;
            }
            Ok(())
        })
    }

    fn kind(&self) -> HookKind {
        HookKind::Listener
    }
}
