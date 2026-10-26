//! navree-access integration: route checks evaluated against the member's
//! role in the current workspace, and workspace-scoped record checks.

use std::sync::Arc;

use axum::extract::Path;
use http::Method;
use navree_access::{Abilities, Authorized, CurrentAccess, Update};
use navree_db::Uow;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_workspace::{CurrentWorkspace, InWorkspace, WorkspaceRepo};
use sea_orm::{ActiveValue::Set, EntityTrait};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{World, builder, deal};

/// Deal routes under `/api/deals`, guarded by `InWorkspace`, plus one
/// route without the guard.
struct DealsController;

impl Injectable for DealsController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DealsController) })
    }
}

async fn index(ws: CurrentWorkspace, uow: Uow) -> Result<Json<Value>, AppError> {
    let deals = WorkspaceRepo::<deal::Entity>::select(ws.id())
        .all(&*uow.tx().await?)
        .await?;
    Ok(Json(json!({ "data": deals })))
}

async fn store(
    ws: CurrentWorkspace,
    uow: Uow,
    Json(input): Json<Value>,
) -> Result<Json<deal::Model>, AppError> {
    let created = WorkspaceRepo::<deal::Entity>::insert(
        &uow,
        ws.id(),
        deal::ActiveModel {
            id: Set(Uuid::now_v7()),
            workspace_id: Set(Uuid::nil()),
            title: Set(input["title"].as_str().unwrap_or_default().to_owned()),
        },
    )
    .await?;
    Ok(Json(created))
}

async fn update(deal: Authorized<deal::Entity, Update>) -> Json<Value> {
    Json(json!({ "title": deal.title }))
}

async fn abilities(
    Abilities(ability): Abilities,
    CurrentAccess(access): CurrentAccess,
) -> Json<Value> {
    Json(json!({
        "rules": ability.to_casl_json(),
        "roles": access.roles,
    }))
}

async fn report() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn show(
    ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
) -> Result<Json<deal::Model>, AppError> {
    Ok(Json(
        WorkspaceRepo::<deal::Entity>::find_or_fail(&uow, ws.id(), id).await?,
    ))
}

impl Controller for DealsController {
    fn prefix(&self) -> &'static str {
        "/deals"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", index)
                .permissions(&["deals.read"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::POST, "/", store)
                .permissions(&["deals.create"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::GET, "/{id}", show)
                .permissions(&["deals.read"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::PATCH, "/{id}", update)
                .can("update", "Deal", Some("id"))
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::GET, "/abilities", abilities)
                .authenticated()
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::GET, "/admin-report", report)
                .roles(&["admin"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::GET, "/report", report)
                .can("read", "Report", None)
                .guard(InWorkspace::guard()),
            // Without the guard, route checks see the user's global access.
            RouteDef::on(Method::GET, "/unguarded", report).permissions(&["deals.read"]),
        ]
    }
}

struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DealsController>();
    }
}

async fn world() -> World {
    World::with(builder().module(DealsModule)).await
}

#[tokio::test]
async fn route_permissions_use_the_role_in_the_current_workspace() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let globex = world.workspace(ada, "Globex").await;
    world.join(acme.id, grace, "member").await;
    world.join(globex.id, grace, "viewer").await;
    let client = world.as_user(grace);

    // A member has `deals.*` in Acme; a viewer only `deals.read` in Globex.
    client
        .post("/api/deals")
        .header("x-workspace", "acme")
        .json(&json!({ "title": "Big one" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace_id", json!(acme.id));
    client
        .post("/api/deals")
        .header("x-workspace", "globex")
        .json(&json!({ "title": "Nope" }))
        .send()
        .await
        .assert_error(403, None);
    client
        .get("/api/deals")
        .header("x-workspace", "globex")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
    client
        .get("/api/deals")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.title", json!("Big one"));

    // Roles are the workspace role; non-members are refused before any check.
    client
        .get("/api/deals/admin-report")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_error(403, None);
    world
        .join(
            acme.id,
            world.user("Linus", "linus@example.com").await,
            "admin",
        )
        .await;
    let eve = world.user("Eve", "eve@example.com").await;
    world
        .as_user(eve)
        .get("/api/deals")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.");

    // Without the guard the global (empty) permissions apply.
    client
        .get("/api/deals/unguarded")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn the_abilities_function_sees_the_workspace() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    world.join(acme.id, grace, "viewer").await;

    let response = world
        .as_user(grace)
        .get("/api/deals/abilities")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("roles", json!(["viewer"]));
    let rules = response.json_value()["rules"].clone();
    assert!(
        rules
            .as_array()
            .unwrap()
            .contains(&json!({ "action": "read", "subject": "Deal" })),
        "{rules}"
    );
    assert!(
        rules
            .as_array()
            .unwrap()
            .contains(&json!({ "action": "read", "subject": "Report" })),
        "{rules}"
    );
    world
        .as_user(grace)
        .get("/api/deals/report")
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn records_of_another_workspace_are_not_found() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let globex = world.workspace(ada, "Globex").await;
    let insert = |workspace: Uuid, title: &str| {
        let world = &world;
        let title = title.to_owned();
        async move {
            let uow = world.db().await.uow(None, None).await;
            let created = WorkspaceRepo::<deal::Entity>::insert(
                &uow,
                workspace,
                deal::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    workspace_id: Set(Uuid::nil()),
                    title: Set(title),
                },
            )
            .await
            .unwrap();
            uow.commit().await.unwrap();
            created
        }
    };
    let ours = insert(acme.id, "Ours").await;
    let theirs = insert(globex.id, "Theirs").await;
    let client = world.as_user(ada);

    client
        .patch(&format!("/api/deals/{}", ours.id))
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Ours"));
    // Ada owns both workspaces (`all.*`), so only the workspace check
    // stands between Acme's request and Globex's record.
    client
        .patch(&format!("/api/deals/{}", theirs.id))
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_error(404, None);
    client
        .get(&format!("/api/deals/{}", theirs.id))
        .header("x-workspace", "acme")
        .send()
        .await
        .assert_error(404, None);
    client
        .get(&format!("/api/deals/{}", theirs.id))
        .header("x-workspace", "globex")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn the_repository_scopes_every_query() {
    let world = world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let globex = world.workspace(ada, "Globex").await;
    let uow = world.db().await.uow(None, None).await;
    let created = WorkspaceRepo::<deal::Entity>::insert(
        &uow,
        acme.id,
        deal::ActiveModel {
            id: Set(Uuid::now_v7()),
            workspace_id: Set(globex.id),
            title: Set("Acme deal".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(created.workspace_id, acme.id, "insert sets the workspace");
    assert!(
        WorkspaceRepo::<deal::Entity>::find(&uow, globex.id, created.id)
            .await
            .unwrap()
            .is_none()
    );
    let moved = WorkspaceRepo::<deal::Entity>::update(
        &uow,
        globex.id,
        deal::ActiveModel {
            id: Set(created.id),
            workspace_id: Set(globex.id),
            title: Set("Stolen".into()),
        },
    )
    .await;
    assert!(matches!(moved, Err(sea_orm::DbErr::RecordNotFound(_))));
    let renamed = WorkspaceRepo::<deal::Entity>::update(
        &uow,
        acme.id,
        deal::ActiveModel {
            id: Set(created.id),
            workspace_id: Set(globex.id),
            title: Set("Renamed".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(renamed.workspace_id, acme.id, "the workspace cannot change");
    assert_eq!(renamed.title, "Renamed");
    let refused = WorkspaceRepo::<deal::Entity>::delete(&uow, globex.id, renamed.clone()).await;
    assert!(matches!(refused, Err(sea_orm::DbErr::RecordNotFound(_))));
    WorkspaceRepo::<deal::Entity>::delete(&uow, acme.id, renamed)
        .await
        .unwrap();
    assert!(
        deal::Entity::find()
            .all(&*uow.tx().await.unwrap())
            .await
            .unwrap()
            .is_empty()
    );
    uow.commit().await.unwrap();
}
