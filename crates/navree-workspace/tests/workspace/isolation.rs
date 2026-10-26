//! Database isolation: one SQLite file per workspace, workspace migrations
//! and `WorkspaceUow`.

use std::sync::Arc;

use http::Method;
use navree_db::{MigrationSet, Repo};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TempDirGuard;
use navree_workspace::{
    CurrentWorkspace, ModuleBuilderWorkspaceExt, WorkspaceDatabases, WorkspaceUow, Workspaces,
};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ActiveValue::Set, ConnectionTrait, DbBackend, DbErr, EntityTrait, Statement};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{World, builder};

/// `notes(id, body)`, in every workspace's database.
mod note {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub body: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct CreateNotes;

impl MigrationName for CreateNotes {
    fn name(&self) -> &str {
        "m20261026_000001_create_notes"
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
                    .col(ColumnDef::new("body").string().not_null())
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

struct NotesController;

impl Injectable for NotesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NotesController) })
    }
}

async fn insert(uow: &WorkspaceUow, body: &str) -> Result<note::Model, DbErr> {
    Repo::<note::Entity>::insert(
        uow,
        note::ActiveModel {
            id: Set(Uuid::now_v7()),
            body: Set(body.to_owned()),
        },
    )
    .await
}

async fn store(
    _ws: CurrentWorkspace,
    uow: WorkspaceUow,
    Json(input): Json<Value>,
) -> Result<Json<note::Model>, AppError> {
    Ok(Json(
        insert(&uow, input["body"].as_str().unwrap_or_default()).await?,
    ))
}

async fn store_then_fail(uow: WorkspaceUow) -> Result<Json<Value>, AppError> {
    insert(&uow, "rolled back").await?;
    Err(AppError::bad_request("nope"))
}

async fn index(uow: WorkspaceUow) -> Result<Json<Value>, AppError> {
    let notes = note::Entity::find().all(&*uow.tx().await?).await?;
    Ok(Json(json!({ "data": notes })))
}

impl Controller for NotesController {
    fn prefix(&self) -> &'static str {
        "/notes"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", index).authenticated(),
            RouteDef::on(Method::POST, "/", store).authenticated(),
            RouteDef::on(Method::POST, "/fail", store_then_fail).authenticated(),
        ]
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.workspace_migrations(MigrationSet::new("notes").migration(CreateNotes))
            .controller::<NotesController>();
    }
}

async fn world(dir: &TempDirGuard) -> World {
    let template = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("workspace-{id}.sqlite").display()
    );
    World::with(
        builder()
            .config("workspace.isolation", "database")
            .config("workspace.database_url", template)
            .module(NotesModule),
    )
    .await
}

async fn has_table<C: ConnectionTrait>(conn: &C, table: &str) -> bool {
    conn.query_one_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT 1 AS found FROM sqlite_master WHERE type = 'table' AND name = ?",
        [table.into()],
    ))
    .await
    .unwrap()
    .is_some()
}

#[tokio::test]
async fn each_workspace_writes_to_its_own_database() {
    let dir = TempDirGuard::new();
    let world = world(&dir).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let globex = world.workspace(ada, "Globex").await;
    let client = world.as_user(ada);

    client
        .post("/api/notes")
        .header("x-workspace", "acme")
        .json(&json!({ "body": "Acme's note" }))
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/notes")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.body", json!("Acme's note"));
    client
        .get("/api/notes")
        .header("x-workspace", "globex")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));

    let databases = world.app.get::<WorkspaceDatabases>().await;
    let file = dir
        .path()
        .join(format!("workspace-{}.sqlite", acme.id.simple()));
    assert!(file.exists(), "{}", file.display());
    assert!(
        databases
            .url(globex.id)
            .unwrap()
            .contains(&globex.id.simple().to_string())
    );
    assert!(!has_table(world.db().await.conn(), "notes").await);
    assert!(has_table(databases.db(acme.id).await.unwrap().conn(), "notes").await);
}

#[tokio::test]
async fn a_failed_request_rolls_the_workspace_database_back() {
    let dir = TempDirGuard::new();
    let world = world(&dir).await;
    let ada = world.user("Ada", "ada@example.com").await;
    world.workspace(ada, "Acme").await;
    let client = world.as_user(ada);
    client
        .post("/api/notes/fail")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_error(400, "nope");
    client
        .get("/api/notes")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_json_path("data", json!([]));
}

#[tokio::test]
async fn workspace_migrations_run_once() {
    let dir = TempDirGuard::new();
    let world = world(&dir).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let workspaces = world.app.get::<Workspaces>().await;
    assert_eq!(
        workspaces.migrate(acme.id).await.unwrap(),
        ["notes/m20261026_000001_create_notes"]
    );
    assert!(workspaces.migrate(acme.id).await.unwrap().is_empty());
    world
        .app
        .run_command("workspace:migrate", &["acme"])
        .await
        .unwrap();
}

#[tokio::test]
async fn schema_isolation_refuses_to_boot_without_postgres() {
    let error = builder()
        .config("workspace.isolation", "schema")
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("needs PostgreSQL"), "{error}");
}

#[tokio::test]
async fn invalid_options_fail_the_boot() {
    let error = builder()
        .config("workspace.isolation", "database")
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("workspace.database_url"), "{error}");
}
