//! The fixed global layers of the request pipeline. Outermost first: request
//! context (request id, client ip, tracing span, request log and metrics),
//! CORS, panic catcher, timeout, security headers, compression, problem
//! rendering, maintenance mode, per-IP rate limit, body limit; then the
//! [`LayerSlot`](crate::LayerSlot)s (keel's authentication runs first in the
//! `AuthContext` slot) and the router.

use std::any::Any;
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
use http::{Method, StatusCode};
use http_body_util::BodyExt;
use keel_contracts::{AuthState, RequestId};
use keel_kernel::AppContext;
use tokio::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

use crate::error::{PROBLEM_JSON, PendingProblem};
use crate::limiter::RateLimiter;
use crate::maintenance::MaintenanceMode;
use crate::{AppError, Authenticator, ClientIp, ErrorMapper, RouteMeta};

/// The request id header.
pub(crate) static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Plain-text error bodies up to this size are turned into problem details.
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
// Request context: request id, client ip, app context, span, log, metrics
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct ContextState {
    pub(crate) ctx: AppContext,
    pub(crate) trust_proxy: bool,
}

/// Accepts a valid incoming `X-Request-Id` or generates one, records the
/// client ip and the app context in the request extensions, runs the request
/// in a tracing span, echoes the id and writes the request log line.
pub(crate) async fn request_context(
    State(state): State<Arc<ContextState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let incoming = request
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|value| value.to_str().ok());
    let request_id = RequestId::parse_or_generate(incoming);
    let header = HeaderValue::from_str(request_id.as_str())
        .unwrap_or_else(|_| HeaderValue::from_static("invalid"));
    let ip = ClientIp::resolve(request.headers(), request.extensions(), state.trust_proxy);
    request
        .headers_mut()
        .insert(X_REQUEST_ID.clone(), header.clone());
    request.extensions_mut().insert(request_id.clone());
    request.extensions_mut().insert(ip);
    request.extensions_mut().insert(state.ctx.clone());

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
    request.extensions_mut().insert(RequestSpan(span.clone()));
    let start = Instant::now();
    let mut response = next.run(request).instrument(span.clone()).await;
    response.headers_mut().insert(X_REQUEST_ID.clone(), header);

    let latency = start.elapsed();
    let route = response
        .extensions()
        .get::<Arc<RouteMeta>>()
        .map_or_else(|| "unmatched".to_owned(), |meta| meta.full_path.clone());
    let user = response.extensions().get::<LoggedUser>().map(|u| u.0);
    let status = response.status().as_u16();
    span.in_scope(|| {
        tracing::info!(
            target: "keel::request",
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
    let labels = [
        ("method", method.to_string()),
        ("route", route),
        ("status", status.to_string()),
    ];
    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_request_duration_seconds", &labels).record(latency.as_secs_f64());
    response
}

// ---------------------------------------------------------------------------
// Panic catcher and timeout (outside the problem layer: they render themselves)
// ---------------------------------------------------------------------------

/// Turns a panicking handler or layer into a 500 problem.
pub(crate) async fn catch_panic(request: Request, next: Next) -> Response {
    let request_id = request_id_of(&request);
    match AssertUnwindSafe(next.run(request)).catch_unwind().await {
        Ok(response) => response,
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            tracing::error!(
                target: "keel::error",
                request_id = request_id.as_ref().map(tracing::field::display),
                panic = %message,
                "request handler panicked"
            );
            AppError::from_status(StatusCode::INTERNAL_SERVER_ERROR)
                .render(request_id.as_ref().map(RequestId::as_str))
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
pub(crate) async fn timeout(
    State(default): State<Duration>,
    mut request: Request,
    next: Next,
) -> Response {
    let request_id = request_id_of(&request);
    let route_timeout = TimeoutOverride::default();
    request.extensions_mut().insert(route_timeout.clone());
    let start = Instant::now();
    let mut deadline = start + default;
    let run = next.run(request);
    tokio::pin!(run);
    loop {
        tokio::select! {
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
                    target: "keel::request",
                    request_id = request_id.as_ref().map(tracing::field::display),
                    "request timed out"
                );
                return timeout_error().render(request_id.as_ref().map(RequestId::as_str));
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

pub(crate) async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS.iter() {
        if !headers.contains_key(name) {
            headers.insert(name.clone(), HeaderValue::from_static(value));
        }
    }
    response
}

// ---------------------------------------------------------------------------
// Problem rendering
// ---------------------------------------------------------------------------

pub(crate) struct ProblemState {
    pub(crate) mappers: Vec<ErrorMapper>,
}

/// Renders every error response as problem+json with the request id:
/// [`AppError`]s (mapped by the contributed [`ErrorMapper`]s, 5xx logged) and
/// plain-text or empty error responses from Axum and Tower layers.
pub(crate) async fn problems(
    State(state): State<Arc<ProblemState>>,
    request: Request,
    next: Next,
) -> Response {
    let request_id = request_id_of(&request);
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let response = next.run(request).await;
    finalize(
        response,
        request_id.as_ref(),
        &state.mappers,
        &method,
        &path,
    )
    .await
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
        .remove::<PendingProblem>()
        .and_then(|pending| pending.take());
    let error = match pending {
        Some(error) => error,
        None if needs_normalizing(&response) => {
            let (parts, body) = response.into_parts();
            let text = match body.collect().await {
                Ok(collected) => String::from_utf8_lossy(&collected.to_bytes()).into_owned(),
                Err(_) => String::new(),
            };
            let mut error = AppError::from_status(parts.status);
            let text = text.trim();
            if !text.is_empty() {
                if parts.status.is_server_error() {
                    error.source = Some(text.to_owned().into());
                } else {
                    error.detail = Some(text.to_owned());
                }
            }
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
            target: "keel::error",
            request_id = request_id.map(tracing::field::display),
            method = %method,
            path = %path,
            error = %error.log_chain(),
            "request failed"
        );
    }

    let rendered = error.render(request_id.map(RequestId::as_str));
    let (mut parts, _) = response.into_parts();
    let (rendered_parts, body) = rendered.into_parts();
    parts.status = rendered_parts.status;
    parts.headers.remove(CONTENT_LENGTH);
    for (name, value) in &rendered_parts.headers {
        parts.headers.insert(name, value.clone());
    }
    Response::from_parts(parts, body)
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
        Some(value) => value
            .to_str()
            .is_ok_and(|v| v.starts_with("text/plain") && !v.starts_with(PROBLEM_JSON)),
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

/// The state of the maintenance layer: the mode and the paths it never blocks.
pub(crate) struct MaintenanceGate {
    pub(crate) mode: Arc<MaintenanceMode>,
    /// `HttpOptions::maintenance_except`: exact request paths.
    pub(crate) except: Vec<String>,
}

pub(crate) async fn maintenance(
    State(gate): State<Arc<MaintenanceGate>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if gate.except.iter().any(|except| except == path) {
        return next.run(request).await;
    }
    match gate.mode.current().await {
        Some(info) => info.error().into_response(),
        None => next.run(request).await,
    }
}

pub(crate) async fn per_ip_rate_limit(
    State(limiter): State<Arc<RateLimiter<IpAddr>>>,
    request: Request,
    next: Next,
) -> Response {
    let ip = request
        .extensions()
        .get::<ClientIp>()
        .copied()
        .unwrap_or_else(|| ClientIp::resolve(request.headers(), request.extensions(), false));
    match limiter.check(&ip.0) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            tracing::warn!(target: "keel::security", ip = %ip.0, "per-IP rate limit exceeded");
            AppError::too_many_requests(retry_after).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Authentication (first layer of the AuthContext slot)
// ---------------------------------------------------------------------------

/// Calls the registered [`Authenticator`] and stores the [`AuthState`].
pub(crate) async fn authenticate(
    State(authenticator): State<Option<Arc<dyn Authenticator>>>,
    request: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = request.into_parts();
    let state = match &authenticator {
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
    parts.extensions.insert(state);
    let mut response = next.run(Request::from_parts(parts, body)).await;
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
