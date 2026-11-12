//! Deny by default and the authentication / authorization seams:
//! `Authenticator`, `RouteAuthorizer`, `Guard`, throttles, global layer slots,
//! audit actions.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::request::Parts;
use http::{Method, StatusCode};
use navree_contracts::{AuditEntry, AuditKind, AuditSink, AuthState};
use navree_http::body::Body;
use navree_http::extract::Request;
use navree_http::middleware::Next;
use navree_http::{
    AppError, CurrentPrincipal, GlobalLayer, Guard, HttpOptions, LayerSlot, ModuleBuilderHttpExt,
    RouteAuthorizer, RouteDef, RouteMeta,
};
use navree_kernel::{BoxError, BoxFuture, Module, ModuleBuilder};
use uuid::Uuid;

use crate::support::{Counter, Setup, options};

const ALICE: &str = "0192f0c1-0000-7000-8000-00000000000a";
const BOB: &str = "0192f0c1-0000-7000-8000-00000000000b";

fn as_user(method: &str, path: &str, user: &str) -> http::Request<Body> {
    http::Request::builder()
        .method(method)
        .uri(path)
        .header("x-user-id", user)
        .body(Body::empty())
        .unwrap()
}

fn ok_route(method: Method, path: &str) -> RouteDef {
    RouteDef::on(method, path, || async { "ok" })
}

// ---------------------------------------------------------------------------
// Deny by default: boot errors (Review Focus 2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn boot_fails_for_route_without_access_declaration() {
    let error = Setup::new(vec![
        ok_route(Method::GET, "/declared").public(),
        ok_route(Method::GET, "/naked").name("naked"),
    ])
    .authenticated()
    .boot_error()
    .await;

    assert!(error.contains("GET /api/naked"), "{error}");
    assert!(error.contains("no access declaration"), "{error}");
    assert!(error.contains("TestModule"), "names the module: {error}");
    assert!(!error.contains("/api/declared"), "{error}");
}

#[tokio::test]
async fn a_guard_alone_is_not_an_access_declaration() {
    let error = Setup::new(vec![
        ok_route(Method::GET, "/guarded").guard(Arc::new(HeaderGuard)),
    ])
    .authenticated()
    .boot_error()
    .await;
    assert!(error.contains("no access declaration"), "{error}");
}

#[tokio::test]
async fn non_public_route_without_authenticator_fails_boot() {
    let error = Setup::new(vec![
        ok_route(Method::GET, "/me").authenticated(),
        ok_route(Method::GET, "/open").public(),
    ])
    .boot_error()
    .await;
    assert!(error.contains("GET /api/me"), "{error}");
    assert!(error.contains("dyn Authenticator"), "{error}");
    assert!(!error.contains("/api/open"), "{error}");
}

#[tokio::test]
async fn authorization_route_without_authorizer_fails_boot() {
    let error = Setup::new(vec![
        ok_route(Method::GET, "/admin").roles(&["admin"]),
        ok_route(Method::GET, "/deals/{id}").can("update", "Deal", Some("id")),
    ])
    .authenticated()
    .boot_error()
    .await;
    assert!(error.contains("GET /api/admin"), "{error}");
    assert!(error.contains("GET /api/deals/{id}"), "{error}");
    assert!(error.contains("dyn RouteAuthorizer"), "{error}");
}

#[tokio::test]
async fn public_route_with_roles_fails_boot() {
    let error = Setup::new(vec![
        ok_route(Method::GET, "/confused")
            .public()
            .roles(&["admin"]),
    ])
    .authenticated()
    .module(AuthorizerModule::default())
    .boot_error()
    .await;
    assert!(error.contains("is public but also declares"), "{error}");
}

#[tokio::test]
async fn duplicate_and_malformed_routes_fail_boot() {
    let error = Setup::new(vec![
        ok_route(Method::GET, "/twice").public(),
        ok_route(Method::GET, "/twice/").public(),
        ok_route(Method::POST, "/twice").public(),
        ok_route(Method::GET, "/old/:id").public(),
    ])
    .boot_error()
    .await;
    assert!(
        error.contains("GET /api/twice") && error.contains("registered twice"),
        "{error}"
    );
    assert!(
        !error.contains("POST /api/twice"),
        "different methods share a path: {error}"
    );
    assert!(error.contains("`:id`"), "{error}");
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

#[tokio::test]
async fn non_public_route_returns_401_without_principal() {
    let calls = Arc::new(Counter::default());
    let handler_calls = calls.clone();
    let app = Setup::new(vec![
        RouteDef::on(
            Method::GET,
            "/me",
            move |CurrentPrincipal(principal): CurrentPrincipal| {
                handler_calls.hit();
                async move { principal.user_id.to_string() }
            },
        )
        .authenticated(),
        // No extractor: only the pipeline can reject the guest.
        ok_route(Method::GET, "/plain").authenticated(),
    ])
    .authenticated()
    .boot()
    .await;

    app.get("/api/plain").await.assert_error(401);
    let response = app.get("/api/me").await;
    let body = response.assert_error(401);
    assert_eq!(body["message"], "Unauthorized");
    assert_eq!(body["error"], "Unauthorized");
    assert_eq!(calls.get(), 0, "the handler never runs for guests");

    let response = app.send(as_user("GET", "/api/me", ALICE)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.text(), ALICE);
}

#[tokio::test]
async fn unknown_path_is_404_not_401() {
    let app = Setup::new(vec![ok_route(Method::GET, "/private").authenticated()])
        .authenticated()
        .boot()
        .await;

    app.get("/api/nothing-here").await.assert_error(404);
    app.get("/api/private/deeper").await.assert_error(404);
    // A known path with the wrong method is 405, not 401.
    app.send(
        http::Request::delete("/api/private")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .assert_error(405);
}

/// An authenticator that rejects every request with a custom error.
struct RejectingAuthenticator;

impl navree_http::Authenticator for RejectingAuthenticator {
    fn authenticate<'a>(
        &'a self,
        _parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<navree_contracts::Principal>, AppError>> {
        Box::pin(async { Err(AppError::unauthenticated().with_detail("The token has expired.")) })
    }
}

#[tokio::test]
async fn authenticator_errors_are_returned_to_the_client() {
    let app = Setup::new(vec![ok_route(Method::GET, "/open").public()])
        .register(|m| {
            m.value_arc::<dyn navree_http::Authenticator>(Arc::new(RejectingAuthenticator));
        })
        .boot()
        .await;
    let body = app.get("/api/open").await.assert_error(401);
    assert_eq!(body["message"], "The token has expired.");
    assert_eq!(body["error"], "Unauthorized");
}

// ---------------------------------------------------------------------------
// Authorization and guards
// ---------------------------------------------------------------------------

/// Records the metadata it is called with; allows only `x-role: admin`.
/// (method, full path, roles, `can` from-parameter) of one authorize call.
type Seen = (String, String, Vec<String>, Option<String>);

#[derive(Default)]
struct RecordingAuthorizer {
    seen: Mutex<Vec<Seen>>,
}

impl RouteAuthorizer for RecordingAuthorizer {
    fn authorize<'a>(
        &'a self,
        parts: &'a mut Parts,
        meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            self.seen.lock().unwrap().push((
                meta.method.clone(),
                meta.full_path.clone(),
                meta.access.roles.clone(),
                meta.access.can.as_ref().and_then(|c| c.from.clone()),
            ));
            let principal_present = parts
                .extensions
                .get::<AuthState>()
                .is_some_and(|s| s.0.is_some());
            assert!(principal_present, "authorization runs after authentication");
            match parts.headers.get("x-role") {
                Some(role) if role == "admin" => Ok(()),
                _ => Err(AppError::forbidden().with_detail("Admins only.")),
            }
        })
    }
}

#[derive(Default)]
struct AuthorizerModule {
    authorizer: Arc<RecordingAuthorizer>,
}

impl Module for AuthorizerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc::<dyn RouteAuthorizer>(self.authorizer.clone());
    }
}

#[tokio::test]
async fn authorizer_called_with_route_meta() {
    let authorizer = Arc::new(RecordingAuthorizer::default());
    let calls = Arc::new(Counter::default());
    let handler_calls = calls.clone();
    let app = Setup::new(vec![
        RouteDef::on(Method::POST, "/reports/{id}", move || {
            handler_calls.hit();
            async { "done" }
        })
        .roles(&["admin"])
        .can("export", "Report", Some("id")),
        ok_route(Method::GET, "/mine").authenticated(),
    ])
    .version("1")
    .authenticated()
    .module(AuthorizerModule {
        authorizer: authorizer.clone(),
    })
    .boot()
    .await;

    let response = app.send(as_user("POST", "/api/v1/reports/7", ALICE)).await;
    let body = response.assert_error(403);
    assert_eq!(body["message"], "Admins only.");
    assert_eq!(body["error"], "Forbidden");
    assert_eq!(calls.get(), 0);
    assert_eq!(
        authorizer.seen.lock().unwrap().clone(),
        vec![(
            "POST".to_owned(),
            "/api/v1/reports/{id}".to_owned(),
            vec!["admin".to_owned()],
            Some("id".to_owned()),
        )]
    );

    let mut request = as_user("POST", "/api/v1/reports/7", ALICE);
    request
        .headers_mut()
        .insert("x-role", "admin".parse().unwrap());
    assert_eq!(app.send(request).await.text(), "done");
    assert_eq!(calls.get(), 1);

    // Guests are rejected before the authorizer runs; routes without
    // roles/permissions/can never reach it.
    app.send(
        http::Request::post("/api/v1/reports/7")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .assert_error(401);
    assert_eq!(
        app.send(as_user("GET", "/api/v1/mine", ALICE)).await.text(),
        "ok"
    );
    assert_eq!(authorizer.seen.lock().unwrap().len(), 2);
}

/// Requires the `x-pass` header.
#[derive(Debug)]
struct HeaderGuard;

impl Guard for HeaderGuard {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            if parts.headers.contains_key("x-pass") {
                Ok(())
            } else {
                Err(AppError::forbidden().with_detail("Guard says no."))
            }
        })
    }
}

#[tokio::test]
async fn guard_runs_before_handler() {
    let calls = Arc::new(Counter::default());
    let handler_calls = calls.clone();
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/guarded", move || {
            handler_calls.hit();
            async { "through" }
        })
        .public()
        .guard(Arc::new(HeaderGuard)),
    ])
    .boot()
    .await;

    let body = app.get("/api/guarded").await.assert_error(403);
    assert_eq!(body["message"], "Guard says no.");
    assert_eq!(calls.get(), 0, "handler not called");

    let response = app
        .send(
            http::Request::get("/api/guarded")
                .header("x-pass", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.text(), "through");
    assert_eq!(calls.get(), 1);
}

#[tokio::test]
async fn per_user_throttle() {
    let app = Setup::new(vec![
        ok_route(Method::GET, "/limited")
            .authenticated()
            .throttle(2, Duration::from_secs(60)),
        ok_route(Method::GET, "/public-limited")
            .public()
            .throttle(1, Duration::from_secs(60)),
        ok_route(Method::GET, "/other").authenticated(),
    ])
    .authenticated()
    .boot()
    .await;

    for _ in 0..2 {
        let response = app.send(as_user("GET", "/api/limited", ALICE)).await;
        assert_eq!(response.status, StatusCode::OK);
    }
    let response = app.send(as_user("GET", "/api/limited", ALICE)).await;
    response.assert_error(429);
    assert!(response.header("retry-after").is_some());

    // Other users and other routes have their own budgets.
    assert_eq!(
        app.send(as_user("GET", "/api/limited", BOB)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.send(as_user("GET", "/api/other", ALICE)).await.status,
        StatusCode::OK
    );

    // Guests are throttled by client IP.
    assert_eq!(app.get("/api/public-limited").await.status, StatusCode::OK);
    app.get("/api/public-limited").await.assert_error(429);
}

/// A guest's route throttle counts IPv6 clients by network (a /64 by default).
#[tokio::test]
async fn guest_throttle_counts_ipv6_clients_by_network() {
    let app = Setup::new(vec![
        ok_route(Method::GET, "/public-limited")
            .public()
            .throttle(1, Duration::from_secs(60)),
    ])
    .options(HttpOptions {
        trust_proxy: true,
        ..options()
    })
    .boot()
    .await;
    let from = |ip: &str| {
        http::Request::get("/api/public-limited")
            .header("x-forwarded-for", ip.to_owned())
            .body(Body::empty())
            .unwrap()
    };

    assert_eq!(
        app.send(from("2001:db8:5:5::1")).await.status,
        StatusCode::OK
    );
    app.send(from("2001:db8:5:5:1:2:3:4"))
        .await
        .assert_error(429);
    assert_eq!(
        app.send(from("2001:db8:5:6::1")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.send(from("::ffff:192.0.2.7")).await.status,
        StatusCode::OK
    );
    app.send(from("192.0.2.7")).await.assert_error(429);
}

// ---------------------------------------------------------------------------
// Global layer slots
// ---------------------------------------------------------------------------

/// A layer that appends `name` (and `+auth` when `AuthState` is present) to
/// the `x-trail` request header.
fn trail_layer(slot: LayerSlot, name: &'static str) -> GlobalLayer {
    GlobalLayer::new(slot, name, move |_ctx, router| {
        Ok(router.layer(navree_http::middleware::from_fn(
            move |mut request: Request, next: Next| async move {
                let mut trail = request
                    .headers()
                    .get("x-trail")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned)
                    .unwrap_or_default();
                if !trail.is_empty() {
                    trail.push(',');
                }
                trail.push_str(name);
                if request.extensions().get::<AuthState>().is_some() {
                    trail.push_str("+auth");
                }
                request
                    .headers_mut()
                    .insert("x-trail", trail.parse().unwrap());
                next.run(request).await
            },
        )))
    })
}

struct LayerModule<const N: usize>(LayerSlot, &'static str);

impl<const N: usize> Module for LayerModule<N> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(trail_layer(self.0, self.1));
    }
}

#[tokio::test]
async fn global_layers_applied_in_slot_order() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/trail", |request: Request| async move {
            request
                .headers()
                .get("x-trail")
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default()
        })
        .public(),
    ])
    .authenticated()
    // Registered out of order on purpose.
    .module(LayerModule::<5>(LayerSlot::Context, "context"))
    .module(LayerModule::<1>(LayerSlot::Uow, "uow"))
    .module(LayerModule::<2>(LayerSlot::Session, "session-a"))
    .module(LayerModule::<3>(LayerSlot::AuthContext, "auth-context"))
    .module(LayerModule::<4>(LayerSlot::Session, "session-b"))
    .boot()
    .await;

    let response = app.get("/api/trail").await;
    assert_eq!(
        response.text(),
        "session-a,session-b,auth-context+auth,uow+auth,context+auth",
        "slots outermost first, module order within a slot, navree's authentication first in AuthContext"
    );
    // Global layers also wrap the 404 fallback.
    app.get("/api/missing").await.assert_error(404);
}

/// With no `Session` layer, navree's authentication runs as the last fixed
/// stage; the contributed layers still see the request authenticated.
#[tokio::test]
async fn authentication_precedes_the_slots_without_session_layers() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/trail", |request: Request| async move {
            request
                .headers()
                .get("x-trail")
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default()
        })
        .public(),
        RouteDef::on(Method::GET, "/private", || async { "secret" }).authenticated(),
    ])
    .authenticated()
    .module(LayerModule::<1>(LayerSlot::Uow, "uow"))
    .module(LayerModule::<3>(LayerSlot::AuthContext, "auth-context"))
    .boot()
    .await;

    assert_eq!(
        app.get("/api/trail").await.text(),
        "auth-context+auth,uow+auth"
    );
    app.get("/api/private").await.assert_error(401);
    let response = app.send(as_user("GET", "/api/private", ALICE)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.text(), "secret");
    app.get("/api/missing").await.assert_error(404);
}

/// A module whose `Edge` layer answers `/edge/...` itself (reporting
/// whether the request already had a request id) and marks every other
/// response it lets through.
struct EdgeModule;

impl Module for EdgeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(GlobalLayer::new(LayerSlot::Edge, "edge", |_ctx, router| {
            Ok(router.layer(navree_http::middleware::from_fn(
                |request: Request, next: Next| async move {
                    if request.uri().path().starts_with("/edge") {
                        let seen = request
                            .extensions()
                            .get::<navree_contracts::RequestId>()
                            .is_some();
                        return navree_http::response::IntoResponse::into_response(format!(
                            "edge saw a request id: {seen}"
                        ));
                    }
                    let mut response = next.run(request).await;
                    response
                        .headers_mut()
                        .insert("x-edge", http::HeaderValue::from_static("passed"));
                    response
                },
            )))
        }));
    }
}

#[tokio::test]
async fn edge_layers_run_before_every_fixed_layer() {
    let app = Setup::new(vec![ok_route(Method::GET, "/inner").public()])
        .module(LayerModule::<1>(LayerSlot::Session, "session"))
        .module(EdgeModule)
        .boot()
        .await;

    let edge = app.get("/edge/call").await;
    assert_eq!(edge.status, StatusCode::OK);
    assert_eq!(edge.text(), "edge saw a request id: false");
    assert!(
        edge.headers.get("x-request-id").is_none(),
        "an edge layer that answers itself bypasses the request-id layer"
    );

    let inner = app.get("/api/inner").await;
    assert_eq!(inner.status, StatusCode::OK);
    assert_eq!(inner.headers["x-edge"], "passed");
    assert!(inner.headers.contains_key("x-request-id"));
    // The edge layer also wraps the fixed layers' own responses.
    let missing = app.get("/api/missing").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.headers["x-edge"], "passed");
}

/// A layer in `slot` that panics on `/boom`, and on `/refuse` answers a
/// plain 403 itself.
struct TroubleModule(LayerSlot);

impl Module for TroubleModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(GlobalLayer::new(self.0, "trouble", |_ctx, router| {
            Ok(router.layer(navree_http::middleware::from_fn(
                |request: Request, next: Next| async move {
                    match request.uri().path() {
                        "/api/boom" => panic!("the layer exploded"),
                        "/api/refuse" => navree_http::response::IntoResponse::into_response(
                            StatusCode::FORBIDDEN,
                        ),
                        _ => next.run(request).await,
                    }
                },
            )))
        }));
    }
}

#[tokio::test]
async fn a_panic_in_a_global_layer_is_a_json_error_with_every_header() {
    for slot in [
        LayerSlot::Session,
        LayerSlot::AuthContext,
        LayerSlot::Uow,
        LayerSlot::Context,
    ] {
        let app = Setup::new(vec![ok_route(Method::GET, "/inner").public()])
            .module(TroubleModule(slot))
            .boot()
            .await;

        let response = app.get("/api/boom").await;
        let body = response.assert_error(500);
        assert_eq!(body["message"], "Internal server error", "{slot:?}");
        assert!(response.header("x-request-id").is_some(), "{slot:?}");
        let response = app
            .send(crate::support::from_origin(
                "GET",
                "/api/boom",
                crate::support::ORIGIN,
            ))
            .await;
        assert_eq!(
            response.header("access-control-allow-origin"),
            Some(crate::support::ORIGIN),
            "{slot:?}"
        );

        // The next request is served: the stack is not left broken.
        assert_eq!(app.get("/api/inner").await.status, StatusCode::OK);

        // A plain refusal from a layer renders in the JSON error format.
        let body = app.get("/api/refuse").await.assert_error(403);
        assert_eq!(body["error"], "Forbidden", "{slot:?}");
    }
}

/// An edge layer is outside the panic catcher and the error stage, which the
/// documentation of `LayerSlot::Edge` says.
#[tokio::test]
async fn a_panic_at_the_edge_is_not_caught_and_a_refusal_is_not_rendered() {
    let app = Setup::new(vec![ok_route(Method::GET, "/inner").public()])
        .module(TroubleModule(LayerSlot::Edge))
        .boot()
        .await;

    let refused = app.get("/api/refuse").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.text(), "");
    assert!(refused.header("x-request-id").is_none());

    let request = Request::get("/api/boom").body(Body::empty()).unwrap();
    let outcome =
        futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(app.send(request)))
            .await;
    assert!(outcome.is_err(), "the panic reaches whoever serves the app");
}

#[tokio::test]
async fn failing_global_layer_fails_boot() {
    struct BrokenLayer;
    impl Module for BrokenLayer {
        fn register(&self, m: &mut ModuleBuilder) {
            m.http_layer(GlobalLayer::new(
                LayerSlot::Uow,
                "broken",
                |_ctx, _router| Err(BoxError::from("no database configured")),
            ));
        }
    }
    let error = Setup::new(vec![ok_route(Method::GET, "/x").public()])
        .module(BrokenLayer)
        .boot_error()
        .await;
    assert!(error.contains("broken"), "{error}");
    assert!(error.contains("no database configured"), "{error}");
}

// ---------------------------------------------------------------------------
// Audit actions
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CapturingSink {
    entries: Mutex<Vec<AuditEntry>>,
}

impl AuditSink for CapturingSink {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.entries.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn audit_action_recorded_on_success() {
    let sink = Arc::new(CapturingSink::default());
    let registered = sink.clone();
    let app = Setup::new(vec![
        ok_route(Method::POST, "/publish")
            .authenticated()
            .audit("post.published"),
        RouteDef::on(Method::POST, "/fail", || async {
            Err::<(), _>(AppError::conflict("Already published."))
        })
        .authenticated()
        .audit("post.failed"),
    ])
    .authenticated()
    .register(move |m| {
        m.value_arc::<dyn AuditSink>(registered.clone());
    })
    .boot()
    .await;

    let mut request = as_user("POST", "/api/publish", ALICE);
    request
        .headers_mut()
        .insert("user-agent", "navree-test/1.0".parse().unwrap());
    let response = app.send(request).await;
    assert_eq!(response.status, StatusCode::OK);
    app.send(as_user("POST", "/api/fail", ALICE))
        .await
        .assert_error(409);

    let entries = sink.entries.lock().unwrap().clone();
    assert_eq!(entries.len(), 1, "only 2xx responses are audited");
    let entry = &entries[0];
    assert_eq!(entry.kind, AuditKind::Custom);
    assert_eq!(entry.action, "post.published");
    assert_eq!(entry.actor_id, Some(Uuid::parse_str(ALICE).unwrap()));
    assert_eq!(entry.request_id.as_deref(), response.header("x-request-id"));
    assert_eq!(entry.user_agent.as_deref(), Some("navree-test/1.0"));
    assert_eq!(entry.metadata["route"], "/api/publish");
}

/// Security events reach the audit sink as well as the log: rate-limit
/// refusals (once per client and window, so a flood does not flood the
/// audit log) and 403s that a handler (or a guard) returns itself, which
/// no `RouteAuthorizer` saw.
#[tokio::test]
async fn security_events_are_audited() {
    let sink = Arc::new(CapturingSink::default());
    let registered = sink.clone();
    let app = Setup::new(vec![
        ok_route(Method::GET, "/limited")
            .authenticated()
            .throttle(1, Duration::from_secs(60)),
        RouteDef::on(Method::POST, "/owner", || async {
            Err::<(), _>(AppError::forbidden().with_detail("Only managers may change the owner"))
        })
        .authenticated(),
        ok_route(Method::GET, "/guarded")
            .authenticated()
            .guard(Arc::new(HeaderGuard)),
    ])
    .options(navree_http::HttpOptions {
        per_ip_rate_limit: Some(6),
        ..crate::support::options()
    })
    .authenticated()
    .register(move |m| {
        m.value_arc::<dyn AuditSink>(registered.clone());
    })
    .boot()
    .await;
    let actions = || {
        sink.entries
            .lock()
            .unwrap()
            .iter()
            .map(|entry| entry.action.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        app.send(as_user("GET", "/api/limited", ALICE)).await.status,
        StatusCode::OK
    );
    for _ in 0..2 {
        app.send(as_user("GET", "/api/limited", ALICE))
            .await
            .assert_error(429);
    }
    assert_eq!(actions(), ["http.rate_limited"], "once per window");
    let entry = sink.entries.lock().unwrap()[0].clone();
    assert_eq!(entry.kind, AuditKind::Security);
    assert_eq!(entry.actor_id, Some(Uuid::parse_str(ALICE).unwrap()));
    assert_eq!(entry.metadata["limit"], "route");
    assert_eq!(entry.metadata["route"], "/api/limited");
    assert!(entry.request_id.is_some());

    app.send(as_user("POST", "/api/owner", ALICE))
        .await
        .assert_error(403);
    app.send(as_user("GET", "/api/guarded", ALICE))
        .await
        .assert_error(403);
    let entries = sink.entries.lock().unwrap().clone();
    let denied: Vec<_> = entries
        .iter()
        .filter(|entry| entry.action == "access.denied")
        .collect();
    assert_eq!(denied.len(), 2, "{entries:?}");
    assert_eq!(denied[0].kind, AuditKind::Security);
    assert_eq!(denied[0].metadata["check"], "handler");
    assert_eq!(denied[0].metadata["route"], "/api/owner");
    assert_eq!(denied[0].metadata["method"], "POST");
    assert_eq!(denied[0].actor_id, Some(Uuid::parse_str(ALICE).unwrap()));
    assert_eq!(denied[1].metadata["check"], "guard");

    // The per-IP limit (6 a minute; 6 used after the next request): the
    // second refusal is not audited again.
    app.send(as_user("POST", "/api/owner", BOB))
        .await
        .assert_error(403);
    for _ in 0..2 {
        app.send(as_user("GET", "/api/guarded", BOB))
            .await
            .assert_error(429);
    }
    let limited: Vec<_> = sink
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter(|entry| entry.action == "http.rate_limited" && entry.metadata["limit"] == "per_ip")
        .cloned()
        .collect();
    assert_eq!(limited.len(), 1, "{limited:?}");
    assert_eq!(limited[0].metadata["path"], "/api/guarded");
    assert_eq!(limited[0].ip.as_deref(), Some("0.0.0.0"));
}

// One address, two answers: a guest gets the public page, a signed-in reader the app's,
// each handler with its own extractors (the signed-in one's refuses guests, and is never
// asked for one). The answer varies by what signs a request in.
#[tokio::test]
async fn one_address_answers_a_guest_and_a_signed_in_reader_apart() {
    let app = Setup::new(vec![RouteDef::on_guest_or(
        Method::GET,
        "/home",
        || async { "the public site" },
        |CurrentPrincipal(who): CurrentPrincipal| async move { format!("desk of {}", who.user_id) },
    )])
    .authenticated()
    .boot()
    .await;

    let guest = app.get("/api/home").await;
    assert_eq!(guest.status, StatusCode::OK);
    assert_eq!(guest.text(), "the public site");
    assert_eq!(guest.header("vary"), Some("cookie, authorization"));

    let reader = app.send(as_user("GET", "/api/home", ALICE)).await;
    assert_eq!(reader.status, StatusCode::OK);
    assert_eq!(reader.text(), format!("desk of {ALICE}"));
    assert_eq!(reader.header("vary"), Some("cookie, authorization"));
}

// A signed-in reader's answer never reaches a shared cache, whatever its handler said: with
// no Cache-Control, or one that lets a shared cache keep it, it goes out private and
// unstored. A handler that kept it private says so itself, and the guest's answer is as
// its handler wrote it.
#[tokio::test]
async fn a_signed_in_readers_answer_is_never_left_to_a_shared_cache() {
    let with = |cache: Option<&'static str>| {
        move || async move {
            let mut response = http::Response::new(Body::from("page"));
            if let Some(cache) = cache {
                response.headers_mut().insert(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static(cache),
                );
            }
            response
        }
    };
    let app = Setup::new(vec![
        RouteDef::on_guest_or(
            Method::GET,
            "/none",
            with(Some("public, max-age=300")),
            with(None),
        ),
        RouteDef::on_guest_or(
            Method::GET,
            "/public",
            with(None),
            with(Some("public, max-age=300")),
        ),
        RouteDef::on_guest_or(
            Method::GET,
            "/shared",
            with(None),
            with(Some("max-age=60, S-MaxAge=600, private")),
        ),
        RouteDef::on_guest_or(Method::GET, "/aged", with(None), with(Some("max-age=60"))),
        RouteDef::on_guest_or(
            Method::GET,
            "/private",
            with(None),
            with(Some("private, max-age=60")),
        ),
        RouteDef::on_guest_or(Method::GET, "/unstored", with(None), with(Some("no-store"))),
    ])
    .authenticated()
    .boot()
    .await;

    let guest = app.get("/api/none").await;
    assert_eq!(guest.header("cache-control"), Some("public, max-age=300"));
    for path in ["/api/none", "/api/public", "/api/shared", "/api/aged"] {
        let reader = app.send(as_user("GET", path, ALICE)).await;
        assert_eq!(reader.status, StatusCode::OK, "{path}");
        assert_eq!(
            reader.header("cache-control"),
            Some("private, no-store"),
            "{path}"
        );
        assert_eq!(
            reader.header("vary"),
            Some("cookie, authorization"),
            "{path}"
        );
    }
    let kept = app.send(as_user("GET", "/api/private", ALICE)).await;
    assert_eq!(kept.header("cache-control"), Some("private, max-age=60"));
    let unstored = app.send(as_user("GET", "/api/unstored", ALICE)).await;
    assert_eq!(unstored.header("cache-control"), Some("no-store"));
}
