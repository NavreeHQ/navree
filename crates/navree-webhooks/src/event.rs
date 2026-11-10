//! Events that become webhooks: [`WebhookEvent`], [`WebhookContext`] and
//! their registration.

use std::borrow::Cow;
use std::marker::PhantomData;

use navree_db::Db;
use navree_events::{Event, Listener};
use navree_kernel::{AppContext, BoxError, BoxFuture, Injectable, KernelError, Resolver};
use serde_json::Value;
use uuid::Uuid;

use crate::Webhooks;

/// An [`Event`] that is also sent as a webhook: its [`NAME`](Event::NAME)
/// is the webhook's type, [`payload`](WebhookEvent::payload) is what goes in
/// `data`, and [`workspace_id`](WebhookEvent::workspace_id) says whose
/// endpoints receive it. Register it with
/// [`WebhooksModule::for_feature`](crate::WebhooksModule::for_feature).
///
/// ```
/// use navree_events::Event;
/// use navree_webhooks::WebhookEvent;
/// use serde::{Deserialize, Serialize};
/// use uuid::Uuid;
///
/// #[derive(Clone, Debug, Serialize, Deserialize)]
/// struct DealWon {
///     deal_id: Uuid,
///     workspace_id: Uuid,
/// }
///
/// impl Event for DealWon {
///     const NAME: &'static str = "deal.won";
/// }
///
/// impl WebhookEvent for DealWon {
///     const DESCRIPTION: &'static str = "A deal moved to won.";
///
///     fn workspace_id(&self) -> Option<Uuid> {
///         Some(self.workspace_id)
///     }
/// }
/// ```
pub trait WebhookEvent: Event {
    /// What the event means, for the list of event types.
    const DESCRIPTION: &'static str;

    /// The workspace whose endpoints receive it; `None` (the default) sends
    /// it to app-wide endpoints only.
    fn workspace_id(&self) -> Option<Uuid> {
        None
    }

    /// What goes in `data`. Built once per event, on a queue worker, before
    /// the deliveries are stored; the default is the event itself as JSON.
    fn payload<'a>(&'a self, ctx: &'a WebhookContext) -> BoxFuture<'a, Result<Value, BoxError>> {
        let _ = ctx;
        Box::pin(async move { Ok(serde_json::to_value(self)?) })
    }
}

/// What [`WebhookEvent::payload`] can use: the app and its database.
#[derive(Clone)]
pub struct WebhookContext {
    app: AppContext,
    db: Db,
}

impl WebhookContext {
    pub(crate) fn new(app: AppContext, db: Db) -> Self {
        Self { app, db }
    }

    /// The app, to resolve services (`ctx.app().get::<DealService>()`).
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// The database: pass it to model reads (`Deal::find(ctx.db(), id)`,
    /// `Deal::query().count(ctx.db())`) or run navree-db statements on it.
    pub fn db(&self) -> &Db {
        &self.db
    }
}

impl std::fmt::Debug for WebhookContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookContext").finish_non_exhaustive()
    }
}

/// Builds the `data` of a webhook from an event's JSON form.
pub(crate) type BuildFn =
    for<'a> fn(Value, &'a WebhookContext) -> BoxFuture<'a, Result<Value, BoxError>>;

/// A registered webhook event type (a contribution).
#[derive(Clone)]
pub(crate) struct WebhookTypeRegistration {
    pub(crate) name: Cow<'static, str>,
    pub(crate) description: Cow<'static, str>,
    /// For event types: deserializes the event and runs its `payload`.
    pub(crate) build: Option<BuildFn>,
}

impl WebhookTypeRegistration {
    /// The registration of event `E`.
    pub(crate) fn event<E: WebhookEvent>() -> Self {
        Self {
            name: Cow::Borrowed(E::NAME),
            description: Cow::Borrowed(E::DESCRIPTION),
            build: Some(build::<E>),
        }
    }

    /// A type dispatched by name only.
    pub(crate) fn named(name: Cow<'static, str>, description: Cow<'static, str>) -> Self {
        Self {
            name,
            description,
            build: None,
        }
    }
}

/// Decodes `E` from `event` and builds its payload (a [`BuildFn`]).
fn build<'a, E: WebhookEvent>(
    event: Value,
    ctx: &'a WebhookContext,
) -> BoxFuture<'a, Result<Value, BoxError>> {
    Box::pin(async move {
        let event: E = serde_json::from_value(event)
            .map_err(|error| format!("invalid event `{}`: {error}", E::NAME))?;
        event.payload(ctx).await
    })
}

/// Hands an emitted [`WebhookEvent`] to the fan-out job (subscribed by
/// [`WebhooksModule::for_feature`](crate::WebhooksModule::for_feature)).
/// It runs during `emit`, so inside a unit of work the job joins it.
pub(crate) struct FanOutListener<E>(PhantomData<fn() -> E>);

impl<E: WebhookEvent> Injectable for FanOutListener<E> {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(FanOutListener(PhantomData)) })
    }
}

impl<E: WebhookEvent> Listener<E> for FanOutListener<E> {
    fn handle<'a>(
        &'a self,
        event: &'a E,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let webhooks = ctx.get::<Webhooks>().await?;
            webhooks
                .queue_event(E::NAME, event.workspace_id(), serde_json::to_value(event)?)
                .await?;
            Ok(())
        })
    }
}
