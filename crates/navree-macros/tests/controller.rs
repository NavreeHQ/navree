//! `#[controller]` and the route attributes: real routers built by navree-http
//! from macro-generated controllers.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use http::request::Parts;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use navree_contracts::{AuthVia, Principal};
use navree_http::{
    AppError, Authenticator, HttpModule, HttpOptions, RouteAuthorizer, RouteMeta, build_router,
    route_table,
};
use navree_kernel::{App, BoxFuture, Module, Navree};
use tower::ServiceExt;
use uuid::Uuid;

const ALICE: &str = "0192f0c1-0000-7000-8000-00000000000a";

/// Authenticates requests carrying `x-user-id: <uuid>`; everyone else is a guest.
struct HeaderAuthenticator;

impl Authenticator for HeaderAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            Ok(parts
                .headers
                .get("x-user-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| Uuid::parse_str(v).ok())
                .map(|user_id| Principal {
                    user_id,
                    via: AuthVia::Test,
                    token_abilities: None,
                    session_id: None,
                }))
        })
    }
}

/// Lets every authenticated request through.
struct AllowAll;

impl RouteAuthorizer for AllowAll {
    fn authorize<'a>(
        &'a self,
        _parts: &'a mut Parts,
        _meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Boots `HttpModule` plus `module`, with the fake authenticator and the
/// allow-all authorizer, and builds the router.
async fn boot(module: impl Module) -> (App, Router) {
    let options = HttpOptions {
        per_ip_rate_limit: None,
        ..HttpOptions::default()
    };
    let app = Navree::app()
        .module(HttpModule::for_root(options))
        .module(module)
        .override_value::<dyn Authenticator>(Arc::new(HeaderAuthenticator))
        .override_value::<dyn RouteAuthorizer>(Arc::new(AllowAll))
        .boot()
        .await
        .expect("app boots");
    let router = build_router(app.ctx()).await.expect("router builds");
    (app, router)
}

/// Sends `request` and returns the status and the body as text.
async fn send(router: &Router, request: Request<Body>) -> (StatusCode, String) {
    let response = router.clone().oneshot(request).await.expect("infallible");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn request(method: &str, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn as_alice(method: &str, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("x-user-id", ALICE)
        .body(Body::empty())
        .unwrap()
}

/// `(method, full path, access)` of every route, sorted.
async fn routes(app: &App) -> Vec<(String, String, String)> {
    let mut routes: Vec<_> = route_table(app.ctx())
        .await
        .expect("route table")
        .into_iter()
        .map(|meta| (meta.method, meta.full_path, meta.access.describe()))
        .collect();
    routes.sort();
    routes
}

fn route(method: &str, path: &str, access: &str) -> (String, String, String) {
    (method.to_owned(), path.to_owned(), access.to_owned())
}

mod mounted {
    use navree_macros::{controller, injectable, module};

    #[injectable]
    pub struct DealController;

    #[controller("/deals", version = "1")]
    impl DealController {
        #[get("/")]
        #[public]
        async fn index(&self) -> &'static str {
            "index"
        }

        #[get("/{id}")]
        #[roles("admin", "manager")]
        async fn show(&self) -> &'static str {
            "show"
        }

        #[post("/")]
        #[permissions("deals.create")]
        async fn store(&self) -> http::StatusCode {
            http::StatusCode::CREATED
        }

        #[put("/{id}")]
        #[authenticated]
        async fn replace(&self) -> &'static str {
            "replace"
        }

        #[patch("/{id}")]
        #[permissions("deals.update", "deals.view")]
        async fn update(&self) -> &'static str {
            "update"
        }

        #[delete("/{id}")]
        #[roles("admin")]
        async fn destroy(&self) -> http::StatusCode {
            http::StatusCode::NO_CONTENT
        }

        #[get("!/deals-health")]
        #[public]
        async fn health(&self) -> &'static str {
            "healthy"
        }
    }

    #[module(controllers = [DealController])]
    pub struct DealsModule;
}

#[tokio::test]
async fn controller_routes_are_mounted() {
    let (app, router) = boot(mounted::DealsModule).await;

    assert_eq!(
        routes(&app).await,
        vec![
            route("DELETE", "/api/v1/deals/{id}", "roles:admin"),
            route("GET", "/api/v1/deals", "public"),
            route("GET", "/api/v1/deals/{id}", "roles:admin|manager"),
            route("GET", "/deals-health", "public"),
            route(
                "PATCH",
                "/api/v1/deals/{id}",
                "permissions:deals.update,deals.view"
            ),
            route("POST", "/api/v1/deals", "permissions:deals.create"),
            route("PUT", "/api/v1/deals/{id}", "auth"),
        ]
    );

    let index = send(&router, request("GET", "/api/v1/deals")).await;
    assert_eq!(index, (StatusCode::OK, "index".to_owned()));
    let show = send(&router, as_alice("GET", "/api/v1/deals/7")).await;
    assert_eq!(show, (StatusCode::OK, "show".to_owned()));
    let store = send(&router, as_alice("POST", "/api/v1/deals")).await;
    assert_eq!(store.0, StatusCode::CREATED);
    let replace = send(&router, as_alice("PUT", "/api/v1/deals/7")).await;
    assert_eq!(replace, (StatusCode::OK, "replace".to_owned()));
    let update = send(&router, as_alice("PATCH", "/api/v1/deals/7")).await;
    assert_eq!(update, (StatusCode::OK, "update".to_owned()));
    let destroy = send(&router, as_alice("DELETE", "/api/v1/deals/7")).await;
    assert_eq!(destroy.0, StatusCode::NO_CONTENT);
    let health = send(&router, request("GET", "/deals-health")).await;
    assert_eq!(health, (StatusCode::OK, "healthy".to_owned()));

    // Not public: guests are turned away before the handler.
    let guest = send(&router, request("GET", "/api/v1/deals/7")).await;
    assert_eq!(guest.0, StatusCode::UNAUTHORIZED);
}

mod guarded {
    use std::sync::Arc;

    use http::request::Parts;
    use navree_http::{AppError, Guard};
    use navree_kernel::{BoxFuture, ModuleBuilder};
    use navree_macros::{controller, injectable, module};

    /// Rejects requests without the header `0` (403, naming the header).
    #[derive(Debug)]
    pub struct RequireHeader(pub &'static str);

    impl Guard for RequireHeader {
        fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                if parts.headers.contains_key(self.0) {
                    Ok(())
                } else {
                    Err(AppError::forbidden().with_detail(format!("missing {}", self.0)))
                }
            })
        }
    }

    #[injectable]
    pub struct GuardedController {
        team_guard: Arc<RequireHeader>,
    }

    #[controller("/guarded")]
    #[guard(Arc::new(RequireHeader("x-a")))]
    impl GuardedController {
        #[get("/")]
        #[public]
        #[guard(Arc::new(RequireHeader("x-b")))]
        async fn index(&self) -> &'static str {
            "index"
        }

        // Guards are expressions evaluated when the routes are built, with
        // `self` (the controller) in scope.
        #[get("/team")]
        #[public]
        #[guard(self.team_guard.clone())]
        async fn team(&self) -> &'static str {
            "team"
        }
    }

    #[module(controllers = [GuardedController], register = Self::guards)]
    pub struct GuardedModule;

    impl GuardedModule {
        fn guards(&self, m: &mut ModuleBuilder) {
            m.value(RequireHeader("x-team"));
        }
    }
}

fn with_headers(path: &str, headers: &[&str]) -> Request<Body> {
    let mut request = Request::builder().method("GET").uri(path);
    for header in headers {
        request = request.header(*header, "1");
    }
    request.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn guards_run_before_the_handler_impl_guards_first() {
    let (_app, router) = boot(guarded::GuardedModule).await;

    let (status, body) = send(&router, with_headers("/api/guarded", &[])).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body.contains("missing x-a"),
        "impl guard runs first: {body}"
    );

    let (status, body) = send(&router, with_headers("/api/guarded", &["x-a"])).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("missing x-b"), "then the route guard: {body}");

    let ok = send(&router, with_headers("/api/guarded", &["x-a", "x-b"])).await;
    assert_eq!(ok, (StatusCode::OK, "index".to_owned()));

    let (status, body) = send(&router, with_headers("/api/guarded/team", &["x-a"])).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("missing x-team"), "injected guard: {body}");
    let ok = send(
        &router,
        with_headers("/api/guarded/team", &["x-a", "x-team"]),
    )
    .await;
    assert_eq!(ok, (StatusCode::OK, "team".to_owned()));
}

mod open {
    use navree_macros::{controller, injectable, module};

    #[injectable]
    pub struct StatusController;

    #[controller("/status")]
    #[public]
    impl StatusController {
        #[get("/")]
        async fn index(&self) -> &'static str {
            "up"
        }

        #[get("/version")]
        async fn version(&self) -> &'static str {
            "1.0"
        }

        /// Not a route: helpers stay plain methods.
        #[allow(dead_code)]
        fn helper(&self) -> u8 {
            1
        }
    }

    #[module(controllers = [StatusController])]
    pub struct StatusModule;
}

#[tokio::test]
async fn impl_level_public_applies() {
    // No authenticator and no authorizer: navree-http only boots this app when
    // every route is public.
    let app = Navree::app()
        .module(HttpModule::for_root(HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        }))
        .module(open::StatusModule)
        .boot()
        .await
        .expect("only public routes: no authenticator needed");
    let router = build_router(app.ctx()).await.expect("router builds");

    assert_eq!(
        routes(&app).await,
        vec![
            route("GET", "/api/status", "public"),
            route("GET", "/api/status/version", "public"),
        ]
    );
    let index = send(&router, request("GET", "/api/status")).await;
    assert_eq!(index, (StatusCode::OK, "up".to_owned()));
    let version = send(&router, request("GET", "/api/status/version")).await;
    assert_eq!(version, (StatusCode::OK, "1.0".to_owned()));
}

mod abilities {
    use navree_access::{Read, Subject, Update};
    use navree_macros::{controller, injectable, module};

    pub struct Deal;

    impl Subject for Deal {
        const SUBJECT: &'static str = "Deal";
        const PERMISSION_PREFIX: &'static str = "deals";
    }

    #[injectable]
    pub struct DealController;

    #[controller("/deals")]
    impl DealController {
        #[get("/")]
        #[can(Read, Deal)]
        async fn index(&self) -> &'static str {
            "index"
        }

        #[patch("/{id}")]
        #[can(Update, Deal, from = "id")]
        async fn update(&self) -> &'static str {
            "update"
        }

        // A custom action without a marker type: a string is used as is.
        #[post("/{id}/publish")]
        #[can("publish", Deal, from = "id")]
        async fn publish(&self) -> &'static str {
            "publish"
        }
    }

    #[injectable]
    pub struct ReportController;

    #[controller("/reports")]
    #[can(navree_access::Read, "Report")]
    impl ReportController {
        #[get("/")]
        async fn index(&self) -> &'static str {
            "reports"
        }
    }

    #[module(controllers = [DealController, ReportController])]
    pub struct AbilitiesModule;
}

#[tokio::test]
async fn can_uses_action_and_subject_markers() {
    let (app, router) = boot(abilities::AbilitiesModule).await;

    assert_eq!(
        routes(&app).await,
        vec![
            route("GET", "/api/deals", "can:read Deal"),
            route("GET", "/api/reports", "can:read Report"),
            route("PATCH", "/api/deals/{id}", "can:update Deal from id"),
            route(
                "POST",
                "/api/deals/{id}/publish",
                "can:publish Deal from id"
            ),
        ]
    );
    let update = send(&router, as_alice("PATCH", "/api/deals/7")).await;
    assert_eq!(update, (StatusCode::OK, "update".to_owned()));
    let guest = send(&router, request("GET", "/api/reports")).await;
    assert_eq!(guest.0, StatusCode::UNAUTHORIZED);
}

mod extractors {
    use std::sync::Arc;

    use axum::extract::{Path, Query};
    use navree_http::{CurrentPrincipal, Json};
    use navree_kernel::ModuleBuilder;
    use navree_macros::{controller, injectable, module};
    use serde::Deserialize;
    use serde_json::{Value, json};

    pub struct Greeting(pub &'static str);

    #[derive(Deserialize)]
    pub struct Search {
        q: String,
    }

    #[derive(Deserialize)]
    pub struct Rename {
        title: String,
    }

    #[injectable]
    pub struct NoteController {
        greeting: Arc<Greeting>,
    }

    #[controller("/notes")]
    #[authenticated]
    impl NoteController {
        #[get("/{id}")]
        async fn show(&self, Path(id): Path<u64>, Query(search): Query<Search>) -> Json<Value> {
            Json(json!({ "id": id, "q": search.q, "greeting": self.greeting.0 }))
        }

        // The body extractor comes last, as Axum requires.
        #[patch("/{team}/{id}")]
        async fn rename(
            &self,
            CurrentPrincipal(user): CurrentPrincipal,
            Path((team, id)): Path<(String, u64)>,
            Json(body): Json<Rename>,
        ) -> Json<Value> {
            Json(json!({
                "user": user.user_id.to_string(),
                "team": team,
                "id": id,
                "title": body.title,
            }))
        }
    }

    #[module(controllers = [NoteController], register = Self::values)]
    pub struct NotesModule;

    impl NotesModule {
        fn values(&self, m: &mut ModuleBuilder) {
            m.value(Greeting("hi"));
        }
    }
}

#[tokio::test]
async fn extractors_forwarded() {
    let (_app, router) = boot(extractors::NotesModule).await;

    let (status, body) = send(&router, as_alice("GET", "/api/notes/7?q=acme")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({ "id": 7, "q": "acme", "greeting": "hi" })
    );

    let request = Request::builder()
        .method("PATCH")
        .uri("/api/notes/sales/9")
        .header("x-user-id", ALICE)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"title":"Renewal"}"#))
        .unwrap();
    let (status, body) = send(&router, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({ "user": ALICE, "team": "sales", "id": 9, "title": "Renewal" })
    );

    // Extractor rejections still render as errors: `id` is not a number.
    let (status, _) = send(&router, as_alice("GET", "/api/notes/x?q=acme")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

mod documented {
    use navree_macros::{controller, injectable, module};

    #[derive(utoipa::ToSchema)]
    #[allow(dead_code)]
    pub struct CreateDeal {
        title: String,
    }

    #[derive(utoipa::ToSchema)]
    #[allow(dead_code)]
    pub struct DealDto {
        id: u64,
        title: String,
    }

    #[injectable]
    pub struct DealController;

    #[controller("/deals", version = 1)]
    impl DealController {
        #[post("/")]
        #[permissions("deals.create")]
        #[api(
            summary = "Create a deal",
            description = "Creates a deal in the first stage.",
            tag = "Deals",
            request = CreateDeal,
            response = DealDto,
            responses(400, 404),
            status = 201,
        )]
        async fn store(&self) -> http::StatusCode {
            http::StatusCode::CREATED
        }

        #[get("/")]
        #[public]
        #[api(tags("Deals", "Pipeline"))]
        async fn index(&self) -> &'static str {
            "index"
        }

        #[get("/{id}")]
        #[public]
        async fn show(&self) -> &'static str {
            "show"
        }
    }

    #[module(controllers = [DealController])]
    pub struct DocumentedModule;
}

#[tokio::test]
async fn api_meta_populated() {
    let (app, _router) = boot(documented::DocumentedModule).await;
    let table = route_table(app.ctx()).await.expect("route table");
    let meta = |method: &str, path: &str| {
        table
            .iter()
            .find(|m| m.method == method && m.full_path == path)
            .unwrap_or_else(|| panic!("{method} {path} in {table:?}"))
    };

    let store = meta("POST", "/api/v1/deals")
        .api
        .as_ref()
        .expect("store is documented");
    assert_eq!(store.summary.as_deref(), Some("Create a deal"));
    assert_eq!(
        store.description.as_deref(),
        Some("Creates a deal in the first stage.")
    );
    assert_eq!(store.tags, ["Deals"]);
    assert_eq!(store.responses, [400, 404]);
    assert_eq!(store.success_status, 201);
    let (request_name, request_schema) = store.request.expect("request schema")();
    assert_eq!(request_name, "CreateDeal");
    assert!(request_schema["properties"]["title"].is_object());
    let (response_name, response_schema) = store.response.expect("response schema")();
    assert_eq!(response_name, "DealDto");
    assert!(response_schema["properties"]["id"].is_object());

    let index = meta("GET", "/api/v1/deals").api.as_ref().expect("index");
    assert_eq!(index.tags, ["Deals", "Pipeline"]);
    assert_eq!(index.summary, None);
    assert_eq!(index.success_status, 0, "0 means the method's default");
    assert!(index.request.is_none() && index.response.is_none());

    assert!(meta("GET", "/api/v1/deals/{id}").api.is_none());
}

mod options {
    use navree_macros::{controller, injectable, module};

    #[injectable]
    pub struct InvoiceController;

    #[controller("/invoices")]
    #[throttle(limit = 60, per = "1m")]
    impl InvoiceController {
        #[post("/{id}/send")]
        #[permissions("invoices.send")]
        #[name("invoices.send")]
        #[throttle(limit = 10, per = "1h")]
        #[timeout("500ms")]
        #[transactional]
        #[audit(action = "invoice.sent")]
        async fn send(&self) -> &'static str {
            "sent"
        }

        #[get("/")]
        #[public]
        #[timeout("5s")]
        async fn index(&self) -> &'static str {
            "index"
        }
    }

    #[module(controllers = [InvoiceController])]
    pub struct InvoicesModule;
}

#[tokio::test]
async fn route_options_fill_meta() {
    use std::time::Duration;

    let (app, router) = boot(options::InvoicesModule).await;
    let table = route_table(app.ctx()).await.expect("route table");
    let send_meta = table.iter().find(|m| m.method == "POST").expect("send");
    assert_eq!(send_meta.name.as_deref(), Some("invoices.send"));
    let throttle = send_meta.throttle.expect("route throttle");
    assert_eq!(
        (throttle.limit, throttle.per),
        (10, Duration::from_secs(3600))
    );
    assert_eq!(send_meta.timeout, Some(Duration::from_millis(500)));
    assert!(send_meta.transactional);
    assert_eq!(send_meta.audit_action.as_deref(), Some("invoice.sent"));
    assert_eq!(
        send_meta.handler.as_deref(),
        Some("InvoiceController::send"),
        "the controller method, for `routes --json` and Navigator"
    );

    let index = table.iter().find(|m| m.method == "GET").expect("index");
    assert_eq!(index.handler.as_deref(), Some("InvoiceController::index"));
    let throttle = index.throttle.expect("impl-level throttle");
    assert_eq!(
        (throttle.limit, throttle.per),
        (60, Duration::from_secs(60))
    );
    assert_eq!(index.timeout, Some(Duration::from_secs(5)));
    assert!(!index.transactional);
    assert_eq!(index.name, None);
    assert_eq!(index.audit_action, None);

    let sent = send(&router, as_alice("POST", "/api/invoices/3/send")).await;
    assert_eq!(sent, (StatusCode::OK, "sent".to_owned()));
}

mod gated {
    use navree_macros::{controller, injectable, module};

    #[injectable]
    pub struct FeatureController;

    #[controller("/features")]
    #[public]
    impl FeatureController {
        #[get("/on")]
        #[cfg(test)]
        async fn on(&self) -> &'static str {
            "on"
        }

        /// Compiled out: neither the method nor its route exist.
        #[get("/off")]
        #[cfg(not(test))]
        async fn off(&self) -> &'static str {
            "off"
        }
    }

    #[module(controllers = [FeatureController])]
    pub struct GatedModule;
}

#[tokio::test]
async fn cfg_on_a_route_gates_the_route() {
    let (app, _router) = boot(gated::GatedModule).await;
    assert_eq!(
        routes(&app).await,
        vec![route("GET", "/api/features/on", "public")]
    );
}

mod generic {
    use std::marker::PhantomData;

    use navree_macros::{controller, injectable, module};

    pub trait Greeter: Send + Sync + 'static {
        fn greet() -> &'static str;
    }

    pub struct English;

    impl Greeter for English {
        fn greet() -> &'static str {
            "hello"
        }
    }

    #[injectable]
    pub struct GreetController<G: Greeter> {
        #[inject(default)]
        greeter: PhantomData<fn() -> G>,
    }

    #[controller("/greet")]
    #[public]
    impl<G: Greeter> GreetController<G> {
        #[get("/")]
        async fn index(&self) -> &'static str {
            let _ = self.greeter;
            G::greet()
        }
    }

    #[module(controllers = [GreetController<English>])]
    pub struct GreetModule;
}

#[tokio::test]
async fn generic_controllers_are_supported() {
    let (_app, router) = boot(generic::GreetModule).await;
    let greeting = send(&router, request("GET", "/api/greet")).await;
    assert_eq!(greeting, (StatusCode::OK, "hello".to_owned()));
}

mod combined {
    use navree_macros::{controller, injectable, module};

    #[injectable]
    pub struct AdminController;

    #[controller("/admin")]
    #[roles("admin")]
    impl AdminController {
        #[get("/users")]
        async fn users(&self) -> &'static str {
            "users"
        }

        // Adds to the impl block's roles: both are checked.
        #[post("/users")]
        #[roles("owner")]
        #[permissions("users.create")]
        async fn create(&self) -> &'static str {
            "created"
        }
    }

    #[injectable]
    pub struct TeamController;

    #[controller("/teams/{team}")]
    #[can("manage", "Team", from = "team")]
    impl TeamController {
        #[get("/members")]
        async fn members(&self) -> &'static str {
            "members"
        }
    }

    #[module(controllers = [AdminController, TeamController])]
    pub struct CombinedModule;
}

#[tokio::test]
async fn impl_level_access_adds_to_route_access() {
    let (app, _router) = boot(combined::CombinedModule).await;
    assert_eq!(
        routes(&app).await,
        vec![
            route("GET", "/api/admin/users", "roles:admin"),
            route(
                "GET",
                "/api/teams/{team}/members",
                "can:manage Team from team"
            ),
            route(
                "POST",
                "/api/admin/users",
                "roles:admin|owner permissions:users.create"
            ),
        ]
    );
}

mod empty {
    use navree_macros::{controller, injectable};

    #[injectable]
    pub struct EmptyController;

    /// No routes yet: the controller still implements `Controller`.
    #[controller("/empty")]
    impl EmptyController {}
}

#[test]
fn controller_without_routes_has_none() {
    use navree_http::Controller;

    let controller = Arc::new(empty::EmptyController);
    assert_eq!(controller.prefix(), "/empty");
    assert_eq!(controller.version(), None);
    assert!(controller.routes().is_empty());
}

mod security {
    use navree_macros::{controller, injectable};

    #[injectable]
    pub struct AccountController;

    /// Every route refuses impersonation sessions; one also needs a recent
    /// sign-in.
    #[controller("/account")]
    #[authenticated]
    #[not_while_impersonating]
    impl AccountController {
        #[get("/")]
        async fn show(&self) -> &'static str {
            "account"
        }

        #[delete("/")]
        #[recent_auth(minutes = 10)]
        async fn destroy(&self) -> &'static str {
            "deleted"
        }
    }

    #[injectable]
    pub struct ReportController;

    #[controller("/reports")]
    impl ReportController {
        #[post("/export")]
        #[authenticated]
        #[recent_auth(minutes = 5)]
        #[navree_macros::not_while_impersonating]
        async fn export(&self) -> &'static str {
            "exported"
        }
    }
}

#[test]
fn security_attributes_add_the_auth_guards() {
    use navree_http::Controller;

    let guards = |route: &navree_http::RouteDef| -> Vec<String> {
        route
            .meta
            .guards
            .iter()
            .map(|guard| format!("{guard:?}"))
            .collect()
    };
    let routes = Arc::new(security::AccountController).routes();
    assert_eq!(guards(&routes[0]), ["not_while_impersonating"]);
    assert_eq!(
        guards(&routes[1]),
        ["not_while_impersonating", "recent_auth(600s)"]
    );
    let export = Arc::new(security::ReportController).routes();
    assert_eq!(
        guards(&export[0]),
        ["not_while_impersonating", "recent_auth(300s)"]
    );
    assert_eq!(export[0].meta.access.describe(), "auth");
}
