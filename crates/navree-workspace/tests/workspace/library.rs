//! Shared libraries: global, organization and workspace items seen
//! together, the visibility scope, the write guard, the `CHECK` constraint,
//! seeding global items and cloning.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::Method;
use navree_db::Executor as _;
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::prelude::*;
use navree_workspace::library::{
    Library, LibraryColumns, LibraryOwner, LibraryRepo, LibraryScope, LibraryViewer,
    LibraryVisibility, unguarded,
};
use navree_workspace::{
    CurrentWorkspace, InWorkspace, ModuleBuilderWorkspaceExt, Organization, Workspace,
    WorkspaceError,
};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{World, org_builder};

/// A template: global, an organization's or a workspace's.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "templates", timestamps)]
pub struct Template {
    pub id: Uuid,
    pub scope: LibraryScope,
    pub organization_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub cloned_from_id: Option<Uuid>,
    pub slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A block of a template's body (copied with the template).
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "template_blocks")]
pub struct TemplateBlock {
    pub id: Uuid,
    pub template_id: Uuid,
    pub body: String,
}

impl Library for Template {
    fn cloning(&mut self, source: &Self) {
        self.name = format!("{} (copy)", source.name);
    }

    fn cloned<'a>(
        uow: &'a Uow,
        source: &'a Record<Self>,
        copy: &'a Record<Self>,
    ) -> BoxFuture<'a, navree_orm::Result<()>> {
        Box::pin(async move {
            let blocks = TemplateBlock::query()
                .where_(TemplateBlock::TEMPLATE_ID.eq(source.id))
                .order_by(TemplateBlock::BODY)
                .get(uow)
                .await?;
            for block in blocks {
                let mut block = block.replicate();
                block.template_id = copy.id;
                block.save(uow).await?;
            }
            Ok(())
        })
    }
}

/// `templates` (with the library columns) and `template_blocks`.
struct CreateTemplates;

impl Migration for CreateTemplates {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let library = LibraryColumns::of::<Template>(schema.target().await?.dialect());
        schema
            .create("templates", |t| {
                library.uuid(t, "id").primary();
                library.add(t);
                t.string("slug");
                t.string("name");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
            })
            .await?;
        schema
            .create("template_blocks", |t| {
                library.uuid(t, "id").primary();
                library.uuid(t, "template_id").index();
                t.string("body");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("template_blocks").await?;
        schema.drop_if_exists("templates").await
    }
}

/// The template routes under `/api/templates`, in the request's workspace.
struct TemplatesController;

impl Injectable for TemplatesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(TemplatesController) })
    }
}

pub fn names(templates: &[Record<Template>]) -> Vec<String> {
    let mut names: Vec<String> = templates.iter().map(|t| t.name.clone()).collect();
    names.sort();
    names
}

/// Every template the query sees (the visibility scope of the request's
/// workspace).
async fn index(_ws: CurrentWorkspace, uow: Uow) -> Result<Json<Value>, AppError> {
    let templates = Template::query().get(&uow).await?;
    Ok(Json(json!({ "data": names(&templates) })))
}

async fn show(
    _ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let template = Template::find_or_fail(&uow, id).await?;
    Ok(Json(json!(template.model())))
}

async fn store(
    _ws: CurrentWorkspace,
    uow: Uow,
    Json(input): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let template = Template::create(
        &uow,
        Template {
            slug: input["slug"].as_str().unwrap_or_default().to_owned(),
            name: input["name"].as_str().unwrap_or_default().to_owned(),
            ..Template::default()
        },
    )
    .await?;
    Ok(Json(json!(template.model())))
}

/// Renames any template the workspace sees: the guard decides.
async fn rename(
    _ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let mut template = Template::find_or_fail(&uow, id).await?;
    template.name = input["name"].as_str().unwrap_or_default().to_owned();
    template.save(&uow).await?;
    Ok(Json(json!(template.model())))
}

/// Renames a template found with `find_editable_or_fail`.
async fn rename_checked(
    ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let mut template = LibraryRepo::<Template>::find_editable_or_fail(&uow, &ws, id).await?;
    template.name = input["name"].as_str().unwrap_or_default().to_owned();
    template.save(&uow).await?;
    Ok(Json(json!(template.model())))
}

async fn destroy(
    _ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let mut template = Template::find_or_fail(&uow, id).await?;
    template.delete(&uow).await?;
    Ok(Json(json!({ "deleted": id })))
}

async fn clone(
    ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let copy = LibraryRepo::<Template>::clone_into(&uow, id, ws.id()).await?;
    Ok(Json(json!(copy.model())))
}

impl Controller for TemplatesController {
    fn prefix(&self) -> &'static str {
        "/templates"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        [
            RouteDef::on(Method::GET, "/", index),
            RouteDef::on(Method::POST, "/", store),
            RouteDef::on(Method::GET, "/{id}", show),
            RouteDef::on(Method::PATCH, "/{id}", rename),
            RouteDef::on(Method::PATCH, "/{id}/checked", rename_checked),
            RouteDef::on(Method::DELETE, "/{id}", destroy),
            RouteDef::on(Method::POST, "/{id}/clone", clone),
        ]
        .into_iter()
        .map(|route| route.authenticated().guard(InWorkspace::guard()))
        .collect()
    }
}

struct TemplatesModule;

impl Module for TemplatesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("templates")
                .after(["workspace"])
                .migration(SchemaMigration::new(
                    "m20261028_000001_create_templates",
                    CreateTemplates,
                )),
        )
        .library::<Template>()
        .controller::<TemplatesController>();
    }
}

/// Organization `Group` with workspaces Agra and Ilaichi, organization
/// `Other` with Hariyali, and the standalone Gulati, each with library items.
pub struct Fixture {
    pub world: World,
    pub ada: Uuid,
    pub group: Organization,
    pub other: Organization,
    pub agra: Workspace,
    pub ilaichi: Workspace,
    pub gulati: Workspace,
    pub welcome: Uuid,
    pub handbook: Uuid,
    pub other_handbook: Uuid,
    pub agra_only: Uuid,
    pub gulati_only: Uuid,
}

async fn create(db: &Db, owner: LibraryOwner, slug: &str, name: &str) -> Uuid {
    LibraryRepo::<Template>::create(
        db,
        owner,
        Template {
            slug: slug.to_owned(),
            name: name.to_owned(),
            ..Template::default()
        },
    )
    .await
    .unwrap()
    .id
}

async fn fixture() -> Fixture {
    fixture_with(org_builder()).await
}

/// The fixture on the app `builder` makes (organizations on), with the
/// templates module.
pub async fn fixture_with(builder: navree_testing::TestAppBuilder) -> Fixture {
    let world = World::with(builder.module(TemplatesModule)).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let group = world.organization(ada, "Group").await;
    let other = world.organization(ada, "Other").await;
    let agra = world.workspace(ada, "Agra").await;
    let ilaichi = world.workspace(ada, "Ilaichi").await;
    let hariyali = world.workspace(ada, "Hariyali").await;
    let gulati = world.workspace(ada, "Gulati").await;
    world.attach(group.id, agra.id).await;
    world.attach(group.id, ilaichi.id).await;
    world.attach(other.id, hariyali.id).await;
    let workspaces = world.workspaces().await;
    let agra = workspaces.find(agra.id).await.unwrap().unwrap();
    let ilaichi = workspaces.find(ilaichi.id).await.unwrap().unwrap();
    let gulati = workspaces.find(gulati.id).await.unwrap().unwrap();
    let db = world.db().await;
    let welcome = create(&db, LibraryOwner::Global, "welcome", "Welcome").await;
    let handbook = create(
        &db,
        LibraryOwner::Organization(group.id),
        "handbook",
        "Group handbook",
    )
    .await;
    let other_handbook = create(
        &db,
        LibraryOwner::Organization(other.id),
        "handbook",
        "Other handbook",
    )
    .await;
    let agra_only = create(&db, LibraryOwner::Workspace(agra.id), "agra", "Agra only").await;
    let gulati_only = create(
        &db,
        LibraryOwner::Workspace(gulati.id),
        "gulati",
        "Gulati only",
    )
    .await;
    for body in ["Hello", "Bye"] {
        TemplateBlock::create(
            db.as_ref(),
            TemplateBlock {
                template_id: welcome,
                body: body.to_owned(),
                ..TemplateBlock::default()
            },
        )
        .await
        .unwrap();
    }
    Fixture {
        world,
        ada,
        group,
        other,
        agra,
        ilaichi,
        gulati,
        welcome,
        handbook,
        other_handbook,
        agra_only,
        gulati_only,
    }
}

pub async fn visible(db: &Db, viewer: impl Into<LibraryViewer>) -> Vec<String> {
    names(
        &LibraryRepo::<Template>::visible_to(viewer)
            .get(db)
            .await
            .unwrap(),
    )
}

async fn owned(db: &Db, owner: LibraryOwner) -> Vec<String> {
    names(
        &LibraryRepo::<Template>::owned_by(owner)
            .get(db)
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn each_viewer_sees_the_global_items_its_organization_s_and_its_own() {
    let lib = fixture().await;
    let db = lib.world.db().await;
    assert_eq!(
        visible(&db, &lib.agra).await,
        ["Agra only", "Group handbook", "Welcome"]
    );
    assert_eq!(
        visible(&db, &lib.ilaichi).await,
        ["Group handbook", "Welcome"]
    );
    assert_eq!(visible(&db, &lib.gulati).await, ["Gulati only", "Welcome"]);
    assert_eq!(
        visible(&db, LibraryViewer::organization(lib.group.id)).await,
        ["Group handbook", "Welcome"]
    );
    assert_eq!(
        visible(&db, LibraryViewer::organization(lib.other.id)).await,
        ["Other handbook", "Welcome"]
    );
    assert_eq!(visible(&db, LibraryViewer::global()).await, ["Welcome"]);

    assert_eq!(
        owned(&db, LibraryOwner::Organization(lib.group.id)).await,
        ["Group handbook"]
    );
    assert_eq!(
        owned(&db, LibraryOwner::Workspace(lib.agra.id)).await,
        ["Agra only"]
    );
    assert_eq!(owned(&db, LibraryOwner::Global).await, ["Welcome"]);
    assert_eq!(
        names(
            &LibraryRepo::<Template>::global()
                .get(db.as_ref())
                .await
                .unwrap()
        ),
        ["Welcome"]
    );

    let found = LibraryRepo::<Template>::find_visible(db.as_ref(), &lib.agra, lib.handbook)
        .await
        .unwrap()
        .expect("the organization's handbook");
    assert_eq!(
        found.library_owner().unwrap(),
        LibraryOwner::Organization(lib.group.id)
    );
    assert!(
        LibraryRepo::<Template>::find_visible(db.as_ref(), &lib.agra, lib.gulati_only)
            .await
            .unwrap()
            .is_none()
    );
    let missing =
        LibraryRepo::<Template>::find_visible_or_fail(db.as_ref(), &lib.agra, lib.other_handbook)
            .await
            .unwrap_err();
    assert_eq!(missing.to_app_error().status.as_u16(), 404);
}

#[tokio::test]
async fn queries_see_what_the_current_workspace_sees() {
    let lib = fixture().await;
    let db = lib.world.db().await;
    // No current workspace: the global items only.
    assert_eq!(
        names(&Template::query().get(db.as_ref()).await.unwrap()),
        ["Welcome"]
    );
    assert_eq!(Template::query().count(db.as_ref()).await.unwrap(), 1);
    // A task working for a workspace sees its library.
    let agra = navree_orm::workspace::scope(lib.agra.id, Template::query().get(db.as_ref()))
        .await
        .unwrap();
    assert_eq!(names(&agra), ["Agra only", "Group handbook", "Welcome"]);
    let gulati = navree_orm::workspace::scope(lib.gulati.id, Template::query().get(db.as_ref()))
        .await
        .unwrap();
    assert_eq!(names(&gulati), ["Gulati only", "Welcome"]);
    // A unit of work that entered a workspace.
    let uow = db.uow(None, None).await;
    uow.set_workspace(Some(lib.ilaichi.id));
    assert_eq!(
        names(&Template::query().get(&uow).await.unwrap()),
        ["Group handbook", "Welcome"]
    );
    assert!(Template::find(&uow, lib.agra_only).await.unwrap().is_none());
    uow.rollback().await.unwrap();
    // The scope can be left out on purpose.
    assert_eq!(
        Template::query()
            .without_global_scope::<LibraryVisibility>()
            .count(db.as_ref())
            .await
            .unwrap(),
        5
    );

    // Over HTTP, in each workspace's requests.
    let client = lib.world.as_user(lib.ada);
    client
        .get("/api/templates")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!(["Agra only", "Group handbook", "Welcome"]));
    client
        .get("/api/templates")
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!(["Gulati only", "Welcome"]));
    client
        .get(&format!("/api/templates/{}", lib.gulati_only))
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn the_check_constraint_refuses_contradicting_owners() {
    let lib = fixture().await;
    let db = lib.world.db().await;
    let (org, ws) = (lib.group.id.to_string(), lib.agra.id.to_string());
    // UUIDs are 16 bytes on MySQL.
    let mysql = db.backend() == navree_db::Backend::MySql;
    let uuid = |id: &str| {
        if mysql {
            format!("UUID_TO_BIN('{id}')")
        } else {
            format!("'{id}'")
        }
    };
    let insert = |scope: &str, organization: Option<&str>, workspace: Option<&str>| {
        let quote = |id: Option<&str>| id.map_or("NULL".to_owned(), uuid);
        format!(
            "INSERT INTO templates (id, scope, organization_id, workspace_id, slug, name, created_at, updated_at) \
             VALUES ({}, '{scope}', {}, {}, 'raw', 'Raw', '2026-10-28 00:00:00', '2026-10-28 00:00:00')",
            uuid(&Uuid::now_v7().to_string()),
            quote(organization),
            quote(workspace)
        )
    };
    for (scope, organization, workspace) in [
        ("global", Some(org.as_str()), None),
        ("global", None, Some(ws.as_str())),
        ("organization", None, None),
        ("organization", Some(org.as_str()), Some(ws.as_str())),
        ("workspace", None, None),
        ("workspace", Some(org.as_str()), Some(ws.as_str())),
        ("shared", None, None),
    ] {
        let refused = db
            .execute_script(&insert(scope, organization, workspace))
            .await;
        assert_eq!(
            refused.unwrap_err().kind(),
            navree_db::ErrorKind::CheckViolation,
            "{scope} {organization:?} {workspace:?}"
        );
    }
    for (scope, organization, workspace) in [
        ("global", None, None),
        ("organization", Some(org.as_str()), None),
        ("workspace", None, Some(ws.as_str())),
    ] {
        db.execute_script(&insert(scope, organization, workspace))
            .await
            .unwrap();
    }

    // The ORM refuses them before the database does, naming the field.
    let contradicting = Template {
        scope: LibraryScope::Global,
        workspace_id: Some(lib.agra.id),
        slug: "bad".into(),
        name: "Bad".into(),
        ..Template::default()
    };
    let error = WorkspaceError::from(
        Template::create(db.as_ref(), contradicting)
            .await
            .unwrap_err(),
    );
    let response = error.to_app_error();
    assert_eq!(response.status.as_u16(), 400);
    assert_eq!(
        response.body()["errors"]["scope"][0],
        "a global item has no organization_id and no workspace_id"
    );
    // Outside a workspace, a new item names its owner.
    let ownerless = Template {
        slug: "ownerless".into(),
        name: "Ownerless".into(),
        ..Template::default()
    };
    let error = WorkspaceError::from(Template::create(db.as_ref(), ownerless).await.unwrap_err());
    assert_eq!(
        error.to_app_error().body()["errors"]["scope"][0],
        "a workspace's item needs a workspace_id"
    );
}

#[tokio::test]
async fn inside_a_workspace_shared_items_are_read_only() {
    let lib = fixture().await;
    let client = lib.world.as_user(lib.ada);
    let rename = |id: Uuid, name: &'static str| {
        client
            .patch(&format!("/api/templates/{id}"))
            .header("x-workspace", "agra")
            .json(&json!({ "name": name }))
            .send()
    };
    for shared in [lib.welcome, lib.handbook] {
        rename(shared, "Mine now")
            .await
            .assert_status(403)
            .assert_json_path("code", json!("library_read_only"))
            .assert_json_path(
                "message",
                json!("This library item is shared: clone it to change it."),
            );
        client
            .delete(&format!("/api/templates/{shared}"))
            .header("x-workspace", "agra")
            .send()
            .await
            .assert_status(403)
            .assert_json_path("code", json!("library_read_only"));
    }
    rename(lib.agra_only, "Agra's own")
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Agra's own"));
    // The checked route says the same before trying.
    client
        .patch(&format!("/api/templates/{}/checked", lib.welcome))
        .header("x-workspace", "agra")
        .json(&json!({ "name": "Mine" }))
        .send()
        .await
        .assert_status(403)
        .assert_json_path("code", json!("library_read_only"));
    client
        .patch(&format!("/api/templates/{}/checked", lib.gulati_only))
        .header("x-workspace", "agra")
        .json(&json!({ "name": "Mine" }))
        .send()
        .await
        .assert_status(404);
    client
        .patch(&format!("/api/templates/{}/checked", lib.agra_only))
        .header("x-workspace", "agra")
        .json(&json!({ "name": "Checked" }))
        .send()
        .await
        .assert_status(200);

    // A new item joins the workspace's library.
    let created = client
        .post("/api/templates")
        .header("x-workspace", "agra")
        .json(&json!({ "slug": "notes", "name": "Notes" }))
        .send()
        .await
        .assert_status(200)
        .json::<Value>();
    assert_eq!(created["scope"], "workspace");
    assert_eq!(created["workspace_id"], json!(lib.agra.id));
    assert_eq!(created["organization_id"], Value::Null);

    // Nothing shared changed.
    let db = lib.world.db().await;
    let welcome = LibraryRepo::<Template>::global()
        .find(db.as_ref(), lib.welcome)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(welcome.name, "Welcome");
    assert_eq!(
        LibraryRepo::<Template>::owned_by(LibraryOwner::Organization(lib.group.id))
            .count(db.as_ref())
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn the_guard_keeps_a_workspace_to_its_own_items() {
    let lib = fixture().await;
    let db = lib.world.db().await;
    let agra = lib.agra.id;
    let any = |id| {
        Template::query()
            .without_global_scope::<LibraryVisibility>()
            .find_or_fail(db.as_ref(), id)
    };

    // Another workspace's item, however it was read: a 404.
    let mut theirs = any(lib.gulati_only).await.unwrap();
    theirs.name = "Taken".into();
    let error = navree_orm::workspace::scope(agra, theirs.save(db.as_ref()))
        .await
        .unwrap_err();
    assert!(error.is_not_found(), "{error}");
    let error = navree_orm::workspace::scope(agra, theirs.delete(db.as_ref()))
        .await
        .unwrap_err();
    assert!(error.is_not_found(), "{error}");

    // Creating another owner's item, or moving its own item away: a 403.
    let global = Template {
        scope: LibraryScope::Global,
        slug: "sneaky".into(),
        name: "Sneaky".into(),
        ..Template::default()
    };
    let error = navree_orm::workspace::scope(
        agra,
        LibraryRepo::<Template>::create(db.as_ref(), LibraryOwner::Global, global.clone()),
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_app_error().code, "library_read_only");
    let mut own = any(lib.agra_only).await.unwrap();
    own.set_library_owner(LibraryOwner::Organization(lib.group.id))
        .unwrap();
    let error = WorkspaceError::from(
        navree_orm::workspace::scope(agra, own.save(db.as_ref()))
            .await
            .unwrap_err(),
    );
    assert_eq!(error.to_app_error().code, "library_read_only");

    // `unguarded` lifts the workspace rules for a deliberate write.
    let published = navree_orm::workspace::scope(
        agra,
        unguarded(LibraryRepo::<Template>::create(
            db.as_ref(),
            LibraryOwner::Organization(lib.group.id),
            Template {
                slug: "published".into(),
                name: "Published".into(),
                ..Template::default()
            },
        )),
    )
    .await
    .unwrap();
    assert_eq!(
        published.library_owner().unwrap(),
        LibraryOwner::Organization(lib.group.id)
    );

    // Outside a workspace any owner's items may be written.
    let mut welcome = any(lib.welcome).await.unwrap();
    welcome.name = "Welcome!".into();
    assert!(welcome.save(db.as_ref()).await.unwrap());
    let mut handbook = any(lib.handbook).await.unwrap();
    handbook.delete(db.as_ref()).await.unwrap();
    LibraryRepo::<Template>::create(db.as_ref(), LibraryOwner::Global, global)
        .await
        .unwrap();
}

#[tokio::test]
async fn cloning_copies_an_item_into_the_workspace() {
    let lib = fixture().await;
    let mut events = lib.world.events(&["library.item_cloned"]).await;
    let client = lib.world.as_user(lib.ada);
    let copy = client
        .post(&format!("/api/templates/{}/clone", lib.welcome))
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .json::<Value>();
    let copy_id: Uuid = serde_json::from_value(copy["id"].clone()).unwrap();
    assert_ne!(copy_id, lib.welcome);
    assert_eq!(copy["scope"], "workspace");
    assert_eq!(copy["workspace_id"], json!(lib.agra.id));
    assert_eq!(copy["organization_id"], Value::Null);
    assert_eq!(copy["cloned_from_id"], json!(lib.welcome));
    assert_eq!(copy["name"], "Welcome (copy)");
    assert_eq!(copy["slug"], "welcome");

    // `cloned` copied the blocks.
    let db = lib.world.db().await;
    let blocks = TemplateBlock::query()
        .where_(TemplateBlock::TEMPLATE_ID.eq(copy_id))
        .order_by(TemplateBlock::BODY)
        .pluck(db.as_ref(), TemplateBlock::BODY)
        .await
        .unwrap();
    assert_eq!(blocks, ["Bye", "Hello"]);

    // The event, after the commit.
    let received = events.drain();
    assert_eq!(received.len(), 1, "{received:?}");
    let (name, payload) = &received[0];
    assert_eq!(name, "library.item_cloned");
    assert_eq!(
        payload,
        &json!({
            "library": "templates",
            "item_id": lib.welcome.to_string(),
            "clone_id": copy_id.to_string(),
            "source": { "scope": "global" },
            "owner": { "scope": "workspace", "id": lib.agra.id },
            "cloned_by": lib.ada,
        })
    );

    // The copy is Agra's: in its library, editable there, unseen elsewhere.
    client
        .get("/api/templates")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_json_path(
            "data",
            json!(["Agra only", "Group handbook", "Welcome", "Welcome (copy)"]),
        );
    client
        .patch(&format!("/api/templates/{copy_id}"))
        .header("x-workspace", "agra")
        .json(&json!({ "name": "Our welcome" }))
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/templates")
        .header("x-workspace", "ilaichi")
        .send()
        .await
        .assert_json_path("data", json!(["Group handbook", "Welcome"]));
    let copy = LibraryRepo::<Template>::find_visible_or_fail(db.as_ref(), &lib.agra, copy_id)
        .await
        .unwrap();
    assert_eq!(copy.cloned_from().unwrap(), Some(lib.welcome));
    let original = LibraryRepo::<Template>::global()
        .find(db.as_ref(), lib.welcome)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.cloned_from().unwrap(), None);
    assert_eq!(original.name, "Welcome");

    // Organization items clone too; the clones of an item are found back.
    client
        .post(&format!("/api/templates/{}/clone", lib.handbook))
        .header("x-workspace", "ilaichi")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Group handbook (copy)"));
    let indrani_clones = navree_orm::workspace::scope(
        lib.ilaichi.id,
        LibraryRepo::<Template>::clones_of(lib.handbook).count(db.as_ref()),
    )
    .await
    .unwrap();
    assert_eq!(indrani_clones, 1);
    assert_eq!(
        LibraryRepo::<Template>::clones_of(lib.welcome)
            .without_global_scope::<LibraryVisibility>()
            .count(db.as_ref())
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn clones_need_an_item_the_target_sees() {
    let lib = fixture().await;
    let client = lib.world.as_user(lib.ada);
    for invisible in [lib.gulati_only, lib.other_handbook] {
        client
            .post(&format!("/api/templates/{invisible}/clone"))
            .header("x-workspace", "agra")
            .send()
            .await
            .assert_status(404);
    }
    let db = lib.world.db().await;
    let uow = db.uow(None, None).await;
    let error = LibraryRepo::<Template>::clone_into(&uow, lib.welcome, Uuid::now_v7())
        .await
        .unwrap_err();
    assert!(
        matches!(error, WorkspaceError::WorkspaceNotFound(_)),
        "{error}"
    );
    let error = LibraryRepo::<Template>::clone_to(
        &uow,
        lib.welcome,
        LibraryOwner::Organization(Uuid::now_v7()),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, WorkspaceError::OrganizationNotFound(_)),
        "{error}"
    );
    // An organization clones a global item, or one of its own.
    let copy = LibraryRepo::<Template>::clone_to(
        &uow,
        lib.welcome,
        LibraryOwner::Organization(lib.other.id),
    )
    .await
    .unwrap();
    assert_eq!(
        copy.library_owner().unwrap(),
        LibraryOwner::Organization(lib.other.id)
    );
    let error = LibraryRepo::<Template>::clone_to(
        &uow,
        lib.handbook,
        LibraryOwner::Organization(lib.other.id),
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_app_error().status.as_u16(), 404);
    // It takes its workspaces' items too: a workspace's item is published
    // to its organization, never to another one.
    let published = LibraryRepo::<Template>::clone_to(
        &uow,
        lib.agra_only,
        LibraryOwner::Organization(lib.group.id),
    )
    .await
    .unwrap();
    assert_eq!(
        published.library_owner().unwrap(),
        LibraryOwner::Organization(lib.group.id)
    );
    assert_eq!(published.cloned_from().unwrap(), Some(lib.agra_only));
    for (item, organization) in [
        (lib.agra_only, lib.other.id),
        (lib.gulati_only, lib.group.id),
    ] {
        let error =
            LibraryRepo::<Template>::clone_to(&uow, item, LibraryOwner::Organization(organization))
                .await
                .unwrap_err();
        assert_eq!(error.to_app_error().status.as_u16(), 404);
    }
    // The global library takes global items only.
    let error = LibraryRepo::<Template>::clone_to(&uow, lib.agra_only, LibraryOwner::Global)
        .await
        .unwrap_err();
    assert_eq!(error.to_app_error().status.as_u16(), 404);
    uow.commit().await.unwrap();

    // Inside a workspace, cloning for someone else is refused.
    let uow = db.uow(None, None).await;
    uow.set_workspace(Some(lib.agra.id));
    let error = LibraryRepo::<Template>::clone_into(&uow, lib.welcome, lib.gulati.id)
        .await
        .unwrap_err();
    assert_eq!(error.to_app_error().code, "library_read_only");
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn a_rolled_back_clone_leaves_nothing() {
    let lib = fixture().await;
    let mut events = lib.world.events(&["library.item_cloned"]).await;
    let db = lib.world.db().await;
    let uow = db.uow(Some(lib.ada), None).await;
    let copy = LibraryRepo::<Template>::clone_into(&uow, lib.welcome, lib.agra.id)
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(
        Template::query()
            .without_global_scope::<LibraryVisibility>()
            .find(db.as_ref(), copy.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        TemplateBlock::query()
            .where_(TemplateBlock::TEMPLATE_ID.eq(copy.id))
            .count(db.as_ref())
            .await
            .unwrap(),
        0
    );
    assert!(events.drain().is_empty());
}

#[tokio::test]
async fn seeders_place_global_items_by_a_unique_column() {
    let lib = fixture().await;
    let db = lib.world.db().await;
    let items = |welcome: &str| {
        [("welcome", welcome), ("reset", "Reset your password")].map(|(slug, name)| Template {
            slug: slug.to_owned(),
            name: name.to_owned(),
            ..Template::default()
        })
    };
    let seeded = LibraryRepo::<Template>::seed_global(db.as_ref(), "slug", items("Welcome"))
        .await
        .unwrap();
    assert_eq!(
        (seeded.created, seeded.updated, seeded.unchanged),
        (1, 0, 1)
    );
    let seeded = LibraryRepo::<Template>::seed_global(db.as_ref(), "slug", items("Welcome aboard"))
        .await
        .unwrap();
    assert_eq!(
        (seeded.created, seeded.updated, seeded.unchanged),
        (0, 1, 1)
    );
    let global = LibraryRepo::<Template>::global()
        .order_by(Template::SLUG)
        .get(db.as_ref())
        .await
        .unwrap();
    assert_eq!(names(&global), ["Reset your password", "Welcome aboard"]);
    let welcome = global.iter().find(|t| t.slug == "welcome").unwrap();
    assert_eq!(welcome.id, lib.welcome, "the existing row was updated");
    assert!(global.iter().all(|t| t.scope == LibraryScope::Global));
    // Organization items with the same slug are not touched.
    let handbook = LibraryRepo::<Template>::owned_by(LibraryOwner::Organization(lib.group.id))
        .find(db.as_ref(), lib.handbook)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(handbook.name, "Group handbook");
    let error = LibraryRepo::<Template>::seed_global(db.as_ref(), "missing", items("x"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no `missing` column"), "{error}");
}
