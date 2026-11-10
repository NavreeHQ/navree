//! PostgreSQL integration: row-level security through `navree.workspace`,
//! `navree.organization` and `navree.partition`, schema isolation through
//! `search_path`, and demo resets under both.
//!
//! Opt-in: build with `--features test-postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` (or start the test servers, which write
//! `target/test-databases.env`) to a superuser URL of a throwaway server, for
//! example `postgres://postgres:postgres@127.0.0.1:55432/postgres`. Each
//! test creates its own database and a non-superuser login role (superusers
//! bypass row-level security), and drops both at the end. Every model is a
//! navree-orm model: row-level security and schemas cover their queries.
#![cfg(feature = "test-postgres")]

use std::sync::Arc;

use http::Method;
use navree_access::{AccessModule, AccessOptions};
use navree_contracts::{ConformityOptions, ConformityReport};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{
    DatabaseModule, DbOptions, Executor, MigrationSet, ModuleBuilderDbExt, Pool, Statement,
    Value as DbValue,
};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{
    BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_orm::Model as _;
use navree_orm::factory::{Factory, FactoryContext, ModelFactory};
use navree_testing::{IsolationReport, TestApp, TestAppBuilder, principal};
use navree_workspace::testing::IsolationSuiteExt;
use navree_workspace::{
    CurrentOrganization, CurrentWorkspace, DemoSeed, DemoSeeder, DemoWorkspaces,
    ModuleBuilderWorkspaceExt, NewOrganization, NewPartition, NewWorkspace, OrgRepo, Organizations,
    PartitionScope, PartitionScoped, Partitions, RlsPolicy, WorkspaceModule, WorkspaceRepo,
    WorkspaceScoped, WorkspaceUow, Workspaces,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// `items(id, workspace_id, title)` with the row-level security policy (row
/// isolation), or in each workspace's schema.
#[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, serde::Serialize)]
#[model(table = "items")]
struct Item {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
}

impl WorkspaceScoped for Item {}

/// An item of `workspace` titled `title`, with a new id.
fn new_item(workspace: Uuid, title: &str) -> Item {
    Item {
        id: Uuid::now_v7(),
        workspace_id: workspace,
        title: title.to_owned(),
    }
}

/// `m20261026_000001_create_items`.
fn create_items() -> SchemaMigration<CreateItems> {
    SchemaMigration::new("m20261026_000001_create_items", CreateItems)
}

struct CreateItems;

impl Migration for CreateItems {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("items", |t| {
                t.uuid("id").primary();
                t.uuid("workspace_id");
                t.string("title");
            })
            .await?;
        schema
            .statement(&RlsPolicy::new("items").create_sql())
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("items").await
    }
}

struct ItemsController;

impl Injectable for ItemsController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ItemsController) })
    }
}

async fn store(
    ws: CurrentWorkspace,
    uow: WorkspaceUow,
    Json(input): Json<Value>,
) -> Result<Json<Item>, AppError> {
    let item = new_item(ws.id(), input["title"].as_str().unwrap_or_default());
    Ok(Json(Item::create(&*uow, item).await?.into_inner()))
}

/// Every row the transaction can see: no `WHERE`, so only the database's
/// isolation (the policy, or the schema) scopes it.
async fn index(_ws: CurrentWorkspace, uow: WorkspaceUow) -> Result<Json<Value>, AppError> {
    let items = Item::query().get(&*uow).await?.into_models();
    Ok(Json(json!({ "data": items })))
}

/// One item by id, again without a workspace filter.
async fn show(
    _ws: CurrentWorkspace,
    uow: WorkspaceUow,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    Ok(Json(Item::find_or_fail(&*uow, id).await?.into_inner()))
}

/// Items for isolation suites; the suite sets `workspace_id`.
#[derive(Default)]
struct ItemFactory;

impl ModelFactory for ItemFactory {
    type Model = Item;

    fn definition(&self, ctx: &mut FactoryContext<'_, Item>) -> Item {
        new_item(Uuid::nil(), &format!("Item {}", ctx.index()))
    }
}

/// The suites' factory of items.
fn items() -> Factory<ItemFactory> {
    Factory::new()
}

impl Controller for ItemsController {
    fn prefix(&self) -> &'static str {
        "/items"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", index).authenticated(),
            RouteDef::on(Method::POST, "/", store).authenticated(),
            RouteDef::on(Method::GET, "/{id}", show).authenticated(),
        ]
    }
}

/// Row isolation: `items` in the shared schema with the policy.
struct RowItemsModule;

impl Module for RowItemsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("items")
                .after(["workspace"])
                .migration(create_items()),
        )
        .controller::<ItemsController>();
    }
}

/// Schema isolation: `items` in every workspace's schema (the policy is
/// harmless there).
struct SchemaItemsModule;

impl Module for SchemaItemsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.workspace_migrations(MigrationSet::new("items").migration(create_items()))
            .controller::<ItemsController>();
    }
}

/// A fresh database owned by a fresh non-superuser role; dropped with it.
struct Scratch {
    admin: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn new() -> Option<Scratch> {
        // The variable, or `target/test-databases.env`.
        let admin = navree_orm::testing::database::TestServer::Postgres.url()?;
        let name = format!("navree_ws_{}", Uuid::now_v7().simple());
        run(
            &admin,
            &format!("CREATE ROLE {name} LOGIN PASSWORD 'secret' NOSUPERUSER NOBYPASSRLS"),
        )
        .await;
        run(&admin, &format!("CREATE DATABASE {name} OWNER {name}")).await;
        let rest = admin.split_once('@').unwrap().1;
        let host = rest.split('/').next().unwrap();
        let url = format!("postgres://{name}:secret@{host}/{name}");
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        run(
            &self.admin,
            &format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name),
        )
        .await;
        run(&self.admin, &format!("DROP ROLE IF EXISTS {}", self.name)).await;
    }
}

/// Runs `sql` on the server of `admin`.
async fn run(admin: &str, sql: &str) {
    let pool = Pool::connect(&DbOptions {
        url: admin.to_owned(),
        max_connections: 1,
        ..DbOptions::default()
    })
    .await
    .unwrap();
    pool.execute_script(sql).await.unwrap();
    pool.close().await;
}

/// A test app on `scratch`.
fn builder(scratch: &Scratch) -> TestAppBuilder {
    TestApp::builder().config("database.url", scratch.url.clone())
}

/// The single row of `sql`'s column `column`, read in `uow`.
async fn scalar<T: navree_db::FromValue>(uow: &navree_db::Uow, sql: &str, column: &str) -> T {
    uow.fetch_one(&Statement::text(sql))
        .await
        .unwrap()
        .get::<T>(column)
        .unwrap()
}

async fn app(scratch: &Scratch, isolation: &str, items: impl Module) -> TestApp {
    builder(scratch)
        .config("database.migrate_on_boot", true)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.isolation", isolation)
        .config("workspace.users_table", "")
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(items)
        .module(WorkspaceModule::from_config())
        .build()
        .await
}

async fn create(app: &TestApp, owner: Uuid, name: &str) -> navree_workspace::Workspace {
    let workspaces = app.get::<Workspaces>().await;
    let uow = app.get::<navree_db::Db>().await.uow(None, None).await;
    let created = workspaces
        .create(&uow, owner, NewWorkspace::named(name))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    created
}

#[tokio::test]
async fn row_level_security_scopes_every_query_to_the_request_workspace() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch, "row", RowItemsModule).await;
    let ada = Uuid::now_v7();
    let agra = create(&app, ada, "Agra").await;
    let gulati = create(&app, ada, "Gulati").await;
    let client = app.acting_as(principal(ada));

    client
        .post("/api/items")
        .header("x-workspace", "agra")
        .json(&json!({ "title": "Agra item" }))
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/items")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_json_path("data.0.title", json!("Agra item"));
    client
        .get("/api/items")
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));

    let db = app.get::<navree_db::Db>().await;
    let workspaces = app.get::<Workspaces>().await;
    let count = |uow: navree_db::Uow| async move {
        scalar::<i64>(&uow, "SELECT count(*)::bigint AS n FROM items", "n").await
    };
    // No workspace: the policy hides every row.
    assert_eq!(count(db.uow(None, None).await).await, 0);
    // A job enters a workspace explicitly.
    let uow = db.uow(None, None).await;
    workspaces.enter(&uow, gulati.id).await.unwrap();
    assert_eq!(count(uow.clone()).await, 0);
    uow.rollback().await.unwrap();
    let uow = db.uow(None, None).await;
    workspaces.enter(&uow, agra.id).await.unwrap();
    assert_eq!(count(uow.clone()).await, 1);
    // Writing another workspace's row fails the policy check.
    let refused = Item::create(&uow, new_item(gulati.id, "Smuggled")).await;
    assert!(refused.is_err(), "{refused:?}");
    uow.rollback().await.unwrap();
    // WorkspaceRepo scopes on its own too.
    let uow = db.uow(None, None).await;
    workspaces.enter(&uow, agra.id).await.unwrap();
    assert_eq!(
        WorkspaceRepo::<Item>::query(gulati.id)
            .count(&uow)
            .await
            .unwrap(),
        0
    );
    uow.rollback().await.unwrap();

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

#[tokio::test]
async fn schema_isolation_gives_each_workspace_its_own_tables() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch, "schema", SchemaItemsModule).await;
    let ada = Uuid::now_v7();
    let agra = create(&app, ada, "Agra").await;
    create(&app, ada, "Gulati").await;
    let client = app.acting_as(principal(ada));

    client
        .post("/api/items")
        .header("x-workspace", "agra")
        .json(&json!({ "title": "In Agra's schema" }))
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/items")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_json_path("data.0.title", json!("In Agra's schema"));
    client
        .get("/api/items")
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));

    let db = app.get::<navree_db::Db>().await;
    let schemas: Vec<String> = db
        .fetch_all(&Statement::text(
            "SELECT table_schema::text AS s FROM information_schema.tables WHERE table_name = 'items' ORDER BY 1",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String>("s").unwrap())
        .collect();
    assert_eq!(schemas.len(), 2, "{schemas:?}");
    assert!(schemas.contains(&format!("workspace_{}", agra.id.simple())));

    // Deleting the workspace drops its schema.
    let workspaces = app.get::<Workspaces>().await;
    let uow = db.uow(None, None).await;
    workspaces.delete(&uow, agra.id).await.unwrap();
    uow.commit().await.unwrap();
    let left = db
        .fetch_optional(&Statement::portable(
            "SELECT 1 AS found FROM information_schema.schemata WHERE schema_name = ?",
            [DbValue::from(format!("workspace_{}", agra.id.simple()))],
        ))
        .await
        .unwrap();
    assert!(left.is_none());

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

/// `notes(id, workspace_id, partition_id, body)` with a policy that lets
/// organization requests read and narrows partition requests.
#[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, serde::Serialize)]
#[model(table = "notes")]
struct Note {
    id: Uuid,
    workspace_id: Uuid,
    partition_id: Option<Uuid>,
    body: String,
}

impl WorkspaceScoped for Note {}

impl PartitionScoped for Note {}

/// A note of `workspace` and `partition` saying `body`, with a new id.
fn new_note(workspace: Uuid, partition: Option<Uuid>, body: &str) -> Note {
    Note {
        id: Uuid::now_v7(),
        workspace_id: workspace,
        partition_id: partition,
        body: body.to_owned(),
    }
}

struct CreateNotes;

impl Migration for CreateNotes {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("notes", |t| {
                t.uuid("id").primary();
                t.uuid("workspace_id");
                t.uuid("partition_id").nullable();
                t.string("body");
            })
            .await?;
        schema
            .statement(
                &RlsPolicy::new("notes")
                    .organizations()
                    .partitioned()
                    .create_sql(),
            )
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("notes").await
    }
}

struct NotesController;

impl Injectable for NotesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NotesController) })
    }
}

/// Every note the transaction can see: no `WHERE`, so only the policy
/// scopes it.
async fn every_note(uow: &navree_db::Uow) -> Result<Value, AppError> {
    let mut bodies: Vec<String> = Note::query()
        .get(uow)
        .await?
        .into_models()
        .into_iter()
        .map(|note| note.body)
        .collect();
    bodies.sort();
    Ok(json!(bodies))
}

async fn org_notes(
    _org: CurrentOrganization,
    uow: navree_db::Uow,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "data": every_note(&uow).await? })))
}

async fn partition_notes(
    _scope: PartitionScope,
    uow: navree_db::Uow,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "data": every_note(&uow).await? })))
}

impl Controller for NotesController {
    fn prefix(&self) -> &'static str {
        "/notes"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/organization", org_notes).authenticated(),
            RouteDef::on(Method::GET, "/partition", partition_notes).authenticated(),
        ]
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").after(["workspace"]).migration(
            SchemaMigration::new("m20261026_000001_create_notes", CreateNotes),
        ))
        .controller::<NotesController>();
    }
}

async fn org_app(scratch: &Scratch) -> TestApp {
    builder(scratch)
        .config("database.migrate_on_boot", true)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.users_table", "")
        .config("workspace.organizations.enabled", true)
        .config("workspace.partitions.enabled", true)
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(NotesModule)
        .module(WorkspaceModule::from_config())
        .build()
        .await
}

/// Inserts a note as a job would: inside its workspace.
async fn note(app: &TestApp, workspace: Uuid, partition: Option<Uuid>, body: &str) {
    let db = app.get::<navree_db::Db>().await;
    let uow = db.uow(None, None).await;
    app.get::<Workspaces>()
        .await
        .enter(&uow, workspace)
        .await
        .unwrap();
    Note::create(&uow, new_note(workspace, partition, body))
        .await
        .unwrap();
    uow.commit().await.unwrap();
}

#[tokio::test]
async fn row_level_security_lets_organization_requests_read_their_workspaces_only() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = org_app(&scratch).await;
    let ada = Uuid::now_v7();
    let bob = Uuid::now_v7();
    let organizations = app.get::<Organizations>().await;
    let db = app.get::<navree_db::Db>().await;
    let uow = db.uow(None, None).await;
    let a = organizations
        .create(&uow, ada, NewOrganization::named("Org A"))
        .await
        .unwrap();
    let b = organizations
        .create(&uow, bob, NewOrganization::named("Org B"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let a1 = create(&app, ada, "A One").await;
    let a2 = create(&app, ada, "A Two").await;
    let b1 = create(&app, bob, "B One").await;
    let loose = create(&app, ada, "Loose").await;
    let uow = db.uow(None, None).await;
    for (org, workspace) in [(a.id, a1.id), (a.id, a2.id), (b.id, b1.id)] {
        organizations.attach(&uow, org, workspace).await.unwrap();
    }
    uow.commit().await.unwrap();
    note(&app, a1.id, None, "a1").await;
    note(&app, a2.id, None, "a2").await;
    note(&app, b1.id, None, "b1").await;
    note(&app, loose.id, None, "loose").await;

    // The policy alone keeps each organization to its workspaces.
    app.acting_as(principal(ada))
        .get("/api/notes/organization")
        .header("x-organization", "org-a")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!(["a1", "a2"]));
    app.acting_as(principal(bob))
        .get("/api/notes/organization")
        .header("x-organization", "org-b")
        .send()
        .await
        .assert_json_path("data", json!(["b1"]));

    // Jobs enter an organization; it reads, but writes need a workspace.
    let uow = db.uow(None, None).await;
    organizations.enter(&uow, a.id).await.unwrap();
    assert_eq!(every_note(&uow).await.unwrap(), json!(["a1", "a2"]));
    let refused = Note::create(&uow, new_note(a1.id, None, "org-level write")).await;
    assert!(refused.is_err(), "{refused:?}");
    uow.rollback().await.unwrap();
    // Nor can it update or delete the rows it reads.
    let uow = db.uow(None, None).await;
    organizations.enter(&uow, a.id).await.unwrap();
    let deleted = uow
        .execute(&Statement::text("DELETE FROM notes"))
        .await
        .unwrap();
    assert_eq!(deleted.rows_affected, 0);
    let updated = uow
        .execute(&Statement::text("UPDATE notes SET body = 'changed'"))
        .await
        .unwrap();
    assert_eq!(updated.rows_affected, 0);
    assert_eq!(every_note(&uow).await.unwrap(), json!(["a1", "a2"]));
    uow.rollback().await.unwrap();
    // OrgRepo narrows further, to the workspaces a member reaches.
    let uow = db.uow(None, None).await;
    organizations.enter(&uow, a.id).await.unwrap();
    let only_a2 = OrgRepo::<Note>::query(&[a2.id, b1.id])
        .get(&uow)
        .await
        .unwrap()
        .into_models();
    assert_eq!(
        only_a2.iter().map(|n| n.body.as_str()).collect::<Vec<_>>(),
        ["a2"]
    );
    uow.rollback().await.unwrap();

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

#[tokio::test]
async fn row_level_security_narrows_partition_requests() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = org_app(&scratch).await;
    let ada = Uuid::now_v7();
    let agra = create(&app, ada, "Agra").await;
    let partitions = app.get::<Partitions>().await;
    let db = app.get::<navree_db::Db>().await;
    let uow = db.uow(None, None).await;
    let uk = partitions
        .create(&uow, agra.id, NewPartition::named("agra.co.uk"))
        .await
        .unwrap();
    let de = partitions
        .create(&uow, agra.id, NewPartition::named("agra.de"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    note(&app, agra.id, Some(uk.id), "uk").await;
    note(&app, agra.id, Some(de.id), "de").await;
    note(&app, agra.id, None, "unsorted").await;
    let client = app.acting_as(principal(ada));

    client
        .get("/api/notes/partition")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!(["de", "uk", "unsorted"]));
    client
        .get("/api/notes/partition")
        .header("x-workspace", "agra")
        .header("x-partition", "agra-co-uk")
        .send()
        .await
        .assert_json_path("data", json!(["uk"]));

    // Inside a partition, rows of another partition can't be written.
    let uow = db.uow(None, None).await;
    app.get::<Workspaces>()
        .await
        .enter(&uow, agra.id)
        .await
        .unwrap();
    partitions.enter(&uow, uk.id).await.unwrap();
    assert_eq!(every_note(&uow).await.unwrap(), json!(["uk"]));
    let refused = Note::create(&uow, new_note(agra.id, Some(de.id), "smuggled")).await;
    assert!(refused.is_err(), "{refused:?}");
    uow.rollback().await.unwrap();

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

/// `memos(id, workspace_id)`: indexed by workspace, without the policy.
#[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, serde::Serialize)]
#[model(table = "memos")]
struct Memo {
    id: Uuid,
    workspace_id: Uuid,
}

impl WorkspaceScoped for Memo {}

struct CreateMemos;

impl Migration for CreateMemos {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("memos", |t| {
                t.uuid("id").primary();
                t.uuid("workspace_id");
                t.index("workspace_id").name("memos_workspace_id_idx");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("memos").await
    }
}

/// `items` (policy, no workspace index) and `memos` (index, no policy),
/// both classified as workspace entities.
struct ClassifiedModule;

impl Module for ClassifiedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("classified")
                .after(["workspace"])
                .migration(create_items())
                .migration(SchemaMigration::new(
                    "m20261026_000002_create_memos",
                    CreateMemos,
                )),
        )
        .workspace_entity::<Item>()
        .workspace_entity::<Memo>();
    }
}

/// `workspace.tables` reads indexes and row-level security from the
/// PostgreSQL catalog.
#[tokio::test]
async fn workspace_tables_reads_indexes_and_row_level_security_from_the_catalog() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch, "row", ClassifiedModule).await;
    let report = ConformityReport::run(
        app.ctx(),
        &ConformityOptions::new(".").only(["workspace.tables".to_owned()]),
    )
    .await;
    let check = report.check("workspace.tables").unwrap();
    assert_eq!(check.outcome.checked, 2);
    let findings: Vec<(&str, &str)> = check
        .outcome
        .findings
        .iter()
        .map(|f| (f.subject.as_str(), f.message.as_str()))
        .collect();
    assert_eq!(
        findings,
        [
            ("items", "no index starts with `workspace_id`"),
            ("memos", "row-level security is off"),
        ]
    );

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

/// `GET /api/items/{id}: ok: 404 ...` for every route of a report.
fn lines(report: &IsolationReport) -> Vec<String> {
    report
        .routes
        .iter()
        .map(|r| {
            let detail = match &r.outcome {
                navree_testing::IsolationOutcome::Passed(d) => format!("ok: {d}"),
                navree_testing::IsolationOutcome::Failed(d) => format!("FAIL: {d}"),
                navree_testing::IsolationOutcome::Skipped(d) => format!("skip: {d}"),
            };
            format!("{} {}: {detail}", r.method, r.path)
        })
        .collect()
}

const PASSING: [&str; 3] = [
    "GET /api/items: ok: another workspace's records are not listed",
    "POST /api/items: ok: 200; the record stays in the caller's workspace",
    "GET /api/items/{id}: ok: 404 from another workspace, 200 in its own",
];

/// The id of the workspace the suite named `name`.
async fn workspace_named(app: &TestApp, name: &str) -> Uuid {
    let db = app.get::<navree_db::Db>().await;
    let row = db
        .fetch_one(&Statement::portable(
            "SELECT id FROM workspaces WHERE name = ?",
            [DbValue::from(name)],
        ))
        .await
        .unwrap();
    row.get::<Uuid>("id").unwrap()
}

/// The handlers read by id without a workspace filter: row-level security
/// alone keeps the suite's workspaces apart, and the suite's records pass
/// the policy because they are inserted inside their workspace.
#[tokio::test]
async fn isolation_suites_pass_on_row_level_security_alone() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch, "row", RowItemsModule).await;
    let report = app
        .isolation_suite::<RowItemsModule>()
        .factory(items())
        .body("POST", "/api/items", json!({ "title": "Renewal" }))
        .run()
        .await;
    assert_eq!(lines(&report), PASSING);

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

/// Under schema isolation the suite's records go into each workspace's
/// schema, with the workspace column filled when the table has one.
#[tokio::test]
async fn isolation_suites_pass_under_schema_isolation() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch, "schema", SchemaItemsModule).await;
    let report = app
        .isolation_suite::<SchemaItemsModule>()
        .factory(items())
        .body("POST", "/api/items", json!({ "title": "Renewal" }))
        .run()
        .await;
    assert_eq!(lines(&report), PASSING);

    // Inside B (the policy on `items` hides rows outside a workspace).
    let b = workspace_named(&app, "Isolation B").await;
    let uow = app.get::<navree_db::Db>().await.uow(None, None).await;
    app.get::<Workspaces>().await.enter(&uow, b).await.unwrap();
    let owners: Vec<Uuid> = uow
        .fetch_all(&Statement::text(format!(
            "SELECT workspace_id AS w FROM workspace_{}.items",
            b.simple()
        )))
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<Uuid>("w").unwrap())
        .collect();
    uow.rollback().await.unwrap();
    assert!(!owners.is_empty());
    assert!(owners.iter().all(|w| *w == b), "{owners:?} in {b}");

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

/// `items` as ORM models: one without the ORM's workspace scope, so only
/// row-level security (or the schema) keeps workspaces apart, and one with
/// it.
mod orm_items {
    use navree_orm::Model;
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Model, Clone, Debug, Default, Serialize)]
    #[model(table = "items", key_type = "manual")]
    pub struct PlainItem {
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub title: String,
    }

    #[derive(Model, Clone, Debug, Default, Serialize)]
    #[model(table = "items", workspace)]
    pub struct ScopedItem {
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub title: String,
    }
}

/// A unit of work, inside workspace `id` when given.
async fn uow_in(app: &TestApp, id: Option<Uuid>) -> navree_db::Uow {
    let uow = app.get::<navree_db::Db>().await.uow(None, None).await;
    if let Some(id) = id {
        app.get::<Workspaces>().await.enter(&uow, id).await.unwrap();
    }
    uow
}

/// ORM queries inside a workspace transaction: row-level security
/// (`navree.workspace`) filters them even without the ORM's own scope, and
/// refuses writes into another workspace.
#[tokio::test]
async fn row_level_security_covers_orm_queries() {
    use navree_orm::Model;
    use orm_items::{PlainItem, ScopedItem};

    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch, "row", RowItemsModule).await;
    let ada = Uuid::now_v7();
    let agra = create(&app, ada, "Agra").await;
    let gulati = create(&app, ada, "Gulati").await;

    let uow = uow_in(&app, Some(agra.id)).await;
    let created = PlainItem::create(
        &uow,
        PlainItem {
            id: Uuid::now_v7(),
            workspace_id: agra.id,
            title: "Agra item".into(),
        },
    )
    .await
    .unwrap();
    ScopedItem::create(
        &uow,
        ScopedItem {
            title: "Filled by the ORM".into(),
            ..ScopedItem::default()
        },
    )
    .await
    .unwrap();
    uow.commit().await.unwrap();

    let uow = uow_in(&app, Some(agra.id)).await;
    assert_eq!(PlainItem::query().count(&uow).await.unwrap(), 2);
    assert_eq!(ScopedItem::query().count(&uow).await.unwrap(), 2);
    uow.rollback().await.unwrap();

    // Another workspace: the policy hides the rows from every ORM read.
    let uow = uow_in(&app, Some(gulati.id)).await;
    assert_eq!(PlainItem::query().count(&uow).await.unwrap(), 0);
    assert!(PlainItem::find(&uow, created.id).await.unwrap().is_none());
    assert_eq!(
        ScopedItem::query()
            .without_workspace_scope()
            .count(&uow)
            .await
            .unwrap(),
        0,
        "without the ORM's scope, still nothing: the policy's"
    );
    // ... and refuses an ORM write into Agra.
    let smuggled = PlainItem::create(
        &uow,
        PlainItem {
            id: Uuid::now_v7(),
            workspace_id: agra.id,
            title: "Smuggled".into(),
        },
    )
    .await;
    assert!(smuggled.is_err(), "{smuggled:?}");
    uow.rollback().await.unwrap();

    // Outside any workspace nothing is visible.
    let uow = uow_in(&app, None).await;
    assert_eq!(PlainItem::query().count(&uow).await.unwrap(), 0);
    uow.rollback().await.unwrap();

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

/// Under schema isolation ORM queries in a workspace transaction read the
/// workspace's schema (its `search_path`).
#[tokio::test]
async fn orm_queries_follow_the_workspace_schema() {
    use navree_orm::Model;
    use orm_items::PlainItem;

    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch, "schema", SchemaItemsModule).await;
    let ada = Uuid::now_v7();
    let agra = create(&app, ada, "Agra").await;
    let gulati = create(&app, ada, "Gulati").await;

    let uow = uow_in(&app, Some(agra.id)).await;
    PlainItem::create(
        &uow,
        PlainItem {
            id: Uuid::now_v7(),
            workspace_id: agra.id,
            title: "In Agra's schema".into(),
        },
    )
    .await
    .unwrap();
    uow.commit().await.unwrap();

    for (workspace, expected) in [(agra.id, 1), (gulati.id, 0)] {
        let uow = uow_in(&app, Some(workspace)).await;
        assert_eq!(PlainItem::query().count(&uow).await.unwrap(), expected);
        uow.rollback().await.unwrap();
    }
    // The row is in Agra's schema (read inside Agra: the table's policy
    // hides rows outside a workspace).
    let uow = uow_in(&app, Some(agra.id)).await;
    let sql = format!(
        "SELECT count(*)::bigint AS n FROM workspace_{}.items",
        agra.id.simple()
    );
    assert_eq!(scalar::<i64>(&uow, &sql, "n").await, 1);
    uow.rollback().await.unwrap();

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

/// Seeds two items into a demo workspace.
struct ItemsDemo;

impl Injectable for ItemsDemo {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ItemsDemo) })
    }
}

#[derive(serde::Deserialize)]
struct ItemFixture {
    title: String,
}

impl DemoSeeder for ItemsDemo {
    fn name(&self) -> &'static str {
        "items"
    }

    fn tables(&self) -> &'static [&'static str] {
        &["items"]
    }

    fn seed<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let items: Vec<ItemFixture> =
                demo.parse_fixture(r#"[{ "title": "Demo one" }, { "title": "Demo two" }]"#)?;
            for fixture in items {
                WorkspaceRepo::<Item>::insert(
                    demo.uow(),
                    demo.id(),
                    new_item(demo.id(), &fixture.title),
                )
                .await?;
            }
            Ok(())
        })
    }
}

/// `items` (row or schema isolation) with the demo seeder.
struct DemoItemsModule<M>(M);

impl<M: Module> Module for DemoItemsModule<M> {
    fn register(&self, m: &mut ModuleBuilder) {
        self.0.register(m);
        m.demo_seeder::<ItemsDemo>();
    }
}

/// The titles of `workspace`'s items, read in a transaction that entered it.
async fn titles(app: &TestApp, workspace: Uuid) -> Vec<String> {
    let db = app.get::<navree_db::Db>().await;
    let uow = db.uow(None, None).await;
    app.get::<Workspaces>()
        .await
        .enter(&uow, workspace)
        .await
        .unwrap();
    let mut titles: Vec<String> = Item::query()
        .get(&uow)
        .await
        .unwrap()
        .into_models()
        .into_iter()
        .map(|item| item.title)
        .collect();
    uow.rollback().await.unwrap();
    titles.sort();
    titles
}

async fn demo_workspace(app: &TestApp, owner: Uuid) -> navree_workspace::Workspace {
    let demos = app.get::<DemoWorkspaces>().await;
    let uow = app.get::<navree_db::Db>().await.uow(None, None).await;
    let created = demos
        .create(&uow, owner, NewWorkspace::named("Demo"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    created
}

/// Seeds a demo, writes to it and to a live workspace, resets the demo and
/// checks that only the demo's rows changed.
async fn seed_write_and_reset(app: &TestApp) {
    let ada = Uuid::now_v7();
    let live = create(app, ada, "Agra").await;
    let demo = demo_workspace(app, ada).await;
    assert_eq!(titles(app, demo.id).await, ["Demo one", "Demo two"]);
    assert!(titles(app, live.id).await.is_empty());

    let client = app.acting_as(principal(ada));
    for (workspace, title) in [("agra", "Real"), ("demo", "Experiment")] {
        let response = client
            .post("/api/items")
            .header("x-workspace", workspace)
            .json(&json!({ "title": title }))
            .send()
            .await
            .assert_status(200);
        let expected = (workspace == "demo").then_some("reset");
        assert_eq!(response.header("x-navree-demo"), expected);
    }
    assert_eq!(
        titles(app, demo.id).await,
        ["Demo one", "Demo two", "Experiment"]
    );

    let reset = app
        .get::<DemoWorkspaces>()
        .await
        .reset(demo.id)
        .await
        .unwrap();
    assert_eq!(reset.seeders, ["items"]);
    assert_eq!(titles(app, demo.id).await, ["Demo one", "Demo two"]);
    assert_eq!(titles(app, live.id).await, ["Real"]);
}

#[tokio::test]
async fn demo_resets_clear_only_the_demo_rows_under_row_level_security() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = demo_app(&scratch, "row", DemoItemsModule(RowItemsModule)).await;
    seed_write_and_reset(&app).await;
    app.shutdown().await.unwrap();
    scratch.drop().await;
}

#[tokio::test]
async fn demo_resets_clear_the_demo_schema_under_schema_isolation() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = demo_app(&scratch, "schema", DemoItemsModule(SchemaItemsModule)).await;
    seed_write_and_reset(&app).await;
    app.shutdown().await.unwrap();
    scratch.drop().await;
}

async fn demo_app(scratch: &Scratch, isolation: &str, items: impl Module) -> TestApp {
    builder(scratch)
        .config("database.migrate_on_boot", true)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.isolation", isolation)
        .config("workspace.users_table", "")
        .config("workspace.demo.mode", "reset")
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(items)
        .module(WorkspaceModule::from_config())
        .build()
        .await
}
