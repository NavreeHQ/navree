//! The attribute macros expanded against the facade, as in an app that
//! depends on `navree` alone: the generated code names `::navree::kernel`,
//! `::navree::http` and `::navree::access`, and the result boots, routes and
//! authorizes through the real modules.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use navree::contracts::{AuditEntry, AuditSink};
use navree::http::{CurrentPrincipal, Guard};
use navree::kernel::RequestScope;
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use navree::{BoxFuture, ModuleId};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Dependency injection: `#[injectable]` and `#[module]`.

/// A value the root configures.
pub struct Greeting(&'static str);

/// Upper-cases text.
#[injectable]
pub struct Shouter;

impl Shouter {
    fn shout(&self, text: &str) -> String {
        text.to_uppercase()
    }
}

/// Exports `Shouter` to the modules that import it.
#[module(providers = [Shouter], exports = [Shouter])]
pub struct ShoutModule;

/// Every field kind `#[injectable]` fills: `Arc<T>`, a named `Arc<T>`,
/// `Lazy<T>` and a default value.
#[injectable]
pub struct Greeter {
    greeting: Arc<Greeting>,
    #[inject(name = "punctuation")]
    punctuation: Arc<String>,
    shouter: Lazy<Shouter>,
    #[inject(default)]
    greeted: AtomicUsize,
}

impl Greeter {
    async fn greet(&self, name: &str) -> String {
        self.greeted.fetch_add(1, Ordering::Relaxed);
        let shouter = self.shouter.get().await.expect("Shouter resolves");
        format!(
            "{}, {}{}",
            self.greeting.0,
            shouter.shout(name),
            self.punctuation
        )
    }
}

/// Numbers every request-scoped instance.
static STAMPS: AtomicUsize = AtomicUsize::new(0);

/// A per-request value.
pub struct Stamp(usize);

impl Default for Stamp {
    fn default() -> Self {
        Stamp(STAMPS.fetch_add(1, Ordering::Relaxed))
    }
}

/// Built once per request scope.
#[injectable(scope = "request")]
pub struct RequestStamp {
    #[inject(default)]
    stamp: Stamp,
}

/// The body of `GET /api/v1/greetings/{name}`.
#[derive(Serialize, navree::openapi::ToSchema)]
pub struct GreetingBody {
    /// The greeting.
    message: String,
}

/// Fails unless the request carries `x-gate: open`.
#[derive(Debug)]
pub struct HeaderGate;

impl Guard for HeaderGate {
    fn check<'a>(
        &'a self,
        parts: &'a mut http::request::Parts,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        let open = parts
            .headers
            .get("x-gate")
            .is_some_and(|value| value == "open");
        Box::pin(async move {
            if open {
                Ok(())
            } else {
                Err(AppError::forbidden())
            }
        })
    }
}

/// Routes covering every route attribute.
#[injectable]
pub struct GreetingController {
    greeter: Arc<Greeter>,
}

#[controller("/greetings", version = "1")]
impl GreetingController {
    #[get("/me")]
    #[authenticated]
    async fn me(&self, CurrentPrincipal(me): CurrentPrincipal) -> Json<Value> {
        Json(json!({ "user_id": me.user_id }))
    }

    #[get("/{name}")]
    #[public]
    #[name("greetings.show")]
    #[api(summary = "Greets someone", tag = "Greetings", response = GreetingBody)]
    async fn show(&self, Path(name): Path<String>) -> Json<GreetingBody> {
        Json(GreetingBody {
            message: self.greeter.greet(&name).await,
        })
    }

    #[post("/")]
    #[roles("admin")]
    async fn store(&self) -> NoContent {
        NoContent
    }

    #[get("!/greetings-health")]
    #[public]
    async fn health(&self) -> &'static str {
        "ok"
    }

    #[get("/gated/door")]
    #[public]
    #[guard(Arc::new(HeaderGate))]
    async fn gated(&self) -> &'static str {
        "in"
    }

    #[get("/limited/tap")]
    #[public]
    #[throttle(limit = 2, per = "1m")]
    async fn limited(&self) -> &'static str {
        "tap"
    }

    /// Two `Scoped` extractions of a request-scoped provider in one request.
    #[get("/stamps/pair")]
    #[public]
    async fn stamps(&self, a: Scoped<RequestStamp>, b: Scoped<RequestStamp>) -> Json<Value> {
        Json(json!({ "a": a.stamp.0, "b": b.stamp.0 }))
    }

    #[get("/slow/nap")]
    #[public]
    #[timeout("100ms")]
    async fn slow(&self) -> &'static str {
        tokio::time::sleep(Duration::from_secs(2)).await;
        "late"
    }

    #[post("/audited/wave")]
    #[authenticated]
    #[audit(action = "greetings.waved")]
    async fn wave(&self) -> &'static str {
        "waved"
    }
}

/// A module with every `#[module]` key.
#[module(
    imports = [ShoutModule],
    providers = [Greeter, RequestStamp],
    controllers = [GreetingController],
    exports = [Greeter],
    register = Self::wire,
)]
pub struct GreetingModule {
    greeting: &'static str,
}

impl GreetingModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.value(Greeting(self.greeting));
        m.factory(
            Factory::<String>::new(|_r| async { Ok(Arc::new("!".to_owned())) })
                .named("punctuation"),
        );
    }
}

// ---------------------------------------------------------------------------
// Authorization: `#[can]` with the facade's action markers and a string.

/// A subject that is not an entity.
pub struct Note;

impl Subject for Note {
    const SUBJECT: &'static str = "Note";
    const PERMISSION_PREFIX: &'static str = "notes";
}

/// Routes checked with `#[can]`.
#[injectable]
pub struct NoteController;

#[controller("/notes")]
impl NoteController {
    #[get("/")]
    #[can(Read, Note)]
    async fn index(&self) -> Json<Vec<&'static str>> {
        Json(vec!["first"])
    }

    #[delete("/{id}")]
    #[can(Delete, Note)]
    async fn destroy(&self, Path(_id): Path<u32>) -> NoContent {
        NoContent
    }

    #[post("/{id}/archive")]
    #[can("archive", Note)]
    async fn archive(&self, Path(id): Path<u32>) -> Json<Value> {
        Json(json!({ "archived": id }))
    }
}

/// Registers `Note` as a subject and its controller.
#[module(
    imports = [AccessModule::for_subject::<Note>()],
    controllers = [NoteController],
)]
pub struct NotesModule;

/// Signed-in users read and archive notes but never delete them.
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx.user.is_some() {
        rules.can(READ, "Note");
        rules.can("archive", "Note");
        rules.cannot(DELETE, "Note");
    }
}

/// Records audit entries in memory.
#[derive(Default)]
struct RecordingSink(Mutex<Vec<AuditEntry>>);

impl RecordingSink {
    fn actions(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|entry| entry.action.clone())
            .collect()
    }
}

impl AuditSink for RecordingSink {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
        Box::pin(async { Ok(()) })
    }
}

async fn app() -> TestApp {
    app_with_sink(Arc::new(RecordingSink::default())).await
}

async fn app_with_sink(sink: Arc<RecordingSink>) -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("observe.metrics", false)
        .override_value::<dyn AuditSink>(sink)
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: None,
        }))
        .module(ApiModule::from_config())
        .module(GreetingModule { greeting: "Hello" })
        .module(NotesModule)
        .build()
        .await
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn injectable_fields_and_module_wiring_resolve_through_the_facade() {
    let app = app().await;
    let greeter = app.get::<Greeter>().await;
    assert_eq!(greeter.greet("anu").await, "Hello, ANU!");
    assert_eq!(greeter.greeted.load(Ordering::Relaxed), 1);
    // `exports = [Greeter]` makes it visible to the root; `Shouter` came
    // through `imports = [ShoutModule]`.
    let report = navree::Navree::app()
        .module(navree::config::ConfigModule::for_root(ConfigSources::new()))
        .module(GreetingModule { greeting: "Hi" })
        .validate()
        .expect("the graph is valid");
    let greetings = report
        .modules
        .iter()
        .find(|m| m.name == "GreetingModule")
        .expect("GreetingModule is in the report");
    assert_eq!(greetings.imports, ["ShoutModule"]);
}

#[tokio::test]
async fn request_scoped_injectable_lives_for_one_scope() {
    let app = app().await;
    let module = ModuleId::of::<GreetingModule>();
    let scope = RequestScope::new();
    let first = app
        .ctx()
        .request_resolver(module, scope.clone())
        .get::<RequestStamp>()
        .await
        .expect("resolves in a request scope");
    let again = app
        .ctx()
        .request_resolver(module, scope)
        .get::<RequestStamp>()
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    let next = app
        .ctx()
        .request_resolver(module, RequestScope::new())
        .get::<RequestStamp>()
        .await
        .unwrap();
    assert_ne!(first.stamp.0, next.stamp.0);
    // Outside a request it is a wiring error, not a silent singleton.
    assert!(app.ctx().get::<RequestStamp>().await.is_err());
}

#[tokio::test]
async fn scoped_extractor_shares_a_request_scoped_provider_within_a_request() {
    let app = app().await;
    let stamps = || async {
        app.client()
            .get("/api/v1/greetings/stamps/pair")
            .send()
            .await
            .assert_status(200)
            .json_value()
    };
    let first = stamps().await;
    assert_eq!(first["a"], first["b"], "one instance per request: {first}");
    let second = stamps().await;
    assert_ne!(first["a"], second["a"], "a new instance per request");
}

#[tokio::test]
async fn controller_routes_mount_with_prefix_version_and_access() {
    let app = app().await;
    let mut routes: Vec<(String, String, String)> = navree::http::route_table(app.ctx())
        .await
        .expect("route table")
        .into_iter()
        .filter(|meta| meta.full_path.contains("greetings") || meta.full_path.contains("notes"))
        .map(|meta| (meta.method, meta.full_path, meta.access.describe()))
        .collect();
    routes.sort();
    let route = |method: &str, path: &str, access: &str| {
        (method.to_owned(), path.to_owned(), access.to_owned())
    };
    assert_eq!(
        routes,
        [
            route("DELETE", "/api/notes/{id}", "can:delete Note"),
            route("GET", "/api/notes", "can:read Note"),
            route("GET", "/api/v1/greetings/gated/door", "public"),
            route("GET", "/api/v1/greetings/limited/tap", "public"),
            route("GET", "/api/v1/greetings/me", "auth"),
            route("GET", "/api/v1/greetings/slow/nap", "public"),
            route("GET", "/api/v1/greetings/stamps/pair", "public"),
            route("GET", "/api/v1/greetings/{name}", "public"),
            route("GET", "/greetings-health", "public"),
            route("POST", "/api/notes/{id}/archive", "can:archive Note"),
            route("POST", "/api/v1/greetings", "roles:admin"),
            route("POST", "/api/v1/greetings/audited/wave", "auth"),
        ]
    );
}

#[tokio::test]
async fn public_and_authenticated_routes_answer() {
    let app = app().await;
    app.client()
        .get("/api/v1/greetings/anu")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("message", json!("Hello, ANU!"));
    app.client()
        .get("/greetings-health")
        .send()
        .await
        .assert_status(200);
    app.client()
        .get("/api/v1/greetings/me")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    let alice = Uuid::now_v7();
    app.acting_as(principal(alice))
        .get("/api/v1/greetings/me")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(alice));
}

#[tokio::test]
async fn can_and_roles_routes_check_abilities() {
    let app = app().await;
    let user = app.acting_as(principal(Uuid::now_v7()));
    user.get("/api/notes").send().await.assert_status(200);
    user.delete("/api/notes/1")
        .send()
        .await
        .assert_error(403, "Forbidden");
    user.post("/api/notes/7/archive")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("archived", json!(7));
    user.post("/api/v1/greetings")
        .send()
        .await
        .assert_status(403);
    app.client()
        .get("/api/notes")
        .send()
        .await
        .assert_status(401);
    // A guest gets no rule at all.
    let mut rules = Rules::new();
    abilities(&AbilityCtx::guest(), &mut rules);
    assert!(!rules.build().can(READ, "Note", None));
}

#[tokio::test]
async fn guard_throttle_and_timeout_attributes_apply() {
    let app = app().await;
    let client = app.client();
    client
        .get("/api/v1/greetings/gated/door")
        .send()
        .await
        .assert_status(403);
    client
        .get("/api/v1/greetings/gated/door")
        .header("x-gate", "open")
        .send()
        .await
        .assert_status(200);

    client
        .get("/api/v1/greetings/limited/tap")
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/v1/greetings/limited/tap")
        .send()
        .await
        .assert_status(200);
    let limited = client
        .get("/api/v1/greetings/limited/tap")
        .send()
        .await
        .assert_status(429);
    assert!(limited.header("retry-after").is_some());

    client
        .get("/api/v1/greetings/slow/nap")
        .send()
        .await
        .assert_error(503, "The server did not finish the request in time.");
}

#[tokio::test]
async fn audit_attribute_records_an_entry_after_success() {
    let sink = Arc::new(RecordingSink::default());
    let app = app_with_sink(sink.clone()).await;
    app.acting_as(principal(Uuid::now_v7()))
        .post("/api/v1/greetings/audited/wave")
        .send()
        .await
        .assert_status(200);
    assert!(
        sink.actions()
            .iter()
            .any(|action| action == "greetings.waved"),
        "{:?}",
        sink.actions()
    );
}

#[tokio::test]
async fn api_attribute_documents_the_route() {
    let app = app().await;
    let doc = app
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let show = &doc["paths"]["/api/v1/greetings/{name}"]["get"];
    assert_eq!(show["summary"], json!("Greets someone"));
    assert_eq!(show["tags"], json!(["Greetings"]));
    assert!(
        doc["components"]["schemas"]["GreetingBody"].is_object(),
        "{doc:#}"
    );
    assert!(doc["paths"]["/api/notes/{id}/archive"]["post"].is_object());
}

// ---------------------------------------------------------------------------
// Security attributes: the guards come from `::navree::auth`.

/// Closing an account: refused while impersonating, and only within 15
/// minutes of signing in or confirming.
#[injectable]
pub struct AccountController;

#[controller("/account")]
#[authenticated]
#[not_while_impersonating]
impl AccountController {
    #[delete("/")]
    #[recent_auth(minutes = 15)]
    async fn close(&self) -> &'static str {
        "closed"
    }
}

#[test]
fn security_attributes_expand_through_the_facade() {
    use navree::http::Controller;

    let routes = Arc::new(AccountController).routes();
    let guards: Vec<String> = routes[0]
        .meta
        .guards
        .iter()
        .map(|guard| format!("{guard:?}"))
        .collect();
    assert_eq!(guards, ["not_while_impersonating", "recent_auth(900s)"]);
}
