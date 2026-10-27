//! The streamable HTTP transport, mounted on the app's own router: `POST`,
//! `GET` and `DELETE` at `mcp.path`, the OAuth protected resource metadata
//! and the `WWW-Authenticate` challenge of 401s.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use http::header::{HOST, ORIGIN, WWW_AUTHENTICATE};
use http::{HeaderValue, Method, StatusCode};
use navree_cache::Cache;
use navree_contracts::AuthState;
use navree_http::{
    ApiMeta, AppError, Controller, GlobalLayer, Json, LayerSlot, RouteDef, RouteMeta,
};
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, Resolver};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::handler::{McpHandler, Shared};
use crate::notifier::McpNotifier;
use crate::options::{McpAuth, McpSettings};
use crate::registry::McpRegistry;
use crate::sessions::{CacheSessionStore, Sessions};

/// The session header.
const SESSION_HEADER: &str = "mcp-session-id";

/// Serves the HTTP transport (registered by `McpModule`).
pub(crate) struct McpController {
    transport: Arc<Transport>,
}

/// The rmcp service and what wraps it.
struct Transport {
    settings: Arc<McpSettings>,
    service: StreamableHttpService<McpHandler, LocalSessionManager>,
    sessions: Arc<Sessions>,
    notifier: Arc<McpNotifier>,
}

impl Injectable for McpController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let settings = r.get::<McpSettings>().await?;
            let registry = r.get::<McpRegistry>().await?;
            let notifier = r.get::<McpNotifier>().await?;
            let app = r.app();
            let shared = Shared::new(&app, registry, &settings, notifier.clone())
                .await
                .map_err(|error| KernelError::Construction {
                    provider: std::any::type_name::<McpController>().to_owned(),
                    source: error.into(),
                })?;
            let cache = app.try_get::<Cache>().await;
            let ttl = Duration::from_secs(settings.session_ttl_secs);
            Ok(McpController {
                transport: Arc::new(Transport::new(
                    Arc::new(shared),
                    settings,
                    notifier,
                    cache,
                    ttl,
                )),
            })
        })
    }
}

impl Transport {
    fn new(
        shared: Arc<Shared>,
        settings: Arc<McpSettings>,
        notifier: Arc<McpNotifier>,
        cache: Option<Arc<Cache>>,
        ttl: Duration,
    ) -> Self {
        let mut manager = LocalSessionManager::default();
        manager.session_config.keep_alive = Some(ttl);
        let mut config = StreamableHttpServerConfig::default()
            // The app's own pipeline and the Origin check below guard the
            // endpoint; rmcp's loopback-only Host check would refuse every
            // deployed app.
            .disable_allowed_hosts()
            .disable_allowed_origins()
            .with_json_response(true)
            .with_max_request_body_bytes(settings.max_request_bytes)
            .with_cancellation_token(tokio_util::sync::CancellationToken::new());
        if let Some(cache) = &cache {
            config.session_store = Some(Arc::new(CacheSessionStore::new(cache.clone(), ttl)));
        }
        let service = StreamableHttpService::new(
            move || Ok(McpHandler::new(shared.clone(), None)),
            Arc::new(manager),
            config,
        );
        Self {
            settings,
            service,
            sessions: Arc::new(Sessions::new(cache, ttl)),
            notifier,
        }
    }

    /// One request of the transport: the Origin check, the session owner
    /// check, then rmcp.
    async fn handle(&self, mut request: Request) -> Response {
        // HTTP/1.1 always has a Host; requests built in process (tests) may
        // not, and rmcp needs one (any value: the host check is off).
        if request.uri().authority().is_none() && !request.headers().contains_key(HOST) {
            request
                .headers_mut()
                .insert(HOST, HeaderValue::from_static("localhost"));
        }
        if let Some(origin) = request.headers().get(ORIGIN) {
            let allowed = origin
                .to_str()
                .is_ok_and(|origin| self.settings.origin_allowed(origin));
            if !allowed {
                return AppError::forbidden()
                    .with_detail("This origin may not call the MCP endpoint")
                    .into_response();
            }
        }
        let user = request
            .extensions()
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id);
        let session = request
            .headers()
            .get(SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if let (Some(session), Some(user)) = (&session, user) {
            match self.sessions.owner(session).await {
                Some(owner) if owner != user => {
                    return AppError::not_found()
                        .with_detail("Session not found")
                        .into_response();
                }
                Some(_) => self.sessions.touch(session).await,
                None => {}
            }
        }
        let method = request.method().clone();
        let response = self.service.handle(request).await;
        match (&session, user) {
            (None, Some(user)) => {
                if let Some(created) = response
                    .headers()
                    .get(SESSION_HEADER)
                    .and_then(|value| value.to_str().ok())
                {
                    self.sessions.claim(created, user).await;
                }
            }
            (Some(session), _) if method == Method::DELETE && response.status().is_success() => {
                self.sessions.forget(session).await;
                self.notifier.close_session(session);
            }
            _ => {}
        }
        response.map(Body::new)
    }
}

impl Controller for McpController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let settings = &self.transport.settings;
        let path = format!("!{}", settings.path);
        // The body limit leaves room for the request line's overhead; rmcp
        // enforces `max_request_bytes` on the JSON-RPC message itself.
        let body_limit = settings.max_request_bytes;
        // One name per route: navree-api turns names into operation ids.
        let transport_routes = [
            (
                Method::POST,
                "mcp.message",
                "Send an MCP JSON-RPC message (JSON or SSE answer)",
            ),
            (
                Method::GET,
                "mcp.stream",
                "Open the SSE stream of server-sent MCP messages",
            ),
            (Method::DELETE, "mcp.close", "End the MCP session"),
        ];
        let mut routes: Vec<RouteDef> = transport_routes
            .into_iter()
            .map(|(method, name, summary)| {
                let transport = self.transport.clone();
                RouteDef::on(method, path.clone(), move |request: Request| {
                    let transport = transport.clone();
                    async move { transport.handle(request).await }
                })
                .authenticated()
                .body_limit(body_limit)
                .name(name)
                .api(api_meta(summary))
            })
            .collect();
        if let Some(oauth) = &settings.oauth {
            let document = oauth.metadata(&settings.title);
            let metadata_routes = [
                (
                    "!/.well-known/oauth-protected-resource".to_owned(),
                    "mcp.metadata",
                ),
                (
                    format!("!/.well-known/oauth-protected-resource{}", settings.path),
                    "mcp.metadata.endpoint",
                ),
            ];
            for (path, name) in metadata_routes {
                let document = document.clone();
                routes.push(
                    RouteDef::on(Method::GET, path, move || {
                        let document = document.clone();
                        async move { Json(document) }
                    })
                    .public()
                    .name(name)
                    .api(api_meta(
                        "The MCP endpoint's OAuth protected resource metadata (RFC 9728)",
                    )),
                );
            }
        }
        routes
    }
}

/// The OpenAPI metadata of an MCP route: tag `MCP` and `summary`.
fn api_meta(summary: &str) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["MCP".to_owned()],
        ..ApiMeta::default()
    }
}

/// The layer that adds the `WWW-Authenticate` challenge to the endpoint's
/// 401s: `Bearer` with `auth = "bearer"`, `Bearer resource_metadata="..."`
/// with `auth = "oauth"` (keeping an authenticator's `error` parameters).
pub(crate) fn challenge_layer() -> GlobalLayer {
    GlobalLayer::new(
        LayerSlot::Session,
        "mcp-challenge",
        |_ctx, router: Router| Ok(router.layer(from_fn(challenge))),
    )
}

async fn challenge(request: Request, next: Next) -> Response {
    let settings = match request.extensions().get::<AppContext>() {
        Some(ctx) => ctx.get::<McpSettings>().await.ok(),
        None => None,
    };
    let Some(settings) = settings.filter(|settings| request.uri().path() == settings.path) else {
        return next.run(request).await;
    };
    let response = next.run(request).await;
    if response.status() != StatusCode::UNAUTHORIZED {
        return response;
    }
    let existing = response
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let value = match (settings.auth, &settings.oauth) {
        (McpAuth::Oauth, Some(oauth)) => {
            let metadata = format!("resource_metadata=\"{}\"", oauth.metadata_url());
            match existing.as_deref().and_then(|e| e.strip_prefix("Bearer")) {
                Some(params) if !params.trim().is_empty() => {
                    format!(
                        "Bearer {metadata}, {}",
                        params.trim().trim_start_matches(',')
                    )
                }
                _ => format!("Bearer {metadata}"),
            }
        }
        // An authenticator's own challenge (`error="invalid_token"`) stays.
        _ if existing.is_some() => return response,
        _ => "Bearer".to_owned(),
    };
    let Ok(value) = HeaderValue::from_str(&value) else {
        return response;
    };
    rebuild(response, value)
}

/// The 401 as a fresh response with the challenge. The rendered error body
/// and headers are kept; the pending error in the extensions is dropped, so
/// the error layer does not render it again over the new header. The
/// route's metadata stays for request metrics.
fn rebuild(response: Response, challenge: HeaderValue) -> Response {
    let (parts, body) = response.into_parts();
    let mut rebuilt = Response::new(body);
    *rebuilt.status_mut() = parts.status;
    *rebuilt.headers_mut() = parts.headers;
    rebuilt.headers_mut().insert(WWW_AUTHENTICATE, challenge);
    if let Some(meta) = parts.extensions.get::<Arc<RouteMeta>>() {
        rebuilt.extensions_mut().insert(meta.clone());
    }
    rebuilt
}
