//! PostgreSQL integration: row-level security through `navree.workspace`,
//! `navree.organization` and `navree.partition`, and schema isolation
//! through `search_path`.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a superuser URL of a throwaway server, for
//! example `postgres://postgres:postgres@127.0.0.1:55432/postgres`. Each
//! test creates its own database and a non-superuser login role (superusers
//! bypass row-level security), and drops both at the end.
#![cfg(feature = "postgres")]

use std::sync::Arc;

use http::Method;
use navree_access::{AccessModule, AccessOptions};
use navree_db::{DatabaseModule, MigrationSet, ModuleBuilderDbExt, Repo};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, principal};
use navree_workspace::{
    CurrentOrganization, CurrentWorkspace, ModuleBuilderWorkspaceExt, NewOrganization,
    NewPartition, NewWorkspace, OrgRepo, Organizations, PartitionScope, PartitionScoped,
    Partitions, RlsPolicy, WorkspaceModule, WorkspaceRepo, WorkspaceScoped, WorkspaceUow,
    Workspaces,
};
use sea_orm::sea_query::{ColumnDef, Table};
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

impl Controller for ItemsController {
    fn prefix(&self) -> &'static str {
        "/items"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", index).authenticated(),
            RouteDef::on(Method::POST, "/", store).authenticated(),
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

/// `notes(id, workspace_id, partition_id, body)` with a policy that lets
/// organization requests read and narrows partition requests.
mod note {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub partition_id: Option<Uuid>,
        pub body: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl WorkspaceScoped for note::Entity {}

impl PartitionScoped for note::Entity {}

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
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("partition_id").uuid().null())
                    .col(ColumnDef::new("body").string().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                &RlsPolicy::new("notes")
                    .organizations()
                    .partitioned()
                    .create_sql(),
            )
            .await
            .map(drop)
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

/// Every note the transaction can see: no `WHERE`, so only the policy
/// scopes it.
async fn every_note(uow: &navree_db::Uow) -> Result<Value, AppError> {
    let mut bodies: Vec<String> = note::Entity::find()
        .all(&*uow.tx().await?)
        .await?
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
        m.migrations(
            MigrationSet::new("notes")
                .after(["workspace"])
                .migration(CreateNotes),
        )
        .controller::<NotesController>();
    }
}

async fn org_app(scratch: &Scratch) -> TestApp {
    TestApp::builder()
        .config("database.url", scratch.url.clone())
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
    Repo::<note::Entity>::insert(
        &uow,
        note::ActiveModel {
            id: Set(Uuid::now_v7()),
            workspace_id: Set(workspace),
            partition_id: Set(partition),
            body: Set(body.to_owned()),
        },
    )
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
    let refused = Repo::<note::Entity>::insert(
        &uow,
        note::ActiveModel {
            id: Set(Uuid::now_v7()),
            workspace_id: Set(a1.id),
            partition_id: Set(None),
            body: Set("org-level write".into()),
        },
    )
    .await;
    assert!(refused.is_err(), "{refused:?}");
    uow.rollback().await.unwrap();
    // Nor can it update or delete the rows it reads.
    let uow = db.uow(None, None).await;
    organizations.enter(&uow, a.id).await.unwrap();
    let tx = uow.tx().await.unwrap();
    let deleted = tx.execute_unprepared("DELETE FROM notes").await.unwrap();
    assert_eq!(deleted.rows_affected(), 0);
    let updated = tx
        .execute_unprepared("UPDATE notes SET body = 'changed'")
        .await
        .unwrap();
    assert_eq!(updated.rows_affected(), 0);
    drop(tx);
    assert_eq!(every_note(&uow).await.unwrap(), json!(["a1", "a2"]));
    uow.rollback().await.unwrap();
    // OrgRepo narrows further, to the workspaces a member reaches.
    let uow = db.uow(None, None).await;
    organizations.enter(&uow, a.id).await.unwrap();
    let only_a2 = OrgRepo::<note::Entity>::select(&[a2.id, b1.id])
        .all(&*uow.tx().await.unwrap())
        .await
        .unwrap();
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
    let acme = create(&app, ada, "Acme").await;
    let partitions = app.get::<Partitions>().await;
    let db = app.get::<navree_db::Db>().await;
    let uow = db.uow(None, None).await;
    let uk = partitions
        .create(&uow, acme.id, NewPartition::named("acme.co.uk"))
        .await
        .unwrap();
    let de = partitions
        .create(&uow, acme.id, NewPartition::named("acme.de"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    note(&app, acme.id, Some(uk.id), "uk").await;
    note(&app, acme.id, Some(de.id), "de").await;
    note(&app, acme.id, None, "unsorted").await;
    let client = app.acting_as(principal(ada));

    client
        .get("/api/notes/partition")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!(["de", "uk", "unsorted"]));
    client
        .get("/api/notes/partition")
        .header("x-workspace", "acme")
        .header("x-partition", "acme-co-uk")
        .send()
        .await
        .assert_json_path("data", json!(["uk"]));

    // Inside a partition, rows of another partition can't be written.
    let uow = db.uow(None, None).await;
    app.get::<Workspaces>()
        .await
        .enter(&uow, acme.id)
        .await
        .unwrap();
    partitions.enter(&uow, uk.id).await.unwrap();
    assert_eq!(every_note(&uow).await.unwrap(), json!(["uk"]));
    let refused = Repo::<note::Entity>::insert(
        &uow,
        note::ActiveModel {
            id: Set(Uuid::now_v7()),
            workspace_id: Set(acme.id),
            partition_id: Set(Some(de.id)),
            body: Set("smuggled".into()),
        },
    )
    .await;
    assert!(refused.is_err(), "{refused:?}");
    uow.rollback().await.unwrap();

    app.shutdown().await.unwrap();
    scratch.drop().await;
}
