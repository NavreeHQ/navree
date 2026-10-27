//! Models behind HTTP routes: a missing record is a 404 in Navree's error
//! shape, pages serialize as Navree's page shapes, bad cursors are 400s,
//! and writes run in the request's unit of work.

use std::sync::Arc;

use axum::extract::Path;
use http::{Method, StatusCode};
use navree_db::{Db, PageQuery, Uow};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_orm::{CursorPage, CursorRequest, Model, Page, Record};
use serde_json::json;
use uuid::Uuid;

use crate::models::{Company, Deal};
use crate::support::{app, count_rows};

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
            RouteDef::on(Method::GET, "/{id}", |uow: Uow, Path(id): Path<Uuid>| async move {
                let deal = Deal::find_or_fail(&uow, id).await?;
                Ok::<_, AppError>(Json(deal))
            })
            .public(),
            RouteDef::on(Method::GET, "/", |uow: Uow, page: PageQuery| async move {
                let page: Page<Record<Deal>> = Deal::query().order_by(Deal::TITLE).paginate(&uow, page).await?;
                Ok::<_, AppError>(page)
            })
            .public(),
            RouteDef::on(Method::POST, "/titled/{title}", |uow: Uow, Path(title): Path<String>| async move {
                let deal = Deal::create(&uow, Deal::new(&title, 100)).await?;
                Ok::<_, AppError>((StatusCode::CREATED, Json(deal)))
            })
            .public(),
            RouteDef::on(Method::POST, "/titled/{title}/fail", |uow: Uow, Path(title): Path<String>| async move {
                Deal::create(&uow, Deal::new(&title, 100)).await?;
                Err::<(), _>(AppError::bad_request("changed my mind"))
            })
            .public(),
        ]
    }
}

/// Reads through the injected pool (no unit of work).
struct FeedController {
    db: Arc<Db>,
}

impl Injectable for FeedController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Db>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(FeedController { db: r.get::<Db>().await? }) })
    }
}

impl Controller for FeedController {
    fn prefix(&self) -> &'static str {
        "/feed"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let db = self.db.clone();
        vec![
            RouteDef::on(Method::GET, "/", move |cursor: CursorRequest| {
                let db = db.clone();
                async move {
                    let page: CursorPage<Record<Company>> =
                        Company::query().cursor_paginate(&db, cursor).await?;
                    Ok::<_, AppError>(page)
                }
            })
            .public(),
        ]
    }
}

struct Routes;

impl Module for Routes {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DealsController>();
        m.controller::<FeedController>();
    }
}

#[tokio::test]
async fn a_missing_record_is_a_404_in_the_error_shape() {
    let (app, _db, _clock) = app(vec![Box::new(Routes)]).await;
    let response = app
        .client()
        .get(&format!("/api/deals/{}", Uuid::now_v7()))
        .send()
        .await
        .assert_error(404, None);
    let body = response.json_value();
    assert_eq!(body["statusCode"], 404);
    assert_eq!(body["error"], "Not Found");
}

#[tokio::test]
async fn records_and_pages_serialize_in_navrees_shapes() {
    let (app, db, _clock) = app(vec![Box::new(Routes)]).await;
    let created = app
        .client()
        .post("/api/deals/titled/Renewal")
        .send()
        .await
        .assert_status(201)
        .json_value();
    assert_eq!(created["title"], "Renewal");
    let id = created["id"].as_str().unwrap().to_owned();
    app.client()
        .get(&format!("/api/deals/{id}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Renewal"));

    for title in ["A", "B"] {
        Deal::create(&*db, Deal::new(title, 1)).await.unwrap();
    }
    app.client()
        .get("/api/deals?page=1&per_page=2")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta", json!({"current_page": 1, "per_page": 2, "total": 3, "last_page": 2}))
        .assert_json_path("data.0.title", json!("A"));
}

#[tokio::test]
async fn writes_commit_or_roll_back_with_the_request() {
    let (app, db, _clock) = app(vec![Box::new(Routes)]).await;
    app.client().post("/api/deals/titled/Kept").send().await.assert_status(201);
    app.client()
        .post("/api/deals/titled/Dropped/fail")
        .send()
        .await
        .assert_error(400, "changed my mind");
    assert_eq!(count_rows(&db, "deals").await, 1);
}

#[tokio::test]
async fn cursor_requests_reject_malformed_cursors() {
    let (app, db, _clock) = app(vec![Box::new(Routes)]).await;
    for name in ["A", "B", "C"] {
        Company::create(&*db, Company { name: name.into(), ..Company::default() })
            .await
            .unwrap();
    }
    let first = app.client().get("/api/feed?limit=2").send().await.assert_status(200).json_value();
    assert_eq!(first["data"].as_array().unwrap().len(), 2);
    assert!(first["prev_cursor"].is_null());
    let next = first["next_cursor"].as_str().unwrap().to_owned();
    app.client()
        .get(&format!("/api/feed?limit=2&cursor={next}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.name", json!("C"))
        .assert_json_path("next_cursor", json!(null));

    app.client()
        .get("/api/feed?cursor=not-a-cursor")
        .send()
        .await
        .assert_validation_error("cursor");
    app.client()
        .get("/api/feed?limit=0")
        .send()
        .await
        .assert_validation_error("limit");
}
