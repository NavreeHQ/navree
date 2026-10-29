//! `app.isolation_suite::<M>()` with `navree_workspace::testing`: real
//! workspaces, records from a model factory, and two deal modules, one
//! scoped with `WorkspaceRepo` and the record checks, one that reads by id.

use std::sync::Arc;

use axum::extract::Path;
use http::{Method, StatusCode};
use navree_access::{Authorized, Update};
use navree_db::Uow;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::factory::{Factory, FactoryContext, ModelFactory};
use navree_orm::prelude::*;
use navree_testing::IsolationOutcome;
use navree_workspace::testing::{IsolationSuiteExt, WorkspaceTenancy};
use navree_workspace::{CurrentWorkspace, InWorkspace, WorkspaceRepo};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Deal, World, builder, new_deal};

/// Deals with defaults; `workspace_id` is set by the suite.
#[derive(Default)]
struct DealFactory;

impl ModelFactory for DealFactory {
    type Model = Deal;

    fn definition(&self, ctx: &mut FactoryContext<'_, Deal>) -> Deal {
        new_deal(Uuid::nil(), &format!("Deal {}", ctx.index()))
    }
}

/// The suite's factory of deals.
fn deals() -> Factory<DealFactory> {
    Factory::new()
}

fn title(input: &Value) -> String {
    input["title"].as_str().unwrap_or("Renamed").to_owned()
}

// --- Scoped: WorkspaceRepo and the workspace record check -----------------

async fn index(ws: CurrentWorkspace, uow: Uow) -> Result<Json<Value>, AppError> {
    let deals = WorkspaceRepo::<Deal>::query(ws.id())
        .get(&uow)
        .await?
        .into_models();
    Ok(Json(json!({ "data": deals })))
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

/// `can(update, Deal, from = "id")`: the workspace check runs before the
/// handler.
async fn update(
    ws: CurrentWorkspace,
    uow: Uow,
    deal: Authorized<Deal, Update>,
    Json(input): Json<Value>,
) -> Result<Json<Deal>, AppError> {
    let mut record = deal.into_inner();
    record.title = title(&input);
    WorkspaceRepo::<Deal>::update(&uow, ws.id(), &mut record).await?;
    Ok(Json(record.into_inner()))
}

async fn destroy(
    ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let found = WorkspaceRepo::<Deal>::find_or_fail(&uow, ws.id(), id).await?;
    WorkspaceRepo::<Deal>::delete(&uow, ws.id(), found).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn store(
    ws: CurrentWorkspace,
    uow: Uow,
    Json(input): Json<Value>,
) -> Result<Json<Deal>, AppError> {
    let model = new_deal(Uuid::nil(), &title(&input));
    Ok(Json(
        WorkspaceRepo::<Deal>::insert(&uow, ws.id(), model)
            .await?
            .into_inner(),
    ))
}

struct DealsController;

impl Injectable for DealsController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DealsController) })
    }
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
            RouteDef::on(Method::GET, "/{id}", show)
                .permissions(&["deals.read"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::PATCH, "/{id}", update)
                .can("update", "Deal", Some("id"))
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::DELETE, "/{id}", destroy)
                .permissions(&["deals.delete"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::POST, "/", store)
                .permissions(&["deals.create"])
                .guard(InWorkspace::guard()),
        ]
    }
}

struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DealsController>();
    }
}

// --- Leaky: reads and writes by id, takes the workspace from the body -----

async fn leaky_index(uow: Uow) -> Result<Json<Value>, AppError> {
    let deals = Deal::query().get(&uow).await?.into_models();
    Ok(Json(json!({ "data": deals })))
}

async fn leaky_show(uow: Uow, Path(id): Path<Uuid>) -> Result<Json<Deal>, AppError> {
    Ok(Json(Deal::find_or_fail(&uow, id).await?.into_inner()))
}

async fn leaky_update(
    uow: Uow,
    Path(id): Path<Uuid>,
    Json(input): Json<Value>,
) -> Result<Json<Deal>, AppError> {
    let mut record = Deal::find_or_fail(&uow, id).await?;
    record.title = title(&input);
    record.save(&uow).await?;
    Ok(Json(record.into_inner()))
}

async fn leaky_destroy(uow: Uow, Path(id): Path<Uuid>) -> Result<StatusCode, AppError> {
    let mut found = Deal::find_or_fail(&uow, id).await?;
    found.delete(&uow).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn leaky_store(
    ws: CurrentWorkspace,
    uow: Uow,
    Json(input): Json<Value>,
) -> Result<Json<Deal>, AppError> {
    let workspace = input["workspace_id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or(ws.id());
    let model = new_deal(workspace, &title(&input));
    Ok(Json(Deal::create(&uow, model).await?.into_inner()))
}

struct LeakyController;

impl Injectable for LeakyController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(LeakyController) })
    }
}

impl Controller for LeakyController {
    fn prefix(&self) -> &'static str {
        "/leaky-deals"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", leaky_index)
                .permissions(&["deals.read"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::GET, "/{id}", leaky_show)
                .permissions(&["deals.read"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::PATCH, "/{id}", leaky_update)
                .permissions(&["deals.update"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::DELETE, "/{id}", leaky_destroy)
                .permissions(&["deals.delete"])
                .guard(InWorkspace::guard()),
            RouteDef::on(Method::POST, "/", leaky_store)
                .permissions(&["deals.create"])
                .guard(InWorkspace::guard()),
        ]
    }
}

struct LeakyModule;

impl Module for LeakyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<LeakyController>();
    }
}

async fn world() -> World {
    World::with(builder().module(DealsModule).module(LeakyModule)).await
}

fn details(report: &navree_testing::IsolationReport) -> Vec<(String, String)> {
    report
        .routes
        .iter()
        .map(|r| {
            let detail = match &r.outcome {
                IsolationOutcome::Passed(d) => format!("ok: {d}"),
                IsolationOutcome::Failed(d) => format!("FAIL: {d}"),
                IsolationOutcome::Skipped(d) => format!("skip: {d}"),
            };
            (format!("{} {}", r.method, r.path), detail)
        })
        .collect()
}

#[tokio::test]
async fn a_module_scoped_with_workspace_repo_passes() {
    let world = world().await;
    let report = world
        .app
        .isolation_suite::<DealsModule>()
        .factory(deals())
        .body("POST", "/api/deals", json!({ "title": "Renewal" }))
        .run()
        .await;
    assert_eq!(
        details(&report),
        [
            (
                "GET /api/deals".to_owned(),
                "ok: another workspace's records are not listed".to_owned()
            ),
            (
                "GET /api/deals/{id}".to_owned(),
                "ok: 404 from another workspace, 200 in its own".to_owned()
            ),
            (
                "PATCH /api/deals/{id}".to_owned(),
                "ok: 404 from another workspace; the record is unchanged".to_owned()
            ),
            (
                "DELETE /api/deals/{id}".to_owned(),
                "ok: 404 from another workspace; the record is unchanged".to_owned()
            ),
            (
                "POST /api/deals".to_owned(),
                "ok: 200; the record stays in the caller's workspace".to_owned()
            ),
        ]
    );
}

#[tokio::test]
async fn a_module_that_reads_by_id_fails_on_every_route() {
    let world = world().await;
    let report = world
        .app
        .isolation_suite::<LeakyModule>()
        .factory(deals())
        .body("POST", "/api/leaky-deals", json!({ "title": "Renewal" }))
        .try_run()
        .await
        .unwrap();
    assert_eq!(
        details(&report),
        [
            (
                "GET /api/leaky-deals".to_owned(),
                "FAIL: another workspace's record is listed (200)".to_owned()
            ),
            (
                "GET /api/leaky-deals/{id}".to_owned(),
                "FAIL: answered 200 to another workspace: its record was read".to_owned()
            ),
            (
                "PATCH /api/leaky-deals/{id}".to_owned(),
                "FAIL: answered 200 to another workspace: its record was changed".to_owned()
            ),
            (
                "DELETE /api/leaky-deals/{id}".to_owned(),
                "FAIL: answered 204 to another workspace: its record was deleted".to_owned()
            ),
            (
                "POST /api/leaky-deals".to_owned(),
                "FAIL: answered 200 and put the record in another workspace (workspace_id)"
                    .to_owned()
            ),
        ]
    );
}

#[tokio::test]
async fn factory_for_binds_a_named_parameter_and_tenancy_can_be_set() {
    let world = world().await;
    let report = world
        .app
        .isolation_suite::<DealsModule>()
        .tenancy(WorkspaceTenancy::new().owner_field("workspace_id"))
        .factory_for("id", deals())
        .skip("POST", "/api/deals", "creates are covered elsewhere")
        .run()
        .await;
    assert_eq!(report.passed(), 4);
    assert_eq!(report.skipped(), 1);
}

#[tokio::test]
async fn the_header_resolver_is_required() {
    let world = World::with(
        builder()
            .config("workspace.resolvers", "path")
            .module(DealsModule),
    )
    .await;
    let error = world
        .app
        .isolation_suite::<DealsModule>()
        .factory(deals())
        .try_run()
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "isolation suites name the workspace in the `X-Workspace` header, but workspace.resolvers has no `header`: add it in the test app"
    );
}
