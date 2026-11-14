//! Resolving a request's workspace: the resolvers, `CurrentWorkspace` and
//! its error answers.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::Method;
use http::request::Parts;
use navree_db::Uow;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RequestWorkspace, RouteDef};
use navree_kernel::{
    AppContext, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_workspace::{CurrentWorkspace, PinnedWorkspace};
use serde_json::{Value, json};

use crate::support::{World, builder};

/// `GET /api/current` and `GET /api/w/{workspace}/current`: the resolved
/// workspace.
struct CurrentController;

impl Injectable for CurrentController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(CurrentController) })
    }
}

async fn current(ws: CurrentWorkspace) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({
        "id": ws.id(),
        "slug": ws.slug,
        "role": ws.role().as_str(),
        "resolver": ws.resolver().map(|r| r.as_str()),
        "permissions": ws.permissions(),
        "mail_scope": ws.mail_scope(),
    })))
}

impl Controller for CurrentController {
    fn prefix(&self) -> &'static str {
        "/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/current", current).authenticated(),
            RouteDef::on(Method::GET, "/w/{workspace}/current", current).authenticated(),
        ]
    }
}

struct CurrentModule;

impl Module for CurrentModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<CurrentController>();
    }
}

async fn world() -> World {
    World::with(
        builder()
            .config("workspace.base_domain", "app.test")
            .module(CurrentModule),
    )
    .await
}

fn jwt(claims: Value) -> String {
    format!(
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.{}.c2lnbmF0dXJl",
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

#[tokio::test]
async fn every_resolver_finds_the_workspace() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let client = world.as_user(ada);

    client
        .get("/api/current")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("agra"))
        .assert_json_path("role", json!("owner"))
        .assert_json_path("resolver", json!("header"));
    client
        .get("/api/current")
        .header("x-workspace", agra.id.to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(agra.id))
        .assert_json_path(
            "mail_scope",
            json!({ "workspace_id": agra.id, "organization_id": null }),
        );
    client
        .get("/api/w/agra/current")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("resolver", json!("path"));
    client
        .get("/api/current")
        .header("host", "agra.app.test")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("resolver", json!("subdomain"));
    client
        .get("/api/current")
        .bearer(&jwt(json!({ "sub": ada, "workspace": "agra" })))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("resolver", json!("claim"));
}

#[tokio::test]
async fn the_header_wins_by_default_and_the_order_is_configurable() {
    let world = World::with(
        builder()
            .config("workspace.base_domain", "app.test")
            .config("workspace.resolvers", "subdomain,header")
            .module(CurrentModule),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    world.workspace(ada, "Agra").await;
    world.workspace(ada, "Gulati").await;
    world
        .as_user(ada)
        .get("/api/current")
        .header("host", "gulati.app.test")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("gulati"));
}

#[tokio::test]
async fn errors_follow_the_framework_shape() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let eve = world.user("Ela", "ela@example.com").await;
    world.workspace(ada, "Agra").await;

    world
        .app
        .client()
        .get("/api/current")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    world
        .as_user(ada)
        .get("/api/current")
        .send()
        .await
        .assert_error(400, "The request names no workspace.");
    world
        .as_user(ada)
        .get("/api/current")
        .header("x-workspace", "ilaichi")
        .send()
        .await
        .assert_error(404, "Workspace not found.");
    world
        .as_user(eve)
        .get("/api/current")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.")
        .assert_error_code(403, "not_a_member");
}

#[tokio::test]
async fn the_role_comes_from_the_membership() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(grace, "Gulati").await;
    world.join(agra.id, grace, "viewer").await;

    let client = world.as_user(grace);
    client
        .get("/api/current")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_json_path("role", json!("viewer"))
        .assert_json_path(
            "permissions",
            json!(["deals.read", "workspace.members.read", "workspace.read"]),
        );
    client
        .get("/api/current")
        .header("x-workspace", gulati.slug.as_str())
        .send()
        .await
        .assert_json_path("role", json!("owner"))
        .assert_json_path("permissions", json!(["all.*"]));
}

/// Pins requests that send `x-test-pin: <workspace id>` to that workspace,
/// as an authentication strategy for workspace-bound credentials does.
struct PinModule;

impl Module for PinModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(navree_http::GlobalLayer::new(
            navree_http::LayerSlot::AuthContext,
            "test-pin",
            |_ctx, router| Ok(router.layer(navree_http::middleware::from_fn(pin_from_header))),
        ));
    }
}

async fn pin_from_header(
    mut request: navree_http::extract::Request,
    next: navree_http::middleware::Next,
) -> navree_http::Response {
    let pin = request
        .headers()
        .get("x-test-pin")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| uuid::Uuid::parse_str(value).ok());
    if let Some(pin) = pin {
        request.extensions_mut().insert(PinnedWorkspace(pin));
    }
    next.run(request).await
}

#[tokio::test]
async fn a_pinned_request_uses_its_workspace_and_no_other() {
    let world = World::with(
        builder()
            .config("workspace.base_domain", "app.test")
            .module(CurrentModule)
            .module(PinModule),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    world.workspace(ada, "Gulati").await;
    let client = world.as_user(ada);
    let pin = agra.id.to_string();

    // Naming no workspace: the pinned one.
    client
        .get("/api/current")
        .header("x-test-pin", pin.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("agra"));
    // Naming the pinned one: fine.
    client
        .get("/api/current")
        .header("x-test-pin", pin.as_str())
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200);
    // Naming another one, through a resolver or a route parameter: 403.
    client
        .get("/api/current")
        .header("x-test-pin", pin.as_str())
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_error(403, "This token is limited to another workspace.")
        .assert_error_code(403, "token_workspace_mismatch");
    client
        .get("/api/w/gulati/current")
        .header("x-test-pin", pin.as_str())
        .send()
        .await
        .assert_error(403, "This token is limited to another workspace.");
    // Without a pin, nothing changes.
    client
        .get("/api/current")
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_status(200);
}

/// `GET /api/which`: the workspace `dyn RequestWorkspace` finds for the
/// request, the workspace the request's unit of work is in, and whether a
/// `CurrentWorkspace` is kept. `GET /api/which/current` takes
/// `CurrentWorkspace` first.
struct WhichController;

impl Injectable for WhichController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(WhichController) })
    }
}

async fn which(mut parts: Parts) -> Result<Json<Value>, AppError> {
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .cloned()
        .ok_or_else(|| AppError::internal("no app in the request"))?;
    let resolver = ctx
        .try_get::<dyn RequestWorkspace>()
        .await
        .ok_or_else(|| AppError::internal("no dyn RequestWorkspace in the app"))?;
    let id = resolver.resolve(&mut parts).await;
    Ok(Json(json!({
        "id": id,
        "entered": parts.extensions.get::<Uow>().and_then(Uow::workspace),
        "current": parts.extensions.get::<CurrentWorkspace>().is_some(),
    })))
}

async fn which_after_current(_: CurrentWorkspace, parts: Parts) -> Result<Json<Value>, AppError> {
    which(parts).await
}

impl Controller for WhichController {
    fn prefix(&self) -> &'static str {
        "/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/which", which).public(),
            RouteDef::on(Method::GET, "/which/current", which_after_current).authenticated(),
        ]
    }
}

struct WhichModule;

impl Module for WhichModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<WhichController>();
    }
}

#[tokio::test]
async fn a_request_workspace_is_the_one_current_workspace_resolves_and_is_left_unentered() {
    let world = World::with(builder().module(WhichModule).module(PinModule)).await;
    let harpreet = world.user("Harpreet Sandhu", "harpreet@example.com").await;
    let meera = world.user("Meera Sharma", "meera@example.com").await;
    let kuldeep = world.user("Kuldeep Gill", "kuldeep@example.com").await;
    let ludhiana = world.workspace(harpreet, "Ludhiana office").await;
    let amritsar = world.workspace(meera, "Amritsar office").await;
    world.join(ludhiana.id, meera, "member").await;
    let named = ludhiana.id.to_string();

    // A member naming the workspace: its id, with nothing entered or kept.
    world
        .as_user(meera)
        .get("/api/which")
        .header("x-workspace", named.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(ludhiana.id))
        .assert_json_path("entered", Value::Null)
        .assert_json_path("current", json!(false));
    // Someone who is not a member, a guest, and an unknown workspace: none.
    world
        .as_user(kuldeep)
        .get("/api/which")
        .header("x-workspace", named.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", Value::Null)
        .assert_json_path("current", json!(false));
    world
        .app
        .client()
        .get("/api/which")
        .header("x-workspace", named.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", Value::Null);
    world
        .as_user(meera)
        .get("/api/which")
        .header("x-workspace", uuid::Uuid::now_v7().to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", Value::Null);
    // A credential pinned to another workspace than the one named: none.
    // Pinned and naming none: the pinned one, still not entered.
    let pin = amritsar.id.to_string();
    world
        .as_user(meera)
        .get("/api/which")
        .header("x-test-pin", pin.as_str())
        .header("x-workspace", named.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", Value::Null);
    world
        .as_user(meera)
        .get("/api/which")
        .header("x-test-pin", pin.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(amritsar.id))
        .assert_json_path("entered", Value::Null)
        .assert_json_path("current", json!(false));
    // After the `CurrentWorkspace` extractor, the answer is the extractor's, and the unit
    // of work is in the workspace: what `entered` reads above does report an entered one.
    world
        .as_user(meera)
        .get("/api/which/current")
        .header("x-workspace", named.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(ludhiana.id))
        .assert_json_path("entered", json!(ludhiana.id))
        .assert_json_path("current", json!(true));
}

#[test]
fn requested_workspace_reads_the_configured_resolvers_without_loading_anything() {
    use navree_workspace::{ResolverKind, WorkspaceConfig, requested_workspace};

    let parts = |request: http::request::Builder| request.body(()).unwrap().into_parts().0;
    let config = WorkspaceConfig::default();

    let header = parts(
        http::Request::builder()
            .uri("/api/deals")
            .header("X-Workspace", "agra"),
    );
    assert_eq!(
        requested_workspace(&config, &header),
        Some((ResolverKind::Header, "agra".to_owned()))
    );
    let path = parts(http::Request::builder().uri("/api/w/gulati/deals"));
    assert_eq!(
        requested_workspace(&config, &path),
        Some((ResolverKind::Path, "gulati".to_owned()))
    );
    let none = parts(http::Request::builder().uri("/api/deals"));
    assert_eq!(requested_workspace(&config, &none), None);
}
