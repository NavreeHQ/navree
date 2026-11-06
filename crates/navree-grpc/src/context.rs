//! [`CallContext`]: what an outgoing call on a Navree channel inherits from
//! the gRPC call or HTTP request being handled (request id, trace context,
//! authorization, workspace, deadline), kept in a task-local.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use http::{HeaderMap, HeaderName, HeaderValue};
use navree_contracts::RequestId;
use navree_http::extract::Request;
use navree_http::middleware::{Next, State, from_fn_with_state};
use navree_http::response::Response;
use navree_http::{GlobalLayer, HttpOptions, LayerSlot};
use navree_kernel::AppContext;
use tokio::sync::OnceCell;
use tokio::time::Instant;

/// `x-workspace`, the workspace header navree-workspace resolves.
pub(crate) static X_WORKSPACE: HeaderName = HeaderName::from_static("x-workspace");
/// The W3C trace context headers.
pub(crate) static TRACEPARENT: HeaderName = HeaderName::from_static("traceparent");
pub(crate) static TRACESTATE: HeaderName = HeaderName::from_static("tracestate");

tokio::task_local! {
    static CURRENT: CallContext;
}

/// The context of the call or request being handled.
#[derive(Clone, Debug, Default)]
pub(crate) struct CallContext {
    pub(crate) request_id: Option<String>,
    pub(crate) authorization: Option<HeaderValue>,
    pub(crate) traceparent: Option<HeaderValue>,
    pub(crate) tracestate: Option<HeaderValue>,
    pub(crate) workspace: Option<HeaderValue>,
    pub(crate) deadline: Option<Instant>,
}

impl CallContext {
    /// The context of a request with `headers`.
    pub(crate) fn of(
        headers: &HeaderMap,
        request_id: Option<&RequestId>,
        deadline: Option<Instant>,
    ) -> Self {
        let header = |name: &HeaderName| headers.get(name).cloned();
        Self {
            request_id: request_id.map(|id| id.as_str().to_owned()),
            authorization: header(&http::header::AUTHORIZATION),
            traceparent: header(&TRACEPARENT),
            tracestate: header(&TRACESTATE),
            workspace: header(&X_WORKSPACE),
            deadline,
        }
    }

    /// The context of the task, if it is handling a call or a request.
    pub(crate) fn current() -> Option<CallContext> {
        CURRENT.try_with(Clone::clone).ok()
    }

    /// Runs `future` with this context.
    pub(crate) async fn scope<F: Future>(self, future: F) -> F::Output {
        CURRENT.scope(self, future).await
    }
}

/// The layer that records an HTTP request's context for the gRPC calls its
/// handler makes. It sits in the `Uow` slot, so it runs inside navree-db's
/// unit-of-work task when `GrpcModule` comes after `DatabaseModule`.
pub(crate) fn http_context_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Uow, "grpc-context", |_ctx, router| {
        let timeout: Arc<OnceCell<Duration>> = Arc::new(OnceCell::new());
        Ok(router.layer(from_fn_with_state(timeout, record_http_context)))
    })
}

async fn record_http_context(
    State(timeout): State<Arc<OnceCell<Duration>>>,
    request: Request,
    next: Next,
) -> Response {
    let timeout = match request.extensions().get::<AppContext>() {
        Some(ctx) => Some(
            *timeout
                .get_or_init(|| async {
                    let secs = match ctx.try_get::<HttpOptions>().await {
                        Some(options) => options.timeout_secs,
                        None => HttpOptions::default().timeout_secs,
                    };
                    Duration::from_secs(secs)
                })
                .await,
        ),
        None => None,
    };
    let context = CallContext::of(
        request.headers(),
        request.extensions().get::<RequestId>(),
        timeout.map(|timeout| Instant::now() + timeout),
    );
    context.scope(next.run(request)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_context_is_the_tasks() {
        assert!(CallContext::current().is_none());
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer t"),
        );
        headers.insert(&X_WORKSPACE, HeaderValue::from_static("agra"));
        let id = RequestId::parse_or_generate(Some("req-1"));
        let context = CallContext::of(&headers, Some(&id), None);
        let seen = context
            .scope(async { CallContext::current().expect("inside the scope") })
            .await;
        assert_eq!(seen.authorization.unwrap(), "Bearer t");
        assert_eq!(seen.workspace.unwrap(), "agra");
        assert_eq!(seen.request_id.as_deref(), Some(id.as_str()));
        assert!(CallContext::current().is_none());
    }
}
