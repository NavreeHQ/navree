//! PostgreSQL integration: row-level security through `navree.workspace`
//! and schema isolation through `search_path`.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a superuser URL of a throwaway server, for
//! example `postgres://postgres:postgres@127.0.0.1:55432/postgres`. Each
//! test creates its own database and a non-superuser login role (superusers
//! bypass row-level security), and drops both at the end.
#![cfg(feature = "postgres")]

use std::sync::Arc;

use axum::extract::Path;
use http::Method;
use navree_access::{AccessModule, AccessOptions};
use navree_contracts::{ConformityOptions, ConformityReport};
use navree_db::{DatabaseModule, Factory, MigrationSet, ModuleBuilderDbExt, Repo};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{IsolationReport, TestApp, principal};
use navree_workspace::testing::IsolationSuiteExt;
use navree_workspace::{
    CurrentWorkspace, ModuleBuilderWorkspaceExt, NewWorkspace, RlsPolicy, WorkspaceModule,
    WorkspaceRepo, WorkspaceScoped, WorkspaceUow, Workspaces,
};
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm::{
    ActiveValue::Set, ConnectionTrait, Database, DbBackend, DbErr, EntityTrait, Statement,
};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::{Value, json};
use uuid::Uuid;

/// `items(id, workspace_id, title)` with the row-level security policy (row
/// isolation), or `items(id, title)` in each workspace's schema.
mod item {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "items")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub title: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl WorkspaceScoped for item::Entity {}

struct CreateItems;

impl MigrationName for CreateItems {
    fn name(&self) -> &str {
        "m20261026_000001_create_items"
    }
}

#[async_trait]
impl MigrationTrait for CreateItems {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("items")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("title").string().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(&RlsPolicy::new("items").create_sql())
            .await
            .map(drop)
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("items").to_owned())
            .await
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
) -> Result<Json<item::Model>, AppError> {
    Ok(Json(
        Repo::<item::Entity>::insert(
            &uow,
            item::ActiveModel {
                id: Set(Uuid::now_v7()),
                workspace_id: Set(ws.id()),
                title: Set(input["title"].as_str().unwrap_or_default().to_owned()),
            },
        )
        .await?,
    ))
}

/// Every row the transaction can see: no `WHERE`, so only the database's
/// isolation (the policy, or the schema) scopes it.
async fn index(_ws: CurrentWorkspace, uow: WorkspaceUow) -> Result<Json<Value>, AppError> {
    let items = item::Entity::find().all(&*uow.tx().await?).await?;
    Ok(Json(json!({ "data": items })))
}

/// One item by id, again without a workspace filter.
async fn show(
    _ws: CurrentWorkspace,
    uow: WorkspaceUow,
    Path(id): Path<Uuid>,
) -> Result<Json<item::Model>, AppError> {
    Ok(Json(Repo::<item::Entity>::find_or_fail(&uow, id).await?))
}

/// Items for isolation suites; the suite sets `workspace_id`.
struct ItemFactory;

impl Factory for ItemFactory {
    type Entity = item::Entity;

    fn definition(&self, n: usize) -> item::ActiveModel {
        item::ActiveModel {
            id: Set(Uuid::now_v7()),
            workspace_id: Set(Uuid::nil()),
            title: Set(format!("Item {n}")),
        }
    }
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
                .migration(CreateItems),
        )
        .controller::<ItemsController>();
    }
}

/// Schema isolation: `items` in every workspace's schema (the policy is
/// harmless there).
struct SchemaItemsModule;

impl Module for SchemaItemsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.workspace_migrations(MigrationSet::new("items").migration(CreateItems))
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
        let Ok(admin) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
            eprintln!("NAVREE_TEST_POSTGRES_URL is not set: skipping the PostgreSQL test");
            return None;
        };
        let name = format!("navree_ws_{}", Uuid::now_v7().simple());
        let conn = Database::connect(&admin).await.unwrap();
        conn.execute_unprepared(&format!(
            "CREATE ROLE {name} LOGIN PASSWORD 'secret' NOSUPERUSER NOBYPASSRLS"
        ))
        .await
        .unwrap();
        conn.execute_unprepared(&format!("CREATE DATABASE {name} OWNER {name}"))
            .await
            .unwrap();
        conn.close().await.unwrap();
        let rest = admin.split_once('@').unwrap().1;
        let host = rest.split('/').next().unwrap();
        let url = format!("postgres://{name}:secret@{host}/{name}");
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        let conn = Database::connect(&self.admin).await.unwrap();
        conn.execute_unprepared(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .await
        .unwrap();
        conn.execute_unprepared(&format!("DROP ROLE IF EXISTS {}", self.name))
            .await
            .unwrap();
    }
}

async fn app(scratch: &Scratch, isolation: &str, items: impl Module) -> TestApp {
    TestApp::builder()
        .config("database.url", scratch.url.clone())
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
    let acme = create(&app, ada, "Acme").await;
    let globex = create(&app, ada, "Globex").await;
    let client = app.acting_as(principal(ada));

    client
        .post("/api/items")
        .header("x-workspace", "acme")
        .json(&json!({ "title": "Acme item" }))
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/items")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_json_path("data.0.title", json!("Acme item"));
    client
        .get("/api/items")
        .header("x-workspace", "globex")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));

    let db = app.get::<navree_db::Db>().await;
    let workspaces = app.get::<Workspaces>().await;
    let count = |uow: navree_db::Uow| async move {
        let tx = uow.tx().await.unwrap();
        let row = tx
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT count(*)::bigint AS n FROM items",
            ))
            .await
            .unwrap()
            .unwrap();
        row.try_get::<i64>("", "n").unwrap()
    };
    // No workspace: the policy hides every row.
    assert_eq!(count(db.uow(None, None).await).await, 0);
    // A job enters a workspace explicitly.
    let uow = db.uow(None, None).await;
    workspaces.enter(&uow, globex.id).await.unwrap();
    assert_eq!(count(uow.clone()).await, 0);
    uow.rollback().await.unwrap();
    let uow = db.uow(None, None).await;
    workspaces.enter(&uow, acme.id).await.unwrap();
    assert_eq!(count(uow.clone()).await, 1);
    // Writing another workspace's row fails the policy check.
    let refused = Repo::<item::Entity>::insert(
        &uow,
        item::ActiveModel {
            id: Set(Uuid::now_v7()),
            workspace_id: Set(globex.id),
            title: Set("Smuggled".into()),
        },
    )
    .await;
    assert!(refused.is_err(), "{refused:?}");
    uow.rollback().await.unwrap();
    // WorkspaceRepo scopes on its own too.
    let uow = db.uow(None, None).await;
    workspaces.enter(&uow, acme.id).await.unwrap();
    assert_eq!(
        WorkspaceRepo::<item::Entity>::select(globex.id)
            .all(&*uow.tx().await.unwrap())
            .await
            .unwrap()
            .len(),
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
    let acme = create(&app, ada, "Acme").await;
    create(&app, ada, "Globex").await;
    let client = app.acting_as(principal(ada));

    client
        .post("/api/items")
        .header("x-workspace", "acme")
        .json(&json!({ "title": "In Acme's schema" }))
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/items")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_json_path("data.0.title", json!("In Acme's schema"));
    client
        .get("/api/items")
        .header("x-workspace", "globex")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));

    let db = app.get::<navree_db::Db>().await;
    let schemas: Vec<String> = db
        .conn()
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT table_schema AS s FROM information_schema.tables WHERE table_name = 'items' ORDER BY 1",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "s").unwrap())
        .collect();
    assert_eq!(schemas.len(), 2, "{schemas:?}");
    assert!(schemas.contains(&format!("workspace_{}", acme.id.simple())));

    // Deleting the workspace drops its schema.
    let workspaces = app.get::<Workspaces>().await;
    let uow = db.uow(None, None).await;
    workspaces.delete(&uow, acme.id).await.unwrap();
    uow.commit().await.unwrap();
    let left = db
        .conn()
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT 1 AS found FROM information_schema.schemata WHERE schema_name = $1",
            [format!("workspace_{}", acme.id.simple()).into()],
        ))
        .await
        .unwrap();
    assert!(left.is_none());

    app.shutdown().await.unwrap();
    scratch.drop().await;
}

/// `notes(id, workspace_id)`: indexed by workspace, without the policy.
mod note {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub workspace_id: Uuid,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl WorkspaceScoped for note::Entity {}

struct CreateNotes;

impl MigrationName for CreateNotes {
    fn name(&self) -> &str {
        "m20261026_000002_create_notes"
    }
}

#[async_trait]
impl MigrationTrait for CreateNotes {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("notes")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("notes_workspace_id_idx")
                    .table("notes")
                    .col("workspace_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("notes").to_owned())
            .await
    }
}

/// `items` (policy, no workspace index) and `notes` (index, no policy),
/// both classified as workspace entities.
struct ClassifiedModule;

impl Module for ClassifiedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("classified")
                .after(["workspace"])
                .migration(CreateItems)
                .migration(CreateNotes),
        )
        .workspace_entity::<item::Entity>()
        .workspace_entity::<note::Entity>();
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
            ("notes", "row-level security is off"),
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
        .conn()
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT id FROM workspaces WHERE name = $1",
            [name.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    row.try_get::<Uuid>("", "id").unwrap()
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
        .factory(ItemFactory)
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
        .factory(ItemFactory)
        .body("POST", "/api/items", json!({ "title": "Renewal" }))
        .run()
        .await;
    assert_eq!(lines(&report), PASSING);

    // Inside B (the policy on `items` hides rows outside a workspace).
    let b = workspace_named(&app, "Isolation B").await;
    let uow = app.get::<navree_db::Db>().await.uow(None, None).await;
    app.get::<Workspaces>().await.enter(&uow, b).await.unwrap();
    let owners: Vec<Uuid> = uow
        .tx()
        .await
        .unwrap()
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "SELECT workspace_id AS w FROM workspace_{}.items",
                b.simple()
            ),
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<Uuid>("", "w").unwrap())
        .collect();
    uow.rollback().await.unwrap();
    assert!(!owners.is_empty());
    assert!(owners.iter().all(|w| *w == b), "{owners:?} in {b}");

    app.shutdown().await.unwrap();
    scratch.drop().await;
}
