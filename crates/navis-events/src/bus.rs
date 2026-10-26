//! [`EventBus`]: typed emit, JSON publish and JSON subscriptions.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};

use navis_contracts::EventSink;
use navis_kernel::{AppContext, BoxError, BoxFuture, Contributed, RequestScope, WeakAppContext};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::registry::{DecodeFn, Delivered, ListenerRegistration};
use crate::{Event, EventsError, ListenerFailure};

/// Buffered JSON events per name before a slow subscriber starts lagging.
const JSON_CHANNEL_CAPACITY: usize = 256;

/// The app's event bus, provided and exported by the global
/// [`EventsModule`](crate::EventsModule): inject `Arc<EventBus>` (or
/// `Arc<dyn EventSink>` to depend on contracts only).
///
/// An event is a name plus a payload. [`emit`](EventBus::emit) sends a typed
/// [`Event`]; [`EventSink::publish`] / [`publish_json`](EventBus::publish_json)
/// send a name and a JSON payload, which also reaches the typed listeners of
/// every event type with that [`Event::NAME`]. Both forms reach
/// [`subscribe_json`](EventBus::subscribe_json) subscribers.
///
/// Delivery is sequential: listeners run one after another in subscription
/// order, inside the emitting task, so `emit` returns once every sync
/// listener has run and every queued listener's job has been dispatched.
/// Each emit is one unit of work: request-scoped listeners get a fresh
/// [`RequestScope`]. A listener may emit further events.
///
/// Do not emit from a provider's constructor: listeners are resolved on
/// delivery, and one that depends on the provider being built would wait for
/// it forever. Emit from methods, lifecycle hooks, commands or handlers.
///
/// The bus holds the app weakly, so it never keeps the app alive. After
/// graceful shutdown ([`App::shutdown`]), whose `on_shutdown` hook detaches
/// the bus, or once the app is dropped, every emit fails with
/// [`EventsError::ShutDown`].
///
/// [`App::shutdown`]: navis_kernel::App::shutdown
pub struct EventBus {
    /// The app (weakly), until it shuts down (see [`EventBus::detach`]).
    app: RwLock<Option<WeakAppContext>>,
    /// Listener registrations by event type, in registration order.
    by_type: HashMap<TypeId, Vec<Contributed<ListenerRegistration>>>,
    /// The event types registered under each name, with their decoders.
    by_name: HashMap<&'static str, Vec<(TypeId, DecodeFn)>>,
    /// One broadcast channel per subscribed event name.
    json: Mutex<HashMap<String, broadcast::Sender<Value>>>,
    /// Whether the "queued listener ran inline" warning was logged.
    warned_inline: AtomicBool,
}

impl EventBus {
    pub(crate) fn new(app: AppContext) -> Self {
        let mut by_type: HashMap<TypeId, Vec<Contributed<ListenerRegistration>>> = HashMap::new();
        let mut by_name: HashMap<&'static str, Vec<(TypeId, DecodeFn)>> = HashMap::new();
        for registration in app.contributions::<ListenerRegistration>() {
            let types = by_name.entry(registration.value.event_name).or_default();
            if !types.iter().any(|(t, _)| *t == registration.value.event) {
                types.push((registration.value.event, registration.value.decode));
            }
            by_type
                .entry(registration.value.event)
                .or_default()
                .push(registration);
        }
        Self {
            app: RwLock::new(Some(app.downgrade())),
            by_type,
            by_name,
            json: Mutex::default(),
            warned_inline: AtomicBool::new(false),
        }
    }

    /// Emits `event`: sends its JSON form to the subscribers of
    /// [`E::NAME`](Event::NAME), runs its sync listeners and dispatches its
    /// queued listeners' jobs, in subscription order.
    ///
    /// A failing listener (including a failed dispatch) is logged at target
    /// `navis::events` and does not stop the others; afterwards the error is
    /// [`EventsError::ListenersFailed`] (boxed) listing every failure.
    pub async fn emit<E: Event>(&self, event: E) -> Result<(), BoxError> {
        let app = self.app()?;
        self.broadcast_json(E::NAME, || serde_json::to_value(&event));
        let failures = self.deliver(&app, E::NAME, TypeId::of::<E>(), &event).await;
        into_result(E::NAME, failures)
    }

    /// Publishes event `name` with a JSON payload (what
    /// [`EventSink::publish`] does): the payload is deserialized into every
    /// event type registered with that name, then sent to the JSON
    /// subscribers, then delivered to those types' listeners as by
    /// [`emit`](EventBus::emit).
    ///
    /// If the payload does not deserialize, nothing is delivered and the
    /// error is [`EventsError::InvalidPayload`] (boxed). A name with no typed
    /// listeners only reaches JSON subscribers.
    pub async fn publish_json(&self, name: &str, payload: Value) -> Result<(), BoxError> {
        let app = self.app()?;
        let mut decoded = Vec::new();
        for (event_type, decode) in self.by_name.get(name).into_iter().flatten() {
            let event = decode(&payload).map_err(|source| EventsError::InvalidPayload {
                event: name.to_owned(),
                source,
            })?;
            decoded.push((*event_type, event));
        }
        self.broadcast_json(name, || Ok(payload));
        let mut failures = Vec::new();
        for (event_type, event) in &decoded {
            failures.extend(self.deliver(&app, name, *event_type, event.as_ref()).await);
        }
        into_result(name, failures)
    }

    /// Runs (or queues) every listener of `event_type`, in registration order,
    /// and returns the failures.
    async fn deliver(
        &self,
        app: &AppContext,
        name: &str,
        event_type: TypeId,
        event: &(dyn Any + Send + Sync),
    ) -> Vec<ListenerFailure> {
        let registrations = self.by_type.get(&event_type).map_or(&[][..], Vec::as_slice);
        tracing::debug!(
            target: "navis::events",
            event = name,
            listeners = registrations.len(),
            "event emitted"
        );
        let mut failures = Vec::new();
        // One unit of work per emit: request-scoped listeners (and their
        // request-scoped dependencies) are built once for this event.
        let scope = RequestScope::new();
        for registration in registrations {
            let listener = registration.value.listener;
            let module = registration.module.name;
            let delivered =
                (registration.value.deliver)(app, registration.module.id, &scope, event);
            match delivered.await {
                Ok(Delivered::Inline) => self.warn_inline(name, listener),
                Ok(Delivered::Handled | Delivered::Queued) => {}
                Err(error) => {
                    tracing::error!(
                        target: "navis::events",
                        event = name,
                        listener,
                        module,
                        error = %error,
                        "event listener failed"
                    );
                    failures.push(ListenerFailure {
                        listener,
                        module,
                        error,
                    });
                }
            }
        }
        failures
    }

    /// Subscribes to the JSON form of every event named `name`, emitted or
    /// published from now on (for bridges such as broadcasting or webhooks).
    ///
    /// Each subscriber has a buffer of 256 events; one that falls further
    /// behind gets [`RecvError::Lagged`](broadcast::error::RecvError::Lagged)
    /// and skips the oldest events. Dropping the receiver unsubscribes.
    pub fn subscribe_json(&self, name: &str) -> broadcast::Receiver<Value> {
        self.channels()
            .entry(name.to_owned())
            .or_insert_with(|| broadcast::channel(JSON_CHANNEL_CAPACITY).0)
            .subscribe()
    }

    /// Sends the JSON form of event `name` to its subscribers, if any; the
    /// payload is only built when someone listens.
    fn broadcast_json(
        &self,
        name: &str,
        payload: impl FnOnce() -> Result<Value, serde_json::Error>,
    ) {
        let sender = {
            let mut channels = self.channels();
            match channels.get(name) {
                Some(sender) if sender.receiver_count() > 0 => sender.clone(),
                Some(_) => {
                    // Every subscriber is gone: forget the channel.
                    channels.remove(name);
                    return;
                }
                None => return,
            }
        };
        match payload() {
            // A send error only means the last subscriber just left.
            Ok(payload) => drop(sender.send(payload)),
            Err(error) => tracing::error!(
                target: "navis::events",
                event = name,
                error = %error,
                "event could not be serialized for JSON subscribers"
            ),
        }
    }

    /// The app, or [`EventsError::ShutDown`] once it has shut down or been
    /// dropped.
    fn app(&self) -> Result<AppContext, EventsError> {
        self.app
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(WeakAppContext::upgrade)
            .ok_or(EventsError::ShutDown)
    }

    /// Drops the bus's handle to the app, so emits fail from now on even
    /// while the app is still referenced; runs in the module's
    /// `on_shutdown` hook.
    pub(crate) fn detach(&self) {
        self.app
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    fn channels(&self) -> MutexGuard<'_, HashMap<String, broadcast::Sender<Value>>> {
        self.json.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Logs once per bus that queued listeners run inline.
    fn warn_inline(&self, event: &str, listener: &'static str) {
        if !self.warned_inline.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                target: "navis::events",
                event,
                listener,
                "no job dispatcher is registered (add QueueModule): queued listeners run inline"
            );
        }
    }
}

/// `Ok` when nothing failed, else [`EventsError::ListenersFailed`].
fn into_result(event: &str, failures: Vec<ListenerFailure>) -> Result<(), BoxError> {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(EventsError::ListenersFailed {
            event: event.to_owned(),
            failures,
        }
        .into())
    }
}

impl EventSink for EventBus {
    fn publish(&self, name: &str, payload: Value) -> BoxFuture<'_, Result<(), BoxError>> {
        let name = name.to_owned();
        Box::pin(async move { self.publish_json(&name, payload).await })
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names: Vec<&str> = self.by_name.keys().copied().collect();
        names.sort_unstable();
        f.debug_struct("EventBus")
            .field("events_with_listeners", &names)
            .field(
                "listeners",
                &self.by_type.values().map(Vec::len).sum::<usize>(),
            )
            .finish_non_exhaustive()
    }
}
