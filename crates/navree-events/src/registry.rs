//! Listener registration: [`ModuleBuilderEventsExt`] and the job behind
//! queued listeners.

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use navree_contracts::{Dispatcher, JobContext, JobEnvelope, JobOptions, JobRegistration};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, ModuleBuilder, ModuleId, ProviderKey,
    RequestScope, Scope,
};
use serde_json::Value;

use crate::{Event, Listener};

/// Delivers a type-erased event (`&E` behind `dyn Any`) to one listener,
/// resolved from its module within the emit's request scope.
pub(crate) type DeliverFn = for<'a> fn(
    &'a AppContext,
    ModuleId,
    &'a RequestScope,
    &'a (dyn Any + Send + Sync),
) -> BoxFuture<'a, Result<Delivered, BoxError>>;

/// Deserializes a JSON payload into the event type `E`, boxed as `dyn Any`.
pub(crate) type DecodeFn = fn(&Value) -> Result<Box<dyn Any + Send + Sync>, serde_json::Error>;

/// How a listener got an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivered {
    /// A sync listener handled it.
    Handled,
    /// A queued listener's job was dispatched.
    Queued,
    /// A queued listener handled it inline: the app has no `Dispatcher`.
    Inline,
}

/// One subscription of listener `L` to event `E`, contributed by the
/// subscribing module and indexed by the [`EventBus`](crate::EventBus).
pub(crate) struct ListenerRegistration {
    /// `TypeId` of `E`.
    pub(crate) event: TypeId,
    /// [`Event::NAME`] of `E`.
    pub(crate) event_name: &'static str,
    /// Decodes a published payload into `E`.
    pub(crate) decode: DecodeFn,
    /// `TypeId` of `L`.
    pub(crate) listener_type: TypeId,
    /// The type name of `L`.
    pub(crate) listener: &'static str,
    /// Runs `L` for one event.
    pub(crate) deliver: DeliverFn,
}

/// Subscribes listeners to events from a module's `register` (see the
/// [crate documentation](crate) for a complete example).
///
/// Listeners of one event run in module dependency order (imported modules
/// first), then in subscription order within a module. Each subscription also
/// contributes a [`JobRegistration`] named
/// [`listener_job_name::<E, L>()`](listener_job_name),
/// so the queue can run the listener when it is [queued](Listener::queued).
pub trait ModuleBuilderEventsExt {
    /// Registers `L` as a provider of this module (scope from
    /// [`Injectable::scope`]) and subscribes it to `E`.
    ///
    /// Registering the same `L` twice in one module is a duplicate provider
    /// error: subscribe an already registered listener with
    /// [`on`](ModuleBuilderEventsExt::on).
    fn listen<E: Event, L: Listener<E> + Injectable>(&mut self) -> &mut Self;

    /// Subscribes `L` to `E`, where `L` is a provider this module can see (its
    /// own, or exported by a module it imports or by a global module). The
    /// boot check fails, naming the module and `L`, when it cannot see `L`,
    /// and when the same `L` is subscribed to the same `E` twice in a module.
    fn on<E: Event, L: Listener<E>>(&mut self) -> &mut Self;
}

impl ModuleBuilderEventsExt for ModuleBuilder {
    fn listen<E: Event, L: Listener<E> + Injectable>(&mut self) -> &mut Self {
        self.provider::<L>().on::<E, L>()
    }

    fn on<E: Event, L: Listener<E>>(&mut self) -> &mut Self {
        self.factory(
            Factory::<ListenerBinding<E, L>>::new(|_| async {
                Ok(Arc::new(ListenerBinding(PhantomData)))
            })
            .scope(Scope::Transient)
            .deps(vec![ProviderKey::of::<L>()]),
        )
        .contribute(ListenerRegistration {
            event: TypeId::of::<E>(),
            event_name: E::NAME,
            decode: decode::<E>,
            listener_type: TypeId::of::<L>(),
            listener: type_name::<L>(),
            deliver: deliver::<E, L>,
        })
        .contribute(JobRegistration {
            name: listener_job_name::<E, L>(),
            options: JobOptions::default(),
            run: run_queued::<E, L>,
        })
    }
}

/// A transient marker provider per subscription, never constructed. Its
/// declared dependency on `L` makes the boot check verify that `L` is visible
/// to the subscribing module, and registering it twice is a duplicate
/// provider error.
struct ListenerBinding<E, L>(PhantomData<fn() -> (E, L)>);

/// The job type under which a queued listener `L` of event `E` runs:
/// `events.listener.<E::NAME>.<full type name of L>`, e.g.
/// `events.listener.deal.won.crm_api::deals::NotifyOwner`. Useful to assert
/// on dispatched jobs in tests.
///
/// The job uses the default [`JobOptions`] (queue `default`, 3 attempts,
/// exponential backoff).
pub fn listener_job_name<E: Event, L: Listener<E>>() -> &'static str {
    // `JobRegistration::name` is `&'static str`: intern one leaked string per
    // (event, listener) pair, so repeated boots do not leak more.
    static NAMES: OnceLock<Mutex<HashMap<(TypeId, TypeId), &'static str>>> = OnceLock::new();
    let mut names = NAMES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    names
        .entry((TypeId::of::<E>(), TypeId::of::<L>()))
        .or_insert_with(|| {
            let name = format!("events.listener.{}.{}", E::NAME, type_name::<L>());
            Box::leak(name.into_boxed_str())
        })
}

/// Decodes a published JSON payload into `E` (a [`DecodeFn`]).
fn decode<E: Event>(payload: &Value) -> Result<Box<dyn Any + Send + Sync>, serde_json::Error> {
    E::deserialize(payload).map(|event| Box::new(event) as Box<dyn Any + Send + Sync>)
}

/// Resolves `L` from `module` and hands it the event (a [`DeliverFn`]): sync
/// listeners handle it now; queued ones are dispatched as a job, or handled
/// inline when the app has no [`Dispatcher`].
fn deliver<'a, E: Event, L: Listener<E>>(
    ctx: &'a AppContext,
    module: ModuleId,
    scope: &'a RequestScope,
    event: &'a (dyn Any + Send + Sync),
) -> BoxFuture<'a, Result<Delivered, BoxError>> {
    Box::pin(async move {
        let event = event
            .downcast_ref::<E>()
            .ok_or("internal error: event type mismatch")?;
        let listener = ctx
            .request_resolver(module, scope.clone())
            .get::<L>()
            .await?;
        if !listener.queued() {
            listener.handle(event, ctx).await?;
            return Ok(Delivered::Handled);
        }
        let Some(dispatcher) = ctx.try_get::<dyn Dispatcher>().await else {
            listener.handle(event, ctx).await?;
            return Ok(Delivered::Inline);
        };
        dispatcher
            .dispatch_envelope(JobEnvelope {
                job_type: listener_job_name::<E, L>().to_owned(),
                payload: serde_json::to_value(event)?,
                options: JobOptions::default(),
                delay_secs: None,
                unique_key: None,
            })
            .await?;
        Ok(Delivered::Queued)
    })
}

/// The job behind a queued listener: deserializes the event and runs `L`,
/// resolved (in a fresh request scope) from the first module that subscribed
/// it to `E`.
fn run_queued<E: Event, L: Listener<E>>(
    payload: Value,
    job: JobContext,
) -> BoxFuture<'static, Result<(), BoxError>> {
    Box::pin(async move {
        let event: E = serde_json::from_value(payload)
            .map_err(|error| format!("invalid payload for event `{}`: {error}", E::NAME))?;
        let module = job
            .app
            .contributions::<ListenerRegistration>()
            .into_iter()
            .find(|r| {
                r.value.event == TypeId::of::<E>() && r.value.listener_type == TypeId::of::<L>()
            })
            .map(|r| r.module.id)
            .ok_or_else(|| {
                format!(
                    "listener `{}` is not registered for event `{}` in this app",
                    type_name::<L>(),
                    E::NAME
                )
            })?;
        let listener = job
            .app
            .request_resolver(module, RequestScope::new())
            .get::<L>()
            .await?;
        listener.handle(&event, &job.app).await
    })
}
