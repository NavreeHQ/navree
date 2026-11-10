//! Workspaces in server-rendered pages: the `session` resolver,
//! `POST /workspace/switch` and the scoping that follows a switch, with the
//! session layer running as in a browser app.

use std::sync::{Arc, Mutex, PoisonError};

use http::Method;
use http::header::CONTENT_TYPE;
use http::request::Parts;
use navree_contracts::{
    AuditEntry, AuditSink, BoxError, Broadcaster, ZoneLevel, ZoneRequest, ZoneSource,
};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::Uow;
use navree_http::CurrentPrincipal;
use navree_http::body::Body;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, Response, RouteDef};
use navree_kernel::AppContext;
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestClient, TestResponse, principal};
use navree_workspace::{
    CurrentWorkspace, SESSION_KEY, SWITCH_ROUTE, SessionChoice, WorkspaceRepo, WorkspaceSwitcher,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Deal, World, builder, new_deal};

/// Records audit entries in memory.
#[derive(Default)]
struct RecordingSink(Mutex<Vec<AuditEntry>>);

impl RecordingSink {
    fn with_action(&self, action: &str) -> Vec<AuditEntry> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|entry| entry.action == action)
            .cloned()
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

/// `GET /api/switching/current`, `/deals`, `/stream` and `/switcher`: the
/// resolved workspace, its deals, a stream opened inside it, and the
/// switcher drawn for the request.
struct ProbeController;

impl Injectable for ProbeController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ProbeController) })
    }
}

async fn current(ws: CurrentWorkspace) -> Json<Value> {
    Json(json!({
        "id": ws.id(),
        "slug": ws.slug,
        "resolver": ws.resolver().map(|r| r.as_str()),
    }))
}

async fn deals(ws: CurrentWorkspace, uow: Uow) -> Result<Json<Value>, AppError> {
    let mut titles: Vec<String> = WorkspaceRepo::<Deal>::query(ws.id())
        .get(&uow)
        .await
        .map_err(AppError::internal)?
        .into_models()
        .into_iter()
        .map(|deal| deal.title)
        .collect();
    titles.sort();
    Ok(Json(json!({ "workspace": ws.slug, "titles": titles })))
}

/// What a live region's stream does at connect: it opens inside the
/// request's workspace and only ever sends that workspace's events.
async fn stream(ws: CurrentWorkspace) -> Response {
    let body = format!("event: hello\ndata: {}\n\n", ws.id());
    http::Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from(body))
        .expect("a valid response")
}

async fn switcher(switcher: WorkspaceSwitcher) -> String {
    navree_ui::render(&navree_ui::Node::from(switcher))
}

/// What the app's live hub does to find a stream's workspace: ask every
/// workspace-level `dyn ZoneSource` for the request's owner, with the
/// request's principal and head.
async fn hub_workspace(
    CurrentPrincipal(me): CurrentPrincipal,
    mut parts: Parts,
) -> Result<Json<Value>, AppError> {
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .cloned()
        .ok_or_else(|| AppError::internal("outside the pipeline"))?;
    let sources = ctx
        .get_all::<dyn ZoneSource>()
        .await
        .map_err(AppError::internal)?;
    for source in sources
        .iter()
        .filter(|source| source.level() == ZoneLevel::Workspace)
    {
        let mut request = ZoneRequest::new(Some(&me), &mut parts);
        if let Some(owner) = source
            .zone_of_request(&mut request)
            .await
            .map_err(AppError::internal)?
        {
            return Ok(Json(json!({ "workspace": owner.id })));
        }
    }
    Ok(Json(json!({ "workspace": null })))
}

impl Controller for ProbeController {
    fn prefix(&self) -> &'static str {
        "/switching"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/current", current).authenticated(),
            RouteDef::on(Method::GET, "/deals", deals).authenticated(),
            RouteDef::on(Method::GET, "/stream", stream).authenticated(),
            RouteDef::on(Method::GET, "/switcher", switcher).authenticated(),
            RouteDef::on(Method::GET, "/hub-workspace", hub_workspace).authenticated(),
        ]
    }
}

struct ProbeModule;

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ProbeController>();
    }
}

/// The session cookie's name under the default options.
fn session_cookie() -> String {
    SessionOptions::default().cookie_name()
}

/// A world whose app runs the session layer and resolves workspaces with
/// `resolvers`, auditing into `sink`.
async fn world_with(resolvers: Value, sink: Arc<RecordingSink>) -> World {
    World::with(
        builder()
            .config("app.key", generate_key())
            .config("workspace.resolvers", resolvers)
            .module(CryptoModule)
            .module(SessionModule::for_root(SessionOptions::default()))
            .module(ProbeModule)
            .override_value::<dyn AuditSink>(sink),
    )
    .await
}

async fn world() -> (World, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::default());
    let world = world_with(json!(["session", "header"]), sink.clone()).await;
    (world, sink)
}

/// `POST /workspace/switch` as a plain HTML form (with the session's token).
async fn switch(client: &TestClient, workspace: &str, next: &str) -> TestResponse {
    if client.cookie("XSRF-TOKEN").is_none() {
        client.csrf_cookie().await;
    }
    client
        .post("/workspace/switch")
        .xsrf()
        .form(&[("workspace", workspace), ("next", next)])
        .send()
        .await
}

async fn current_of(client: &TestClient) -> TestResponse {
    client.get("/api/switching/current").send().await
}

// A member switches; the `session` resolver names that
// workspace from then on, and the switch is audited.
#[tokio::test]
async fn a_member_switches_and_the_session_names_the_workspace() {
    let (world, sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    let client = world.as_user(ada);

    let response = switch(&client, "agra", "/deals?stage=won")
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/deals?stage=won"));
    current_of(&client)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(agra.id))
        .assert_json_path("resolver", json!("session"));

    // By id as well as by slug; the session id changes with the switch.
    let before = client.cookie(&session_cookie()).expect("a session cookie");
    switch(&client, &gulati.id.to_string(), "/")
        .await
        .assert_status(303);
    let after = client.cookie(&session_cookie()).expect("a session cookie");
    assert_ne!(before, after, "a switch gives the session a new id");
    current_of(&client)
        .await
        .assert_json_path("slug", json!("gulati"))
        .assert_json_path("resolver", json!("session"));

    let switched = sink.with_action("workspace.switched");
    assert_eq!(switched.len(), 2);
    let last = &switched[1];
    assert_eq!(last.actor_id, Some(ada));
    assert_eq!(
        last.subject_id.as_deref(),
        Some(gulati.id.to_string().as_str())
    );
    let metadata = &last.metadata;
    assert_eq!(metadata["from"], json!(agra.id));
    assert_eq!(metadata["to"], json!(gulati.id));
}

// JSON clients get the workspace back, and an unsafe `next` never
// leaves the site.
#[tokio::test]
async fn json_switches_answer_the_workspace_and_next_stays_on_the_site() {
    let (world, _sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let client = world.as_user(ada);
    client.csrf_cookie().await;
    client
        .post("/workspace/switch")
        .xsrf()
        .json(&json!({ "workspace": "agra" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(agra.id))
        .assert_json_path("slug", json!("agra"))
        .assert_json_path("name", json!("Agra"));
    for next in ["//evil.test/", "https://evil.test/", "/\\evil.test"] {
        let response = switch(&client, "agra", next).await.assert_status(303);
        assert_eq!(response.header("location"), Some("/"), "{next}");
    }
    // Nothing named: a 400 on `workspace`.
    client
        .post("/workspace/switch")
        .xsrf()
        .form(&[("next", "/")])
        .send()
        .await
        .assert_status(400);
}

// A user can't switch into a workspace they don't belong to:
// 403 `not_a_member`, nothing stored, the old choice kept. An unknown one is
// a 404.
#[tokio::test]
async fn switching_into_a_workspace_of_others_is_refused() {
    let (world, sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(grace, "Gulati").await;
    let client = world.as_user(ada);

    switch(&client, "agra", "/").await.assert_status(303);
    switch(&client, "gulati", "/")
        .await
        .assert_error_code(403, "not_a_member");
    switch(&client, &gulati.id.to_string(), "/")
        .await
        .assert_error_code(403, "not_a_member");
    switch(&client, "no-such-workspace", "/")
        .await
        .assert_error_code(404, "workspace_not_found");
    current_of(&client)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(agra.id))
        .assert_json_path("resolver", json!("session"));
    assert_eq!(sink.with_action("workspace.switched").len(), 1);

    // Guests can't switch at all; nor can a form without the token.
    world
        .app
        .client()
        .post("/workspace/switch")
        .form(&[("workspace", "agra")])
        .send()
        .await
        .assert_status(401);
    client
        .post("/workspace/switch")
        .form(&[("workspace", "agra")])
        .send()
        .await
        .assert_status(403);
}

// With nothing chosen, the next resolver answers; with no
// resolver answering, the request names no workspace (400).
#[tokio::test]
async fn without_a_choice_the_next_resolver_answers() {
    let (world, _sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    world.workspace(ada, "Agra").await;
    let client = world.as_user(ada);
    client
        .get("/api/switching/current")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("resolver", json!("header"));
    current_of(&client)
        .await
        .assert_error_code(400, "workspace_required");
}

/// A world as [`world`] is, with `workspace.session_default` on.
async fn defaulting_world() -> World {
    World::with(
        builder()
            .config("app.key", generate_key())
            .config("workspace.resolvers", json!(["session", "header"]))
            .config("workspace.session_default", true)
            .module(CryptoModule)
            .module(SessionModule::for_root(SessionOptions::default()))
            .module(ProbeModule),
    )
    .await
}

// With `workspace.session_default`, a browser that has chosen nothing is in
// the first workspace its user belongs to, keeps it, and can still switch;
// a user who belongs to none is still told so, and a header still places
// a request when the session names nothing.
#[tokio::test]
async fn a_browser_that_chose_nothing_is_in_its_first_workspace_when_asked() {
    let world = defaulting_world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let nobody = world.user("Manjeet", "manjeet@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    let client = world.as_user(ada);

    current_of(&client)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(agra.id))
        .assert_json_path("resolver", json!("session"));
    // It keeps the choice: the session names it from now on.
    current_of(&client)
        .await
        .assert_json_path("id", json!(agra.id))
        .assert_json_path("resolver", json!("session"));
    // Switching still works.
    switch(&client, "gulati", "/").await.assert_status(303);
    current_of(&client)
        .await
        .assert_json_path("id", json!(gulati.id))
        .assert_json_path("resolver", json!("session"));

    // Someone who belongs to no workspace has none to be put in.
    current_of(&world.as_user(nobody))
        .await
        .assert_error_code(400, "workspace_required");
}

// A choice the user may no longer enter (they left it, or it
// was deleted) is forgotten and the next resolver answers: never a 403 on
// every page.
#[tokio::test]
async fn a_stale_choice_is_forgotten_and_the_next_resolver_answers() {
    let (world, _sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(grace, "Gulati").await;
    world.join(gulati.id, ada, "member").await;
    let client = world.as_user(ada);

    switch(&client, "gulati", "/").await.assert_status(303);
    current_of(&client)
        .await
        .assert_json_path("slug", json!("gulati"));
    let members = world.members().await;
    world
        .work(|uow| async move { members.remove(&uow, gulati.id, ada).await })
        .await
        .unwrap();
    client
        .get("/api/switching/current")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(agra.id))
        .assert_json_path("resolver", json!("header"));
    // Forgotten: without the header nothing is named any more.
    current_of(&client)
        .await
        .assert_error_code(400, "workspace_required");

    // A deleted workspace goes the same way.
    let doomed = world.workspace(ada, "Doomed").await;
    switch(&client, "doomed", "/").await.assert_status(303);
    let workspaces = world.workspaces().await;
    world
        .work(|uow| async move { workspaces.delete(&uow, doomed.id).await })
        .await
        .unwrap();
    current_of(&client)
        .await
        .assert_error_code(400, "workspace_required");
}

// A stolen session cookie carries no workspace across users or
// into a workspace its owner can't enter.
#[tokio::test]
async fn a_stolen_session_cookie_reaches_no_other_workspace() {
    let (world, _sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let mallory = world.user("Manjeet", "manjeet@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(grace, "Gulati").await;
    let victim = world.as_user(ada);
    switch(&victim, "agra", "/").await.assert_status(303);
    let stolen = victim
        .cookie(&session_cookie())
        .expect("the victim's cookie");

    // Another user replaying the cookie: Anu's choice is not theirs.
    let thief = world.as_user(mallory);
    thief.set_cookie(&session_cookie(), &stolen);
    current_of(&thief)
        .await
        .assert_error_code(400, "workspace_required");
    thief
        .get("/api/switching/current")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error_code(403, "not_a_member");
    thief
        .get("/api/switching/deals")
        .header("x-workspace", agra.id.to_string())
        .send()
        .await
        .assert_error_code(403, "not_a_member");

    // Replayed as Anu herself, it reaches Anu's workspaces only. The session's
    // choice is the first resolver that names one, so a header naming Gulati is
    // not read and the request stays in Agra; switching the session into Gulati
    // is refused.
    let replay = world.as_user(ada);
    replay.set_cookie(&session_cookie(), &stolen);
    current_of(&replay)
        .await
        .assert_json_path("id", json!(agra.id));
    replay
        .get("/api/switching/current")
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(agra.id))
        .assert_json_path("resolver", json!("session"));
    replay.csrf_cookie().await;
    switch(&replay, &gulati.id.to_string(), "/")
        .await
        .assert_error_code(403, "not_a_member");
    current_of(&replay)
        .await
        .assert_json_path("id", json!(agra.id));

    // Gauri signs in on Anu's browser (the same cookie jar): Anu's choice is
    // ignored, and Gauri reaches her own workspace only.
    let browser = world.app.client();
    let as_ada = browser.clone().acting_as(principal(ada));
    switch(&as_ada, "agra", "/").await.assert_status(303);
    let as_grace = browser.clone().acting_as(principal(grace));
    current_of(&as_grace)
        .await
        .assert_error_code(400, "workspace_required");
    as_grace
        .get("/api/switching/current")
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(gulati.id));
}

// Tables follow the switch: a workspace-scoped query lists the
// switched-to workspace's rows only, before and after switching.
#[tokio::test]
async fn tables_list_the_switched_workspace_rows_only() {
    let (world, _sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    let uow = world.db().await.uow(None, None).await;
    for (workspace, title) in [
        (agra.id, "Agra renewal"),
        (agra.id, "Agra upsell"),
        (gulati.id, "Gulati pilot"),
    ] {
        WorkspaceRepo::<Deal>::insert(&uow, workspace, new_deal(Uuid::nil(), title))
            .await
            .unwrap();
    }
    uow.commit().await.unwrap();
    let client = world.as_user(ada);

    switch(&client, "agra", "/").await.assert_status(303);
    client
        .get("/api/switching/deals")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace", json!("agra"))
        .assert_json_path("titles", json!(["Agra renewal", "Agra upsell"]));
    switch(&client, "gulati", "/").await.assert_status(303);
    client
        .get("/api/switching/deals")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace", json!("gulati"))
        .assert_json_path("titles", json!(["Gulati pilot"]));
}

// A stream opens inside the session's workspace at connect;
// after a switch, the next connection opens inside the new one.
#[tokio::test]
async fn streams_open_inside_the_switched_workspace() {
    let (world, _sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    let client = world.as_user(ada);

    switch(&client, "agra", "/").await.assert_status(303);
    let first = client
        .get("/api/switching/stream")
        .header("accept", "text/event-stream")
        .send()
        .await
        .assert_status(200);
    assert_eq!(first.header("content-type"), Some("text/event-stream"));
    assert!(first.text().contains(&format!("data: {}", agra.id)));
    assert!(!first.text().contains(&gulati.id.to_string()));

    switch(&client, "gulati", "/").await.assert_status(303);
    let second = client
        .get("/api/switching/stream")
        .header("accept", "text/event-stream")
        .send()
        .await
        .assert_status(200);
    assert!(second.text().contains(&format!("data: {}", gulati.id)));
    assert!(!second.text().contains(&agra.id.to_string()));
}

// The switcher lists the user's workspaces, marks the current one
// and posts to the configured route with the session's token.
#[tokio::test]
async fn the_switcher_lists_the_users_workspaces() {
    let sink = Arc::new(RecordingSink::default());
    let world = World::with(
        builder()
            .config("app.key", generate_key())
            .config("workspace.resolvers", json!(["session", "header"]))
            .config("workspace.switch_path", "/app/switch")
            .module(CryptoModule)
            .module(SessionModule::for_root(SessionOptions::default()))
            .module(ProbeModule)
            .override_value::<dyn AuditSink>(sink),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    let hidden = world.workspace(grace, "Ilaichi").await;
    let client = world.as_user(ada);
    client.csrf_cookie().await;
    client
        .post("/app/switch")
        .xsrf()
        .form(&[("workspace", "gulati")])
        .send()
        .await
        .assert_status(303);
    // The default path is not mounted when another is configured.
    client
        .post("/workspace/switch")
        .xsrf()
        .form(&[("workspace", "agra")])
        .send()
        .await
        .assert_status(404);

    let html = client
        .get("/api/switching/switcher?tab=open")
        .send()
        .await
        .assert_status(200)
        .text();
    assert_eq!(html.matches(r#"action="/app/switch""#).count(), 2, "{html}");
    assert!(
        html.contains(&format!(r#"name="workspace" value="{}""#, agra.id)),
        "{html}"
    );
    assert!(
        html.contains(&format!(r#"name="workspace" value="{}""#, gulati.id)),
        "{html}"
    );
    assert!(!html.contains(&hidden.id.to_string()), "{html}");
    assert!(!html.contains("Ilaichi"), "{html}");
    assert_eq!(html.matches(r#"aria-current="true""#).count(), 1, "{html}");
    let current = html.find(r#"aria-current="true""#).unwrap();
    assert!(
        html[current..].contains("Gulati"),
        "the current one is Gulati: {html}"
    );
    let token = client.cookie("XSRF-TOKEN").expect("the token cookie");
    assert!(
        html.contains(&format!(r#"name="_csrf" value="{token}""#)),
        "{html}"
    );
    assert!(
        html.contains(r#"name="next" value="/api/switching/switcher?tab=open""#),
        "{html}"
    );
}

// The route is named, and the choice lives under the documented key.
#[tokio::test]
async fn the_choice_is_kept_under_the_documented_key() {
    let (world, _sink) = world().await;
    let routes = navree_http::route_table(world.app.ctx()).await.unwrap();
    let route = routes
        .iter()
        .find(|route| route.full_path == "/workspace/switch")
        .expect("the switch route is mounted without the API prefix");
    assert_eq!(route.method, "POST");
    assert_eq!(route.name.as_deref(), Some(SWITCH_ROUTE));
    assert!(!route.access.public);
    assert_eq!(SESSION_KEY, "workspace.current");
    let choice = SessionChoice {
        id: Uuid::nil(),
        user: Uuid::nil(),
    };
    assert_eq!(
        serde_json::to_value(choice).unwrap(),
        json!({ "id": Uuid::nil(), "user": Uuid::nil() })
    );
}

/// Records `end_user_subscriptions` calls.
#[derive(Default)]
struct RecordingBroadcaster(Mutex<Vec<Uuid>>);

impl RecordingBroadcaster {
    fn ended(&self) -> Vec<Uuid> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Broadcaster for RecordingBroadcaster {
    fn broadcast(
        &self,
        _channels: Vec<String>,
        _event: String,
        _data: Value,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async { Ok(()) })
    }

    fn end_user_subscriptions(&self, user_id: Uuid) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(user_id);
        Box::pin(async { Ok(()) })
    }
}

// Live hub: the app's live hub finds a stream's workspace through
// the workspace module's zone source, the path `CurrentWorkspace` takes, so a
// page's stream resolves the workspace the browser switched to.
#[tokio::test]
async fn the_live_hubs_workspace_lookup_follows_the_switch() {
    let (world, _sink) = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    let client = world.as_user(ada);
    let hub = |client: &TestClient| {
        let request = client.get("/api/switching/hub-workspace");
        async move { request.send().await.assert_status(200).json_value()["workspace"].clone() }
    };

    assert_eq!(hub(&client).await, Value::Null, "nothing chosen yet");
    switch(&client, "agra", "/").await.assert_status(303);
    assert_eq!(hub(&client).await, json!(agra.id));
    switch(&client, "gulati", "/").await.assert_status(303);
    assert_eq!(hub(&client).await, json!(gulati.id));

    // Another user replaying Anu's cookie reaches none of her workspaces.
    let bob = world.user("Bal", "bal@example.com").await;
    let thief = world.as_user(bob);
    thief.set_cookie(
        &session_cookie(),
        &client.cookie(&session_cookie()).unwrap(),
    );
    assert_eq!(hub(&thief).await, Value::Null);
}

// Live hub: a switch ends the user's live subscriptions, so every
// open page reconnects inside the new workspace instead of keeping the old
// one's updates until its stream expires; choosing the same workspace again
// ends nothing.
#[tokio::test]
async fn a_switch_ends_the_users_live_subscriptions() {
    let broadcaster = Arc::new(RecordingBroadcaster::default());
    let world = World::with(
        builder()
            .config("app.key", generate_key())
            .config("workspace.resolvers", json!(["session", "header"]))
            .module(CryptoModule)
            .module(SessionModule::for_root(SessionOptions::default()))
            .module(ProbeModule)
            .override_value::<dyn Broadcaster>(broadcaster.clone()),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    world.workspace(ada, "Agra").await;
    world.workspace(ada, "Gulati").await;
    let client = world.as_user(ada);

    switch(&client, "agra", "/").await.assert_status(303);
    assert_eq!(broadcaster.ended(), [ada]);
    switch(&client, "agra", "/").await.assert_status(303);
    assert_eq!(
        broadcaster.ended(),
        [ada],
        "the same workspace again: nothing ends"
    );
    switch(&client, "gulati", "/").await.assert_status(303);
    assert_eq!(broadcaster.ended(), [ada, ada]);

    // A refused switch ends nothing.
    let bob = world.user("Bal", "bal@example.com").await;
    world.workspace(bob, "Ilaichi").await;
    switch(&client, "ilaichi", "/").await.assert_status(403);
    assert_eq!(broadcaster.ended(), [ada, ada]);
}
