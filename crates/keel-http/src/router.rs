//! [`build_router`] and [`route_table`]: mounting controller routes behind
//! the per-route pipeline and the global layers.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use http::Method;
use http::header::{CONTENT_LENGTH, HeaderName, HeaderValue};
use http::request::Parts;
use keel_contracts::{AuditEntry, AuditKind, AuditSink, AuthState, RequestId};
use keel_kernel::{AppContext, BoxError};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use uuid::Uuid;

use crate::controller::ControllerRegistration;
use crate::limiter::RateLimiter;
use crate::pipeline::{self, ContextState, ErrorState, TimeoutOverride, X_REQUEST_ID};
use crate::{
    AppError, Authenticator, ClientIp, CorsOptions, ErrorMapper, GlobalLayer, HttpOptions,
    HttpServer, LayerSlot, RouteAuthorizer, RouteDef, RouteMeta,
};

/// Builds the app's full router: every controller route behind its per-route
/// pipeline, the single 404 fallback, the contributed [`GlobalLayer`]s and
/// the fixed global layers. Used by `serve` and by tests.
///
/// Fails, listing every problem, when a route has no access declaration, is
/// mounted twice or has an invalid path, or when routes need an
/// [`Authenticator`] or a [`RouteAuthorizer`] that is not registered.
/// `HttpModule` runs this check at boot, so these mistakes stop the app
/// before it binds a port.
pub async fn build_router(ctx: &AppContext) -> Result<Router, BoxError> {
    let server = ctx
        .get::<HttpServer>()
        .await
        .map_err(|error| format!("build_router needs HttpModule in the app: {error}"))?;
    let options = server.options();
    let routes = collect_routes(ctx, options).await?;

    let authenticator = ctx.try_get::<dyn Authenticator>().await;
    let authorizer = ctx.try_get::<dyn RouteAuthorizer>().await;
    validate(&routes, authenticator.is_some(), authorizer.is_some())?;
    let audit = if routes.iter().any(|r| r.meta.audit_action.is_some()) {
        ctx.try_get::<dyn AuditSink>().await
    } else {
        None
    };

    let shared = Shared {
        authorizer,
        audit,
        body_limit: options.body_limit_bytes,
    };
    let mut router = Router::new();
    for route in routes {
        router = mount(router, route, &shared)?;
    }
    router = router.fallback(|| async { AppError::not_found() });
    router = apply_slots(ctx, router, authenticator)?;

    // Fixed layers, innermost first (`Router::layer` wraps like an onion).
    router = router.layer(DefaultBodyLimit::max(options.body_limit_bytes));
    if let Some(limit) = options.per_ip_rate_limit {
        let limiter = Arc::new(RateLimiter::<IpAddr>::new(limit, Duration::from_secs(60)));
        router = router.layer(from_fn_with_state(limiter, pipeline::per_ip_rate_limit));
    }
    router = router.layer(from_fn_with_state(
        Arc::new(pipeline::MaintenanceGate {
            mode: server.maintenance.clone(),
            except: options.maintenance_except.clone(),
        }),
        pipeline::maintenance,
    ));
    let mappers = ctx
        .contributions::<ErrorMapper>()
        .into_iter()
        .map(|c| (*c.value).clone())
        .collect();
    router = router.layer(from_fn_with_state(
        Arc::new(ErrorState { mappers }),
        pipeline::errors,
    ));
    if options.compression {
        router = router.layer(CompressionLayer::new());
    }
    if options.security_headers {
        router = router.layer(from_fn(pipeline::security_headers));
    }
    router = router.layer(from_fn_with_state(
        Duration::from_secs(options.timeout_secs),
        pipeline::timeout,
    ));
    router = router.layer(from_fn(pipeline::catch_panic));
    if let Some(cors) = cors_layer(&options.cors)? {
        router = router.layer(cors);
    }
    router = router.layer(from_fn_with_state(
        Arc::new(ContextState {
            ctx: ctx.clone(),
            trust_proxy: options.trust_proxy,
        }),
        pipeline::request_context,
    ));
    Ok(router)
}

/// Every route of the app with its mounted path and metadata, modules in
/// dependency order. Used by the `routes` command, keel-api and keel-access.
pub async fn route_table(ctx: &AppContext) -> Result<Vec<RouteMeta>, BoxError> {
    let options = ctx
        .get::<HttpOptions>()
        .await
        .map_err(|error| format!("route_table needs HttpModule in the app: {error}"))?;
    Ok(collect_routes(ctx, &options)
        .await?
        .into_iter()
        .map(|route| route.meta)
        .collect())
}

/// A route ready to mount: its metadata (with full path and module) and handler.
struct Mounted {
    controller: &'static str,
    handler: axum::routing::MethodRouter,
    meta: RouteMeta,
}

async fn collect_routes(ctx: &AppContext, options: &HttpOptions) -> Result<Vec<Mounted>, BoxError> {
    let mut mounted = Vec::new();
    for registration in ctx.contributions::<ControllerRegistration>() {
        let resolver = ctx.resolver_for(registration.value.module);
        let controller = (registration.value.resolve)(resolver).await?;
        for RouteDef {
            method,
            path,
            handler,
            mut meta,
        } in controller.routes
        {
            meta.full_path = full_path(
                &options.api_prefix,
                controller.version,
                controller.prefix,
                &path,
            );
            meta.method = method.to_string();
            meta.module = Some(registration.module.name);
            meta.module_id = Some(registration.value.module);
            mounted.push(Mounted {
                controller: short_name(controller.type_name),
                handler,
                meta,
            });
        }
    }
    Ok(mounted)
}

/// `{api_prefix}/v{version}{prefix}{path}`, normalised (single slashes, no
/// trailing slash). A `!` on the path or the prefix makes it absolute.
pub(crate) fn full_path(
    api_prefix: &str,
    version: Option<&str>,
    prefix: &str,
    path: &str,
) -> String {
    if let Some(absolute) = path.strip_prefix('!') {
        return join(&[absolute]);
    }
    if let Some(absolute) = prefix.strip_prefix('!') {
        return join(&[absolute, path]);
    }
    let version = version.map(|v| format!("v{}", v.trim_start_matches(['v', 'V'])));
    match &version {
        Some(version) => join(&[api_prefix, version, prefix, path]),
        None => join(&[api_prefix, prefix, path]),
    }
}

fn join(parts: &[&str]) -> String {
    let mut out = String::new();
    for segment in parts.iter().flat_map(|p| p.split('/')) {
        if !segment.is_empty() {
            out.push('/');
            out.push_str(segment);
        }
    }
    if out.is_empty() {
        out.push('/');
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

/// Every route problem found while building the router.
#[derive(Debug)]
pub struct RouteErrors(pub Vec<String>);

impl fmt::Display for RouteErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

impl std::error::Error for RouteErrors {}

fn validate(routes: &[Mounted], authenticator: bool, authorizer: bool) -> Result<(), RouteErrors> {
    let mut errors = Vec::new();
    let mut seen: HashMap<(&str, &str), &str> = HashMap::new();
    for route in routes {
        let meta = &route.meta;
        let label = format!(
            "route `{} {}` ({} in module `{}`)",
            meta.method,
            meta.full_path,
            route.controller,
            meta.module.unwrap_or("?")
        );
        if let Err(problem) = check_path(&meta.full_path) {
            errors.push(format!("{label}: {problem}"));
        }
        if let Some(first) = seen.insert((&meta.method, &meta.full_path), route.controller) {
            errors.push(format!("{label} is registered twice (also by {first})"));
        }
        let access = &meta.access;
        if !access.is_declared() {
            errors.push(format!(
                "{label} has no access declaration: add .public(), .authenticated(), .roles(..), .permissions(..) or .can(..) (deny by default)"
            ));
            continue;
        }
        if access.public && (access.authenticated || access.needs_authorizer()) {
            errors.push(format!(
                "{label} is public but also declares {}; remove one of them",
                access.describe()
            ));
        }
        if !access.public && !authenticator {
            errors.push(format!(
                "{label} requires authentication but no `dyn Authenticator` is registered (import the auth module, or mark the route public)"
            ));
        }
        if access.needs_authorizer() && !authorizer {
            errors.push(format!(
                "{label} declares {} but no `dyn RouteAuthorizer` is registered (import the access module)",
                access.describe()
            ));
        }
        if let Some(throttle) = meta.throttle
            && (throttle.limit == 0 || throttle.per.is_zero())
        {
            errors.push(format!(
                "{label} has a throttle of {} per {:?}; both must be non-zero",
                throttle.limit, throttle.per
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(RouteErrors(errors))
    }
}

/// Axum 0.8 path syntax: `/`-prefixed, `{param}` and `{*rest}` captures.
fn check_path(path: &str) -> Result<(), String> {
    for segment in path.split('/') {
        if segment.starts_with(':') || segment.starts_with('*') {
            return Err(format!(
                "segment `{segment}` uses old capture syntax; write `{{param}}` or `{{*rest}}`"
            ));
        }
        if segment.matches('{').count() != segment.matches('}').count() {
            return Err(format!("segment `{segment}` has unbalanced braces"));
        }
    }
    Ok(())
}

/// Values every per-route pipeline shares.
struct Shared {
    authorizer: Option<Arc<dyn RouteAuthorizer>>,
    audit: Option<Arc<dyn AuditSink>>,
    body_limit: usize,
}

fn mount(router: Router, route: Mounted, shared: &Shared) -> Result<Router, RouteErrors> {
    let meta = Arc::new(route.meta);
    let runtime = Arc::new(RouteRuntime {
        throttle: meta.throttle.map(|t| RateLimiter::new(t.limit, t.per)),
        authorizer: if meta.access.needs_authorizer() {
            shared.authorizer.clone()
        } else {
            None
        },
        audit: meta
            .audit_action
            .as_ref()
            .and_then(|_| shared.audit.clone()),
        body_limit: meta.body_limit.unwrap_or(shared.body_limit),
        meta: meta.clone(),
    });
    let mut handler = route.handler;
    if let Some(limit) = meta.body_limit {
        handler = handler.route_layer(DefaultBodyLimit::max(limit));
    }
    handler = handler.route_layer(from_fn_with_state(runtime, route_pipeline));
    let path = meta.full_path.clone();
    std::panic::catch_unwind(AssertUnwindSafe(move || router.route(&path, handler))).map_err(|_| {
        RouteErrors(vec![format!(
            "route `{} {}` cannot be mounted (see the panic message above)",
            meta.method, meta.full_path
        )])
    })
}

/// The per-route state: metadata, throttle, authorizer, audit sink.
struct RouteRuntime {
    meta: Arc<RouteMeta>,
    throttle: Option<RateLimiter<ThrottleKey>>,
    authorizer: Option<Arc<dyn RouteAuthorizer>>,
    audit: Option<Arc<dyn AuditSink>>,
    body_limit: usize,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum ThrottleKey {
    User(Uuid),
    Ip(IpAddr),
}

impl RouteRuntime {
    /// Body limit, authentication, throttle, guards, authorization.
    async fn admit(&self, parts: &mut Parts) -> Result<Option<Uuid>, AppError> {
        let meta = &self.meta;
        if let Some(length) = parts
            .headers
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            && length > self.body_limit as u64
        {
            return Err(AppError::payload_too_large(self.body_limit));
        }

        let user = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id);
        if !meta.access.public && user.is_none() {
            return Err(AppError::unauthenticated());
        }

        if let Some(limiter) = &self.throttle {
            let key = match user {
                Some(user) => ThrottleKey::User(user),
                None => ThrottleKey::Ip(client_ip(parts)),
            };
            if let Err(retry_after) = limiter.check(&key) {
                tracing::warn!(
                    target: "keel::security",
                    route = %meta.full_path,
                    user_id = user.map(tracing::field::display),
                    "route throttle exceeded"
                );
                return Err(AppError::too_many_requests(retry_after));
            }
        }

        for guard in &meta.guards {
            guard.check(parts).await?;
        }

        if let Some(authorizer) = &self.authorizer {
            authorizer.authorize(parts, meta).await?;
        }
        Ok(user)
    }

    /// Writes the route's audit action after a 2xx response.
    async fn audit(&self, entry: AuditEntry) {
        if let Some(sink) = &self.audit
            && let Err(error) = sink.record(entry).await
        {
            tracing::warn!(
                target: "keel::audit",
                route = %self.meta.full_path,
                %error,
                "could not write the route's audit entry"
            );
        }
    }

    fn audit_entry(&self, parts: &Parts, user: Option<Uuid>) -> Option<AuditEntry> {
        let action = self.meta.audit_action.as_ref()?;
        self.audit.as_ref()?;
        let mut entry = AuditEntry::new(AuditKind::Custom, action)
            .ip(client_ip(parts).to_string())
            .metadata(serde_json::json!({
                "method": self.meta.method,
                "route": self.meta.full_path,
            }));
        if let Some(user) = user {
            entry = entry.actor(user);
        }
        if let Some(request_id) = parts.extensions.get::<RequestId>() {
            entry = entry.request_id(request_id.as_str());
        }
        if let Some(user_agent) = pipeline::user_agent(&parts.headers) {
            entry = entry.user_agent(user_agent);
        }
        Some(entry)
    }
}

fn client_ip(parts: &Parts) -> IpAddr {
    parts
        .extensions
        .get::<ClientIp>()
        .copied()
        .unwrap_or_else(|| ClientIp::resolve(&parts.headers, &parts.extensions, false))
        .0
}

/// The per-route pipeline: route metadata into the extensions, body limit,
/// authentication, per-user throttle, guards, authorization, route timeout,
/// the handler, then the audit action.
async fn route_pipeline(
    State(runtime): State<Arc<RouteRuntime>>,
    request: Request,
    next: Next,
) -> Response {
    let meta = runtime.meta.clone();
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(meta.clone());
    if let (Some(route_timeout), Some(cell)) =
        (meta.timeout, parts.extensions.get::<TimeoutOverride>())
    {
        let _ = cell.0.set(route_timeout);
    }

    let mut response = match runtime.admit(&mut parts).await {
        Err(error) => error.into_response(),
        Ok(user) => {
            let audit = runtime.audit_entry(&parts, user);
            let run = next.run(Request::from_parts(parts, body));
            let response = match meta.timeout {
                Some(limit) => match tokio::time::timeout(limit, run).await {
                    Ok(response) => response,
                    Err(_) => pipeline::timeout_error().into_response(),
                },
                None => run.await,
            };
            if let Some(entry) = audit
                && response.status().is_success()
            {
                runtime.audit(entry).await;
            }
            response
        }
    };
    response.extensions_mut().insert(meta);
    response
}

/// A layer to apply: (slot, order, layer and its module); `None` is keel's
/// authentication layer.
type SlotEntry<'a> = (LayerSlot, usize, Option<(&'a GlobalLayer, &'static str)>);

/// Applies the contributed [`GlobalLayer`]s by slot, with keel's
/// authentication layer first in the `AuthContext` slot.
fn apply_slots(
    ctx: &AppContext,
    mut router: Router,
    authenticator: Option<Arc<dyn Authenticator>>,
) -> Result<Router, BoxError> {
    let contributed = ctx.contributions::<GlobalLayer>();
    let mut layers: Vec<SlotEntry<'_>> = contributed
        .iter()
        .enumerate()
        .map(|(i, c)| (c.value.slot, i + 1, Some((&*c.value, c.module.name))))
        .collect();
    layers.push((LayerSlot::AuthContext, 0, None));
    layers.sort_by_key(|(slot, order, _)| (*slot, *order));
    for (_, _, layer) in layers.into_iter().rev() {
        router = match layer {
            None => router.layer(from_fn_with_state(
                authenticator.clone(),
                pipeline::authenticate,
            )),
            Some((layer, module)) => (layer.apply)(ctx, router).map_err(|error| {
                format!(
                    "global layer `{}` of module `{module}` failed: {error}",
                    layer.name
                )
            })?,
        };
    }
    Ok(router)
}

/// The CORS layer: exact origins, credentials, explicit headers.
fn cors_layer(options: &CorsOptions) -> Result<Option<CorsLayer>, BoxError> {
    if options.allowed_origins.is_empty() {
        return Ok(None);
    }
    let origins = options
        .allowed_origins
        .iter()
        .map(|origin| HeaderValue::from_str(origin.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    let allow_headers = [
        "accept",
        "accept-language",
        "authorization",
        "content-type",
        "if-match",
        "if-none-match",
        "traceparent",
        "tracestate",
        "x-csrf-token",
        "x-request-id",
        "x-requested-with",
        "x-socket-id",
        "x-xsrf-token",
    ]
    .map(HeaderName::from_static);
    let expose_headers = [
        X_REQUEST_ID.clone(),
        HeaderName::from_static("etag"),
        HeaderName::from_static("location"),
        HeaderName::from_static("retry-after"),
        HeaderName::from_static("content-disposition"),
    ];
    Ok(Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_credentials(options.allow_credentials)
            .allow_methods([
                Method::GET,
                Method::HEAD,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers(allow_headers)
            .expose_headers(expose_headers)
            .max_age(Duration::from_secs(options.max_age_secs)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_paths_join_prefix_version_and_path() {
        assert_eq!(full_path("/api", None, "/deals", "/"), "/api/deals");
        assert_eq!(
            full_path("/api", None, "/deals", "/{id}"),
            "/api/deals/{id}"
        );
        assert_eq!(full_path("/api", Some("1"), "/deals", ""), "/api/v1/deals");
        assert_eq!(
            full_path("/api/", Some("v2"), "deals/", "/x/"),
            "/api/v2/deals/x"
        );
        assert_eq!(full_path("", None, "", "/"), "/");
        assert_eq!(
            full_path("/api", Some("1"), "/x", "!/health/live"),
            "/health/live"
        );
        assert_eq!(
            full_path("/api", Some("1"), "!/sanctum", "/csrf-cookie"),
            "/sanctum/csrf-cookie"
        );
    }

    #[test]
    fn old_capture_syntax_is_rejected() {
        assert!(check_path("/api/deals/{id}").is_ok());
        assert!(check_path("/files/{*path}").is_ok());
        assert!(check_path("/api/deals/:id").is_err());
        assert!(check_path("/files/*path").is_err());
        assert!(check_path("/api/{id").is_err());
    }

    #[test]
    fn short_names_drop_the_module_path() {
        assert_eq!(short_name("app::deals::DealController"), "DealController");
        assert_eq!(short_name("Plain"), "Plain");
    }
}
