//! Route model binding: records by route key, by key, by custom bindings,
//! soft-deleted records on opt-in routes, nested children, and the 404s
//! and 500s in Navree's error shape.

use std::sync::Arc;

use http::Method;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::binding::{Bound, BoundChild, ChildOf, RouteBinding, WithTrashed};
use navree_orm::{Builder, Model, Record, Value};
use serde_json::json;

use crate::models::{Company, Deal, Note};
use crate::support::app;

impl ChildOf<Company> for Deal {
    fn children_of(company: &Record<Company>, query: Builder<Deal>) -> Builder<Deal> {
        query.where_(Deal::COMPANY_ID.eq(company.id))
    }
}

/// Deals by key, from `{key}` (their route key is the slug).
struct ByKey;

impl RouteBinding<Deal> for ByKey {
    const PARAM: Option<&'static str> = Some("key");

    fn query(query: Builder<Deal>, value: &str) -> Option<Builder<Deal>> {
        let id = uuid::Uuid::parse_str(value).ok()?;
        Some(query.where_key(id))
    }
}

/// Deals by title, ignoring case.
struct ByTitle;

impl RouteBinding<Deal> for ByTitle {
    fn query(query: Builder<Deal>, value: &str) -> Option<Builder<Deal>> {
        Some(query.where_like(Deal::TITLE, value.to_owned()))
    }
}

struct Routes;

impl Injectable for Routes {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Routes) })
    }
}

impl Controller for Routes {
    fn prefix(&self) -> &'static str {
        "/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "/deals/{deal}",
                |Bound(deal, _): Bound<Deal>| async move { Json(json!({ "title": deal.title })) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/deals/{deal}/trashed",
                |deal: Bound<Deal, WithTrashed>| async move {
                    Json(json!({ "title": deal.title, "trashed": deal.trashed() }))
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/companies/{id}",
                |company: Bound<Company>| async move { Json(json!({ "name": company.name })) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/notes/{note}",
                |note: Bound<Note>| async move { Json(json!({ "body": note.body })) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/by-key/{key}",
                |deal: Bound<Deal, ByKey>| async move { Json(json!({ "title": deal.title })) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/titles/{title}",
                |deal: Bound<Deal, ByTitle>| async move { Json(json!({ "slug": deal.slug })) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/companies/{company}/deals/{deal}",
                |pair: BoundChild<Company, Deal>| async move {
                    Json(json!({ "company": pair.parent().name, "deal": pair.child().title }))
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/accounts/{account}/items/{item}",
                |pair: BoundChild<Company, Deal>| async move {
                    let (company, deal) = pair.into_parts();
                    Json(json!({ "company": company.name, "deal": deal.title }))
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/oops/{a}/{b}",
                |deal: Bound<Deal>| async move {
                    Ok::<_, AppError>(Json(json!({ "title": deal.title })))
                },
            )
            .public(),
        ]
    }
}

struct RoutesModule;

impl Module for RoutesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Routes>();
    }
}

#[tokio::test]
async fn records_bind_by_their_route_key() {
    let (app, db, _clock) = app(vec![Box::new(RoutesModule)]).await;
    let deal = Deal::factory()
        .set(Deal::SLUG, "acme-renewal")
        .set(Deal::TITLE, "Acme renewal")
        .create_one(&*db)
        .await
        .unwrap();
    assert_eq!(deal.route_key().unwrap(), Value::from("acme-renewal"));
    assert_eq!(Deal::route_key_name(), "slug");
    app.client()
        .get("/api/deals/acme-renewal")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Acme renewal"));
    app.client()
        .get("/api/deals/nothing-here")
        .send()
        .await
        .assert_error(404, "Not Found");

    // Without a route key: the key, read as the key's type.
    let company = Company::factory()
        .set(Company::NAME, "Acme")
        .create_one(&*db)
        .await
        .unwrap();
    assert_eq!(Company::route_key_name(), "id");
    app.client()
        .get(&format!("/api/companies/{}", company.id))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Acme"));
    app.client()
        .get("/api/companies/not-a-number")
        .send()
        .await
        .assert_error(404, "Not Found");
}

#[tokio::test]
async fn malformed_keys_are_404s() {
    let (app, db, _clock) = app(vec![Box::new(RoutesModule)]).await;
    let note = Note::factory().create_one(&*db).await.unwrap();
    app.client()
        .get(&format!("/api/notes/{}", note.id))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("body", json!(note.body));
    app.client()
        .get("/api/notes/not-a-ulid")
        .send()
        .await
        .assert_error(404, "Not Found");
    app.client()
        .get("/api/by-key/not-a-uuid")
        .send()
        .await
        .assert_error(404, "Not Found");
}

#[tokio::test]
async fn routes_choose_their_own_bindings() {
    let (app, db, _clock) = app(vec![Box::new(RoutesModule)]).await;
    let deal = Deal::factory()
        .set(Deal::TITLE, "Pilot")
        .set(Deal::SLUG, "pilot")
        .create_one(&*db)
        .await
        .unwrap();
    app.client()
        .get(&format!("/api/by-key/{}", deal.id))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Pilot"));
    app.client()
        .get("/api/titles/PILOT")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("pilot"));
}

#[tokio::test]
async fn soft_deleted_records_bind_only_on_routes_that_opt_in() {
    let (app, db, _clock) = app(vec![Box::new(RoutesModule)]).await;
    let mut deal = Deal::factory()
        .set(Deal::SLUG, "gone")
        .create_one(&*db)
        .await
        .unwrap();
    deal.delete(&*db).await.unwrap();
    app.client()
        .get("/api/deals/gone")
        .send()
        .await
        .assert_error(404, "Not Found");
    app.client()
        .get("/api/deals/gone/trashed")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("trashed", json!(true));
}

#[tokio::test]
async fn children_bind_under_their_parent_only() {
    let (app, db, _clock) = app(vec![Box::new(RoutesModule)]).await;
    let acme = Company::factory()
        .set(Company::NAME, "Acme")
        .create_one(&*db)
        .await
        .unwrap();
    let globex = Company::factory()
        .set(Company::NAME, "Globex")
        .create_one(&*db)
        .await
        .unwrap();
    Deal::factory()
        .for_record(&acme, Deal::COMPANY_ID)
        .set(Deal::SLUG, "acme-deal")
        .set(Deal::TITLE, "Acme deal")
        .create(&*db)
        .await
        .unwrap();
    app.client()
        .get(&format!("/api/companies/{}/deals/acme-deal", acme.id))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("company", json!("Acme"))
        .assert_json_path("deal", json!("Acme deal"));
    app.client()
        .get(&format!("/api/companies/{}/deals/acme-deal", globex.id))
        .send()
        .await
        .assert_error(404, "Not Found");
    // Parameters not named after the models: the first and the last.
    app.client()
        .get(&format!("/api/accounts/{}/items/acme-deal", acme.id))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("deal", json!("Acme deal"));
}

#[tokio::test]
async fn a_route_without_a_usable_parameter_is_a_500() {
    let (app, _db, _clock) = app(vec![Box::new(RoutesModule)]).await;
    app.client()
        .get("/api/oops/1/2")
        .send()
        .await
        .assert_error(500, None);
}
