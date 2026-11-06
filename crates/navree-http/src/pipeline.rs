//! The fixed global layers of the request pipeline. Outermost first: request
//! context (request id, client ip, tracing span, request log and metrics),
//! CORS, panic catcher, timeout, security headers, compression, error
//! rendering, maintenance mode, per-IP rate limit, body limit; then the
//! [`LayerSlot`](crate::LayerSlot)s (navree's authentication runs first in the
//! `AuthContext` slot) and the router. The fixed layers between CORS and
//! compression run as [`Stages`] of one middleware each.

use std::any::Any;
use std::error::Error as StdError;
use std::future::Future;
use std::net::IpAddr;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::FutureExt;
use http::header::{CACHE_CONTROL, VARY};
use http::header::{
    CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderName, HeaderValue,
    REFERRER_POLICY, STRICT_TRANSPORT_SECURITY, USER_AGENT, X_CONTENT_TYPE_OPTIONS,
    X_FRAME_OPTIONS,
};
use http::{HeaderMap, Method, StatusCode};
use http_body_util::BodyExt;
use navree_contracts::{AuditEntry, AuditKind, AuditSink, AuthState, RequestId};
use navree_kernel::{AppContext, RequestScope};
use navree_telemetry::{
    EventKind, ExceptionReported, RequestHandled, RequestInfo, Telemetry, TelemetryContext,
    TelemetryEvent, TracePolicy,
};
use tokio::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

use crate::body::{Body, HttpBody as _};
use crate::codes;
use crate::error::{ErrorFormat, PendingError};
use crate::error_pages::{self, ErrorContext, ErrorPagesHandle, RouteFinder, RouteKind};
use crate::extract::{ProxyTrust, Request};
use crate::limiter::Limiter;
use crate::maintenance::MaintenanceMode;
use crate::middleware::{Next, State};
use crate::network::rate_limit_key;
use crate::response::{IntoResponse, Response};
use crate::{AppError, Authenticator, ClientIp, ErrorMapper, RouteMeta};

/// The request id header.
pub(crate) static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Plain-text error bodies up to this size are turned into error bodies.
const MAX_NORMALIZED_BODY: u64 = 16 * 1024;

/// The authenticated user, recorded on the response for the request log.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoggedUser(pub(crate) Uuid);

/// The request's tracing span, so inner layers can record fields on it.
#[derive(Clone, Debug)]
pub(crate) struct RequestSpan(pub(crate) tracing::Span);

/// Set by a route with its own timeout, so the global timeout waits for it.
#[derive(Clone, Debug, Default)]
pub(crate) struct TimeoutOverride(pub(crate) Arc<OnceLock<Duration>>);

/// The error returned when a request runs out of time.
pub(crate) fn timeout_error() -> AppError {
    AppError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        codes::REQUEST_TIMEOUT,
        "Request Timeout",
    )
    .with_detail("The server did not finish the request in time.")
}

fn request_id_of(request: &Request) -> Option<RequestId> {
    request.extensions().get::<RequestId>().cloned()
}

// ---------------------------------------------------------------------------
// The fixed layers, as stages of as few middlewares as possible
// ---------------------------------------------------------------------------

/// Fixed layers that sit between the same two Tower layers (CORS,
/// compression) and run as one middleware: each stage awaits the next as
/// a plain future, so a request pays for one middleware, not one per
/// layer. Outermost first: the request context; the guard (panic catcher,
/// timeout, security headers); the checks (error rendering, maintenance
/// mode, per-IP rate limit, and authentication when no `Session`-slot
/// layer comes after them). A missing part is skipped.
#[derive(Default)]
pub(crate) struct Stages {
    pub(crate) context: Option<ContextState>,
    pub(crate) guard: Option<GuardStages>,
    pub(crate) checks: Option<CheckStages>,
}

/// The panic catcher, the app-wide timeout and the security headers.
pub(crate) struct GuardStages {
    /// `http.timeout_secs`.
    pub(crate) timeout: Duration,
    /// `http.security_headers`.
    pub(crate) security_headers: bool,
    /// `http.errors`: the panic catcher and the timeout render their
    /// errors themselves, outside the error stage.
    pub(crate) format: ErrorFormat,
    /// The module that answers errors with pages, if any.
    pub(crate) pages: Option<ErrorPagesHandle>,
}

/// Error rendering, maintenance mode and the per-IP rate limit; then, when
/// no `Session`-slot layer comes between them, navree's authentication.
pub(crate) struct CheckStages {
    pub(crate) mappers: Vec<ErrorMapper>,
    /// `http.errors`.
    pub(crate) format: ErrorFormat,
    /// The module that answers errors with pages, if any.
    pub(crate) pages: Option<ErrorPagesHandle>,
    pub(crate) maintenance: MaintenanceGate,
    pub(crate) per_ip: Option<PerIpLimit>,
    pub(crate) authentication: Option<Authentication>,
}

/// Navree's authentication (the first layer of the `AuthContext` slot):
/// the registered [`Authenticator`], if any.
pub(crate) struct Authentication(pub(crate) Option<Arc<dyn Authenticator>>);

/// The middleware that runs [`Stages`].
pub(crate) async fn run_stages(
    State(stages): State<Arc<Stages>>,
    request: Request,
    next: Next,
) -> Response {
    match &stages.context {
        Some(context) => {
            request_context(context, request, |request| stages.guarded(request, next)).await
        }
        None => stages.guarded(request, next).await,
    }
}

impl Stages {
    async fn guarded(&self, request: Request, next: Next) -> Response {
        let Some(guard) = &self.guard else {
            return self.checked(request, next).await;
        };
        let response = catch_panic(
            guard.format,
            guard.pages.as_ref(),
            request,
            move |request| {
                timeout(
                    guard.timeout,
                    guard.format,
                    guard.pages.as_ref(),
                    request,
                    move |request| self.checked(request, next),
                )
            },
        )
        .await;
        // Outside the panic catcher and the timeout, so what they answer carries the headers too.
        if guard.security_headers {
            with_security_headers(response)
        } else {
            response
        }
    }

    async fn checked(&self, request: Request, next: Next) -> Response {
        let Some(checks) = &self.checks else {
            return next.run(request).await;
        };
        render_errors(
            &checks.mappers,
            checks.format,
            checks.pages.as_ref(),
            request,
            move |request| {
                maintenance(&checks.maintenance, request, move |request| async move {
                    match &checks.per_ip {
                        Some(limit) => {
                            per_ip_rate_limit(limit, request, move |request| {
                                checks.authenticated(request, next)
                            })
                            .await
                        }
                        None => checks.authenticated(request, next).await,
                    }
                })
            },
        )
        .await
    }
}

impl CheckStages {
    async fn authenticated(&self, request: Request, next: Next) -> Response {
        match &self.authentication {
            Some(Authentication(authenticator)) => {
                authenticate(authenticator.as_ref(), request, move |request| {
                    next.run(request)
                })
                .await
            }
            None => next.run(request).await,
        }
    }
}

/// Whether a `metrics` recorder is installed for this thread (a local one)
/// or the process (the global one). The `metrics` crate's no-op recorder,
/// what the macros fall back to without one, is a zero-sized type.
fn metrics_recorder_installed() -> bool {
    metrics::with_recorder(|recorder| std::mem::size_of_val(recorder) != 0)
}

// ---------------------------------------------------------------------------
// Request context: request id, client ip, app context, span, log, metrics
// ---------------------------------------------------------------------------

/// Where the matched route's metadata is kept for the request context. The
/// route sets it before its handler runs, so a handler that panics (and
/// never answers with the metadata) is still reported under its route.
#[derive(Clone, Default)]
pub(crate) struct MatchedRoute(pub(crate) Arc<std::sync::OnceLock<Arc<RouteMeta>>>);

pub(crate) struct ContextState {
    pub(crate) ctx: AppContext,
    pub(crate) proxies: ProxyTrust,
    /// The app's telemetry registry (disabled without sinks).
    pub(crate) telemetry: Telemetry,
    /// How a request's trace starts (`observe.tracing`).
    pub(crate) policy: TracePolicy,
    /// The module that answers errors with pages, if any: with one, each
    /// request carries the facts it decides with.
    pub(crate) pages: Option<ErrorPagesHandle>,
    /// `http.api_prefix`.
    pub(crate) api_prefix: Arc<str>,
    /// Finds a request's route for an error raised before any route ran.
    pub(crate) finder: Option<RouteFinder>,
}

/// The `traceparent` and `tracestate` headers (W3C Trace Context).
static TRACEPARENT: HeaderName = HeaderName::from_static("traceparent");
static TRACESTATE: HeaderName = HeaderName::from_static("tracestate");

/// Accepts a valid incoming `X-Request-Id` or generates one, records the
/// client ip and the app context in the request extensions, starts or
/// continues the request's trace (`traceparent`), runs the request in a
/// tracing span and its [`TelemetryContext`], echoes the id, writes the
/// request log line, emits [`RequestHandled`] and records the request
/// metrics (when a `metrics` recorder is installed).
async fn request_context<F, Fut>(state: &ContextState, mut request: Request, next: F) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let incoming = request
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|value| value.to_str().ok());
    let request_id = RequestId::parse_or_generate(incoming);
    let header = HeaderValue::from_str(request_id.as_str())
        .unwrap_or_else(|_| HeaderValue::from_static("invalid"));
    let ip = ClientIp::resolve(request.headers(), request.extensions(), &state.proxies);
    let trace = state.policy.start(
        header_str(request.headers(), &TRACEPARENT),
        header_str(request.headers(), &TRACESTATE),
    );
    let mut telemetry_context =
        TelemetryContext::new(state.telemetry.clone()).with_request_id(request_id.as_str());
    if let Some(trace) = trace {
        telemetry_context = telemetry_context.with_trace(trace);
    }
    request
        .headers_mut()
        .insert(X_REQUEST_ID.clone(), header.clone());
    let matched_route = MatchedRoute::default();
    let error_context = state.pages.is_some().then(|| ErrorContext {
        method: request.method().clone(),
        uri: request.uri().clone(),
        headers: error_pages::capture(request.headers()),
        ctx: state.ctx.clone(),
        request_id: Some(request_id.clone()),
        route: None,
        user: None,
        api_prefix: state.api_prefix.clone(),
        matched: Some(matched_route.0.clone()),
        finder: state.finder.clone(),
    });
    let extensions = request.extensions_mut();
    extensions.insert(request_id.clone());
    extensions.insert(ip);
    extensions.insert(state.ctx.clone());
    extensions.insert(telemetry_context.clone());
    // Request-scoped providers are built once per request (see `Scoped`).
    extensions.insert(RequestScope::new());
    extensions.insert(matched_route.clone());
    if let Some(error_context) = error_context {
        extensions.insert(Arc::new(error_context));
    }
    let path = state
        .telemetry
        .enabled(EventKind::RequestHandled)
        .then(|| request.uri().path().to_owned());

    let method = request.method().clone();
    // Every log line inside the request carries these fields; `user_id` is
    // recorded once the request is authenticated.
    let span = tracing::info_span!(
        "request",
        method = %method,
        path = %request.uri().path(),
        request_id = %request_id,
        user_id = tracing::field::Empty,
    );
    if !span.is_disabled() {
        request.extensions_mut().insert(RequestSpan(span.clone()));
    }
    let start = Instant::now();
    let mut response = telemetry_context
        .clone()
        .scope(next(request).instrument(span.clone()))
        .await;
    response.headers_mut().insert(X_REQUEST_ID.clone(), header);

    let latency = start.elapsed();
    let matched = response
        .extensions()
        .get::<Arc<RouteMeta>>()
        .or_else(|| matched_route.0.get());
    let route = matched.map_or("unmatched", |meta| meta.full_path.as_str());
    let user = response.extensions().get::<LoggedUser>().map(|u| u.0);
    let status = response.status().as_u16();
    // Counted by method: any token is a method to HTTP, so made-up ones
    // share one name rather than each making a series of its own.
    let counted_method = method_name(&method);
    let event = path.map(|path| RequestHandled {
        method: counted_method.to_owned(),
        route: matched.map(|meta| meta.full_path.clone()),
        path,
        status,
        duration: latency,
        user_id: user
            .map(|user| user.to_string())
            .or_else(|| telemetry_context.user_id().map(str::to_owned)),
        workspace_id: telemetry_context.workspace_id().map(str::to_owned),
        request_id: request_id.as_str().to_owned(),
        client_ip: Some(ip.0.to_string()),
    });
    // Still in the request's context: the event and the log line carry its
    // request id and trace.
    telemetry_context.sync_scope(|| {
        if let Some(event) = event {
            state.telemetry.emit(TelemetryEvent::RequestHandled(event));
        }
        span.in_scope(|| {
            tracing::info!(
                target: "navree::request",
                method = %method,
                route = %route,
                status,
                latency_ms = latency.as_secs_f64() * 1000.0,
                request_id = %request_id,
                user_id = user.map(tracing::field::display),
                ip = %ip.0,
                "request completed"
            );
        });
    });
    if metrics_recorder_installed() {
        let labels = [
            ("method", counted_method.to_owned()),
            ("route", route.to_owned()),
            ("status", status.to_string()),
        ];
        metrics::counter!("http_requests_total", &labels).increment(1);
        metrics::histogram!("http_request_duration_seconds", &labels).record(latency.as_secs_f64());
    }
    response
}

/// The name a request is counted under: one of the methods HTTP defines,
/// or `OTHER`.
fn method_name(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        Method::CONNECT => "CONNECT",
        Method::TRACE => "TRACE",
        _ => "OTHER",
    }
}

// ---------------------------------------------------------------------------
// Panic catcher and timeout (outside the error stage: they render themselves)
// ---------------------------------------------------------------------------

/// Turns a panicking handler or layer into a 500 error (and an
/// [`ExceptionReported`] event with the panic's location).
async fn catch_panic<F, Fut>(
    format: ErrorFormat,
    pages: Option<&ErrorPagesHandle>,
    request: Request,
    next: F,
) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let request_id = request_id_of(&request);
    let context = pages.and_then(|_| error_context_of(&request));
    let reported = TelemetryContext::with_current(|context| {
        context.telemetry().enabled(EventKind::ExceptionReported)
    })
    .unwrap_or(false);
    let request_info = reported.then(|| {
        (
            request.method().to_string(),
            request.uri().path().to_owned(),
        )
    });
    match AssertUnwindSafe(next(request)).catch_unwind().await {
        Ok(response) => response,
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            tracing::error!(
                target: "navree::error",
                request_id = request_id.as_ref().map(tracing::field::display),
                panic = %message,
                "request handler panicked"
            );
            if let Some((method, path)) = request_info {
                let location = TelemetryContext::with_current(|context| {
                    context.panic_location().map(str::to_owned)
                })
                .flatten();
                Telemetry::current().emit(TelemetryEvent::ExceptionReported(ExceptionReported {
                    message: message.clone(),
                    chain: vec![message],
                    location,
                    panic: true,
                    request: Some(RequestInfo {
                        method,
                        path,
                        route: None,
                        status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    }),
                }));
            }
            let error = AppError::from_status(StatusCode::INTERNAL_SERVER_ERROR);
            error_response(&error, format, pages, context.as_deref(), None, None).await
        }
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

/// The app-wide request timeout. A route with its own timeout records it in
/// [`TimeoutOverride`]; the deadline then moves to the route's.
async fn timeout<F, Fut>(
    default: Duration,
    format: ErrorFormat,
    pages: Option<&ErrorPagesHandle>,
    mut request: Request,
    next: F,
) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let request_id = request_id_of(&request);
    let context = pages.and_then(|_| error_context_of(&request));
    let route_timeout = TimeoutOverride::default();
    request.extensions_mut().insert(route_timeout.clone());
    let start = Instant::now();
    let mut deadline = start + default;
    let run = next(request);
    tokio::pin!(run);
    loop {
        tokio::select! {
            // The request first: one that finishes in its first poll never
            // arms the timer.
            biased;
            response = &mut run => return response,
            () = tokio::time::sleep_until(deadline) => {
                if let Some(route) = route_timeout.0.get() {
                    let extended = start + *route;
                    if extended > deadline {
                        deadline = extended;
                        continue;
                    }
                }
                tracing::warn!(
                    target: "navree::request",
                    request_id = request_id.as_ref().map(tracing::field::display),
                    "request timed out"
                );
                return error_response(&timeout_error(), format, pages, context.as_deref(), None, None)
                    .await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Security headers
// ---------------------------------------------------------------------------

/// The default security headers, set unless the response already has them.
static SECURITY_HEADERS: [(HeaderName, &str); 5] = [
    (X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (X_FRAME_OPTIONS, "DENY"),
    (REFERRER_POLICY, "no-referrer"),
    (
        STRICT_TRANSPORT_SECURITY,
        "max-age=31536000; includeSubDomains",
    ),
    (
        CONTENT_SECURITY_POLICY,
        "default-src 'none'; frame-ancestors 'none'; base-uri 'none'",
    ),
];

fn with_security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS.iter() {
        if !headers.contains_key(name) {
            headers.insert(name.clone(), HeaderValue::from_static(value));
        }
    }
    response
}

// ---------------------------------------------------------------------------
// Error rendering
// ---------------------------------------------------------------------------

/// Renders every error response in the JSON error shape: [`AppError`]s (mapped
/// by the contributed [`ErrorMapper`]s, 5xx logged with the request id) and
/// plain-text or empty error responses from Tower layers and extractors.
async fn render_errors<F, Fut>(
    mappers: &[ErrorMapper],
    format: ErrorFormat,
    pages: Option<&ErrorPagesHandle>,
    request: Request,
    next: F,
) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let request_id = request_id_of(&request);
    let method = request.method().clone();
    let uri = request.uri().clone();
    let context = pages.and_then(|_| error_context_of(&request));
    let response = next(request).await;
    finalize(
        response,
        request_id.as_ref(),
        mappers,
        format,
        (pages, context.as_deref()),
        &method,
        uri.path(),
    )
    .await
}

/// The facts the request context kept for the module that renders error pages.
fn error_context_of(request: &Request) -> Option<Arc<ErrorContext>> {
    request.extensions().get::<Arc<ErrorContext>>().cloned()
}

/// How long the module that renders pages gets to answer. It runs on the error path, and
/// an error is no reason to hold a connection.
const PAGES_DEADLINE: Duration = Duration::from_secs(3);

/// The response for `error`: the page the module renders when it answers, else the JSON
/// body. `route` and `user` are what the request found out on its way in: a route that ran
/// has set `route`; for an error raised before one did (or for a panic or a timeout, which
/// leave no response behind) the router is asked which route the request names. A panic or a
/// timeout does not know the user.
async fn error_response(
    error: &AppError,
    format: ErrorFormat,
    pages: Option<&ErrorPagesHandle>,
    context: Option<&ErrorContext>,
    route: Option<Arc<RouteMeta>>,
    user: Option<Uuid>,
) -> Response {
    let (Some(pages), Some(context)) = (pages, context) else {
        return error.render(format);
    };
    let route = match route {
        Some(route) => Some(route),
        None => context.resolve_route().await,
    };
    if route
        .as_ref()
        .is_some_and(|meta| meta.kind == RouteKind::Json)
    {
        return error.render(format);
    }
    let mut context = context.clone();
    context.route = route;
    context.user = user;
    // A module that fails or panics here leaves the error as JSON: the answer to a failure
    // can't be another failure.
    let page = tokio::time::timeout(
        PAGES_DEADLINE,
        AssertUnwindSafe(pages.0.render(&context, error)).catch_unwind(),
    )
    .await;
    if let Ok(Ok(Some(page))) = page {
        return page;
    }
    // The same address answers a page to the runtime and to a browser, and JSON to a script:
    // a cache must keep them apart, and must not keep an error at all.
    let mut response = error.render(format);
    let headers = response.headers_mut();
    headers.append(VARY, HeaderValue::from_static("accept, x-navree-ui"));
    if !headers.contains_key(CACHE_CONTROL) {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

/// Whether a header describes the body it comes with: what an answer replaces, never adds to.
fn describes_body(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "content-type"
            | "content-length"
            | "content-encoding"
            | "content-language"
            | "content-location"
            | "content-disposition"
            | "content-range"
            | "content-security-policy"
            | "cache-control"
            | "etag"
            | "last-modified"
            | "expires"
            | "pragma"
            | "accept-ranges"
            | "transfer-encoding"
            | "speculation-rules"
    )
}

/// Puts `answer`'s headers on `headers`, the response the error was raised on.
///
/// What described the old body goes, and the answer's own description takes its place. Every
/// other header stays: a session's `Set-Cookie`, a layer's marks. The answer adds its own to
/// them, a header with a value the response already has once, so a cookie the answer sets and
/// one the session set both reach the client.
fn merge_answer(headers: &mut HeaderMap, answer: &HeaderMap) {
    let stale: Vec<HeaderName> = headers
        .keys()
        .filter(|name| describes_body(name))
        .cloned()
        .collect();
    for name in stale {
        headers.remove(&name);
    }
    for (name, value) in answer {
        if describes_body(name) || !headers.get_all(name).iter().any(|have| have == value) {
            headers.append(name.clone(), value.clone());
        }
    }
}

async fn finalize(
    mut response: Response,
    request_id: Option<&RequestId>,
    mappers: &[ErrorMapper],
    format: ErrorFormat,
    (pages, context): (Option<&ErrorPagesHandle>, Option<&ErrorContext>),
    method: &Method,
    path: &str,
) -> Response {
    let pending = response
        .extensions_mut()
        .remove::<PendingError>()
        .and_then(|pending| pending.take());
    let error = match pending {
        Some(error) => error,
        None if needs_normalizing(&response) => {
            let (parts, body) = response.into_parts();
            let text = match body.collect().await {
                Ok(collected) => String::from_utf8_lossy(&collected.to_bytes()).into_owned(),
                Err(_) => String::new(),
            };
            let error = AppError::rejection(parts.status, text);
            response = Response::from_parts(parts, Body::empty());
            error
        }
        None => return response,
    };

    let error = if error.status == StatusCode::INTERNAL_SERVER_ERROR && error.source.is_some() {
        ErrorMapper::apply(mappers, &error).unwrap_or(error)
    } else {
        error
    };
    if error.status.is_server_error() {
        tracing::error!(
            target: "navree::error",
            request_id = request_id.map(tracing::field::display),
            method = %method,
            path = %path,
            error = %error.log_chain(),
            "request failed"
        );
        report_exception(&error, &response, method, path);
    }

    let route = response.extensions().get::<Arc<RouteMeta>>().cloned();
    let user = response.extensions().get::<LoggedUser>().map(|u| u.0);
    let rendered = error_response(&error, format, pages, context, route, user).await;
    let (mut parts, _) = response.into_parts();
    let (rendered_parts, body) = rendered.into_parts();
    parts.status = rendered_parts.status;
    parts.headers.remove(CONTENT_LENGTH);
    merge_answer(&mut parts.headers, &rendered_parts.headers);
    Response::from_parts(parts, body)
}

/// Emits an [`ExceptionReported`] for a server error that is a failure (a
/// 500, or any 5xx with an underlying error), not a deliberate 503 such as
/// maintenance mode.
fn report_exception(error: &AppError, response: &Response, method: &Method, path: &str) {
    if error.status != StatusCode::INTERNAL_SERVER_ERROR && error.source.is_none() {
        return;
    }
    let telemetry = Telemetry::current();
    if !telemetry.enabled(EventKind::ExceptionReported) {
        return;
    }
    let mut exception = match error.source.as_deref() {
        Some(source) => ExceptionReported::from_error(source as &(dyn StdError + 'static)),
        None => {
            let message = error.detail.clone().unwrap_or_else(|| error.title.clone());
            ExceptionReported {
                chain: vec![message.clone()],
                message,
                location: None,
                panic: false,
                request: None,
            }
        }
    };
    exception.request = Some(RequestInfo {
        method: method.to_string(),
        path: path.to_owned(),
        route: response
            .extensions()
            .get::<Arc<RouteMeta>>()
            .map(|meta| meta.full_path.clone()),
        status: error.status.as_u16(),
    });
    telemetry.emit(TelemetryEvent::ExceptionReported(exception));
}

/// The header's value, when it is valid text.
fn header_str<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// An error response with no body or a small plain-text body.
fn needs_normalizing(response: &Response) -> bool {
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return false;
    }
    let plain = match response.headers().get(CONTENT_TYPE) {
        None => true,
        Some(value) => value.to_str().is_ok_and(|v| v.starts_with("text/plain")),
    };
    let small = response
        .body()
        .size_hint()
        .upper()
        .is_some_and(|n| n <= MAX_NORMALIZED_BODY);
    plain && small
}

// ---------------------------------------------------------------------------
// Maintenance mode and per-IP rate limit
// ---------------------------------------------------------------------------

/// The state of the maintenance stage: the mode and the paths it never blocks.
pub(crate) struct MaintenanceGate {
    pub(crate) mode: Arc<MaintenanceMode>,
    /// `HttpOptions::maintenance_except`: exact request paths.
    pub(crate) except: Vec<String>,
}

async fn maintenance<F, Fut>(gate: &MaintenanceGate, request: Request, next: F) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let path = request.uri().path();
    if gate.except.iter().any(|except| except == path) {
        return next(request).await;
    }
    match gate.mode.current().await {
        Some(info) => info.error().into_response(),
        None => next(request).await,
    }
}

/// The per-IP limit and where its refusals are audited.
pub(crate) struct PerIpLimit {
    pub(crate) limiter: Limiter<IpAddr>,
    /// `http.rate_limit_ipv6_prefix`: IPv6 clients are counted by network.
    pub(crate) ipv6_prefix: u8,
    pub(crate) audit: Option<Arc<dyn AuditSink>>,
}

async fn per_ip_rate_limit<F, Fut>(limit: &PerIpLimit, request: Request, next: F) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let ip = request
        .extensions()
        .get::<ClientIp>()
        .copied()
        .unwrap_or_else(|| {
            ClientIp::resolve(request.headers(), request.extensions(), &ProxyTrust::none())
        });
    match limit
        .limiter
        .check(&rate_limit_key(ip.0, limit.ipv6_prefix))
        .await
    {
        Ok(()) => next(request).await,
        Err(limited) => {
            tracing::warn!(target: "navree::security", ip = %ip.0, "per-IP rate limit exceeded");
            if limited.report
                && let Some(sink) = &limit.audit
            {
                let entry = security_entry(
                    "http.rate_limited",
                    request.headers(),
                    request.extensions(),
                    ip,
                    serde_json::json!({
                        "limit": "per_ip",
                        "method": request.method().as_str(),
                        "path": request.uri().path(),
                        "retry_after_secs": limited.retry_after.as_secs().max(1),
                    }),
                );
                record_security(sink.as_ref(), entry).await;
            }
            AppError::too_many_requests(limited.retry_after).into_response()
        }
    }
}

/// A security audit entry (`AuditKind::Security`) about the request with
/// `headers` and `extensions` from `ip`: its request id and user agent,
/// and the authenticated user when there is one.
pub(crate) fn security_entry(
    action: &str,
    headers: &HeaderMap,
    extensions: &http::Extensions,
    ip: ClientIp,
    metadata: serde_json::Value,
) -> AuditEntry {
    Requester::of(headers, extensions, ip).security_entry(action, metadata)
}

/// What a security audit entry says about a request: taken (cheaply) while
/// the request is at hand, so the entry is only built if it is recorded.
pub(crate) struct Requester {
    ip: ClientIp,
    actor: Option<Uuid>,
    request_id: Option<RequestId>,
    user_agent: Option<HeaderValue>,
}

impl Requester {
    /// The request with `headers` and `extensions`, from `ip`.
    pub(crate) fn of(headers: &HeaderMap, extensions: &http::Extensions, ip: ClientIp) -> Self {
        Self {
            ip,
            actor: extensions
                .get::<AuthState>()
                .and_then(|state| state.0.as_ref())
                .map(|user| user.user_id),
            request_id: extensions.get::<RequestId>().cloned(),
            user_agent: headers.get(USER_AGENT).cloned(),
        }
    }

    /// The security audit entry `action` about this request.
    pub(crate) fn security_entry(&self, action: &str, metadata: serde_json::Value) -> AuditEntry {
        let mut entry = AuditEntry::new(AuditKind::Security, action)
            .ip(self.ip.0.to_string())
            .metadata(metadata);
        if let Some(user) = self.actor {
            entry = entry.actor(user);
        }
        if let Some(request_id) = &self.request_id {
            entry = entry.request_id(request_id.as_str());
        }
        if let Some(agent) = self
            .user_agent
            .as_ref()
            .and_then(|value| value.to_str().ok())
        {
            entry = entry.user_agent(agent.to_owned());
        }
        entry
    }
}

/// Hands `entry` to `sink`, logging a failure (the response stands).
pub(crate) async fn record_security(sink: &dyn AuditSink, entry: AuditEntry) {
    let action = entry.action.clone();
    if let Err(error) = sink.record(entry).await {
        tracing::warn!(target: "navree::audit", %error, %action, "could not write a security audit entry");
    }
}

// ---------------------------------------------------------------------------
// Authentication (first layer of the AuthContext slot)
// ---------------------------------------------------------------------------

/// [`authenticate`] as a layer of its own, in the `AuthContext` slot (when
/// `Session`-slot layers must run before it).
pub(crate) async fn authenticate_layer(
    State(authenticator): State<Option<Arc<dyn Authenticator>>>,
    request: Request,
    next: Next,
) -> Response {
    authenticate(authenticator.as_ref(), request, move |request| {
        next.run(request)
    })
    .await
}

/// Calls the registered [`Authenticator`] and stores the [`AuthState`].
async fn authenticate<F, Fut>(
    authenticator: Option<&Arc<dyn Authenticator>>,
    request: Request,
    next: F,
) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let (mut parts, body) = request.into_parts();
    let state = match authenticator {
        Some(authenticator) => match authenticator.authenticate(&mut parts).await {
            Ok(principal) => AuthState(principal),
            Err(error) => return error.into_response(),
        },
        None => parts
            .extensions
            .get::<AuthState>()
            .cloned()
            .unwrap_or_default(),
    };
    let user = state.0.as_ref().map(|p| p.user_id);
    if let (Some(user), Some(span)) = (user, parts.extensions.get::<RequestSpan>()) {
        span.0.record("user_id", tracing::field::display(user));
    }
    if let (Some(user), Some(context)) = (user, parts.extensions.get::<TelemetryContext>()) {
        context.set_user_id(user);
    }
    parts.extensions.insert(state);
    let mut response = next(Request::from_parts(parts, body)).await;
    if let Some(user) = user {
        response.extensions_mut().insert(LoggedUser(user));
    }
    response
}

/// The `User-Agent` of a request, for audit entries.
pub(crate) fn user_agent(headers: &http::HeaderMap) -> Option<String> {
    headers
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_empty_error_responses_need_normalizing() {
        let empty = StatusCode::METHOD_NOT_ALLOWED.into_response();
        assert!(needs_normalizing(&empty));
        let text = (StatusCode::UNPROCESSABLE_ENTITY, "bad json").into_response();
        assert!(needs_normalizing(&text));
        let ok = (StatusCode::OK, "fine").into_response();
        assert!(!needs_normalizing(&ok));
        let json = (
            StatusCode::BAD_REQUEST,
            crate::Json(serde_json::json!({"custom": true})),
        )
            .into_response();
        assert!(
            !needs_normalizing(&json),
            "custom JSON errors are left alone"
        );
    }

    #[test]
    fn an_answer_replaces_what_described_the_old_body_and_keeps_the_rest() {
        let mut have = HeaderMap::new();
        for (name, value) in [
            ("content-type", "application/json"),
            ("etag", "\"abc\""),
            ("content-disposition", "attachment"),
            ("cache-control", "max-age=60"),
            ("set-cookie", "sid=1"),
            ("set-cookie", "csrf=2"),
            ("x-rate-limit-remaining", "3"),
            ("retry-after", "5"),
        ] {
            have.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_static(value),
            );
        }
        let mut answer = HeaderMap::new();
        for (name, value) in [
            ("content-type", "text/html"),
            ("cache-control", "no-store"),
            ("set-cookie", "flash=; Max-Age=0"),
            ("set-cookie", "sid=1"),
            ("retry-after", "5"),
            ("vary", "accept"),
        ] {
            answer.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_static(value),
            );
        }
        merge_answer(&mut have, &answer);
        let all = |name: &str| -> Vec<&str> {
            have.get_all(name)
                .iter()
                .map(|v| v.to_str().unwrap())
                .collect()
        };
        assert_eq!(all("content-type"), ["text/html"]);
        assert_eq!(all("cache-control"), ["no-store"]);
        assert!(
            all("etag").is_empty() && all("content-disposition").is_empty(),
            "the old body's own"
        );
        assert_eq!(
            all("set-cookie"),
            ["sid=1", "csrf=2", "flash=; Max-Age=0"],
            "every cookie, none twice"
        );
        assert_eq!(all("x-rate-limit-remaining"), ["3"]);
        assert_eq!(all("retry-after"), ["5"], "the same value is not doubled");
        assert_eq!(all("vary"), ["accept"]);
    }

    #[test]
    fn panic_messages_are_extracted() {
        assert_eq!(panic_message(&"static"), "static");
        assert_eq!(panic_message(&String::from("owned")), "owned");
        assert_eq!(panic_message(&42), "non-string panic payload");
    }
}
