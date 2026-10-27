//! Resolving a request's workspace: the resolvers, `CurrentWorkspace` and
//! its error answers.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::Method;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
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
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let client = world.as_user(ada);

    client
        .get("/api/current")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("acme"))
        .assert_json_path("role", json!("owner"))
        .assert_json_path("resolver", json!("header"));
    client
        .get("/api/current")
        .header("x-workspace", acme.id.to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(acme.id));
    client
        .get("/api/w/acme/current")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("resolver", json!("path"));
    client
        .get("/api/current")
        .header("host", "acme.app.test")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("resolver", json!("subdomain"));
    client
        .get("/api/current")
        .bearer(&jwt(json!({ "sub": ada, "workspace": "acme" })))
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
    let ada = world.user("Ada", "ada@example.com").await;
    world.workspace(ada, "Acme").await;
    world.workspace(ada, "Globex").await;
    world
        .as_user(ada)
        .get("/api/current")
        .header("host", "globex.app.test")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("globex"));
}

#[tokio::test]
async fn errors_follow_the_framework_shape() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let eve = world.user("Eve", "eve@example.com").await;
    world.workspace(ada, "Acme").await;

    world
        .app
        .client()
        .get("/api/current")
        .header("x-workspace", "acme")
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
        .header("x-workspace", "initech")
        .send()
        .await
        .assert_error(404, "Workspace not found.");
    world
        .as_user(eve)
        .get("/api/current")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.");
}

#[tokio::test]
async fn the_role_comes_from_the_membership() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let globex = world.workspace(grace, "Globex").await;
    world.join(acme.id, grace, "viewer").await;

    let client = world.as_user(grace);
    client
        .get("/api/current")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_json_path("role", json!("viewer"))
        .assert_json_path(
            "permissions",
            json!(["deals.read", "workspace.members.read", "workspace.read"]),
        );
    client
        .get("/api/current")
        .header("x-workspace", globex.slug.as_str())
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
            |_ctx, router| Ok(router.layer(axum::middleware::from_fn(pin_from_header))),
        ));
    }
}

async fn pin_from_header(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
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
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    world.workspace(ada, "Globex").await;
    let client = world.as_user(ada);
    let pin = acme.id.to_string();

    // Naming no workspace: the pinned one.
    client
        .get("/api/current")
        .header("x-test-pin", pin.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("acme"));
    // Naming the pinned one: fine.
    client
        .get("/api/current")
        .header("x-test-pin", pin.as_str())
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200);
    // Naming another one, through a resolver or a route parameter: 403.
    client
        .get("/api/current")
        .header("x-test-pin", pin.as_str())
        .header("x-workspace", "globex")
        .send()
        .await
        .assert_error(403, "This token is limited to another workspace.");
    client
        .get("/api/w/globex/current")
        .header("x-test-pin", pin.as_str())
        .send()
        .await
        .assert_error(403, "This token is limited to another workspace.");
    // Without a pin, nothing changes.
    client
        .get("/api/current")
        .header("x-workspace", "globex")
        .send()
        .await
        .assert_status(200);
}

#[test]
fn requested_workspace_reads_the_configured_resolvers_without_loading_anything() {
    use navree_workspace::{ResolverKind, WorkspaceConfig, requested_workspace};

    let parts = |request: http::request::Builder| request.body(()).unwrap().into_parts().0;
    let config = WorkspaceConfig::default();

    let header = parts(
        http::Request::builder()
            .uri("/api/deals")
            .header("X-Workspace", "acme"),
    );
    assert_eq!(
        requested_workspace(&config, &header),
        Some((ResolverKind::Header, "acme".to_owned()))
    );
    let path = parts(http::Request::builder().uri("/api/w/globex/deals"));
    assert_eq!(
        requested_workspace(&config, &path),
        Some((ResolverKind::Path, "globex".to_owned()))
    );
    let none = parts(http::Request::builder().uri("/api/deals"));
    assert_eq!(requested_workspace(&config, &none), None);
}
