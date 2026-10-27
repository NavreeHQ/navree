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

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use futures_util::FutureExt;
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

use crate::error::PendingError;
use crate::extract::ProxyTrust;
use crate::limiter::RateLimiter;
use crate::maintenance::MaintenanceMode;
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
        "request_timeout",
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
/// mode, per-IP rate limit). A missing part is skipped.
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
}

/// Error rendering, maintenance mode and the per-IP rate limit; then, when
/// no `Session`-slot layer comes between them, navree's authentication.
pub(crate) struct CheckStages {
    pub(crate) mappers: Vec<ErrorMapper>,
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
        catch_panic(request, move |request| {
            timeout(guard.timeout, request, move |request| async move {
                let response = self.checked(request, next).await;
                if guard.security_headers {
                    with_security_headers(response)
                } else {
                    response
                }
            })
        })
        .await
    }

    async fn checked(&self, request: Request, next: Next) -> Response {
        let Some(checks) = &self.checks else {
            return next.run(request).await;
        };
        render_errors(&checks.mappers, request, move |request| {
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
        })
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

pub(crate) struct ContextState {
    pub(crate) ctx: AppContext,
    pub(crate) proxies: ProxyTrust,
    /// The app's telemetry registry (disabled without sinks).
    pub(crate) telemetry: Telemetry,
    /// How a request's trace starts (`observe.tracing`).
    pub(crate) policy: TracePolicy,
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
    let extensions = request.extensions_mut();
    extensions.insert(request_id.clone());
    extensions.insert(ip);
    extensions.insert(state.ctx.clone());
    extensions.insert(telemetry_context.clone());
    // Request-scoped providers are built once per request (see `Scoped`).
    extensions.insert(RequestScope::new());
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
    let matched = response.extensions().get::<Arc<RouteMeta>>();
    let route = matched.map_or("unmatched", |meta| meta.full_path.as_str());
    let user = response.extensions().get::<LoggedUser>().map(|u| u.0);
    let status = response.status().as_u16();
    let event = path.map(|path| RequestHandled {
        method: method.to_string(),
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
            ("method", method.to_string()),
            ("route", route.to_owned()),
            ("status", status.to_string()),
        ];
        metrics::counter!("http_requests_total", &labels).increment(1);
        metrics::histogram!("http_request_duration_seconds", &labels)
            .record(latency.as_secs_f64());
    }
    response
}

// ---------------------------------------------------------------------------
// Panic catcher and timeout (outside the error stage: they render themselves)
// ---------------------------------------------------------------------------

/// Turns a panicking handler or layer into a 500 error (and an
/// [`ExceptionReported`] event with the panic's location).
async fn catch_panic<F, Fut>(request: Request, next: F) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let request_id = request_id_of(&request);
    let reported = TelemetryContext::with_current(|context| {
        context
            .telemetry()
            .enabled(EventKind::ExceptionReported)
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
            AppError::from_status(StatusCode::INTERNAL_SERVER_ERROR).render()
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
async fn timeout<F, Fut>(default: Duration, mut request: Request, next: F) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let request_id = request_id_of(&request);
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
                return timeout_error().render();
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
/// plain-text or empty error responses from Axum and Tower layers.
async fn render_errors<F, Fut>(mappers: &[ErrorMapper], request: Request, next: F) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let request_id = request_id_of(&request);
    let method = request.method().clone();
    let uri = request.uri().clone();
    let response = next(request).await;
    finalize(response, request_id.as_ref(), mappers, &method, uri.path()).await
}

async fn finalize(
    mut response: Response,
    request_id: Option<&RequestId>,
    mappers: &[ErrorMapper],
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

    let rendered = error.render();
    let (mut parts, _) = response.into_parts();
    let (rendered_parts, body) = rendered.into_parts();
    parts.status = rendered_parts.status;
    parts.headers.remove(CONTENT_LENGTH);
    for (name, value) in &rendered_parts.headers {
        parts.headers.insert(name, value.clone());
    }
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
    use axum::body::HttpBody as _;
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
    pub(crate) limiter: RateLimiter<IpAddr>,
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
    match limit.limiter.check(&ip.0) {
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
    let mut entry = AuditEntry::new(AuditKind::Security, action)
        .ip(ip.0.to_string())
        .metadata(metadata);
    if let Some(user) = extensions
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
    {
        entry = entry.actor(user.user_id);
    }
    if let Some(request_id) = extensions.get::<RequestId>() {
        entry = entry.request_id(request_id.as_str());
    }
    if let Some(agent) = user_agent(headers) {
        entry = entry.user_agent(agent);
    }
    entry
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
            axum::Json(serde_json::json!({"custom": true})),
        )
            .into_response();
        assert!(
            !needs_normalizing(&json),
            "custom JSON errors are left alone"
        );
    }

    #[test]
    fn panic_messages_are_extracted() {
        assert_eq!(panic_message(&"static"), "static");
        assert_eq!(panic_message(&String::from("owned")), "owned");
        assert_eq!(panic_message(&42), "non-string panic payload");
    }
}
