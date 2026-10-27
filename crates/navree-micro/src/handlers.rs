//! [`MessageHandlers`], [`PatternDef`] and [`ModuleBuilderMicroExt`]: how
//! modules contribute message and event handlers.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use axum::handler::Handler;
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use http::StatusCode;
use http::header::CONTENT_TYPE;
use navree_events::{Event, EventBus, Listener, ModuleBuilderEventsExt};
use navree_http::{AccessSpec, AppError, CanSpec, Guard};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, ModuleBuilder, ModuleId, ProviderKey,
    Resolver,
};
use serde::Serialize;

use crate::{Codec, PatternKind, Payload, ServiceClient};

/// A struct whose methods answer message and event patterns: what
/// `#[message_handlers]` implements. Register it with
/// [`ModuleBuilderMicroExt::message_handlers`]; it must also be
/// `Injectable` (usually `#[injectable]`).
pub trait MessageHandlers: Send + Sync + 'static {
    /// The patterns and their handlers.
    fn patterns(self: Arc<Self>) -> Vec<PatternDef>;
}

/// One message or event pattern: its handler (an Axum handler whose
/// arguments are extractors) and its metadata.
///
/// The handler's response is the reply: its status, its body and its
/// `content-type`.
///
/// ```
/// use navree_http::Json;
/// use navree_micro::{PatternDef, Payload};
/// use serde_json::Value;
///
/// let get = PatternDef::message("deals.get", |Payload(input): Payload<Value>| async move {
///     Json(input)
/// })
/// .authenticated();
/// assert_eq!(get.pattern, "deals.get");
/// assert_eq!(get.meta.access.describe(), "auth");
/// ```
pub struct PatternDef {
    /// The pattern (`deals.get`; event patterns may end in `*` or `>`).
    pub pattern: String,
    /// Message or event.
    pub kind: PatternKind,
    /// The handler.
    pub handler: MethodRouter,
    /// Access, guards and options.
    pub meta: PatternMeta,
}

/// Everything known about a pattern besides its handler.
#[derive(Clone, Debug, Default)]
pub struct PatternMeta {
    /// The handler method's name, for `micro:patterns`.
    pub handler: String,
    /// Who may send the message (events take none).
    pub access: AccessSpec,
    /// Guards, in order.
    pub guards: Vec<Arc<dyn Guard>>,
    /// The handler's own time limit (`#[timeout("2s")]`).
    pub timeout: Option<Duration>,
    /// Messages: store the reply under the message id and answer a
    /// repeated id with it.
    pub dedupe: bool,
    /// At most this many at once in one process.
    pub concurrency: Option<usize>,
    /// Events: attempts before dead-lettering.
    pub max_attempts: Option<u32>,
    /// Events: one instance at a time consumes the pattern.
    pub ordered: bool,
    /// Events: every instance gets every event; at most once; no retries.
    pub broadcast: bool,
}

impl PatternDef {
    fn new<H, T>(pattern: &str, kind: PatternKind, handler: H) -> Self
    where
        H: Handler<T, ()>,
        T: 'static,
    {
        Self {
            pattern: pattern.to_owned(),
            kind,
            handler: axum::routing::post(handler),
            meta: PatternMeta::default(),
        }
    }

    /// A message pattern answered by `handler`.
    pub fn message<H, T>(pattern: &str, handler: H) -> Self
    where
        H: Handler<T, ()>,
        T: 'static,
    {
        Self::new(pattern, PatternKind::Message, handler)
    }

    /// An event pattern handled by `handler`.
    pub fn event<H, T>(pattern: &str, handler: H) -> Self
    where
        H: Handler<T, ()>,
        T: 'static,
    {
        Self::new(pattern, PatternKind::Event, handler)
    }

    /// The handler method's name.
    pub fn handler_name(mut self, name: &str) -> Self {
        self.meta.handler = name.to_owned();
        self
    }

    /// Anyone may send it.
    pub fn public(mut self) -> Self {
        self.meta.access.public = true;
        self
    }

    /// Any authenticated caller.
    pub fn authenticated(mut self) -> Self {
        self.meta.access.authenticated = true;
        self
    }

    /// Any of the roles.
    pub fn roles(mut self, roles: &[&str]) -> Self {
        self.meta
            .access
            .roles
            .extend(roles.iter().map(|r| (*r).to_owned()));
        self
    }

    /// All of the permissions.
    pub fn permissions(mut self, permissions: &[&str]) -> Self {
        self.meta
            .access
            .permissions
            .extend(permissions.iter().map(|p| (*p).to_owned()));
        self
    }

    /// The ability `action` on `subject`; with `from`, on the record whose
    /// id is that top-level payload field.
    pub fn can(mut self, action: &str, subject: &str, from: Option<&str>) -> Self {
        self.meta.access.can = Some(CanSpec {
            action: action.to_owned(),
            subject: subject.to_owned(),
            from: from.map(str::to_owned),
        });
        self
    }

    /// Adds a guard (run after authentication, before the access check).
    pub fn guard(mut self, guard: Arc<dyn Guard>) -> Self {
        self.meta.guards.push(guard);
        self
    }

    /// The handler's time limit.
    pub fn timeout(mut self, limit: Duration) -> Self {
        self.meta.timeout = Some(limit);
        self
    }

    /// Store the reply under the message id (`dedupe`).
    pub fn dedupe(mut self) -> Self {
        self.meta.dedupe = true;
        self
    }

    /// At most `limit` at once in one process.
    pub fn concurrency(mut self, limit: usize) -> Self {
        self.meta.concurrency = Some(limit);
        self
    }

    /// Attempts before dead-lettering.
    pub fn max_attempts(mut self, attempts: u32) -> Self {
        self.meta.max_attempts = Some(attempts);
        self
    }

    /// One instance at a time consumes the pattern.
    pub fn ordered(mut self) -> Self {
        self.meta.ordered = true;
        self
    }

    /// Every instance gets every event.
    pub fn broadcast(mut self) -> Self {
        self.meta.broadcast = true;
        self
    }
}

impl fmt::Debug for PatternDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PatternDef")
            .field("pattern", &self.pattern)
            .field("kind", &self.kind)
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

/// A handler's result as a reply: `Ok(value)` is status 200 and the value
/// encoded with the codec of the request; an error is the error reply in
/// Navree's JSON error shape. `#[message_handlers]` wraps every handler's
/// result in this.
pub struct MessageReply {
    response: Response,
}

impl MessageReply {
    /// The reply for `result`, encoded with `codec`.
    pub fn of<T, E>(codec: ReplyCodec, result: Result<T, E>) -> MessageReply
    where
        T: Serialize,
        E: Into<AppError>,
    {
        let response = match result {
            Ok(value) => codec.encode(&value),
            Err(error) => error.into().into_response(),
        };
        MessageReply { response }
    }
}

impl IntoResponse for MessageReply {
    fn into_response(self) -> Response {
        self.response
    }
}

impl fmt::Debug for MessageReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageReply")
            .field("status", &self.response.status())
            .finish_non_exhaustive()
    }
}

/// The codec replies use: the request's (an extractor the generated
/// handlers take first).
#[derive(Clone, Copy, Debug)]
pub struct ReplyCodec(pub Codec);

impl ReplyCodec {
    fn encode<T: Serialize + ?Sized>(self, value: &T) -> Response {
        match self.0.encode(value) {
            Ok(bytes) => {
                let mut response = Response::new(axum::body::Body::from(bytes));
                response.headers_mut().insert(
                    CONTENT_TYPE,
                    http::HeaderValue::from_static(self.0.content_type()),
                );
                response
            }
            Err(error) => {
                AppError::internal(format!("the reply does not encode: {error}")).into_response()
            }
        }
    }
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ReplyCodec {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let codec = parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(Codec::from_content_type)
            .unwrap_or_default();
        Ok(ReplyCodec(codec))
    }
}

/// Handlers resolved from their module, ready to mount.
pub(crate) struct ResolvedHandlers {
    pub(crate) type_name: &'static str,
    pub(crate) patterns: Vec<PatternDef>,
}

type ResolveHandlers = Arc<
    dyn Fn(Resolver) -> BoxFuture<'static, Result<ResolvedHandlers, KernelError>> + Send + Sync,
>;

/// Handlers registered by a module (a contribution).
#[derive(Clone)]
pub(crate) struct HandlerRegistration {
    pub(crate) module: ModuleId,
    pub(crate) resolve: ResolveHandlers,
}

impl HandlerRegistration {
    fn of<T: MessageHandlers + Injectable>(module: ModuleId) -> Self {
        Self {
            module,
            resolve: Arc::new(|r: Resolver| {
                Box::pin(async move {
                    let handlers = r.get::<T>().await?;
                    Ok(ResolvedHandlers {
                        type_name: std::any::type_name::<T>(),
                        patterns: handlers.patterns(),
                    })
                })
            }),
        }
    }

    /// `receive_event::<E>()`: a broadcast handler emitting `E` on the
    /// app's `EventBus`.
    fn receive<E: Event>(module: ModuleId) -> Self {
        Self {
            module,
            resolve: Arc::new(|r: Resolver| {
                Box::pin(async move {
                    let bus = r.get::<EventBus>().await?;
                    let handler = move |Payload(event): Payload<E>| {
                        let bus = bus.clone();
                        async move {
                            // Not forwarded again by `forward_event`.
                            RECEIVING
                                .scope((), bus.emit(event))
                                .await
                                .map_err(|error| AppError::internal(error.to_string()))?;
                            Ok::<_, AppError>(StatusCode::NO_CONTENT)
                        }
                    };
                    Ok(ResolvedHandlers {
                        type_name: std::any::type_name::<ReceivedEvent<E>>(),
                        patterns: vec![
                            PatternDef::event(E::NAME, handler)
                                .handler_name("receive_event")
                                .broadcast(),
                        ],
                    })
                })
            }),
        }
    }
}

/// Names `receive_event::<E>()` registrations in `micro:patterns`.
struct ReceivedEvent<E>(PhantomData<E>);

/// Message registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderMicroExt {
    /// Registers `T` as a singleton provider of this module and its
    /// message and event handlers. They resolve their dependencies with
    /// the module's visibility, like its controllers.
    fn message_handlers<T: MessageHandlers + Injectable>(&mut self) -> &mut Self;

    /// Publishes every `E` emitted on the app's `EventBus` under
    /// [`E::NAME`](Event::NAME), as `client.emit` would (inside a unit of
    /// work, through the outbox). Events that `receive_event` put on the
    /// bus are not forwarded again.
    fn forward_event<E: Event>(&mut self) -> &mut Self;

    /// The same, with `key(&event)` as the ordering key.
    fn forward_event_keyed<E, K, F>(&mut self, key: F) -> &mut Self
    where
        E: Event,
        K: fmt::Display,
        F: Fn(&E) -> K + Send + Sync + 'static;

    /// A broadcast handler for `E::NAME` that emits every `E` published
    /// anywhere on this instance's `EventBus`.
    fn receive_event<E: Event>(&mut self) -> &mut Self;
}

impl ModuleBuilderMicroExt for ModuleBuilder {
    fn message_handlers<T: MessageHandlers + Injectable>(&mut self) -> &mut Self {
        let registration = HandlerRegistration::of::<T>(self.module_id());
        self.provider::<T>().contribute(registration)
    }

    fn forward_event<E: Event>(&mut self) -> &mut Self {
        self.listen::<E, EventForwarder<E>>()
    }

    fn forward_event_keyed<E, K, F>(&mut self, key: F) -> &mut Self
    where
        E: Event,
        K: fmt::Display,
        F: Fn(&E) -> K + Send + Sync + 'static,
    {
        let key: KeyFn<E> = Arc::new(move |event: &E| key(event).to_string());
        self.value(ForwardKey(key))
            .listen::<E, KeyedEventForwarder<E>>()
    }

    fn receive_event<E: Event>(&mut self) -> &mut Self {
        let registration = HandlerRegistration::receive::<E>(self.module_id());
        self.contribute(registration)
    }
}

tokio::task_local! {
    /// Set while `receive_event` emits a received event on the local bus.
    static RECEIVING: ();
}

/// Whether the current task emits an event `receive_event` received (which
/// is not forwarded again).
fn receiving() -> bool {
    RECEIVING.try_with(|()| ()).is_ok()
}

type KeyFn<E> = Arc<dyn Fn(&E) -> String + Send + Sync>;

/// The key function of `forward_event_keyed::<E>`.
struct ForwardKey<E>(KeyFn<E>);

/// Publishes in-process `E`s (`forward_event`).
struct EventForwarder<E> {
    client: Arc<ServiceClient>,
    _event: PhantomData<fn() -> E>,
}

impl<E: Event> Injectable for EventForwarder<E> {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<ServiceClient>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                client: r.get::<ServiceClient>().await?,
                _event: PhantomData,
            })
        })
    }
}

impl<E: Event> Listener<E> for EventForwarder<E> {
    fn handle<'a>(
        &'a self,
        event: &'a E,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            if !receiving() {
                self.client.emit(E::NAME, event).await?;
            }
            Ok(())
        })
    }
}

/// Publishes in-process `E`s with a key (`forward_event_keyed`).
struct KeyedEventForwarder<E> {
    client: Arc<ServiceClient>,
    key: Arc<ForwardKey<E>>,
}

impl<E: Event> Injectable for KeyedEventForwarder<E> {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<ServiceClient>(),
            ProviderKey::of::<ForwardKey<E>>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                client: r.get::<ServiceClient>().await?,
                key: r.get::<ForwardKey<E>>().await?,
            })
        })
    }
}

impl<E: Event> Listener<E> for KeyedEventForwarder<E> {
    fn handle<'a>(
        &'a self,
        event: &'a E,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            if !receiving() {
                let key = (self.key.0)(event);
                self.client.emit(E::NAME, event).key(key).await?;
            }
            Ok(())
        })
    }
}
