//! [`RouteDef`] and its metadata: access declaration, guards, throttle,
//! timeout, body limit, audit action and OpenAPI metadata.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use axum::handler::Handler;
use axum::routing::{MethodFilter, MethodRouter};
use http::Method;

use crate::Guard;

/// One route: a method, a path, an Axum handler and its metadata.
///
/// The path is relative to the controller (`/{id}`), Axum 0.8 syntax
/// (`{param}`, `{*rest}`). A path starting with `!` is absolute: it is mounted
/// as written, without the API prefix, version or controller prefix
/// (`!/sanctum/csrf-cookie`, `!/health/live`).
///
/// Every route must declare its access ([`public`](RouteDef::public),
/// [`authenticated`](RouteDef::authenticated), [`roles`](RouteDef::roles),
/// [`permissions`](RouteDef::permissions) or [`can`](RouteDef::can)); the app
/// refuses to boot otherwise.
///
/// ```
/// use axum::routing::get;
/// use http::Method;
/// use navree_http::RouteDef;
///
/// let route = RouteDef::new(Method::GET, "/{id}", get(|| async { "deal" }))
///     .name("deals.show")
///     .can("read", "Deal", Some("id"));
/// assert!(route.meta.access.is_declared());
/// ```
#[derive(Clone)]
pub struct RouteDef {
    /// The HTTP method (must match the method `handler` routes).
    pub method: Method,
    /// The path relative to the controller, or absolute when it starts with `!`.
    pub path: String,
    /// The Axum handler, e.g. `axum::routing::get(handler)`.
    pub handler: MethodRouter,
    /// Access declaration, guards, limits and documentation.
    pub meta: RouteMeta,
}

impl RouteDef {
    /// A route with no access declaration yet.
    pub fn new(method: Method, path: impl Into<String>, handler: MethodRouter) -> Self {
        let path = path.into();
        Self {
            meta: RouteMeta {
                method: method.to_string(),
                ..RouteMeta::default()
            },
            method,
            path,
            handler,
        }
    }

    /// A route whose handler answers exactly `method` (so the method and the
    /// handler cannot disagree, as they can with [`new`](RouteDef::new)).
    ///
    /// # Panics
    ///
    /// For extension methods Axum cannot route (anything but the standard
    /// methods).
    pub fn on<H, T>(method: Method, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T, ()>,
        T: 'static,
    {
        let filter = MethodFilter::try_from(method.clone())
            .unwrap_or_else(|_| panic!("axum cannot route the HTTP method `{method}`"));
        Self::new(method, path, axum::routing::on(filter, handler))
    }

    /// No authentication or authorization needed.
    pub fn public(mut self) -> Self {
        self.meta.access.public = true;
        self
    }

    /// Any signed-in user may call the route (no further authorization).
    pub fn authenticated(mut self) -> Self {
        self.meta.access.authenticated = true;
        self
    }

    /// Requires any of the roles (checked by the [`RouteAuthorizer`](crate::RouteAuthorizer)).
    pub fn roles(mut self, roles: &[&str]) -> Self {
        self.meta
            .access
            .roles
            .extend(roles.iter().map(|r| (*r).to_owned()));
        self
    }

    /// Requires the permissions (checked by the [`RouteAuthorizer`](crate::RouteAuthorizer)).
    pub fn permissions(mut self, permissions: &[&str]) -> Self {
        self.meta
            .access
            .permissions
            .extend(permissions.iter().map(|p| (*p).to_owned()));
        self
    }

    /// Requires the ability `action` on `subject`; with `from`, on the record
    /// whose id is the path parameter `from`.
    pub fn can(mut self, action: &str, subject: &str, from: Option<&str>) -> Self {
        self.meta.access.can = Some(CanSpec {
            action: action.to_owned(),
            subject: subject.to_owned(),
            from: from.map(str::to_owned),
        });
        self
    }

    /// The route name (e.g. `deals.show`).
    pub fn name(mut self, name: &str) -> Self {
        self.meta.name = Some(name.to_owned());
        self
    }

    /// At most `limit` requests per `per`, per user (per client IP for guests).
    pub fn throttle(mut self, limit: u32, per: Duration) -> Self {
        self.meta.throttle = Some(Throttle { limit, per });
        self
    }

    /// Overrides the app-wide request timeout (shorter or longer).
    pub fn timeout(mut self, d: Duration) -> Self {
        self.meta.timeout = Some(d);
        self
    }

    /// Marks the handler as transactional. Informational (route table,
    /// OpenAPI, logs): navree-db runs every request in a lazy unit of work
    /// whatever this flag says.
    pub fn transactional(mut self) -> Self {
        self.meta.transactional = true;
        self
    }

    /// Adds a guard, run before authorization and the handler.
    pub fn guard(mut self, guard: Arc<dyn Guard>) -> Self {
        self.meta.guards.push(guard);
        self
    }

    /// OpenAPI metadata.
    pub fn api(mut self, meta: ApiMeta) -> Self {
        self.meta.api = Some(meta);
        self
    }

    /// Writes a custom audit entry with this action after a 2xx response
    /// (when an `AuditSink` is registered).
    pub fn audit(mut self, action: &str) -> Self {
        self.meta.audit_action = Some(action.to_owned());
        self
    }

    /// Overrides the app-wide request body limit.
    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.meta.body_limit = Some(bytes);
        self
    }
}

impl fmt::Debug for RouteDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteDef")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

/// Everything known about a route besides its handler. The pipeline inserts
/// the matched route's `Arc<RouteMeta>` into the request and response
/// extensions.
#[derive(Clone, Debug, Default)]
pub struct RouteMeta {
    /// The route name.
    pub name: Option<String>,
    /// Who may call the route.
    pub access: AccessSpec,
    /// Custom guards, in order.
    pub guards: Vec<Arc<dyn Guard>>,
    /// Per-user rate limit.
    pub throttle: Option<Throttle>,
    /// Request timeout override.
    pub timeout: Option<Duration>,
    /// Declared transactional (informational; see [`RouteDef::transactional`]).
    pub transactional: bool,
    /// OpenAPI metadata.
    pub api: Option<ApiMeta>,
    /// Custom audit action written after a 2xx response.
    pub audit_action: Option<String>,
    /// Request body limit override.
    pub body_limit: Option<usize>,
    /// The module that registered the route (set when mounted).
    pub module: Option<&'static str>,
    /// The mounted path, e.g. `/api/v1/deals/{id}` (set when mounted).
    pub full_path: String,
    /// The HTTP method, e.g. `GET`.
    pub method: String,
}

/// Who may call a route. A route must declare at least one of these.
#[derive(Clone, Debug, Default)]
pub struct AccessSpec {
    /// No authentication or authorization.
    pub public: bool,
    /// Any signed-in user.
    pub authenticated: bool,
    /// Any of these roles.
    pub roles: Vec<String>,
    /// All of these permissions.
    pub permissions: Vec<String>,
    /// An ability check.
    pub can: Option<CanSpec>,
}

impl AccessSpec {
    /// Whether the route declares its access at all (deny by default).
    pub fn is_declared(&self) -> bool {
        self.public || self.authenticated || self.needs_authorizer()
    }

    /// Whether the route declares roles, permissions or an ability, which the
    /// [`RouteAuthorizer`](crate::RouteAuthorizer) checks.
    pub fn needs_authorizer(&self) -> bool {
        !self.roles.is_empty() || !self.permissions.is_empty() || self.can.is_some()
    }

    /// A short description for `routes` output (`public`, `auth`,
    /// `roles:admin|manager`, `can:update Deal from id`, ...).
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.public {
            parts.push("public".to_owned());
        }
        if self.authenticated {
            parts.push("auth".to_owned());
        }
        if !self.roles.is_empty() {
            parts.push(format!("roles:{}", self.roles.join("|")));
        }
        if !self.permissions.is_empty() {
            parts.push(format!("permissions:{}", self.permissions.join(",")));
        }
        if let Some(can) = &self.can {
            let mut s = format!("can:{} {}", can.action, can.subject);
            if let Some(from) = &can.from {
                s.push_str(" from ");
                s.push_str(from);
            }
            parts.push(s);
        }
        if parts.is_empty() {
            "none".to_owned()
        } else {
            parts.join(" ")
        }
    }
}

/// An ability check: `action` on `subject`, optionally on the record whose id
/// is the path parameter `from`.
#[derive(Clone, Debug)]
pub struct CanSpec {
    /// The action, e.g. `update`.
    pub action: String,
    /// The subject type, e.g. `Deal`.
    pub subject: String,
    /// The path parameter holding the record id.
    pub from: Option<String>,
}

/// A rate limit: `limit` requests per `per`.
#[derive(Clone, Copy, Debug)]
pub struct Throttle {
    /// Requests allowed per window.
    pub limit: u32,
    /// The window.
    pub per: Duration,
}

/// OpenAPI metadata of a route (read by navree-api).
#[derive(Clone, Debug, Default)]
pub struct ApiMeta {
    /// One-line summary.
    pub summary: Option<String>,
    /// Longer description.
    pub description: Option<String>,
    /// Tags (groups in the docs UI).
    pub tags: Vec<String>,
    /// Documented error statuses (rendered as problem+json responses).
    pub responses: Vec<u16>,
    /// The success status; `0` means the method's default (200).
    pub success_status: u16,
    /// The request body schema.
    pub request: Option<SchemaFn>,
    /// The response body schema.
    pub response: Option<SchemaFn>,
}

/// Returns `(schema name, JSON schema)`; see [`schema_fn`].
pub type SchemaFn = fn() -> (String, serde_json::Value);

/// The OpenAPI schema of `T` as `(name, JSON schema)`, usable as a
/// [`SchemaFn`] (`schema_fn::<CreateDeal>`), so route macros never need navree-api.
///
/// Schemas that `T` references (`$ref: "#/components/schemas/Inner"`) are
/// carried in the extension member `x-navree-components` (`{name: schema}`);
/// navree-api hoists them into `components.schemas` and removes the member.
pub fn schema_fn<T: utoipa::ToSchema>() -> (String, serde_json::Value) {
    let mut schema = serde_json::to_value(T::schema()).unwrap_or_default();
    let mut referenced = Vec::new();
    T::schemas(&mut referenced);
    if !referenced.is_empty()
        && let Some(object) = schema.as_object_mut()
    {
        let components: serde_json::Map<String, serde_json::Value> = referenced
            .into_iter()
            .map(|(name, schema)| (name, serde_json::to_value(schema).unwrap_or_default()))
            .collect();
        object.insert(
            "x-navree-components".to_owned(),
            serde_json::Value::Object(components),
        );
    }
    (T::name().into_owned(), schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;

    #[test]
    fn builders_fill_the_access_spec() {
        let route = RouteDef::new(Method::POST, "/", get(|| async {}))
            .roles(&["admin", "manager"])
            .permissions(&["deals.manage"])
            .can("update", "Deal", Some("id"));
        assert_eq!(route.meta.method, "POST");
        assert!(route.meta.access.is_declared());
        assert!(route.meta.access.needs_authorizer());
        assert_eq!(
            route.meta.access.describe(),
            "roles:admin|manager permissions:deals.manage can:update Deal from id"
        );
    }

    #[tokio::test]
    async fn on_routes_exactly_the_declared_method() {
        use tower::ServiceExt;

        let route = RouteDef::on(Method::PATCH, "/x", || async { "patched" }).public();
        assert_eq!(route.method, Method::PATCH);
        assert_eq!(route.meta.method, "PATCH");
        let call = |method: Method| {
            let handler = route.handler.clone();
            async move {
                let request = http::Request::builder()
                    .method(method)
                    .body(axum::body::Body::empty())
                    .unwrap();
                handler.oneshot(request).await.unwrap().status()
            }
        };
        assert_eq!(call(Method::PATCH).await, http::StatusCode::OK);
        assert_eq!(
            call(Method::GET).await,
            http::StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[test]
    fn nothing_declared_is_not_declared() {
        let route = RouteDef::new(Method::GET, "/", get(|| async {})).name("x");
        assert!(!route.meta.access.is_declared());
        assert_eq!(route.meta.access.describe(), "none");
        let route = route.authenticated();
        assert!(route.meta.access.is_declared() && !route.meta.access.needs_authorizer());
    }

    #[derive(utoipa::ToSchema)]
    #[allow(dead_code)]
    struct Owner {
        id: String,
    }

    #[derive(utoipa::ToSchema)]
    #[allow(dead_code)]
    struct Deal {
        title: String,
        owner: Owner,
    }

    #[test]
    fn schema_fn_returns_name_schema_and_referenced_components() {
        let f: SchemaFn = schema_fn::<Deal>;
        let (name, schema) = f();
        assert_eq!(name, "Deal");
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["title"].is_object());
        assert_eq!(
            schema["properties"]["owner"]["$ref"],
            "#/components/schemas/Owner"
        );
        assert_eq!(
            schema["x-navree-components"]["Owner"]["properties"]["id"]["type"],
            "string"
        );

        let (name, schema) = schema_fn::<Owner>();
        assert_eq!(name, "Owner");
        assert!(schema.get("x-navree-components").is_none());
    }
}
