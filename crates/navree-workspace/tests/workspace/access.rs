//! navree-access integration: route checks evaluated against the member's
//! role in the current workspace, and workspace-scoped record checks.

use std::sync::Arc;

use http::Method;
use navree_access::{Abilities, Authorized, CurrentAccess, Update};
use navree_db::Uow;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::prelude::*;
use navree_workspace::{CurrentWorkspace, InWorkspace, WorkspaceRepo};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Deal, World, builder, new_deal};

/// Deal routes under `/api/deals`, guarded by `InWorkspace`, plus one
/// route without the guard.
struct DealsController;

impl Injectable for DealsController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DealsController) })
    }
}

async fn index(ws: CurrentWorkspace, uow: Uow) -> Result<Json<Value>, AppError> {
    let deals = WorkspaceRepo::<Deal>::query(ws.id())
        .get(&uow)
        .await?
        .into_models();
    Ok(Json(json!({ "data": deals })))
}

async fn store(
    ws: CurrentWorkspace,
    uow: Uow,
    Json(input): Json<Value>,
) -> Result<Json<Deal>, AppError> {
    let created = WorkspaceRepo::<Deal>::insert(
        &uow,
        ws.id(),
        new_deal(Uuid::nil(), input["title"].as_str().unwrap_or_default()),
    )
    .await?;
    Ok(Json(created.into_inner()))
}

async fn update(deal: Authorized<Deal, Update>) -> Json<Value> {
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
) -> Result<Json<Deal>, AppError> {
    Ok(Json(
        WorkspaceRepo::<Deal>::find_or_fail(&uow, ws.id(), id)
            .await?
            .into_inner(),
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
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    world.join(agra.id, grace, "member").await;
    world.join(gulati.id, grace, "viewer").await;
    let client = world.as_user(grace);

    // A member has `deals.*` in Agra; a viewer only `deals.read` in Gulati.
    client
        .post("/api/deals")
        .header("x-workspace", "agra")
        .json(&json!({ "title": "Big one" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace_id", json!(agra.id));
    client
        .post("/api/deals")
        .header("x-workspace", "gulati")
        .json(&json!({ "title": "Nope" }))
        .send()
        .await
        .assert_error(403, None);
    client
        .get("/api/deals")
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
    client
        .get("/api/deals")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.title", json!("Big one"));

    // Roles are the workspace role; non-members are refused before any check.
    client
        .get("/api/deals/admin-report")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error(403, None);
    world
        .join(
            agra.id,
            world.user("Laksh", "laksh@example.com").await,
            "admin",
        )
        .await;
    let eve = world.user("Ela", "ela@example.com").await;
    world
        .as_user(eve)
        .get("/api/deals")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.");

    // Without the guard the global (empty) permissions apply.
    client
        .get("/api/deals/unguarded")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn the_abilities_function_sees_the_workspace() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    world.join(agra.id, grace, "viewer").await;

    let response = world
        .as_user(grace)
        .get("/api/deals/abilities")
        .header("x-workspace", "agra")
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
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn records_of_another_workspace_are_not_found() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    let insert = |workspace: Uuid, title: &str| {
        let world = &world;
        let title = title.to_owned();
        async move {
            let uow = world.db().await.uow(None, None).await;
            let created =
                WorkspaceRepo::<Deal>::insert(&uow, workspace, new_deal(Uuid::nil(), &title))
                    .await
                    .unwrap();
            uow.commit().await.unwrap();
            created
        }
    };
    let ours = insert(agra.id, "Ours").await;
    let theirs = insert(gulati.id, "Theirs").await;
    let client = world.as_user(ada);

    client
        .patch(&format!("/api/deals/{}", ours.id))
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Ours"));
    // Anu owns both workspaces (`all.*`), so only the workspace check
    // stands between Agra's request and Gulati's record.
    client
        .patch(&format!("/api/deals/{}", theirs.id))
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error(404, None);
    client
        .get(&format!("/api/deals/{}", theirs.id))
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_error(404, None);
    client
        .get(&format!("/api/deals/{}", theirs.id))
        .header("x-workspace", "gulati")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn the_repository_scopes_every_query() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(ada, "Gulati").await;
    let uow = world.db().await.uow(None, None).await;
    let created = WorkspaceRepo::<Deal>::insert(&uow, agra.id, new_deal(gulati.id, "Agra deal"))
        .await
        .unwrap();
    assert_eq!(created.workspace_id, agra.id, "insert sets the workspace");
    assert!(
        WorkspaceRepo::<Deal>::find(&uow, gulati.id, created.id)
            .await
            .unwrap()
            .is_none()
    );
    let mut stolen = Record::from(Deal {
        id: created.id,
        workspace_id: gulati.id,
        title: "Stolen".into(),
    });
    let moved = WorkspaceRepo::<Deal>::update(&uow, gulati.id, &mut stolen).await;
    assert!(
        matches!(moved, Err(OrmError::ModelNotFound { .. })),
        "{moved:?}"
    );
    let mut renamed = WorkspaceRepo::<Deal>::find_or_fail(&uow, agra.id, created.id)
        .await
        .unwrap();
    renamed.workspace_id = gulati.id;
    renamed.title = "Renamed".into();
    WorkspaceRepo::<Deal>::update(&uow, agra.id, &mut renamed)
        .await
        .unwrap();
    assert_eq!(renamed.workspace_id, agra.id, "the workspace cannot change");
    assert_eq!(renamed.title, "Renamed");
    let stored = WorkspaceRepo::<Deal>::find_or_fail(&uow, agra.id, created.id)
        .await
        .unwrap();
    assert_eq!(
        (stored.workspace_id, stored.title.as_str()),
        (agra.id, "Renamed")
    );
    // A record built from a model replaces the stored values.
    let mut replaced = Record::from(Deal {
        id: created.id,
        workspace_id: gulati.id,
        title: "Replaced".into(),
    });
    WorkspaceRepo::<Deal>::update(&uow, agra.id, &mut replaced)
        .await
        .unwrap();
    assert_eq!(replaced.workspace_id, agra.id);
    assert_eq!(replaced.title, "Replaced");
    let refused = WorkspaceRepo::<Deal>::delete(&uow, gulati.id, replaced.clone()).await;
    assert!(
        matches!(refused, Err(OrmError::ModelNotFound { .. })),
        "{refused:?}"
    );
    WorkspaceRepo::<Deal>::delete(&uow, agra.id, replaced)
        .await
        .unwrap();
    assert_eq!(
        Deal::query()
            .without_workspace_scope()
            .count(&uow)
            .await
            .unwrap(),
        0
    );
    uow.commit().await.unwrap();
}
