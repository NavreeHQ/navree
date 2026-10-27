//! [`MessageContext`] (what a handler knows about its message) and the
//! call context that follows the work: credentials, request id, trace,
//! workspace, actor and deadline.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use axum::extract::{FromRequestParts, Request};
use axum::middleware::{Next, from_fn};
use axum::response::Response;
use http::header::AUTHORIZATION;
use http::request::Parts;
use navree_contracts::{AuthState, RequestId};
use navree_db::Uow;
use navree_http::{AppError, GlobalLayer, LayerSlot};
use navree_telemetry::{TelemetryContext, TraceContext};
use uuid::Uuid;

use crate::envelope::headers;
use crate::PatternKind;

/// What a handler knows about the message it handles: the pattern, the
/// message id, the sending service, the deadline, the headers, the actor
/// and the attempt.
///
/// ```ignore
/// #[message_pattern("deals.export")]
/// #[permissions("deals.export")]
/// async fn export(&self, msg: MessageContext) -> Result<ExportStarted> {
///     tracing::info!(from = msg.service(), message_id = %msg.message_id(), "export requested");
///     // ...
/// }
/// ```
#[derive(Clone, Debug)]
pub struct MessageContext(pub(crate) Arc<MessageInfo>);

#[derive(Debug)]
pub(crate) struct MessageInfo {
    pub(crate) pattern: String,
    pub(crate) handler_pattern: String,
    pub(crate) kind: PatternKind,
    pub(crate) message_id: String,
    pub(crate) service: String,
    pub(crate) deadline: Option<SystemTime>,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) actor: Option<Uuid>,
    pub(crate) attempt: u32,
    pub(crate) request_id: String,
}

impl MessageContext {
    /// The message's pattern (`deal.won`, also for a `deal.*` handler).
    pub fn pattern(&self) -> &str {
        &self.0.pattern
    }

    /// The pattern of the handler (`deal.*` for a wildcard handler).
    pub fn handler_pattern(&self) -> &str {
        &self.0.handler_pattern
    }

    /// Message or event.
    pub fn kind(&self) -> PatternKind {
        self.0.kind
    }

    /// The message id (`navree-message-id`), the same across retries.
    pub fn message_id(&self) -> &str {
        &self.0.message_id
    }

    /// The sending service (`navree-service`); empty when unknown.
    pub fn service(&self) -> &str {
        &self.0.service
    }

    /// When the caller stops waiting (requests).
    pub fn deadline(&self) -> Option<SystemTime> {
        self.0.deadline
    }

    /// Every header.
    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.0.headers
    }

    /// Header `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.0
            .headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// Events: the id of the user whose action caused the event
    /// (`navree-actor`). Requests: the authenticated caller, if any.
    pub fn actor(&self) -> Option<Uuid> {
        self.0.actor
    }

    /// The attempt number, starting at 1.
    pub fn attempt(&self) -> u32 {
        self.0.attempt
    }

    /// The request id the message carries.
    pub fn request_id(&self) -> &str {
        &self.0.request_id
    }
}

impl<S: Send + Sync> FromRequestParts<S> for MessageContext {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<MessageContext>()
            .cloned()
            .ok_or_else(|| AppError::internal("MessageContext is only available in message handlers"))
    }
}

/// What a `ServiceClient` call copies from the work it is made in.
#[derive(Clone, Debug, Default)]
pub(crate) struct CallContext {
    pub(crate) authorization: Option<String>,
    pub(crate) request_id: Option<String>,
    /// The span the work runs in: what a message sent from it continues.
    pub(crate) trace: Option<TraceContext>,
    pub(crate) workspace: Option<String>,
    pub(crate) actor: Option<Uuid>,
    pub(crate) deadline: Option<SystemTime>,
}

tokio::task_local! {
    static CURRENT: CallContext;
}

/// HTTP requests in flight, by request id: the unit-of-work layer runs the
/// handler in a task of its own, where the task-local context is not
/// visible, so the client finds the request through `Uow::current()`.
static IN_FLIGHT: LazyLock<Mutex<HashMap<String, CallContext>>> = LazyLock::new(Mutex::default);

impl CallContext {
    /// The context of the work the current task does, if any.
    pub(crate) fn current() -> Option<CallContext> {
        if let Ok(context) = CURRENT.try_with(Clone::clone) {
            return Some(context);
        }
        let uow = Uow::current()?;
        let request_id = uow.request_id()?;
        lock(&IN_FLIGHT).get(request_id).cloned()
    }

    /// Runs `future` with this context as the current one.
    pub(crate) async fn scope<F: Future>(self, future: F) -> F::Output {
        CURRENT.scope(self, future).await
    }

    /// The context of an HTTP request (or a message's request parts). The
    /// trace is the current `TelemetryContext`'s (what navree-http and the
    /// message pipeline set), else the one the `traceparent` header
    /// continues.
    pub(crate) fn of_parts(parts: &Parts) -> CallContext {
        let header = |name: &str| {
            parts
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let trace = TelemetryContext::with_current(|context| context.trace().cloned())
            .flatten()
            .or_else(|| {
                header(headers::TRACEPARENT).and_then(|traceparent| {
                    TraceContext::continue_from(&traceparent, header(headers::TRACESTATE).as_deref())
                })
            });
        CallContext {
            authorization: header(AUTHORIZATION.as_str()),
            request_id: parts
                .extensions
                .get::<RequestId>()
                .map(|id| id.as_str().to_owned())
                .or_else(|| header(headers::REQUEST_ID)),
            trace,
            workspace: header(headers::WORKSPACE),
            actor: parts
                .extensions
                .get::<AuthState>()
                .and_then(|state| state.0.as_ref())
                .map(|principal| principal.user_id),
            deadline: None,
        }
    }
}

/// Keeps an HTTP request's context in [`IN_FLIGHT`] while it runs.
struct InFlight(Option<String>);

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Some(id) = self.0.take() {
            lock(&IN_FLIGHT).remove(&id);
        }
    }
}

/// The HTTP layer that makes the current request's context available to
/// `ServiceClient` (the `AuthContext` slot, after authentication).
pub(crate) fn http_context_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::AuthContext, MICRO_CONTEXT_LAYER, |_ctx, router| {
        Ok(router.layer(from_fn(http_context)))
    })
}

/// The name of [`http_context_layer`]; the message pipeline skips it.
pub(crate) const MICRO_CONTEXT_LAYER: &str = "micro-context";

async fn http_context(request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let context = CallContext::of_parts(&parts);
    let _in_flight = InFlight(context.request_id.clone().inspect(|id| {
        lock(&IN_FLIGHT).insert(id.clone(), context.clone());
    }));
    context
        .scope(next.run(Request::from_parts(parts, body)))
        .await
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use navree_contracts::{AuthVia, Principal};

    use super::*;

    #[test]
    fn the_http_context_comes_from_headers_and_extensions() {
        let user = Uuid::now_v7();
        let mut request = http::Request::builder()
            .header("authorization", "Bearer t0k3n")
            .header("traceparent", "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
            .header("x-workspace", "acme")
            .body(())
            .unwrap();
        request
            .extensions_mut()
            .insert(RequestId::parse("req-1").unwrap());
        request.extensions_mut().insert(AuthState(Some(Principal {
            user_id: user,
            via: AuthVia::Test,
            token_abilities: None,
            session_id: None,
        })));
        let (parts, ()) = request.into_parts();
        let context = CallContext::of_parts(&parts);
        assert_eq!(context.authorization.as_deref(), Some("Bearer t0k3n"));
        assert_eq!(context.request_id.as_deref(), Some("req-1"));
        let trace = context.trace.unwrap();
        assert_eq!(trace.trace_id.to_string(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(
            trace.parent_span_id.map(|span| span.to_string()).as_deref(),
            Some("00f067aa0ba902b7"),
            "the header's span is the parent"
        );
        assert_eq!(context.workspace.as_deref(), Some("acme"));
        assert_eq!(context.actor, Some(user));
    }

    #[test]
    fn the_telemetry_context_trace_wins_over_the_header() {
        let request = http::Request::builder()
            .header("traceparent", "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
            .body(())
            .unwrap();
        let (parts, ()) = request.into_parts();
        let span = TraceContext::root(true);
        let context = TelemetryContext::new(navree_telemetry::Telemetry::disabled()).with_trace(span.clone());
        let call = context.sync_scope(|| CallContext::of_parts(&parts));
        assert_eq!(call.trace, Some(span));
    }

    #[tokio::test]
    async fn the_current_context_is_scoped_to_the_task() {
        assert!(CallContext::current().is_none());
        let context = CallContext {
            request_id: Some("r1".into()),
            ..CallContext::default()
        };
        let seen = context
            .scope(async { CallContext::current().and_then(|c| c.request_id) })
            .await;
        assert_eq!(seen.as_deref(), Some("r1"));
        assert!(CallContext::current().is_none());
    }
}
