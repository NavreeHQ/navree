//! The HTTP layer Lighthouse adds to the app (slot `AuthContext`, after the
//! authenticator): for every request outside the dashboard's path, it keeps
//! the (masked) headers, and the JSON body when
//! `lighthouse.capture.request_body` is on, when the request is stored
//! (slow, a 5xx, something reported, or sampled). The dashboard's own
//! requests pass untouched: signing in, the session and its CSRF token
//! belong to the app's session and sign-in pages.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use http::HeaderValue;
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use navree_contracts::RequestId;
use navree_http::body::{Body, Bytes, to_bytes};
use navree_http::extract::Request;
use navree_http::middleware::{Next, from_fn};
use navree_http::response::Response;
use navree_http::{GlobalLayer, LayerSlot};
use navree_kernel::AppContext;
use tokio::sync::OnceCell;

use crate::engine::{EngineHandle, Input, RequestCapture, sampled};
use crate::normalize::path_matches;
use crate::options::LighthouseOptions;
use crate::service::{Lighthouse, REPORTED};
use crate::settings::Tuning;

/// The largest JSON request body kept.
pub(crate) const MAX_BODY: usize = 16 * 1024;

/// The layer, resolving what it needs on the first request.
pub(crate) fn layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::AuthContext, "lighthouse", |ctx, router| {
        let state = Arc::new(State {
            ctx: ctx.clone(),
            resolved: OnceCell::new(),
        });
        Ok(router.layer(from_fn(move |request: Request, next: Next| {
            let state = state.clone();
            async move { state.handle(request, next).await }
        })))
    })
}

struct State {
    ctx: AppContext,
    resolved: OnceCell<Option<Resolved>>,
}

/// What the layer works with, resolved once.
struct Resolved {
    options: Arc<LighthouseOptions>,
    tuning: Arc<Tuning>,
    mount: String,
    engine: Option<EngineHandle>,
    ignored: Vec<String>,
}

impl State {
    async fn resolved(&self) -> Option<&Resolved> {
        self.resolved
            .get_or_init(|| async {
                let lighthouse = self.ctx.try_get::<Lighthouse>().await?;
                let options = Arc::new(lighthouse.options().clone());
                let mount = options.mount_path().to_owned();
                let mut ignored = options.ignore.routes.clone();
                ignored.push(mount.clone());
                ignored.push(format!("{mount}/*"));
                Some(Resolved {
                    engine: lighthouse.engine().cloned(),
                    tuning: lighthouse.tuning().clone(),
                    options,
                    mount,
                    ignored,
                })
            })
            .await
            .as_ref()
    }

    async fn handle(&self, request: Request, next: Next) -> Response {
        let Some(resolved) = self.resolved().await else {
            return next.run(request).await;
        };
        let path = request.uri().path();
        if under(path, &resolved.mount) {
            return next.run(request).await;
        }
        match &resolved.engine {
            Some(engine) if !resolved.ignored.iter().any(|p| path_matches(p, path)) => {
                capture(resolved, engine, request, next).await
            }
            _ => next.run(request).await,
        }
    }
}

/// Whether `path` is `mount` or below it.
pub(crate) fn under(path: &str, mount: &str) -> bool {
    path.strip_prefix(mount)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Keeps the headers (and JSON body) of requests that are stored.
async fn capture(
    resolved: &Resolved,
    engine: &EngineHandle,
    request: Request,
    next: Next,
) -> Response {
    let Some(request_id) = request
        .extensions()
        .get::<RequestId>()
        .map(|id| id.as_str().to_owned())
    else {
        return next.run(request).await;
    };
    let options = &resolved.options;
    let headers = options
        .capture
        .request_headers
        .then(|| request.headers().clone());
    let (request, body) = if options.capture.request_body && json_body_fits(&request) {
        let (parts, body) = request.into_parts();
        match to_bytes(body, MAX_BODY).await {
            Ok(bytes) => (
                Request::from_parts(parts, Body::from(bytes.clone())),
                Some(bytes),
            ),
            Err(_) => (Request::from_parts(parts, Body::empty()), None::<Bytes>),
        }
    } else {
        (request, None)
    };
    let started = Instant::now();
    let reported = Arc::new(AtomicBool::new(false));
    let response = REPORTED.scope(reported.clone(), next.run(request)).await;
    let slow = started.elapsed().as_millis() >= u128::from(resolved.tuning.slow_request_ms());
    let keep = slow
        || response.status().is_server_error()
        || reported.load(Ordering::Relaxed)
        || sampled(&request_id, resolved.tuning.sample_requests());
    if keep && (headers.is_some() || body.is_some()) {
        engine.send(Input::Capture(
            request_id,
            Box::new(RequestCapture {
                headers: headers.unwrap_or_default(),
                body,
            }),
        ));
    }
    response
}

/// Whether the request has a JSON body of a known length (its
/// `Content-Length`, or the size of a body already in memory) up to
/// [`MAX_BODY`].
fn json_body_fits(request: &Request) -> bool {
    let json = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    use navree_http::body::HttpBody as _;
    let length = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value: &HeaderValue| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| request.body().size_hint().upper());
    json && length.is_some_and(|length| length <= MAX_BODY as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_paths_match_whole_segments() {
        assert!(under("/_lighthouse", "/_lighthouse"));
        assert!(under("/_lighthouse/issues", "/_lighthouse"));
        assert!(!under("/_lighthousex", "/_lighthouse"));
        assert!(!under("/api/_lighthouse", "/_lighthouse"));
    }
}
