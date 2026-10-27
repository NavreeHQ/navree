//! The message pipeline: every message reaches its handler as request
//! parts, through the same authentication, guards, access check, unit of
//! work and error rendering as an HTTP request.

use std::collections::HashMap;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::FutureExt;
use http::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use http::request::Parts;
use http::{Method, StatusCode};
use navree_contracts::{
    AuditEntry, AuditKind, AuditSink, AuthState, AuthVia, Principal, RequestId,
};
use navree_db::Uow;
use navree_http::{
    AppError, Authenticator, ErrorMapper, GlobalLayer, LayerSlot, RouteAuthorizer, RouteMeta,
};
use navree_kernel::{AppContext, BoxError, RequestScope};
use navree_telemetry::{Telemetry, TelemetryContext, TraceContext, TracePolicy};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use tower::ServiceExt;
use uuid::Uuid;

use crate::context::{CallContext, MICRO_CONTEXT_LAYER, MessageContext, MessageInfo};
use crate::envelope::headers;
use crate::handlers::{HandlerRegistration, PatternMeta};
use crate::store::{Store, StoredReply};
use crate::{Codec, Envelope, PatternKind, check_pattern, has_wildcard};

/// The method recorded in a pattern's `RouteMeta` (audit entries and denial
/// logs name the pattern in place of the route).
const MESSAGE_METHOD: &str = "MESSAGE";
const EVENT_METHOD: &str = "EVENT";

/// A registered pattern, with where it comes from.
#[derive(Debug)]
pub(crate) struct Registered {
    pub(crate) index: usize,
    pub(crate) pattern: String,
    pub(crate) kind: PatternKind,
    pub(crate) meta: PatternMeta,
    pub(crate) module: &'static str,
    pub(crate) handlers: &'static str,
    pub(crate) route: Arc<RouteMeta>,
}

impl Registered {
    /// `get of DealsHandlers in module DealsModule`, for messages.
    fn label(&self) -> String {
        format!(
            "{} pattern `{}` (`{}` of {} in module `{}`)",
            self.kind, self.pattern, self.meta.handler, self.handlers, self.module
        )
    }

    /// The access column of `micro:patterns`.
    pub(crate) fn access(&self) -> String {
        match self.kind {
            PatternKind::Event => "-".to_owned(),
            PatternKind::Message => self.meta.access.describe(),
        }
    }

    /// The inbox consumer key: the service and the handler's pattern.
    pub(crate) fn consumer(&self, service: &str) -> String {
        format!("{service}|{}", self.pattern)
    }
}

/// What the pipeline needs from the module's options.
#[derive(Clone)]
pub(crate) struct PipelineSettings {
    pub(crate) service: String,
    /// The inbox, with `micro.inbox` and a database.
    pub(crate) inbox: Option<Store>,
    /// `micro.events.timeout_secs`.
    pub(crate) event_timeout: Duration,
    /// Whether the transport supports wildcards, and its name.
    pub(crate) wildcards: bool,
    pub(crate) transport: &'static str,
    /// `observe.tracing`.
    pub(crate) trace: TracePolicy,
    pub(crate) telemetry: Telemetry,
}

/// The app's message handlers behind their pipeline.
pub(crate) struct MessageRouter {
    router: Router,
    pub(crate) patterns: Vec<Arc<Registered>>,
    mappers: Vec<ErrorMapper>,
    trace: TracePolicy,
    telemetry: Telemetry,
}

impl fmt::Debug for MessageRouter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageRouter")
            .field("patterns", &self.patterns.len())
            .finish_non_exhaustive()
    }
}

/// The result of one dispatch.
#[derive(Clone, Debug)]
pub(crate) struct Outcome {
    pub(crate) status: StatusCode,
    pub(crate) content_type: String,
    pub(crate) body: Bytes,
    /// For failures: what went wrong (dead letters keep it).
    pub(crate) error: Option<String>,
}

impl Outcome {
    fn error(error: &AppError) -> Outcome {
        Outcome {
            status: error.status,
            content_type: "application/json".to_owned(),
            body: serde_json::to_vec(&error.body()).unwrap_or_default().into(),
            error: Some(describe(error)),
        }
    }
}

/// The principal a `MicroFake` client acts as (in-process only).
#[derive(Clone)]
pub(crate) struct LocalPrincipal(pub(crate) Principal);

/// An event's actor, made the unit of work's actor.
#[derive(Clone, Copy)]
struct EventActor(Uuid);

/// Marks a request whose `AuthState` holds only the event's actor, so the
/// handler sees a guest.
#[derive(Clone, Copy)]
struct ActorOnly;

impl MessageRouter {
    /// Collects and checks every registered handler, then builds the
    /// router. Every problem is reported at once.
    pub(crate) async fn build(
        ctx: &AppContext,
        settings: &PipelineSettings,
    ) -> Result<Self, BoxError> {
        let authenticator = ctx.try_get::<dyn Authenticator>().await;
        let authorizer = ctx.try_get::<dyn RouteAuthorizer>().await;
        let audit = ctx.try_get::<dyn AuditSink>().await;

        let mut patterns = Vec::new();
        let mut handlers = Vec::new();
        for registration in ctx.contributions::<HandlerRegistration>() {
            let resolver = ctx.resolver_for(registration.value.module);
            let resolved = (registration.value.resolve)(resolver).await?;
            for def in resolved.patterns {
                let index = patterns.len();
                let route = RouteMeta {
                    name: Some(def.pattern.clone()),
                    access: def.meta.access.clone(),
                    guards: def.meta.guards.clone(),
                    timeout: def.meta.timeout,
                    module: Some(registration.module.name),
                    module_id: Some(registration.value.module),
                    full_path: def.pattern.clone(),
                    method: match def.kind {
                        PatternKind::Message => MESSAGE_METHOD.to_owned(),
                        PatternKind::Event => EVENT_METHOD.to_owned(),
                    },
                    ..RouteMeta::default()
                };
                patterns.push(Arc::new(Registered {
                    index,
                    pattern: def.pattern,
                    kind: def.kind,
                    meta: def.meta,
                    module: registration.module.name,
                    handlers: short_name(resolved.type_name),
                    route: Arc::new(route),
                }));
                handlers.push(def.handler);
            }
        }
        check(
            &patterns,
            settings,
            authenticator.is_some(),
            authorizer.is_some(),
        )?;

        let mut router = Router::new();
        for (registered, handler) in patterns.iter().zip(handlers) {
            let inbox = settings.inbox.clone().filter(|_| match registered.kind {
                PatternKind::Message => registered.meta.dedupe,
                PatternKind::Event => !registered.meta.broadcast,
            });
            let runtime = Arc::new(HandlerRuntime {
                registered: registered.clone(),
                authorizer: if registered.meta.access.needs_authorizer() {
                    authorizer.clone()
                } else {
                    None
                },
                audit: audit.clone(),
                inbox,
                consumer: registered.consumer(&settings.service),
                default_timeout: match registered.kind {
                    PatternKind::Event => Some(settings.event_timeout),
                    PatternKind::Message => None,
                },
            });
            let handler = handler.route_layer(from_fn_with_state(runtime, handler_pipeline));
            router = router.route(&path_of(registered), handler);
        }
        router = router.fallback(|| async { AppError::not_found() });

        // Authentication first, then the app's authentication-context and
        // unit-of-work layers (the session slot is HTTP's alone), outermost
        // first.
        let contributed = ctx.contributions::<GlobalLayer>();
        let mut slots: Vec<(LayerSlot, usize, Option<&GlobalLayer>)> = contributed
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.value.slot != LayerSlot::Session && c.value.name != MICRO_CONTEXT_LAYER
            })
            .map(|(i, c)| (c.value.slot, i + 1, Some(&*c.value)))
            .collect();
        slots.push((LayerSlot::AuthContext, 0, None));
        slots.sort_by_key(|(slot, order, _)| (*slot, *order));
        for (_, _, layer) in slots.into_iter().rev() {
            router = match layer {
                None => router.layer(from_fn_with_state(authenticator.clone(), authenticate)),
                Some(layer) => (layer.apply)(ctx, router)
                    .map_err(|error| format!("layer `{}` failed: {error}", layer.name))?,
            };
        }
        let mappers = ctx
            .contributions::<ErrorMapper>()
            .into_iter()
            .map(|c| (*c.value).clone())
            .collect();
        Ok(Self {
            router,
            patterns,
            mappers,
            trace: settings.trace,
            telemetry: settings.telemetry.clone(),
        })
    }

    /// The span a message is handled in: it continues the sender's trace
    /// (`traceparent`, `tracestate`), or starts one sampled as
    /// `observe.tracing.sample_ratio` says; none when tracing is off.
    /// Messages come from the app's own services, so the sender's trace is
    /// continued whatever `trust_incoming` says, as a queued job's is.
    pub(crate) fn trace_of(&self, envelope: &Envelope) -> Option<TraceContext> {
        if !self.trace.enabled {
            return None;
        }
        envelope
            .header(headers::TRACEPARENT)
            .and_then(|traceparent| {
                TraceContext::continue_from(traceparent, envelope.header(headers::TRACESTATE))
            })
            .or_else(|| Some(TraceContext::root_sampled(self.trace.sample_ratio)))
    }

    /// Runs `envelope` through the handler of pattern `index` and returns
    /// its reply.
    pub(crate) async fn dispatch(
        &self,
        ctx: &AppContext,
        index: usize,
        envelope: &Envelope,
        attempt: u32,
    ) -> Outcome {
        let Some(registered) = self.patterns.get(index).cloned() else {
            return Outcome::error(&AppError::not_found());
        };
        let request_id = RequestId::parse_or_generate(envelope.header(headers::REQUEST_ID));
        let request = match self.request(ctx, &registered, envelope, attempt, &request_id) {
            Ok(request) => request,
            Err(error) => return Outcome::error(&error),
        };
        let trace = self.trace_of(envelope);
        let span = tracing::info_span!(
            "message",
            pattern = %envelope.pattern(),
            kind = %registered.kind,
            request_id = %request_id,
            trace_id = trace
                .as_ref()
                .map(|trace| trace.trace_id.to_string())
                .unwrap_or_default(),
        );
        let mut telemetry =
            TelemetryContext::new(self.telemetry.clone()).with_request_id(request_id.as_str());
        if let Some(trace) = trace {
            telemetry = telemetry.with_trace(trace);
        }
        let call = telemetry.scope(tracing::Instrument::instrument(
            self.router.clone().oneshot(request),
            span,
        ));
        let mut response = match AssertUnwindSafe(call).catch_unwind().await {
            Ok(Ok(response)) => response,
            Ok(Err(never)) => match never {},
            Err(panic) => {
                let message = panic
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic payload".to_owned());
                tracing::error!(
                    target: "navree::micro",
                    pattern = %envelope.pattern(),
                    request_id = %request_id,
                    panic = %message,
                    "message handler panicked"
                );
                return Outcome {
                    error: Some(format!("the handler panicked: {message}")),
                    ..Outcome::error(&AppError::from_status(StatusCode::INTERNAL_SERVER_ERROR))
                };
            }
        };
        if let Some(error) = AppError::take_from(&mut response) {
            let error =
                if error.status == StatusCode::INTERNAL_SERVER_ERROR && error.source.is_some() {
                    ErrorMapper::apply(&self.mappers, &error).unwrap_or(error)
                } else {
                    error
                };
            if error.status.is_server_error() {
                tracing::error!(
                    target: "navree::micro",
                    pattern = %envelope.pattern(),
                    request_id = %request_id,
                    error = %describe(&error),
                    "message handler failed"
                );
            }
            return Outcome::error(&error);
        }
        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/json")
            .to_owned();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_default();
        let error = (!status.is_success()).then(|| String::from_utf8_lossy(&body).into_owned());
        Outcome {
            status,
            content_type,
            body,
            error,
        }
    }

    /// The request a message becomes.
    fn request(
        &self,
        ctx: &AppContext,
        registered: &Registered,
        envelope: &Envelope,
        attempt: u32,
        request_id: &RequestId,
    ) -> Result<Request, AppError> {
        let uri = match registered
            .meta
            .access
            .can
            .as_ref()
            .and_then(|can| can.from.as_deref())
        {
            Some(field) => format!("/m/{}/{}", registered.index, record_id(envelope, field)?),
            None => format!("/m/{}", registered.index),
        };
        let mut request = http::Request::builder()
            .method(Method::POST)
            .uri(uri)
            .body(Body::from(envelope.payload().clone()))
            .map_err(AppError::internal)?;
        let request_headers = request.headers_mut();
        for (name, value) in envelope.headers() {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                request_headers.insert(name, value);
            }
        }
        request_headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(envelope.content_type())
                .unwrap_or_else(|_| HeaderValue::from_static("application/json")),
        );
        if let Ok(value) = HeaderValue::from_str(request_id.as_str()) {
            request_headers.insert(HeaderName::from_static(headers::REQUEST_ID), value);
        }
        let actor = envelope
            .header(headers::ACTOR)
            .and_then(|actor| actor.parse::<Uuid>().ok());
        let extensions = request.extensions_mut();
        extensions.insert(ctx.clone());
        extensions.insert(request_id.clone());
        extensions.insert(RequestScope::new());
        extensions.insert(MessageContext(Arc::new(MessageInfo {
            pattern: envelope.pattern().to_owned(),
            handler_pattern: registered.pattern.clone(),
            kind: registered.kind,
            message_id: envelope.message_id().unwrap_or_default().to_owned(),
            service: envelope.service().unwrap_or_default().to_owned(),
            deadline: envelope.deadline(),
            headers: envelope.headers().clone(),
            actor,
            attempt,
            request_id: request_id.as_str().to_owned(),
        })));
        match registered.kind {
            PatternKind::Message => {
                if let Some(principal) = envelope.local_principal() {
                    extensions.insert(LocalPrincipal(principal.clone()));
                }
            }
            PatternKind::Event => {
                extensions.insert(EventKind);
                if let Some(actor) = actor {
                    extensions.insert(EventActor(actor));
                }
            }
        }
        Ok(request)
    }
}

/// Marks an event's request: it is not authenticated.
#[derive(Clone, Copy)]
struct EventKind;

/// The record id of a `can(.., from = "field")` check: the top-level
/// payload field `field`, holding a UUID, a string or an integer.
fn record_id(envelope: &Envelope, field: &str) -> Result<String, AppError> {
    let missing = || {
        AppError::bad_request(format!(
            "The payload has no `{field}` field holding a UUID, a string or an integer."
        ))
    };
    let codec = Codec::from_content_type(envelope.content_type()).ok_or_else(missing)?;
    let value: serde_json::Value = codec.decode(envelope.payload()).map_err(|_| missing())?;
    let id = match &value[field] {
        serde_json::Value::String(id) if !id.is_empty() => id.clone(),
        serde_json::Value::Number(id) if id.is_i64() || id.is_u64() => id.to_string(),
        _ => return Err(missing()),
    };
    Ok(utf8_percent_encode(&id, NON_ALPHANUMERIC).to_string())
}

/// `/m/{index}`, or `/m/{index}/{field}` for a `can(.., from = "field")`
/// check (the access check reads the record id from the path).
fn path_of(registered: &Registered) -> String {
    match registered
        .meta
        .access
        .can
        .as_ref()
        .and_then(|can| can.from.as_deref())
    {
        Some(field) => format!("/m/{}/{{{field}}}", registered.index),
        None => format!("/m/{}", registered.index),
    }
}

/// Checks the registered patterns; every problem at once.
fn check(
    patterns: &[Arc<Registered>],
    settings: &PipelineSettings,
    authenticator: bool,
    authorizer: bool,
) -> Result<(), BoxError> {
    let mut errors = Vec::new();
    let mut seen: HashMap<(PatternKind, &str), &Registered> = HashMap::new();
    for registered in patterns {
        let label = registered.label();
        let meta = &registered.meta;
        let access = &meta.access;
        if let Err(reason) =
            check_pattern(&registered.pattern, registered.kind == PatternKind::Event)
        {
            errors.push(format!("{label} is not a valid pattern: {reason}"));
        }
        if let Some(first) = seen.insert((registered.kind, registered.pattern.as_str()), registered)
        {
            errors.push(format!(
                "{} pattern `{}` is handled twice: by `{}` of {} in module `{}` and by `{}` of {} in module `{}`",
                registered.kind,
                registered.pattern,
                first.meta.handler,
                first.handlers,
                first.module,
                meta.handler,
                registered.handlers,
                registered.module,
            ));
        }
        if meta.concurrency == Some(0) {
            errors.push(format!("{label} has a concurrency of 0"));
        }
        match registered.kind {
            PatternKind::Message => {
                if !access.is_declared() {
                    errors.push(format!(
                        "{label} has no access declaration: add .public(), .authenticated(), .roles(..), .permissions(..) or .can(..) (deny by default)"
                    ));
                } else {
                    if access.public && (access.authenticated || access.needs_authorizer()) {
                        errors.push(format!(
                            "{label} is public but also declares {}; remove one of them",
                            access.describe()
                        ));
                    }
                    if !access.public && !authenticator {
                        errors.push(format!(
                            "{label} requires authentication but no `dyn Authenticator` is registered (import the auth module, or mark the pattern public)"
                        ));
                    }
                    if access.needs_authorizer() && !authorizer {
                        errors.push(format!(
                            "{label} declares {} but no `dyn RouteAuthorizer` is registered (import the access module)",
                            access.describe()
                        ));
                    }
                }
                for (set, option) in [
                    (meta.max_attempts.is_some(), "max_attempts"),
                    (meta.ordered, "ordered"),
                    (meta.broadcast, "broadcast"),
                ] {
                    if set {
                        errors.push(format!(
                            "{label}: `{option}` only applies to event patterns"
                        ));
                    }
                }
            }
            PatternKind::Event => {
                if access.is_declared() {
                    errors.push(format!(
                        "{label} declares access ({}): events are facts and take none; the access check ran where the change was made",
                        access.describe()
                    ));
                }
                if meta.dedupe {
                    errors.push(format!(
                        "{label}: `dedupe` only applies to message patterns; the inbox deduplicates events"
                    ));
                }
                if meta.max_attempts == Some(0) {
                    errors.push(format!("{label}: `max_attempts` must be at least 1"));
                }
                if meta.broadcast && meta.ordered {
                    errors.push(format!(
                        "{label}: `broadcast` and `ordered` exclude each other"
                    ));
                }
                if has_wildcard(&registered.pattern) && !settings.wildcards {
                    errors.push(format!(
                        "{label} uses a wildcard, which the {} transport does not support: name the full pattern",
                        settings.transport
                    ));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

/// The per-pattern state of the pipeline.
struct HandlerRuntime {
    registered: Arc<Registered>,
    authorizer: Option<Arc<dyn RouteAuthorizer>>,
    audit: Option<Arc<dyn AuditSink>>,
    inbox: Option<Store>,
    consumer: String,
    default_timeout: Option<Duration>,
}

impl HandlerRuntime {
    /// Authentication, guards, then the access check.
    async fn admit(&self, parts: &mut Parts) -> Result<(), AppError> {
        let meta = &self.registered.meta;
        let user = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id);
        if self.registered.kind == PatternKind::Message && !meta.access.public && user.is_none() {
            return Err(AppError::unauthenticated());
        }
        for guard in &meta.guards {
            if let Err(error) = guard.check(parts).await {
                if error.status == StatusCode::FORBIDDEN {
                    self.denied(parts, user).await;
                }
                return Err(error);
            }
        }
        if let Some(authorizer) = &self.authorizer {
            authorizer.authorize(parts, &self.registered.route).await?;
        }
        Ok(())
    }

    /// Records a guard's refusal as `access.denied`.
    async fn denied(&self, parts: &Parts, user: Option<Uuid>) {
        let Some(sink) = &self.audit else {
            return;
        };
        let mut entry =
            AuditEntry::new(AuditKind::Security, "access.denied").metadata(serde_json::json!({
                "check": "guard",
                "method": self.registered.route.method,
                "route": self.registered.pattern,
            }));
        if let Some(user) = user {
            entry = entry.actor(user);
        }
        if let Some(request_id) = parts.extensions.get::<RequestId>() {
            entry = entry.request_id(request_id.as_str());
        }
        if let Err(error) = sink.record(entry).await {
            tracing::warn!(target: "navree::audit", %error, "could not write a security audit entry");
        }
    }

    /// The time limit of this run: the handler's, the events' default and
    /// what is left of the caller's deadline, whichever is shortest.
    fn limit(&self, message: Option<&MessageContext>) -> Option<Duration> {
        let left = message.and_then(MessageContext::deadline).map(|deadline| {
            deadline
                .duration_since(std::time::SystemTime::now())
                .unwrap_or(Duration::ZERO)
        });
        [self.registered.meta.timeout, self.default_timeout, left]
            .into_iter()
            .flatten()
            .min()
    }
}

/// The per-pattern pipeline, inside the unit of work: the access check,
/// the inbox, the call context, the time limit and the handler.
async fn handler_pipeline(
    State(runtime): State<Arc<HandlerRuntime>>,
    request: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(runtime.registered.route.clone());
    if parts.extensions.remove::<ActorOnly>().is_some() {
        // The actor only had to reach the unit of work.
        parts.extensions.insert(AuthState(None));
    }
    if let Err(error) = runtime.admit(&mut parts).await {
        return error.into_response();
    }
    let message = parts.extensions.get::<MessageContext>().cloned();
    let uow = parts.extensions.get::<Uow>().cloned();
    let inbox = match (&runtime.inbox, uow, &message) {
        (Some(store), Some(uow), Some(message)) if !message.message_id().is_empty() => {
            Some((store.clone(), uow))
        }
        _ => None,
    };
    if let (Some((store, uow)), Some(message)) = (&inbox, &message) {
        match store
            .inbox_find(uow, &runtime.consumer, message.message_id())
            .await
        {
            Ok(Some(entry)) => {
                tracing::debug!(
                    target: "navree::micro",
                    pattern = message.pattern(),
                    message_id = message.message_id(),
                    "message already handled; skipping it"
                );
                return match entry.reply {
                    Some(reply) => stored_response(reply),
                    None => StatusCode::NO_CONTENT.into_response(),
                };
            }
            Ok(None) => {}
            Err(error) => return AppError::internal(error).into_response(),
        }
    }

    let call = call_context(&parts, message.as_ref());
    let limit = runtime.limit(message.as_ref());
    let run = call.scope(next.run(Request::from_parts(parts, body)));
    let response = match limit {
        Some(limit) => match tokio::time::timeout(limit, run).await {
            Ok(response) => response,
            Err(_) => AppError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "request_timeout",
                "Request Timeout",
            )
            .with_detail("The handler did not finish in time.")
            .into_response(),
        },
        None => run.await,
    };

    match (inbox, message) {
        (Some((store, uow)), Some(message)) if response.status().is_success() => {
            let (response_parts, body) = response.into_parts();
            let body = match axum::body::to_bytes(body, usize::MAX).await {
                Ok(body) => body,
                Err(error) => return AppError::internal(error).into_response(),
            };
            let reply = runtime.registered.meta.dedupe.then(|| StoredReply {
                status: response_parts.status.as_u16(),
                content_type: response_parts
                    .headers
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("application/json")
                    .to_owned(),
                body: body.clone(),
            });
            if let Err(error) = store
                .inbox_insert(
                    &uow,
                    &runtime.consumer,
                    message.message_id(),
                    message.pattern(),
                    reply.as_ref(),
                )
                .await
            {
                return AppError::internal(error).into_response();
            }
            Response::from_parts(response_parts, Body::from(body))
        }
        (_, _) => response,
    }
}

/// A `dedupe` handler's stored reply, answered again.
fn stored_response(reply: StoredReply) -> Response {
    let mut response = Response::new(Body::from(reply.body));
    *response.status_mut() = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::OK);
    if let Ok(value) = HeaderValue::from_str(&reply.content_type) {
        response.headers_mut().insert(CONTENT_TYPE, value);
    }
    response
}

/// The context calls made while handling this message carry: its
/// credentials, request id, trace, workspace, actor and deadline.
fn call_context(parts: &Parts, message: Option<&MessageContext>) -> CallContext {
    let mut call = CallContext::of_parts(parts);
    if call.actor.is_none() {
        call.actor = message.and_then(MessageContext::actor);
    }
    call.deadline = message.and_then(MessageContext::deadline);
    call
}

/// Authenticates a message with the app's `dyn Authenticator` (a
/// `MicroFake` client's principal stands in for it). Events carry no
/// credentials: their actor becomes the unit of work's actor, and the
/// handler sees a guest.
async fn authenticate(
    State(authenticator): State<Option<Arc<dyn Authenticator>>>,
    request: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = request.into_parts();
    let state = if parts.extensions.get::<EventKind>().is_some() {
        match parts.extensions.get::<EventActor>().copied() {
            Some(EventActor(actor)) => {
                parts.extensions.insert(ActorOnly);
                AuthState(Some(Principal {
                    user_id: actor,
                    via: AuthVia::Token,
                    token_abilities: Some(Vec::new()),
                    session_id: None,
                }))
            }
            None => AuthState(None),
        }
    } else if let Some(LocalPrincipal(principal)) =
        parts.extensions.get::<LocalPrincipal>().cloned()
    {
        AuthState(Some(principal))
    } else {
        match &authenticator {
            Some(authenticator) => match authenticator.authenticate(&mut parts).await {
                Ok(principal) => AuthState(principal),
                Err(error) => return error.into_response(),
            },
            None => AuthState(None),
        }
    };
    parts.extensions.insert(state);
    next.run(Request::from_parts(parts, body)).await
}

/// What a failure was, for logs and dead letters: the detail, then the
/// source chain.
pub(crate) fn describe(error: &AppError) -> String {
    let mut out = error.detail.clone().unwrap_or_else(|| error.title.clone());
    let mut next: Option<&(dyn std::error::Error + 'static)> = error
        .source
        .as_deref()
        .map(|e| e as &(dyn std::error::Error + 'static));
    let mut first = true;
    while let Some(cause) = next {
        if first && error.detail.is_none() {
            out = cause.to_string();
        } else {
            out.push_str(": ");
            out.push_str(&cause.to_string());
        }
        first = false;
        next = cause.source();
    }
    out
}

fn short_name(type_name: &'static str) -> &'static str {
    let generics = type_name.find('<').unwrap_or(type_name.len());
    match type_name[..generics].rfind("::") {
        Some(pos) => &type_name[pos + 2..],
        None => type_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_ids_come_from_top_level_fields() {
        let envelope = Envelope::new(
            "deals.get",
            r#"{"id":"0199c1d2-6f7e","n":7,"x":{"y":1},"s":"a b"}"#,
        );
        assert_eq!(record_id(&envelope, "id").unwrap(), "0199c1d2%2D6f7e");
        assert_eq!(record_id(&envelope, "n").unwrap(), "7");
        assert_eq!(record_id(&envelope, "s").unwrap(), "a%20b");
        assert_eq!(
            record_id(&envelope, "x").unwrap_err().status,
            StatusCode::BAD_REQUEST
        );
        assert!(record_id(&envelope, "missing").is_err());
    }

    #[test]
    fn failures_are_described_by_detail_and_source() {
        assert_eq!(describe(&AppError::not_found()), "Not Found");
        assert_eq!(describe(&AppError::bad_request("bad input")), "bad input");
        assert_eq!(
            describe(&AppError::internal("smtp refused the message")),
            "smtp refused the message"
        );
    }
}
