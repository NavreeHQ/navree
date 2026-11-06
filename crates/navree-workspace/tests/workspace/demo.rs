//! Demo workspaces: created and seeded from fixtures by the registered demo
//! seeders, read-only or reset on a schedule, open to guests, and marked
//! with the `X-Navree-Demo` response header.

use std::sync::Arc;

use chrono::TimeZone;
use http::Method;
use navree_db::MigrationSet;
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_orm::prelude::*;
use navree_schedule::{ScheduleModule, Scheduler};
use navree_testing::TempDirGuard;
use navree_workspace::{
    CurrentWorkspace, DEMO_HEADER, DemoMode, DemoSeed, DemoSeeder, DemoWorkspaces, InWorkspace,
    ModuleBuilderWorkspaceExt, NewWorkspace, Workspace, WorkspaceError, WorkspaceKind,
    WorkspaceRepo, WorkspaceUow,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Deal, World, builder, new_deal, uuid};

/// A deal as the fixture file lists it.
#[derive(Deserialize)]
struct DealFixture {
    title: String,
}

/// Seeds the `deals` table from `deals.json`.
struct DealsDemo;

impl Injectable for DealsDemo {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DealsDemo) })
    }
}

impl DemoSeeder for DealsDemo {
    fn name(&self) -> &'static str {
        "deals"
    }

    fn tables(&self) -> &'static [&'static str] {
        &["deals"]
    }

    fn seed<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let deals: Vec<DealFixture> = demo.fixture("deals")?;
            for fixture in deals {
                WorkspaceRepo::<Deal>::insert(
                    demo.uow(),
                    demo.id(),
                    new_deal(demo.id(), &fixture.title),
                )
                .await?;
            }
            Ok(())
        })
    }
}

/// `/api/deals`, behind `InWorkspace`: list, create and a write-free
/// preview.
struct DealsController;

impl Injectable for DealsController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DealsController) })
    }
}

async fn index(ws: CurrentWorkspace, uow: WorkspaceUow) -> Result<Json<Value>, AppError> {
    let deals = WorkspaceRepo::<Deal>::query(ws.id())
        .get(&*uow)
        .await?
        .into_models();
    Ok(Json(json!({
        "demo": ws.is_demo(),
        "guest": ws.is_guest(),
        "role": ws.role().as_str(),
        "data": deals,
    })))
}

async fn store(
    ws: CurrentWorkspace,
    uow: WorkspaceUow,
    Json(input): Json<Value>,
) -> Result<Json<Deal>, AppError> {
    Ok(Json(
        WorkspaceRepo::<Deal>::insert(
            &*uow,
            ws.id(),
            new_deal(ws.id(), input["title"].as_str().unwrap_or_default()),
        )
        .await?
        .into_inner(),
    ))
}

async fn preview(_ws: CurrentWorkspace, Json(input): Json<Value>) -> Json<Value> {
    Json(json!({ "preview": input["title"] }))
}

impl Controller for DealsController {
    fn prefix(&self) -> &'static str {
        "/deals"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", index)
                .authenticated()
                .guard(InWorkspace::guard())
                .name("deals.index"),
            RouteDef::on(Method::POST, "/", store)
                .authenticated()
                .guard(InWorkspace::guard())
                .name("deals.store"),
            RouteDef::on(Method::POST, "/preview", preview)
                .authenticated()
                .guard(InWorkspace::guard())
                .name("deals.preview"),
        ]
    }
}

struct DemoModule;

impl Module for DemoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.demo_seeder::<DealsDemo>().controller::<DealsController>();
    }
}

const FIXTURE: &str = r#"[
    { "title": "Northwind renewal" },
    { "title": "Gulati pilot" },
    { "title": "Indrani expansion" }
]"#;

/// A directory with `deals.json`.
fn fixtures() -> TempDirGuard {
    let dir = TempDirGuard::new();
    std::fs::write(dir.path().join("deals.json"), FIXTURE).unwrap();
    dir
}

async fn world_with(
    dir: &TempDirGuard,
    configure: impl FnOnce(navree_testing::TestAppBuilder) -> navree_testing::TestAppBuilder,
) -> World {
    World::with(configure(
        builder()
            .config("workspace.demo.fixtures", dir.path().display().to_string())
            .module(DemoModule),
    ))
    .await
}

impl World {
    async fn demos(&self) -> Arc<DemoWorkspaces> {
        self.app.get::<DemoWorkspaces>().await
    }

    /// Creates demo workspace `name` owned by `owner` (seeded).
    async fn demo(&self, owner: Uuid, name: &str) -> Workspace {
        let demos = self.demos().await;
        let name = name.to_owned();
        self.work(|uow| async move { demos.create(&uow, owner, NewWorkspace::named(name)).await })
            .await
            .unwrap()
    }

    /// The titles of `workspace`'s deals, sorted.
    async fn deal_titles(&self, workspace: Uuid) -> Vec<String> {
        let mut titles: Vec<String> = WorkspaceRepo::<Deal>::query(workspace)
            .get(&*self.db().await)
            .await
            .unwrap()
            .into_models()
            .into_iter()
            .map(|deal| deal.title)
            .collect();
        titles.sort();
        titles
    }
}

fn seeded() -> Vec<String> {
    let mut titles: Vec<String> = ["Northwind renewal", "Gulati pilot", "Indrani expansion"]
        .map(str::to_owned)
        .to_vec();
    titles.sort();
    titles
}

#[tokio::test]
async fn demo_workspaces_are_created_and_seeded_from_fixtures() {
    let dir = fixtures();
    let world = world_with(&dir, |b| b).await;
    let mut events = world
        .events(&["workspace.created", "workspace.demo_seeded"])
        .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let live = world.workspace(ada, "Agra").await;
    let demo = world.demo(ada, "Agra demo").await;

    assert_eq!(live.kind(), WorkspaceKind::Live);
    assert!(!live.is_demo());
    assert_eq!(demo.kind(), WorkspaceKind::Demo);
    assert!(demo.is_demo());
    assert_eq!(demo.slug, "agra-demo");
    assert_eq!(world.deal_titles(demo.id).await, seeded());
    assert!(world.deal_titles(live.id).await.is_empty());

    let demos = world.demos().await;
    assert_eq!(demos.seeders().await.unwrap(), ["deals"]);
    assert_eq!(demos.mode(), DemoMode::ReadOnly);
    let all: Vec<Uuid> = demos.all().await.unwrap().iter().map(|w| w.id).collect();
    assert_eq!(all, [demo.id]);

    let received = events.drain();
    let seeded_event = received
        .iter()
        .find(|(name, _)| name == "workspace.demo_seeded")
        .map(|(_, payload)| payload.clone())
        .expect("a demo_seeded event");
    assert_eq!(seeded_event["workspace_id"], json!(demo.id));
    assert_eq!(seeded_event["slug"], json!("agra-demo"));
    assert_eq!(seeded_event["seeders"], json!(["deals"]));
    assert_eq!(seeded_event["reset"], json!(false));
}

#[tokio::test]
async fn read_only_demos_refuse_writes_and_mark_their_responses() {
    let dir = fixtures();
    let world = world_with(&dir, |b| b).await;
    let ada = world.user("Anu", "anu@example.com").await;
    world.workspace(ada, "Agra").await;
    let demo = world.demo(ada, "Agra demo").await;
    let client = world.as_user(ada);

    let read = client
        .get("/api/deals")
        .header("x-workspace", "agra-demo")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("demo", json!(true));
    assert_eq!(read.header(DEMO_HEADER), Some("read_only"));
    assert_eq!(read.json_value()["data"].as_array().unwrap().len(), 3);

    let refused = client
        .post("/api/deals")
        .header("x-workspace", "agra-demo")
        .json(&json!({ "title": "Sneaky" }))
        .send()
        .await
        .assert_error(403, "Demo workspaces are read-only.");
    assert_eq!(refused.header(DEMO_HEADER), Some("read_only"));
    assert_eq!(world.deal_titles(demo.id).await, seeded());

    // The workspace routes are writes too.
    client
        .patch("/api/workspaces/agra-demo")
        .json(&json!({ "name": "Renamed" }))
        .send()
        .await
        .assert_error(403, "Demo workspaces are read-only.");
    client
        .get("/api/workspaces/agra-demo")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("kind", json!("demo"))
        .assert_json_path("name", json!("Agra demo"));

    // A live workspace writes, and its responses carry no demo header.
    let live = client
        .post("/api/deals")
        .header("x-workspace", "agra")
        .json(&json!({ "title": "Real deal" }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(live.header(DEMO_HEADER), None);
    let outside = client
        .get("/api/workspaces")
        .send()
        .await
        .assert_status(200);
    assert_eq!(outside.header(DEMO_HEADER), None);
}

#[tokio::test]
async fn editor_roles_and_allowed_routes_write_to_read_only_demos() {
    let dir = fixtures();
    let world = world_with(&dir, |b| {
        b.config("workspace.demo.editor_roles", json!(["admin"]))
            .config(
                "workspace.demo.allow",
                "deals.preview, POST /api/workspaces/{workspace}/leave",
            )
    })
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let linus = world.user("Laksh", "laksh@example.com").await;
    let demo = world.demo(ada, "Agra demo").await;
    world.join(demo.id, grace, "admin").await;
    world.join(demo.id, linus, "member").await;

    // An editor writes.
    world
        .as_user(grace)
        .post("/api/deals")
        .header("x-workspace", "agra-demo")
        .json(&json!({ "title": "Curated deal" }))
        .send()
        .await
        .assert_status(200);
    // Others don't, except on the allowed routes (by name, or by method
    // and path).
    let member = world.as_user(linus);
    member
        .post("/api/deals")
        .header("x-workspace", "agra-demo")
        .json(&json!({ "title": "Nope" }))
        .send()
        .await
        .assert_error(403, "Demo workspaces are read-only.");
    member
        .post("/api/deals/preview")
        .header("x-workspace", "agra-demo")
        .json(&json!({ "title": "What if" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("preview", json!("What if"));
    member
        .post("/api/workspaces/agra-demo/leave")
        .send()
        .await
        .assert_status(204);
    let mut expected = seeded();
    expected.insert(0, "Curated deal".to_owned());
    expected.sort();
    assert_eq!(world.deal_titles(demo.id).await, expected);
}

#[tokio::test]
async fn reset_demos_accept_writes_until_the_reset_restores_the_fixtures() {
    let dir = fixtures();
    let world = world_with(&dir, |b| b.config("workspace.demo.mode", "reset")).await;
    let mut events = world.events(&["workspace.demo_seeded"]).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let live = world.workspace(ada, "Agra").await;
    let demo = world.demo(ada, "Agra demo").await;
    let client = world.as_user(ada);

    let written = client
        .post("/api/deals")
        .header("x-workspace", "agra-demo")
        .json(&json!({ "title": "Prospect's experiment" }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(written.header(DEMO_HEADER), Some("reset"));
    client
        .post("/api/deals")
        .header("x-workspace", "agra")
        .json(&json!({ "title": "Real deal" }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(world.deal_titles(demo.id).await.len(), 4);
    events.drain();

    world
        .app
        .run_command("workspace:demo-reset", &[])
        .await
        .unwrap();
    assert_eq!(world.deal_titles(demo.id).await, seeded());
    assert_eq!(world.deal_titles(live.id).await, ["Real deal"]);
    let received = events.drain();
    assert_eq!(received.len(), 1, "{received:?}");
    assert_eq!(received[0].1["reset"], json!(true));
    assert_eq!(received[0].1["seeders"], json!(["deals"]));

    // The service resets one demo by id.
    let demos = world.demos().await;
    let reset = demos.reset(demo.id).await.unwrap();
    assert_eq!(reset.workspace.id, demo.id);
    assert_eq!(reset.seeders, ["deals"]);
    assert_eq!(world.deal_titles(demo.id).await, seeded());
}

#[tokio::test]
async fn the_reset_command_takes_demo_slugs_only() {
    let dir = fixtures();
    let world = world_with(&dir, |b| b).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let live = world.workspace(ada, "Agra").await;
    world.demo(ada, "First demo").await;
    let second = world.demo(ada, "Second demo").await;

    // Read-only demos reset too, from the command.
    WorkspaceRepo::<Deal>::query(second.id)
        .delete(&*world.db().await)
        .await
        .unwrap();
    assert!(world.deal_titles(second.id).await.is_empty());
    world
        .app
        .run_command("workspace:demo-reset", &["second-demo"])
        .await
        .unwrap();
    assert_eq!(world.deal_titles(second.id).await, seeded());

    let error = world
        .app
        .run_command("workspace:demo-reset", &["agra"])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a demo workspace"), "{error}");
    let error = world
        .app
        .run_command("workspace:demo-reset", &["nowhere"])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("nowhere"), "{error}");
    let demos = world.demos().await;
    assert!(matches!(
        demos.reset(live.id).await,
        Err(WorkspaceError::NotDemo(_))
    ));
}

#[tokio::test]
async fn the_create_command_makes_a_seeded_demo() {
    let dir = fixtures();
    let world = world_with(&dir, |b| b).await;
    let ada = world.user("Anu", "anu@example.com").await;
    world
        .app
        .run_command(
            "workspace:demo-create",
            &[
                "Sales",
                "demo",
                "--owner",
                "anu@example.com",
                "--slug",
                "sales",
            ],
        )
        .await
        .unwrap();
    let demo = world.workspaces().await.resolve("sales").await.unwrap();
    assert_eq!(demo.name, "Sales demo");
    assert_eq!(demo.owner_id, ada);
    assert!(demo.is_demo());
    assert_eq!(world.deal_titles(demo.id).await, seeded());

    world
        .app
        .run_command(
            "workspace:demo-create",
            &["Other", "--owner", ada.to_string().as_str()],
        )
        .await
        .unwrap();
    let other = world.workspaces().await.resolve("other").await.unwrap();
    assert_eq!(other.owner_id, ada);

    for args in [
        &["No owner"][..],
        &["Unknown owner", "--owner", "nobody@example.com"][..],
        &["--owner", "anu@example.com"][..],
    ] {
        assert!(
            world
                .app
                .run_command("workspace:demo-create", args)
                .await
                .is_err(),
            "{args:?}"
        );
    }
}

#[tokio::test]
async fn guests_enter_demos_with_the_guest_role() {
    let dir = fixtures();
    let world = world_with(&dir, |b| b.config("workspace.demo.guest_role", "viewer")).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    world.workspace(ada, "Agra").await;
    let demo = world.demo(ada, "Agra demo").await;
    let guest = world.as_user(grace);

    let entered = guest
        .get("/api/deals")
        .header("x-workspace", "agra-demo")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("guest", json!(true))
        .assert_json_path("role", json!("viewer"));
    assert_eq!(entered.json_value()["data"].as_array().unwrap().len(), 3);
    assert_eq!(entered.header(DEMO_HEADER), Some("read_only"));
    // Guests can't enter live workspaces.
    guest
        .get("/api/deals")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.");
    // The list shows the demo, with the guest role.
    guest
        .get("/api/workspaces")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.slug", json!("agra-demo"))
        .assert_json_path("data.0.kind", json!("demo"))
        .assert_json_path("data.0.role", json!("viewer"));
    let listed = world
        .as_user(ada)
        .get("/api/workspaces")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let kinds: Vec<(&str, &str)> = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| (w["slug"].as_str().unwrap(), w["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(kinds, [("agra", "live"), ("agra-demo", "demo")]);
    // Guests write no more than members do.
    guest
        .post("/api/deals")
        .header("x-workspace", demo.id.to_string())
        .json(&json!({ "title": "Nope" }))
        .send()
        .await
        .assert_error(403, "Demo workspaces are read-only.");
    // They see the workspace, but not its members or invitations.
    guest
        .get("/api/workspaces/agra-demo")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("viewer"))
        .assert_json_path("kind", json!("demo"));
    for path in [
        "/api/workspaces/agra-demo/members",
        "/api/workspaces/agra-demo/invitations",
    ] {
        guest
            .get(path)
            .send()
            .await
            .assert_error(403, "Only members of this workspace can do this.");
    }
    world
        .as_user(ada)
        .get("/api/workspaces/agra-demo/members")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn reset_demo_guests_write_with_their_role_but_never_manage_the_workspace() {
    let dir = fixtures();
    let world = world_with(&dir, |b| {
        b.config("workspace.demo.mode", "reset")
            .config("workspace.demo.guest_role", "member")
    })
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let demo = world.demo(ada, "Agra demo").await;
    let guest = world.as_user(grace);
    guest
        .post("/api/deals")
        .header("x-workspace", "agra-demo")
        .json(&json!({ "title": "Guest's deal" }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(world.deal_titles(demo.id).await.len(), 4);
    guest
        .post("/api/workspaces/agra-demo/invitations")
        .json(&json!({ "email": "friend@example.com" }))
        .send()
        .await
        .assert_error(403, "Only members of this workspace can do this.");
    guest
        .post("/api/workspaces/agra-demo/leave")
        .send()
        .await
        .assert_error(403, "Only members of this workspace can do this.");
}

#[tokio::test]
async fn without_a_guest_role_demos_are_for_members() {
    let dir = fixtures();
    let world = world_with(&dir, |b| b).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    world.demo(ada, "Agra demo").await;
    world
        .as_user(grace)
        .get("/api/deals")
        .header("x-workspace", "agra-demo")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.");
    world
        .as_user(grace)
        .get("/api/workspaces")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
}

#[tokio::test]
async fn the_nightly_reset_runs_at_reset_at() {
    let dir = fixtures();
    let world = world_with(&dir, |b| {
        b.config("workspace.demo.mode", "reset")
            .config("workspace.demo.reset_at", "03:30")
            .module(ScheduleModule::for_root())
    })
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let demo = world.demo(ada, "Agra demo").await;
    world
        .as_user(ada)
        .post("/api/deals")
        .header("x-workspace", "agra-demo")
        .json(&json!({ "title": "Prospect's experiment" }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(world.deal_titles(demo.id).await.len(), 4);

    let scheduler = world.app.get::<Scheduler>().await;
    let tick = |hour: u32, minute: u32| {
        chrono::Utc
            .with_ymd_and_hms(2026, 10, 27, hour, minute, 0)
            .unwrap()
    };
    world.clock.set(tick(3, 29));
    scheduler.tick(tick(3, 29)).await.unwrap();
    assert_eq!(world.deal_titles(demo.id).await.len(), 4);
    world.clock.set(tick(3, 30));
    scheduler.tick(tick(3, 30)).await.unwrap();
    assert_eq!(world.deal_titles(demo.id).await, seeded());
}

#[tokio::test]
async fn read_only_demos_are_not_reset_on_the_schedule() {
    let dir = fixtures();
    let world = world_with(&dir, |b| {
        b.config("workspace.demo.reset_at", "03:30")
            .config("workspace.demo.editor_roles", "owner")
            .module(ScheduleModule::for_root())
    })
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let demo = world.demo(ada, "Agra demo").await;
    world
        .as_user(ada)
        .post("/api/deals")
        .header("x-workspace", "agra-demo")
        .json(&json!({ "title": "Curated" }))
        .send()
        .await
        .assert_status(200);
    let at = chrono::Utc
        .with_ymd_and_hms(2026, 10, 27, 3, 30, 0)
        .unwrap();
    world.clock.set(at);
    world.app.get::<Scheduler>().await.tick(at).await.unwrap();
    assert_eq!(world.deal_titles(demo.id).await.len(), 4);
}

#[tokio::test]
async fn a_missing_fixture_fails_the_seed_and_rolls_back() {
    let dir = TempDirGuard::new();
    let world = world_with(&dir, |b| b).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let demos = world.demos().await;
    let error = world
        .work(|uow| async move {
            demos
                .create(&uow, ada, NewWorkspace::named("Broken demo"))
                .await
        })
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("deals.json"), "{error}");
    assert!(error.contains("deals"), "{error}");
    assert!(
        world
            .workspaces()
            .await
            .find_by_slug("broken-demo")
            .await
            .unwrap()
            .is_none()
    );
}

/// Two seeders with one name.
struct DuplicateModule;

impl Module for DuplicateModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.demo_seeder::<DealsDemo>();
    }
}

#[tokio::test]
async fn seeder_names_must_be_unique() {
    let dir = fixtures();
    let world = world_with(&dir, |b| b.module(DuplicateModule)).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let demos = world.demos().await;
    let error = demos.seeders().await.unwrap_err().to_string();
    assert!(error.contains("`deals`"), "{error}");
    let error = world
        .work(|uow| async move { demos.create(&uow, ada, NewWorkspace::named("Demo")).await })
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("`deals`"), "{error}");
}

#[tokio::test]
async fn invalid_demo_options_fail_the_boot() {
    for (key, value, needle) in [
        ("workspace.demo.mode", json!("sometimes"), "workspace"),
        (
            "workspace.demo.reset_at",
            json!("25:00"),
            "workspace.demo.reset_at",
        ),
        (
            "workspace.demo.guest_role",
            json!("owner"),
            "workspace.demo.guest_role",
        ),
        (
            "workspace.demo.guest_role",
            json!("auditor"),
            "workspace.demo.guest_role",
        ),
        (
            "workspace.demo.editor_roles",
            json!(["auditor"]),
            "workspace.demo.editor_roles",
        ),
        (
            "workspace.demo.allow",
            json!(["POST deals"]),
            "workspace.demo.allow",
        ),
        ("workspace.demo.allow", json!([" "]), "workspace.demo.allow"),
        (
            "workspace.demo.fixtures",
            json!(" "),
            "workspace.demo.fixtures",
        ),
    ] {
        let error = builder()
            .config(key, value.clone())
            .try_build()
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(needle), "{key} = {value}: {error}");
    }
}

// ---------------------------------------------------------------------------
// Database isolation: the demo's data lives in its own database.

/// `notes(id, body)`, in every workspace's database.
#[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, serde::Serialize)]
#[model(table = "notes")]
struct Note {
    id: Uuid,
    body: String,
}

struct CreateNotes;

impl Migration for CreateNotes {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("notes", |t| {
                uuid(t, "id", dialect).primary();
                t.string("body");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("notes").await
    }
}

/// Seeds `notes` from an embedded fixture, with a dated placeholder.
struct NotesDemo;

impl Injectable for NotesDemo {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NotesDemo) })
    }
}

#[derive(Deserialize)]
struct NoteFixture {
    body: String,
    due: chrono::NaiveDate,
}

impl DemoSeeder for NotesDemo {
    fn name(&self) -> &'static str {
        "notes"
    }

    fn tables(&self) -> &'static [&'static str] {
        &["notes"]
    }

    fn seed<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let notes: Vec<NoteFixture> =
                demo.parse_fixture(r#"[{ "body": "Call back", "due": "{{ today + 2d }}" }]"#)?;
            for note in notes {
                Note::create(
                    demo.uow(),
                    Note {
                        id: Uuid::now_v7(),
                        body: format!("{} by {}", note.body, note.due),
                    },
                )
                .await?;
            }
            Ok(())
        })
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.workspace_migrations(MigrationSet::new("notes").migration(SchemaMigration::new(
            "m20261027_000001_create_notes",
            CreateNotes,
        )))
        .demo_seeder::<NotesDemo>();
    }
}

#[tokio::test]
async fn database_isolation_seeds_and_resets_the_demo_database() {
    let dir = TempDirGuard::new();
    let template = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("workspace-{id}.sqlite").display()
    );
    let world = World::with(
        builder()
            .config("workspace.isolation", "database")
            .config("workspace.database_url", template)
            .config("workspace.demo.mode", "reset")
            .module(NotesModule),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let demo = world.demo(ada, "Notes demo").await;
    let databases = world
        .app
        .get::<navree_workspace::WorkspaceDatabases>()
        .await;
    let notes = |db: Arc<navree_db::Db>| async move {
        Note::query()
            .get(db.as_ref())
            .await
            .unwrap()
            .into_models()
            .into_iter()
            .map(|note| note.body)
            .collect::<Vec<_>>()
    };
    let tenant = databases.db(demo.id).await.unwrap();
    // The clock starts on 2026-10-26.
    assert_eq!(notes(tenant.clone()).await, ["Call back by 2026-10-28"]);

    let uow = tenant.uow(None, None).await;
    Note::create(
        &uow,
        Note {
            id: Uuid::now_v7(),
            body: "Scribble".into(),
        },
    )
    .await
    .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(notes(tenant.clone()).await.len(), 2);

    world
        .clock
        .set(chrono::Utc.with_ymd_and_hms(2026, 11, 2, 3, 0, 0).unwrap());
    world.demos().await.reset(demo.id).await.unwrap();
    assert_eq!(notes(tenant).await, ["Call back by 2026-11-04"]);
}
