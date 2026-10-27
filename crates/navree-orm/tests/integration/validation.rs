//! Validation rules that query models: `unique` and `exists` with their
//! options, per workspace, and under `ValidAsync` (the route's record
//! ignored by route key).

use std::sync::Arc;

use garde::Validate;
use http::Method;
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::validation::{exists, unique};
use navree_orm::{Model, Record};
use navree_validation::{AsyncRule, AsyncRules, ValidAsync};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::models::{Company, Deal, Project};
use crate::support::{app, sqlite};

#[tokio::test]
async fn unique_checks_the_model_with_its_options() {
    let test = sqlite().await;
    let db = &test.db;
    let acme = Deal::factory()
        .set(Deal::SLUG, "acme")
        .create_one(db)
        .await
        .unwrap();
    let mut gone = Deal::factory()
        .set(Deal::SLUG, "gone")
        .create_one(db)
        .await
        .unwrap();
    gone.delete(db).await.unwrap();

    let uow = db.uow(None, None).await;
    let passes = |rule: AsyncRule| {
        let uow = uow.clone();
        async move { rule.passes_in(&uow).await.unwrap() }
    };
    assert!(!passes(unique(Deal::SLUG, "acme".to_owned()).into()).await);
    assert!(passes(unique(Deal::SLUG, "fresh".to_owned()).into()).await);
    assert!(
        passes(unique(Deal::SLUG, "acme".to_owned()).ignore(acme.id).into()).await,
        "the record being updated keeps its value"
    );
    assert!(
        !passes(unique(Deal::SLUG, "acme".to_owned()).ignore(gone.id).into()).await,
        "another record does not"
    );
    // Soft-deleted rows still hold their values, unless left out.
    assert!(!passes(unique(Deal::SLUG, "gone".to_owned()).into()).await);
    assert!(
        passes(
            unique(Deal::SLUG, "gone".to_owned())
                .without_trashed()
                .into()
        )
        .await
    );
    // Extra conditions narrow the records that count.
    assert!(
        passes(
            unique(Deal::SLUG, "acme".to_owned())
                .where_(Deal::AMOUNT_CENTS.lt(0))
                .into()
        )
        .await
    );
    // A null value passes without a query.
    assert!(passes(unique(Deal::COMPANY_ID, None::<Option<i64>>).into()).await);
    assert!(passes(unique(Deal::COMPANY_ID, Some(None::<i64>)).into()).await);
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn exists_checks_the_model_with_its_options() {
    let test = sqlite().await;
    let db = &test.db;
    let acme = Company::factory()
        .set(Company::NAME, "Acme")
        .create_one(db)
        .await
        .unwrap();
    let mut closed = Company::factory().create_one(db).await.unwrap();
    closed.delete(db).await.unwrap();

    let uow = db.uow(None, None).await;
    let rule = |id: Option<i64>| exists(Company::ID, id).field("company_id");
    let check = |rule: AsyncRule| {
        let uow = uow.clone();
        async move { rule.passes_in(&uow).await.unwrap() }
    };
    let made: AsyncRule = rule(Some(acme.id)).into();
    assert_eq!((made.field(), made.rule()), ("company_id", "exists"));
    assert!(check(made).await);
    assert!(!check(rule(Some(acme.id + 1_000)).into()).await);
    assert!(check(rule(None).into()).await, "no value, no rule");
    // A deleted company is gone, unless counted.
    assert!(!check(rule(Some(closed.id)).into()).await);
    assert!(check(rule(Some(closed.id)).with_trashed().into()).await);
    assert!(
        !check(
            rule(Some(acme.id))
                .where_(Company::VERIFIED.eq(true))
                .into()
        )
        .await
    );
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn unique_holds_within_the_current_workspace() {
    let test = sqlite().await;
    let db = &test.db;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    let uow = db.uow(None, None).await;
    uow.set_workspace(Some(a));
    Project::factory()
        .set(Project::NAME, "Apollo")
        .create_one(&uow)
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let in_workspace = |workspace: Uuid, rule: AsyncRule| async move {
        let uow = db.uow(None, None).await;
        uow.set_workspace(Some(workspace));
        let passes = rule.passes_in(&uow).await.unwrap();
        uow.rollback().await.unwrap();
        passes
    };
    let apollo = || unique(Project::NAME, "Apollo".to_owned());
    assert!(!in_workspace(a, apollo().into()).await);
    assert!(
        in_workspace(b, apollo().into()).await,
        "another workspace may use the name"
    );
    assert!(
        !in_workspace(
            b,
            apollo()
                .query(|query| query.without_workspace_scope())
                .into()
        )
        .await,
        "unique across every workspace"
    );
    let apollo_exists = || exists(Project::NAME, "Apollo".to_owned());
    assert!(in_workspace(a, apollo_exists().into()).await);
    assert!(!in_workspace(b, apollo_exists().into()).await);

    // Without a workspace the scoped query fails closed: the rule cannot
    // be checked.
    let uow = db.uow(None, None).await;
    let rule: AsyncRule = apollo().into();
    assert!(rule.passes_in(&uow).await.is_err());
    uow.rollback().await.unwrap();
}

/// The body of `POST /deals` and `PATCH /deals/{deal}`.
#[derive(Debug, Deserialize, Validate)]
struct DealInput {
    #[garde(length(min = 1))]
    slug: String,
    #[garde(skip)]
    company_id: Option<i64>,
}

impl AsyncRules for DealInput {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![
            unique(Deal::SLUG, self.slug.clone())
                .ignore_path_param("deal")
                .into(),
            exists(Company::ID, self.company_id)
                .field("company_id")
                .into(),
        ]
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
                Method::POST,
                "/deals",
                |ValidAsync(Json(input)): ValidAsync<Json<DealInput>>| async move {
                    Json(json!({ "slug": input.slug }))
                },
            )
            .public(),
            RouteDef::on(
                Method::PATCH,
                "/deals/{deal}",
                |ValidAsync(Json(input)): ValidAsync<Json<DealInput>>| async move {
                    Json(json!({ "slug": input.slug }))
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
async fn valid_async_runs_model_rules_in_the_request() {
    let (app, db, _clock) = app(vec![Box::new(RoutesModule)]).await;
    let company = Company::factory().create_one(&*db).await.unwrap();
    let _acme: Record<Deal> = Deal::factory()
        .set(Deal::SLUG, "acme")
        .create_one(&*db)
        .await
        .unwrap();
    Deal::factory()
        .set(Deal::SLUG, "pilot")
        .create_one(&*db)
        .await
        .unwrap();

    let response = app
        .client()
        .post("/api/deals")
        .json(&json!({ "slug": "acme", "company_id": company.id + 1_000 }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        response.json_value()["errors"],
        json!({
            "slug": ["slug has already been taken"],
            "company_id": ["company_id does not exist"],
        })
    );
    app.client()
        .post("/api/deals")
        .json(&json!({ "slug": "fresh", "company_id": company.id }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("slug", json!("fresh"));

    // An update keeps its own slug (the route key), not another's.
    app.client()
        .patch("/api/deals/acme")
        .json(&json!({ "slug": "acme", "company_id": null }))
        .send()
        .await
        .assert_status(200);
    app.client()
        .patch("/api/deals/pilot")
        .json(&json!({ "slug": "acme", "company_id": null }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(Deal::query().count(&*db).await.unwrap(), 2);
}
