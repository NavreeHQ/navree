//! Mount: any tower service under a path prefix, checked at boot with the
//! app's routes and other mounts, and served inside the app's pipeline with
//! only the path below its prefix.

#[path = "http/support.rs"]
mod support;

use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::header::HeaderValue;
use http::request::Parts;
use http::{Method, StatusCode};
use http_body_util::BodyExt;
use navree_contracts::{AuthState, RequestId};
use navree_http::body::{Body, BodyLimit, Bytes};
use navree_http::extract::Request;
use navree_http::middleware::{self, Next};
use navree_http::{
    AppError, AppRouter, ClientIp, GlobalLayer, Guard, LayerSlot, ModuleBuilderHttpExt,
    ModuleBuilderMountExt, Mount, Path, PathParams, RouteAuthorizer, RouteDef, RouteMeta,
};
use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::util::BoxCloneSyncService;

use support::{Counter, Setup, TestApp};

/// Harpreet Sandhu, signed in.
const HARPREET: &str = "0192f0c1-0000-7000-8000-0000000000a1";
/// Meera Sharma, signed in.
const MEERA: &str = "0192f0c1-0000-7000-8000-0000000000a2";

/// A request from `user` (the test authenticator reads `x-user-id`).
fn as_user(method: &str, path: &str, user: &str) -> http::Request<Body> {
    http::Request::builder()
        .method(method)
        .uri(path)
        .header("x-user-id", user)
        .body(Body::empty())
        .unwrap()
}

/// A service that counts its calls and answers `{method} {path and query}`.
fn counted(calls: &Arc<Counter>) -> AppRouter {
    let calls = calls.clone();
    AppRouter::from_handler(move |request: Request| {
        calls.hit();
        async move {
            let target = request
                .uri()
                .path_and_query()
                .map_or("", |target| target.as_str());
            format!("{} {target}", request.method())
        }
    })
}

/// The boot error of an app with a signed-in user's authenticator, `routes`
/// and one mount at `prefix` that `mount` declares.
fn mount_error(
    prefix: &'static str,
    mount: impl Fn(Mount) -> Mount + Send + Sync + 'static,
    routes: Vec<RouteDef>,
) -> impl Future<Output = String> {
    let calls = Arc::new(Counter::default());
    Setup::new(routes)
        .authenticated()
        .register(move |m| {
            m.mount(mount(Mount::new(prefix, counted(&calls))));
        })
        .boot_error()
}

#[tokio::test]
async fn a_mount_without_access_or_overlapping_a_route_fails_the_boot() {
    let error = mount_error("/khata", |mount| mount, vec![]).await;
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` has no access declaration: add .public(), .authenticated(), .roles(..), .permissions(..) or .can(..) (deny by default)"
        ),
        "{error}"
    );

    let ok = || RouteDef::on(Method::GET, "!/khatas", || async { "other" }).public();
    let error = mount_error(
        "/khata",
        Mount::public,
        vec![
            RouteDef::on(Method::GET, "!/khata/status", || async { "ok" }).public(),
            RouteDef::on(Method::POST, "!/khata", || async { "ok" }).public(),
            ok(),
        ],
    )
    .await;
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` overlaps the route GET /khata/status of `TestModule`; give the mount a prefix of its own"
        ),
        "{error}"
    );
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` overlaps the route POST /khata of `TestModule`; give the mount a prefix of its own"
        ),
        "{error}"
    );
    assert!(
        !error.contains("/khatas"),
        "a longer segment is another path: {error}"
    );

    let error = mount_error("/khata", |mount| mount.public().roles(&["munshi"]), vec![]).await;
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` is public but also declares public roles:munshi; remove one of them"
        ),
        "{error}"
    );
    let error = mount_error("/khata", |mount| mount.authenticated().methods(&[]), vec![]).await;
    assert!(
        error.contains(
            "mount `/khata` in `TestModule` takes no method: leave out .methods(..) for every method, or name at least one"
        ),
        "{error}"
    );

    // The same mount beside a route of another path boots.
    let calls = Arc::new(Counter::default());
    Setup::new(vec![ok()])
        .register(move |m| {
            m.mount(Mount::new("/khata", counted(&calls)).public());
        })
        .boot()
        .await;
}

#[tokio::test]
async fn a_mount_overlapping_a_parameter_or_catch_all_route_fails_the_boot() {
    for (pattern, shown, overlaps) in [
        ("!/{workspace}/dashboard", "/{workspace}/dashboard", true),
        ("!/{workspace}", "/{workspace}", true),
        ("!/{*path}", "/{*path}", true),
        ("!/admin/{*rest}", "/admin/{*rest}", true),
        ("!/files/{*path}", "/files/{*path}", false),
        ("!/adminx", "/adminx", false),
    ] {
        let calls = Arc::new(Counter::default());
        let setup = Setup::new(vec![
            RouteDef::on(Method::GET, pattern, || async { "ok" }).public(),
        ])
        .register(move |m| {
            m.mount(Mount::new("/admin", counted(&calls)).public());
        });
        if overlaps {
            let error = setup.boot_error().await;
            let expected = format!(
                "mount `/admin` in `TestModule` overlaps the route GET {shown} of `TestModule`; give the mount a prefix of its own"
            );
            assert!(error.contains(&expected), "{pattern}: {error}");
        } else {
            setup.boot().await;
        }
    }
}

#[tokio::test]
async fn a_mount_prefix_under_slash_underscore_is_refused() {
    for (prefix, detail) in [
        ("/_navree/khata", None),
        ("/_khata", None),
        ("khata", None),
        ("/khata/", None),
        ("/", None),
        ("/a//b", None),
        ("/a/../b", None),
        ("/a..b", None),
        ("/{ledger}", None),
        ("/khata*", None),
        (
            "/a b",
            Some("segment `a b` holds ` `, which a static segment may not"),
        ),
        (
            "/%6Bhata",
            Some("segment `%6Bhata` holds `%`, which a static segment may not"),
        ),
        ("/a/./b", Some("`/a/./b` has a `.` or `..` segment")),
        (
            "/:id",
            Some("segment `:id` uses old capture syntax; write `{param}` or `{*rest}`"),
        ),
    ] {
        let error = mount_error(prefix, Mount::public, vec![]).await;
        let detail = detail
            .map(|detail| format!(" ({detail})"))
            .unwrap_or_default();
        let expected = format!(
            "mount `{prefix}` in `TestModule`: a prefix starts with `/` but not with `/_`, has no `{{`, `}}`, `*`, `..` or `//`, and does not end with `/`{detail}"
        );
        assert!(error.contains(&expected), "{prefix}: {error}");
    }
}

#[tokio::test]
async fn two_mounts_at_or_under_one_prefix_fail_the_boot() {
    for (second, overlaps) in [
        ("/khata/reports", true),
        ("/khata", true),
        ("/khatas", false),
        ("/reports", false),
    ] {
        let calls = Arc::new(Counter::default());
        let setup = Setup::new(vec![]).register(move |m| {
            m.mount(Mount::new("/khata", counted(&calls)).public())
                .mount(Mount::new(second, counted(&calls)).public());
        });
        if overlaps {
            let error = setup.boot_error().await;
            let expected =
                format!("mounts `/khata` (TestModule) and `{second}` (TestModule) overlap");
            assert!(error.contains(&expected), "{second}: {error}");
        } else {
            setup.boot().await;
        }
    }
}

// ---------------------------------------------------------------------------
// Serving a mount
// ---------------------------------------------------------------------------

/// What a mounted service saw of one request.
#[derive(Clone, Debug, Default)]
struct Seen {
    method: String,
    /// The path and query it was given.
    target: String,
    header_request_id: Option<String>,
    request_id: Option<String>,
    client_ip: bool,
    context: bool,
    user: Option<String>,
    route: Option<String>,
    body_limit: Option<usize>,
    trail: Option<String>,
    /// Whether the router's decoded capture of the path reached it.
    params: bool,
}

/// A ledger service of another crate, written with tower alone: it records
/// what it sees of each request and answers the path and query it was given.
#[derive(Clone, Default)]
struct Ledger {
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Ledger {
    fn service(&self) -> BoxCloneSyncService<Request, http::Response<Body>, Infallible> {
        let seen = self.seen.clone();
        BoxCloneSyncService::new(tower::service_fn(move |request: Request| {
            let target = request
                .uri()
                .path_and_query()
                .map_or_else(String::new, |target| target.as_str().to_owned());
            let header = |name: &str| {
                request
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let extensions = request.extensions();
            seen.lock().unwrap().push(Seen {
                method: request.method().to_string(),
                target: target.clone(),
                header_request_id: header("x-request-id"),
                request_id: extensions
                    .get::<RequestId>()
                    .map(|id| id.as_str().to_owned()),
                client_ip: extensions.get::<ClientIp>().is_some(),
                context: extensions.get::<AppContext>().is_some(),
                user: extensions
                    .get::<AuthState>()
                    .and_then(|state| state.0.as_ref())
                    .map(|principal| principal.user_id.to_string()),
                route: extensions
                    .get::<Arc<RouteMeta>>()
                    .map(|meta| meta.full_path.clone()),
                body_limit: extensions.get::<BodyLimit>().map(|limit| limit.0),
                trail: header("x-trail"),
                params: extensions.get::<PathParams>().is_some(),
            });
            async move { Ok::<_, Infallible>(http::Response::new(Body::from(target))) }
        }))
    }

    /// How many requests reached the service.
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    /// The last request that reached the service.
    fn last(&self) -> Seen {
        self.seen
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a request reached the ledger")
    }
}

/// A global layer of the `Context` slot, the innermost of the app's layers:
/// it marks the request `x-trail: context`.
fn trail() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Context, "trail", |_, router| {
        Ok(router.layer(middleware::from_fn(
            |mut request: Request, next: Next| async move {
                request
                    .headers_mut()
                    .insert("x-trail", HeaderValue::from_static("context"));
                next.run(request).await
            },
        )))
    })
}

/// An app with the test authenticator and the `trail` layer, whose module
/// mounts what `mount` builds.
fn mounted(mount: impl Fn() -> Mount + Send + Sync + 'static) -> Setup {
    Setup::new(vec![]).authenticated().register(move |m| {
        m.http_layer(trail());
        m.mount(mount());
    })
}

/// A `Ledger` at `/khata`, declared by `declare`, on an app `mounted` builds.
fn ledger_setup(declare: impl Fn(Mount) -> Mount + Send + Sync + 'static) -> (Ledger, Setup) {
    let ledger = Ledger::default();
    let service = ledger.clone();
    let setup = mounted(move || declare(Mount::new("/khata", service.service())));
    (ledger, setup)
}

/// `ledger_setup`, booted.
fn ledger_app(
    declare: impl Fn(Mount) -> Mount + Send + Sync + 'static,
) -> (Ledger, impl Future<Output = TestApp>) {
    let (ledger, setup) = ledger_setup(declare);
    (ledger, setup.boot())
}

#[tokio::test]
async fn a_mounted_service_sees_the_raw_path_under_its_prefix_with_the_pipeline_applied() {
    let (ledger, app) = ledger_app(Mount::authenticated);
    let app = app.await;
    app.get("/khata/entries").await.assert_error(401);
    assert_eq!(ledger.calls(), 0, "a guest never reaches the service");
    for (path, target) in [
        ("/khata", "/"),
        ("/khata?month=kartik", "/?month=kartik"),
        ("/khata/entries", "/entries"),
        (
            "/khata/entries/2026/ludhiana?sort=-amount",
            "/entries/2026/ludhiana?sort=-amount",
        ),
        ("/khata/a/", "/a/"),
        ("/khata/...", "/..."),
        ("/khata/a;b=c", "/a;b=c"),
    ] {
        let response = app.send(as_user("GET", path, HARPREET)).await;
        assert_eq!(response.status, StatusCode::OK, "{path}");
        let seen = ledger.last();
        assert_eq!(seen.target, target, "{path}");
        assert_eq!(
            seen.header_request_id.as_deref(),
            response.header("x-request-id"),
            "{path}"
        );
        assert_eq!(seen.request_id, seen.header_request_id, "{path}");
        assert!(seen.client_ip && seen.context, "{path}");
        assert_eq!(seen.user.as_deref(), Some(HARPREET), "{path}");
        assert_eq!(seen.route.as_deref(), Some("/khata/{*rest}"), "{path}");
        assert_eq!(seen.body_limit, Some(2 * 1024 * 1024), "{path}");
        assert_eq!(
            seen.trail.as_deref(),
            Some("context"),
            "the app's global layers ran first: {path}"
        );
        assert!(!seen.params, "only the path as written: {path}");
    }
    for method in [
        "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD", "PROPFIND",
    ] {
        assert_eq!(
            app.send(as_user(method, "/khata/entries", HARPREET))
                .await
                .status,
            StatusCode::OK,
            "{method}"
        );
        assert_eq!(ledger.last().method, method);
    }
}

#[tokio::test]
async fn an_encoded_question_mark_reaches_the_service_encoded() {
    let (ledger, app) = ledger_app(Mount::authenticated);
    let app = app.await;
    for (path, target) in [
        ("/khata/v1/items%3Fx?y=2", "/v1/items%3Fx?y=2"),
        ("/khata/a%23b", "/a%23b"),
        ("/khata/%3F", "/%3F"),
    ] {
        let response = app.send(as_user("GET", path, HARPREET)).await;
        assert_eq!(response.status, StatusCode::OK, "{path}");
        assert_eq!(ledger.last().target, target, "{path}");
    }
}

#[tokio::test]
async fn an_encoded_slash_reaches_the_service_as_one_segment() {
    let (ledger, app) = ledger_app(Mount::authenticated);
    let app = app.await;
    // The escape keeps the case it was sent in; a trailing one is a trailing
    // slash, as `/khata/a/` is. (`/khata/%2F` and `/khata/a%2f%2fb` decode to
    // an empty segment: the empty-segment test refuses them.)
    for (path, target) in [
        ("/khata/a%2Fb", "/a%2Fb"),
        ("/khata/a%2fb", "/a%2fb"),
        ("/khata/a%2F", "/a%2F"),
    ] {
        let response = app.send(as_user("GET", path, HARPREET)).await;
        assert_eq!(response.status, StatusCode::OK, "{path}");
        assert_eq!(ledger.last().target, target, "{path}");
    }
}

#[tokio::test]
async fn dot_dot_raw_encoded_or_decoded_never_reaches_the_service() {
    let (ledger, app) = ledger_app(Mount::authenticated);
    let app = app.await;
    for path in [
        "/khata/..",
        "/khata/../x",
        "/khata/.",
        "/khata/./x",
        "/khata/%2e%2e/x",
        "/khata/%2E%2e",
        "/khata/.%2e/x",
        "/khata/a%2F..%2Fb",
        "/khata/..%2F",
        "/khata/a/%2E",
    ] {
        let response = app.send(as_user("GET", path, HARPREET)).await;
        let body = response.assert_error(400);
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .starts_with("Invalid URL:"),
            "{path}: {body}"
        );
    }
    assert_eq!(ledger.calls(), 0);
    // Three dots are a name, not a step up.
    assert_eq!(
        app.send(as_user("GET", "/khata/...", HARPREET))
            .await
            .text(),
        "/..."
    );
}

#[tokio::test]
async fn an_empty_segment_after_the_prefix_is_refused() {
    let (ledger, app) = ledger_app(Mount::authenticated);
    let app = app.await;
    for path in [
        "/khata//other.example/x",
        "/khata/a//b",
        "/khata/a/b//",
        // The same written encoded: the router's catch-all decoding lets an
        // empty part through.
        "/khata/%2Fx",
        "/khata/%2Fetc/passwd",
        "/khata/%2F",
        "/khata/a%2F%2Fb",
        "/khata/a%2f%2fb",
        "/khata/a%2F/b",
        "/khata/a/%2Fb",
    ] {
        let body = app
            .send(as_user("GET", path, HARPREET))
            .await
            .assert_error(400);
        assert_eq!(
            body["message"], "Invalid URL: the path has an empty segment",
            "{path}"
        );
    }
    // The access check comes first.
    app.get("/khata/%2Fetc/passwd").await.assert_error(401);
    app.send(as_user("GET", "/khata/", HARPREET))
        .await
        .assert_error(404);
    app.send(as_user("GET", "//khata/x", HARPREET))
        .await
        .assert_error(404);
    assert_eq!(ledger.calls(), 0);
    assert_eq!(
        app.send(as_user("GET", "/khata", HARPREET)).await.text(),
        "/"
    );
    assert_eq!(
        app.send(as_user("GET", "/khata/a%2Fb", HARPREET))
            .await
            .text(),
        "/a%2Fb"
    );
}

#[tokio::test]
async fn a_backslash_a_nul_or_bytes_that_are_not_utf8_never_reach_the_service() {
    let (ledger, app) = ledger_app(Mount::authenticated);
    let app = app.await;
    for path in ["/khata/a\\b", "/khata/a%5Cb", "/khata/a%00", "/khata/%FF"] {
        let body = app
            .send(as_user("GET", path, HARPREET))
            .await
            .assert_error(400);
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .starts_with("Invalid URL:"),
            "{path}: {body}"
        );
    }
    assert_eq!(ledger.calls(), 0);
    assert_eq!(
        app.send(as_user("GET", "/khata/ab", HARPREET)).await.text(),
        "/ab"
    );
}

#[tokio::test]
async fn only_the_prefix_itself_as_written_reaches_the_mount() {
    let (ledger, app) = ledger_app(Mount::authenticated);
    let app = app.await;
    for path in [
        "/KHATA/x",
        "/Khata",
        "/%6Bhata/x",
        "/khatas/x",
        "/khata-old",
    ] {
        app.send(as_user("GET", path, HARPREET))
            .await
            .assert_error(404);
    }
    assert_eq!(ledger.calls(), 0);
    assert_eq!(
        app.send(as_user("GET", "/khata/x", HARPREET)).await.text(),
        "/x"
    );
}

#[tokio::test]
async fn a_long_path_reaches_the_service_whole_and_only_after_the_access_check() {
    let (ledger, app) = ledger_app(Mount::authenticated);
    let app = app.await;
    let rest = "a".repeat(60_000);
    let path = format!("/khata/{rest}");
    app.get(&path).await.assert_error(401);
    assert_eq!(ledger.calls(), 0);
    assert_eq!(
        app.send(as_user("GET", &path, HARPREET)).await.status,
        StatusCode::OK
    );
    assert_eq!(ledger.last().target, format!("/{rest}"));
}

/// Lets a request through only with `x-role: munshi`, and counts how often it
/// is asked.
#[derive(Default)]
struct Munshi {
    asked: Counter,
}

impl RouteAuthorizer for Munshi {
    fn authorize<'a>(
        &'a self,
        parts: &'a mut Parts,
        _meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        self.asked.hit();
        Box::pin(async move {
            match parts.headers.get("x-role") {
                Some(role) if role == "munshi" => Ok(()),
                _ => Err(AppError::forbidden().with_detail("Only a munshi may open the khata.")),
            }
        })
    }
}

/// Registers a `Munshi` as the app's `dyn RouteAuthorizer`.
struct MunshiModule(Arc<Munshi>);

impl Module for MunshiModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc::<dyn RouteAuthorizer>(self.0.clone());
    }
}

/// Lets a request through only with `x-books: open`, and counts its checks.
#[derive(Debug, Default)]
struct OpenBooks {
    checks: Counter,
}

impl Guard for OpenBooks {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        self.checks.hit();
        Box::pin(async move {
            if parts
                .headers
                .get("x-books")
                .is_some_and(|books| books == "open")
            {
                Ok(())
            } else {
                Err(AppError::forbidden().with_detail("The books are closed."))
            }
        })
    }
}

#[tokio::test]
async fn a_mount_checks_its_access_on_every_request() {
    let munshi = Arc::new(Munshi::default());
    let (ledger, setup) = ledger_setup(|mount| mount.roles(&["munshi"]));
    let app = setup.module(MunshiModule(munshi.clone())).boot().await;
    let as_munshi = || {
        let mut request = as_user("GET", "/khata/entries", HARPREET);
        request
            .headers_mut()
            .insert("x-role", HeaderValue::from_static("munshi"));
        request
    };
    assert_eq!(app.send(as_munshi()).await.text(), "/entries");
    let refused = app.send(as_user("GET", "/khata/entries", HARPREET)).await;
    assert_eq!(
        refused.assert_error(403)["message"],
        "Only a munshi may open the khata."
    );
    assert_eq!(app.send(as_munshi()).await.status, StatusCode::OK);
    assert_eq!(munshi.asked.get(), 3, "the authorizer is asked every time");
    assert_eq!(ledger.calls(), 2, "the refused request never reached it");

    // A guard runs on every request too.
    let books = Arc::new(OpenBooks::default());
    let guard = books.clone();
    let (ledger, app) = ledger_app(move |mount| mount.authenticated().guard(guard.clone()));
    let app = app.await;
    let closed = app.send(as_user("GET", "/khata/entries", HARPREET)).await;
    assert_eq!(closed.assert_error(403)["message"], "The books are closed.");
    let mut open = as_user("GET", "/khata/entries", HARPREET);
    open.headers_mut()
        .insert("x-books", HeaderValue::from_static("open"));
    assert_eq!(app.send(open).await.text(), "/entries");
    assert_eq!(books.checks.get(), 2);
    assert_eq!(ledger.calls(), 1);
}

/// Lets a request through when its `x-permission` header names what the
/// mount declares: its permission, or its ability as `{action} {subject}`.
#[derive(Default)]
struct Permits {
    asked: Counter,
}

impl RouteAuthorizer for Permits {
    fn authorize<'a>(
        &'a self,
        parts: &'a mut Parts,
        meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        self.asked.hit();
        let needed = match &meta.access.can {
            Some(can) => format!("{} {}", can.action, can.subject),
            None => meta.access.permissions.join(","),
        };
        Box::pin(async move {
            let given = parts
                .headers
                .get("x-permission")
                .and_then(|value| value.to_str().ok());
            if given == Some(needed.as_str()) {
                Ok(())
            } else {
                Err(AppError::forbidden().with_detail("This khata is not yours to open."))
            }
        })
    }
}

/// Registers a `Permits` as the app's `dyn RouteAuthorizer`.
struct PermitsModule(Arc<Permits>);

impl Module for PermitsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc::<dyn RouteAuthorizer>(self.0.clone());
    }
}

#[tokio::test]
async fn a_mount_checks_a_permission_or_an_ability_on_every_request() {
    for (declare, permission) in [
        (
            (|mount: Mount| mount.permissions(&["khata.read"])) as fn(Mount) -> Mount,
            "khata.read",
        ),
        (|mount: Mount| mount.can("read", "Khata"), "read Khata"),
    ] {
        let permits = Arc::new(Permits::default());
        let (ledger, setup) = ledger_setup(declare);
        let app = setup.module(PermitsModule(permits.clone())).boot().await;
        app.get("/khata/entries").await.assert_error(401);
        let refused = app.send(as_user("GET", "/khata/entries", HARPREET)).await;
        assert_eq!(
            refused.assert_error(403)["message"],
            "This khata is not yours to open.",
            "{permission}"
        );
        let mut allowed = as_user("GET", "/khata/entries", HARPREET);
        allowed
            .headers_mut()
            .insert("x-permission", HeaderValue::from_static(permission));
        assert_eq!(app.send(allowed).await.text(), "/entries", "{permission}");
        assert_eq!(
            (permits.asked.get(), ledger.calls()),
            (2, 1),
            "asked on each signed-in request: {permission}"
        );
    }
}

/// Sends every method a `GET`-only mount at `/khata` does not take (`CONNECT`
/// and an extension method among them) to the prefix and to a path below it,
/// with and without a body, carrying `x-permission: khata.read`, from `user`
/// (a guest when `None`): each is the router's 405 with the `Allow` of a
/// `GET` route.
async fn only_get_is_taken(app: &TestApp, user: Option<&str>) {
    let others = [
        Method::POST,
        Method::PUT,
        Method::DELETE,
        Method::PATCH,
        Method::CONNECT,
        Method::from_bytes(b"PROPFIND").unwrap(),
    ];
    for method in others {
        for path in ["/khata", "/khata/entries"] {
            for body in ["", "₹1,00,000 for the Diwali khata"] {
                let mut request = http::Request::builder()
                    .method(method.clone())
                    .uri(path)
                    .header("x-permission", "khata.read");
                if let Some(user) = user {
                    request = request.header("x-user-id", user);
                }
                if !body.is_empty() {
                    request = request
                        .header("content-type", "text/plain")
                        .header("content-length", body.len());
                }
                let response = app.send(request.body(Body::from(body)).unwrap()).await;
                assert_eq!(
                    response.assert_error(405)["message"],
                    "Method Not Allowed",
                    "{method} {path} {body:?}"
                );
                assert_eq!(
                    response.header("allow"),
                    Some("GET, HEAD, OPTIONS"),
                    "{method} {path} {body:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn a_mount_that_names_its_methods_answers_any_other_as_a_route_does() {
    // A public, read-only mount; a method named twice is kept once.
    let (ledger, app) = ledger_app(|mount| mount.public().methods(&[Method::GET, Method::GET]));
    let app = app.await;
    only_get_is_taken(&app, None).await;
    only_get_is_taken(&app, Some(HARPREET)).await;
    assert_eq!(ledger.calls(), 0, "no other method reached the service");
    assert_eq!(app.get("/khata/entries").await.text(), "/entries");
    let head = app
        .send(
            http::Request::head("/khata/entries")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!((head.status, head.text().as_str()), (StatusCode::OK, ""));
    assert_eq!(ledger.last().method, "HEAD", "HEAD runs the GET answer");
    let options = app
        .send(
            http::Request::options("/khata/entries")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(options.status, StatusCode::NO_CONTENT);
    assert_eq!(options.header("allow"), Some("GET, HEAD, OPTIONS"));
    assert_eq!(ledger.calls(), 2, "OPTIONS is answered without the service");

    // A mount that needs a permission: the router answers the other methods
    // first, before authentication and the access check.
    let permits = Arc::new(Permits::default());
    let (ledger, setup) = ledger_setup(|mount| {
        mount
            .permissions(&["khata.read"])
            .methods(&[Method::GET])
    });
    let app = setup.module(PermitsModule(permits.clone())).boot().await;
    only_get_is_taken(&app, None).await;
    only_get_is_taken(&app, Some(HARPREET)).await;
    assert_eq!(ledger.calls(), 0, "no other method reached the service");
    assert_eq!(permits.asked.get(), 0, "refused before the access check");
    app.get("/khata/entries").await.assert_error(401);
    let mut allowed = as_user("GET", "/khata/entries", HARPREET);
    allowed
        .headers_mut()
        .insert("x-permission", HeaderValue::from_static("khata.read"));
    assert_eq!(app.send(allowed).await.text(), "/entries");
    assert_eq!((permits.asked.get(), ledger.calls()), (1, 1));
}

/// Answers `/slow` after five seconds and any other path at once.
async fn slow_or_fast(request: Request) -> Result<http::Response<Body>, Infallible> {
    if request.uri().path() == "/slow" {
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Ok(http::Response::new(Body::from("on time")))
}

/// Reads the whole body and answers its length; a failed read is the
/// service's error.
async fn measure(request: Request) -> Result<http::Response<Body>, BoxError> {
    let body = request.into_body().collect().await?.to_bytes();
    Ok(http::Response::new(Body::from(body.len().to_string())))
}

#[tokio::test]
async fn a_mount_is_one_route_for_its_throttle_timeout_and_body_limit() {
    // One budget across the mount's paths and methods, per user.
    let (ledger, app) =
        ledger_app(|mount| mount.authenticated().throttle(2, Duration::from_secs(60)));
    let app = app.await;
    assert_eq!(
        app.send(as_user("GET", "/khata/a", HARPREET)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.send(as_user("POST", "/khata/b", HARPREET)).await.status,
        StatusCode::OK
    );
    let limited = app.send(as_user("DELETE", "/khata", HARPREET)).await;
    limited.assert_error(429);
    assert!(
        limited.header("retry-after").is_some(),
        "{:?}",
        limited.headers
    );
    assert_eq!(
        app.send(as_user("GET", "/khata/a", MEERA)).await.status,
        StatusCode::OK,
        "a budget of her own"
    );
    assert_eq!(ledger.calls(), 3);

    // The mount's timeout.
    let app = mounted(|| {
        Mount::new("/khata", tower::service_fn(slow_or_fast))
            .authenticated()
            .timeout(Duration::from_secs(1))
    })
    .boot()
    .await;
    let late = app.send(as_user("GET", "/khata/slow", HARPREET)).await;
    assert_eq!(
        late.assert_error(503)["message"],
        "The server did not finish the request in time."
    );
    assert_eq!(
        app.send(as_user("GET", "/khata/fast", HARPREET))
            .await
            .text(),
        "on time"
    );

    // The mount's body limit, declared and streamed.
    let reads = Arc::new(Counter::default());
    let counter = reads.clone();
    let app = mounted(move || {
        let counter = counter.clone();
        Mount::new(
            "/khata",
            tower::service_fn(move |request: Request| {
                counter.hit();
                measure(request)
            }),
        )
        .authenticated()
        .body_limit(8)
    })
    .boot()
    .await;
    let declared = http::Request::post("/khata/notes")
        .header("x-user-id", HARPREET)
        .header("content-length", "16")
        .body(Body::from("Ludhiana-Jaipur!"))
        .unwrap();
    assert_eq!(
        app.send(declared).await.assert_error(413)["message"],
        "The request body must not exceed 8 bytes."
    );
    assert_eq!(reads.get(), 0, "refused before the service");
    let chunks = futures_util::stream::iter(
        (0..3).map(|_| Ok::<_, std::io::Error>(Bytes::from_static(b"Amritsar"))),
    );
    let streamed = http::Request::post("/khata/notes")
        .header("x-user-id", HARPREET)
        .body(Body::from_stream(chunks))
        .unwrap();
    assert_eq!(
        app.send(streamed).await.assert_error(413)["message"],
        "Failed to buffer the request body: length limit exceeded"
    );
    assert_eq!(
        reads.get(),
        1,
        "the service read up to the limit, no further"
    );
    let fits = http::Request::post("/khata/notes")
        .header("x-user-id", HARPREET)
        .body(Body::from("Ludhiana"))
        .unwrap();
    let response = app.send(fits).await;
    assert_eq!(
        (response.status, response.text().as_str()),
        (StatusCode::OK, "8")
    );
}

/// The error a ledger service answers with.
#[derive(Debug)]
struct LedgerClosed;

impl fmt::Display for LedgerClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the khata is closed for Diwali")
    }
}

impl std::error::Error for LedgerClosed {}

/// Answers by path: a panic, two errors, and 200 for any other path.
async fn torn(request: Request) -> Result<http::Response<Body>, BoxError> {
    match request.uri().path() {
        "/boom" => panic!("the khata of Amritsar is torn"),
        "/closed" => Err(LedgerClosed.into()),
        "/lost" => Err("the khata of Amritsar is lost".into()),
        _ => Ok(http::Response::new(Body::from("the khata is open"))),
    }
}

/// Answers by path with error statuses of its own, each marked
/// `x-ledger: khata`: a 422 with no body, a plain-text 422, a plain-text 404,
/// a 500 and a 422 with JSON bodies of their own shape.
async fn answers(request: Request) -> Result<http::Response<Body>, Infallible> {
    let (status, content_type, body) = match request.uri().path() {
        "/empty" => (StatusCode::UNPROCESSABLE_ENTITY, None, ""),
        "/plain" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("text/plain"),
            "amount must be at least ₹1",
        ),
        "/missing" => (StatusCode::NOT_FOUND, Some("text/plain"), "no such entry"),
        "/broken" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Some("application/json"),
            r#"{"fault":"the ledger of Jaipur is offline"}"#,
        ),
        "/rejected" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("application/json"),
            r#"{"field":"amount"}"#,
        ),
        _ => (StatusCode::OK, Some("text/plain"), "the khata is open"),
    };
    let mut response = http::Response::builder()
        .status(status)
        .header("x-ledger", "khata");
    if let Some(content_type) = content_type {
        response = response.header("content-type", content_type);
    }
    Ok(response.body(Body::from(body)).unwrap())
}

#[tokio::test]
async fn a_mounted_services_own_answer_goes_out_as_it_wrote_it() {
    let app = Setup::new(vec![
        // A route's own plain-text 422, for comparison.
        RouteDef::on(Method::GET, "/plain", || async {
            http::Response::builder()
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .header("content-type", "text/plain")
                .body(Body::from("amount must be at least ₹1"))
                .unwrap()
        })
        .public(),
    ])
    .register(|m| {
        m.mount(Mount::new("/khata", tower::service_fn(answers)).public());
    })
    .boot()
    .await;
    for (path, status, content_type, body) in [
        ("/khata/empty", 422, None, ""),
        (
            "/khata/plain",
            422,
            Some("text/plain"),
            "amount must be at least ₹1",
        ),
        ("/khata/missing", 404, Some("text/plain"), "no such entry"),
        (
            "/khata/broken",
            500,
            Some("application/json"),
            r#"{"fault":"the ledger of Jaipur is offline"}"#,
        ),
        (
            "/khata/rejected",
            422,
            Some("application/json"),
            r#"{"field":"amount"}"#,
        ),
    ] {
        let response = app.get(path).await;
        assert_eq!(response.status.as_u16(), status, "{path}");
        assert_eq!(response.header("content-type"), content_type, "{path}");
        assert_eq!(response.header("x-ledger"), Some("khata"), "{path}");
        assert_eq!(response.text(), body, "{path}");
        assert!(response.header("x-request-id").is_some(), "{path}");
    }
    // A route's own answer of the same kind still takes the app's shape.
    assert_eq!(
        app.get("/api/plain").await.assert_error(400)["message"],
        "amount must be at least ₹1"
    );
}

#[tokio::test]
async fn a_panic_or_an_error_inside_a_mounted_service_answers_in_the_apps_error_shape() {
    let app = Setup::new(vec![])
        .register(|m| {
            m.mount(Mount::new("/khata", tower::service_fn(torn)).public());
            m.map_error::<LedgerClosed>(|_| {
                Some(AppError::conflict("The khata is closed for Diwali."))
            });
        })
        .boot()
        .await;
    let boom = app.get("/khata/boom").await;
    assert_eq!(boom.assert_error(500)["message"], "Internal server error");
    assert!(!boom.text().contains("Amritsar"), "{}", boom.text());
    assert_eq!(
        app.get("/khata/open").await.text(),
        "the khata is open",
        "the next request is served"
    );
    assert_eq!(
        app.get("/khata/closed").await.assert_error(409)["message"],
        "The khata is closed for Diwali."
    );
    let lost = app.get("/khata/lost").await;
    assert_eq!(lost.assert_error(500)["message"], "Internal server error");
    assert!(!lost.text().contains("Amritsar"), "{}", lost.text());
}

/// Reads a response head off `stream`, up to its blank line, within 5 s.
async fn read_head(stream: &mut tokio::net::TcpStream) -> String {
    let mut head = Vec::new();
    let read = async {
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte).await {
                Ok(1..) => head.push(byte[0]),
                _ => break,
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .expect("the head arrives within 5 s");
    String::from_utf8_lossy(&head).into_owned()
}

#[tokio::test]
async fn an_upgrade_or_connect_request_to_a_mount_is_refused_before_the_service() {
    let (ledger, app) = ledger_app(Mount::public);
    let app = app.await;
    let refused = "A mounted service answers requests; it does not take over the connection.";
    let websocket = http::Request::get("/khata/chat")
        .header("connection", "upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .body(Body::empty())
        .unwrap();
    let h2c = http::Request::get("/khata/chat")
        .header("connection", "upgrade, http2-settings")
        .header("upgrade", "h2c")
        .header("http2-settings", "AAMAAABkAAQAAP__")
        .body(Body::empty())
        .unwrap();
    let mut extended = http::Request::connect("/khata/chat")
        .body(Body::empty())
        .unwrap();
    extended
        .extensions_mut()
        .insert(hyper::ext::Protocol::from_static("websocket"));
    let tunnel = http::Request::connect("/khata/chat")
        .body(Body::empty())
        .unwrap();
    for request in [websocket, h2c, extended, tunnel] {
        assert_eq!(
            app.send(request).await.assert_error(501)["message"],
            refused
        );
    }
    assert_eq!(ledger.calls(), 0);
    assert_eq!(
        app.get("/khata/chat").await.text(),
        "/chat",
        "a plain request reaches the service"
    );

    // Over a socket: the handshake is answered 501 and the connection is not
    // switched.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = app.router.clone();
    tokio::spawn(async move {
        let _ = navree_http::serve(listener, router).await;
    });
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /khata/chat HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n")
        .await
        .unwrap();
    let head = read_head(&mut stream).await;
    assert!(head.starts_with("HTTP/1.1 501"), "{head}");
    assert!(
        !head.to_ascii_lowercase().contains("\r\nupgrade:"),
        "{head}"
    );
    assert_eq!(ledger.calls(), 1);
}

#[tokio::test]
async fn an_app_without_a_mount_routes_and_answers_as_before() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/khata", || async { "index" }).public(),
        RouteDef::on(Method::POST, "/khata", || async { "stored" }).public(),
        RouteDef::on(
            Method::GET,
            "/files/{*path}",
            |Path(path): Path<String>| async move { path },
        )
        .public(),
    ])
    .boot()
    .await;
    assert_eq!(app.get("/api/khata").await.text(), "index");
    let wrong = app
        .send(
            http::Request::put("/api/khata")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    wrong.assert_error(405);
    assert_eq!(wrong.header("allow"), Some("GET, HEAD, POST, OPTIONS"));
    let options = app
        .send(
            http::Request::options("/api/khata")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(options.status, StatusCode::NO_CONTENT);
    assert_eq!(options.header("allow"), Some("GET, HEAD, POST, OPTIONS"));
    let head = app
        .send(
            http::Request::head("/api/khata")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        (
            head.status,
            head.text().as_str(),
            head.header("content-length")
        ),
        (StatusCode::OK, "", Some("5"))
    );
    assert_eq!(app.get("/api/files/a/b.txt").await.text(), "a/b.txt");
    app.get("/api/khata/").await.assert_error(404);
    app.get("/api/files/a/../b").await.assert_error(400);
    app.get("/khata").await.assert_error(404);
    let table: Vec<(String, String)> = navree_http::route_table(app.app.ctx())
        .await
        .unwrap()
        .into_iter()
        .map(|meta| (meta.method, meta.full_path))
        .collect();
    assert_eq!(
        table,
        [
            ("GET".into(), "/api/khata".into()),
            ("POST".into(), "/api/khata".into()),
            ("GET".into(), "/api/files/{*path}".into()),
        ]
    );
}
