//! Database isolation: one SQLite file per workspace, workspace migrations
//! and `WorkspaceUow`.

use std::sync::Arc;

use http::Method;
use navree_db::MigrationSet;
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::{Model as _, OrmError};
use navree_testing::TempDirGuard;
use navree_workspace::{
    CurrentWorkspace, ModuleBuilderWorkspaceExt, WorkspaceDatabases, WorkspaceUow, Workspaces,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{World, builder, uuid};

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

struct NotesController;

impl Injectable for NotesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NotesController) })
    }
}

async fn insert(uow: &WorkspaceUow, body: &str) -> Result<Note, OrmError> {
    let note = Note {
        id: Uuid::now_v7(),
        body: body.to_owned(),
    };
    Ok(Note::create(&**uow, note).await?.into_inner())
}

async fn store(
    _ws: CurrentWorkspace,
    uow: WorkspaceUow,
    Json(input): Json<Value>,
) -> Result<Json<Note>, AppError> {
    Ok(Json(
        insert(&uow, input["body"].as_str().unwrap_or_default()).await?,
    ))
}

async fn store_then_fail(uow: WorkspaceUow) -> Result<Json<Value>, AppError> {
    insert(&uow, "rolled back").await?;
    Err(AppError::bad_request("nope"))
}

async fn index(uow: WorkspaceUow) -> Result<Json<Value>, AppError> {
    let notes = Note::query().get(&*uow).await?.into_models();
    Ok(Json(json!({ "data": notes })))
}

/// `notes` as an ORM model: `WorkspaceUow` is its executor.
mod orm_note {
    use navree_orm::Model;
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Model, Clone, Debug, Default, Serialize)]
    #[model(table = "notes")]
    pub struct OrmNote {
        pub id: Uuid,
        pub body: String,
    }
}

async fn orm_store(
    _ws: CurrentWorkspace,
    uow: WorkspaceUow,
    Json(input): Json<Value>,
) -> Result<Json<Value>, AppError> {
    use navree_orm::Model;
    let note = orm_note::OrmNote::create(
        &*uow,
        orm_note::OrmNote {
            body: input["body"].as_str().unwrap_or_default().to_owned(),
            ..orm_note::OrmNote::default()
        },
    )
    .await?;
    Ok(Json(json!({ "id": note.id, "body": note.body })))
}

async fn orm_index(uow: WorkspaceUow) -> Result<Json<Value>, AppError> {
    use navree_orm::Model;
    let notes = orm_note::OrmNote::query()
        .latest_by(orm_note::OrmNote::ID)
        .get(&*uow)
        .await?;
    let bodies: Vec<&str> = notes.iter().map(|note| note.body.as_str()).collect();
    Ok(Json(
        json!({ "data": bodies, "count": orm_note::OrmNote::query().count(&*uow).await? }),
    ))
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
            RouteDef::on(Method::GET, "/orm", orm_index).authenticated(),
            RouteDef::on(Method::POST, "/orm", orm_store).authenticated(),
        ]
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.workspace_migrations(MigrationSet::new("notes").migration(SchemaMigration::new(
            "m20261026_000001_create_notes",
            CreateNotes,
        )))
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

async fn has_table(db: &navree_db::Db, table: &str) -> bool {
    Schema::new(db).has_table(table).await.unwrap()
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
    assert!(!has_table(&*world.db().await, "notes").await);
    assert!(has_table(&*databases.db(acme.id).await.unwrap(), "notes").await);
}

#[tokio::test]
async fn orm_models_run_in_the_workspace_database() {
    let dir = TempDirGuard::new();
    let world = world(&dir).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    world.workspace(ada, "Globex").await;
    let client = world.as_user(ada);

    for body in ["First", "Second"] {
        client
            .post("/api/notes/orm")
            .header("x-workspace", "acme")
            .json(&json!({ "body": body }))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("body", json!(body));
    }
    client
        .get("/api/notes/orm")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!(["Second", "First"]))
        .assert_json_path("count", json!(2));
    // The other route reads the same database.
    client
        .get("/api/notes")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_json_path("data.1.body", json!("Second"));
    client
        .get("/api/notes/orm")
        .header("x-workspace", "globex")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]))
        .assert_json_path("count", json!(0));

    // Outside a request: the workspace's own pool is an executor too.
    let databases = world.app.get::<WorkspaceDatabases>().await;
    let acme_db = databases.db(acme.id).await.unwrap();
    {
        use navree_orm::Model;
        assert_eq!(
            orm_note::OrmNote::query().count(&*acme_db).await.unwrap(),
            2
        );
    }
    assert!(!has_table(&*world.db().await, "notes").await);
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
    if crate::database::Server::current() == crate::database::Server::Postgres {
        return;
    }
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
