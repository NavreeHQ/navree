//! The [`Event`] and [`Listener`] traits.

use navree_kernel::{AppContext, BoxError, BoxFuture};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A typed event (what `#[event]` implements).
///
/// Events are plain values: emitting one hands a reference to every sync
/// listener, serializes it as the payload of queued listeners' jobs, and
/// sends its JSON form to [`subscribe_json`](crate::EventBus::subscribe_json)
/// subscribers. Keep them small and free of secrets: queued payloads are
/// stored by the queue.
///
/// ```
/// use navree_events::Event;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Debug, Serialize, Deserialize)]
/// struct DealWon {
///     deal_id: u64,
///     owner_id: u64,
/// }
///
/// impl Event for DealWon {
///     const NAME: &'static str = "deal.won";
/// }
/// ```
pub trait Event: Clone + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The stable event name, e.g. `deal.won`. It names the JSON channel,
    /// matches [`EventSink::publish`](navree_contracts::EventSink::publish)
    /// calls to this type, and is part of queued listeners' job names, so do
    /// not change it while jobs may be pending.
    const NAME: &'static str;
}

/// Handles events of type `E` (what `#[listener(E)]` implements).
///
/// A listener is a provider: it gets its dependencies by constructor
/// injection and is resolved from the module that subscribed it (see
/// [`ModuleBuilderEventsExt`](crate::ModuleBuilderEventsExt)). One type may
/// implement `Listener` for several events.
pub trait Listener<E: Event>: Send + Sync + 'static {
    /// Handles one event. An error is logged and reported to the emitter
    /// after every other listener has run; it does not stop them.
    fn handle<'a>(
        &'a self,
        event: &'a E,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>>;

    /// Whether the event is handed to the queue instead of being handled
    /// during `emit` (`#[listener(E, queued)]`). A queued listener's job runs
    /// [`handle`](Listener::handle) on a worker with the deserialized event;
    /// in an app without a job dispatcher it is handled inline.
    fn queued(&self) -> bool {
        false
    }
}
