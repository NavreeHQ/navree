//! Every public item the Helm docs page names
//! (`apps/website/content/docs/tools/helm.mdx`): the module and its
//! options, the `Resource` trait and its constants, every field, filter,
//! action and metric builder, the contexts, custom pages, the job, the
//! cookie, the subject, the configuration keys and the JSON API routes. A
//! rename fails this file.

#[path = "helm/servers.rs"]
mod servers;
#[path = "helm/support.rs"]
mod support;

use navree_access::{ActionMarker, Subject};
use navree_config::ConfigSection;
use navree_contracts::Job;
use navree_helm::{
    Action, ActionContext, ActionResult, DEFAULT_MAX_FILE_BYTES, Field, Filter, FormData,
    HELM_SUBJECT, Helm, HelmActionJob, HelmModule, HelmOptions, HelmPage, Metric, MetricRange,
    ModelOf, ModuleBuilderHelmExt, PageContext, RecordOf, Resource, SESSION_COOKIE, SaveContext,
    VIEW,
};
use navree_http::AppError;
use navree_kernel::{BoxFuture, ModuleBuilder};
use navree_orm::{Builder, Record};
use serde_json::json;

use support::{CompanyResource, Deal, DealResource, Move, STAGES};

/// A resource naming every constant and method of the trait.
struct EveryItem;

impl Resource for EveryItem {
    type Model = Deal;

    const LABEL: &'static str = "Deals";
    const SINGULAR: &'static str = "Deal";
    const GROUP: &'static str = "Sales";
    const ICON: &'static str = "handshake";
    const TITLE: &'static str = "title";
    const SEARCH: &'static [&'static str] = &["title"];
    const SLUG: &'static str = "all-deals";
    const PER_PAGE: u64 = 50;
    const DEFAULT_SORT: &'static str = "-amount_cents";
    const ORDER: i32 = 1;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::id(),
            Field::text("title")
                .label("Title")
                .help("Shown under the input")
                .required()
                .min_length(1)
                .max_length(200)
                .pattern("^[A-Za-z ]+$")
                .sortable(),
            Field::textarea("lost_reason").hide_on_index(),
            Field::markdown("notes").hide_on_detail(),
            Field::number("amount_cents").min(0).max(1_000_000),
            Field::money("amount_cents", "currency"),
            Field::boolean("priority").default(false),
            Field::select("stage").options(STAGES).badge(),
            Field::date("expected_close_date"),
            Field::datetime("created_at").readonly(),
            Field::email("title").hide_on_forms(),
            Field::url("title").only_on_forms(),
            Field::file("notes", "local")
                .accept(["image/*"])
                .max_size(DEFAULT_MAX_FILE_BYTES),
            Field::belongs_to::<CompanyResource>("company_id"),
            Field::has_many::<DealResource>("company_id"),
            Field::computed("probability", |deal: &Deal| deal.stage.clone()),
            Field::view("summary", "admin/summary"),
        ]
    }

    fn filters(&self) -> Vec<Filter<Deal>> {
        vec![
            Filter::select("stage").options(STAGES),
            Filter::belongs_to::<CompanyResource>("company_id").label("Company"),
            Filter::date_range("expected_close_date"),
            Filter::boolean("open", "Open deals only", |query, on| {
                if on {
                    query.where_(Deal::CLOSED_AT.is_null())
                } else {
                    query
                }
            }),
            Filter::custom("min", "At least", |query, value| {
                query.where_(Deal::AMOUNT_CENTS.gte(value.parse::<i64>().unwrap_or(0)))
            }),
        ]
    }

    fn actions(&self) -> Vec<Action<Deal>> {
        vec![
            Action::new("mark-lost", "Mark as lost")
                .ability(Move)
                .confirm("Sure?")
                .destructive()
                .fields(vec![Field::textarea("lost_reason").required()])
                .only_on_detail()
                .handle(|ctx: ActionContext, deals: Vec<Record<Deal>>| async move {
                    let _ = (ctx.uow(), ctx.user_id(), ctx.app(), deals.len());
                    let _: String = ctx.input("lost_reason")?;
                    let _ = ctx.inputs();
                    let _ = ctx.get::<HelmOptions>().await?;
                    Ok(ActionResult::message("Done."))
                }),
            Action::new("export", "Export")
                .only_on_index()
                .queued()
                .handle(|_ctx, _deals| async move { Ok(ActionResult::download("/files/a.csv")) }),
            Action::new("open", "Open").handle(|_ctx, _deals| async move {
                Ok(ActionResult::redirect("https://example.com"))
            }),
        ]
    }

    fn metrics(&self) -> Vec<Metric<Deal>> {
        vec![
            Metric::value("Open pipeline")
                .sum(Deal::AMOUNT_CENTS)
                .filter(Deal::CLOSED_AT.is_null())
                .money("USD")
                .on_dashboard(),
            Metric::value("Average").average(Deal::AMOUNT_CENTS),
            Metric::value("Smallest").min(Deal::AMOUNT_CENTS),
            Metric::value("Largest")
                .max(Deal::AMOUNT_CENTS)
                .over(Deal::CREATED_AT),
            Metric::trend("By day").count().by_day(Deal::CREATED_AT, 30),
            Metric::trend("By week")
                .count()
                .by_week(Deal::CREATED_AT, 12),
            Metric::trend("By month")
                .count()
                .by_month(Deal::CREATED_AT, 12),
            Metric::partition("By stage").count().group_by(Deal::STAGE),
        ]
    }

    fn query(&self, query: Builder<Deal>) -> Builder<Deal> {
        query.where_(Deal::CLOSED_AT.is_null())
    }

    fn create<'a>(
        &'a self,
        ctx: &'a SaveContext,
        form: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>, AppError>> {
        Box::pin(async move {
            let _ = (ctx.uow(), ctx.user_id(), form.values(), form.get("title"));
            let _: support::NewDeal = form.parse().await?;
            Err(AppError::conflict("not saved"))
        })
    }

    fn update<'a>(
        &'a self,
        ctx: &'a SaveContext,
        record: Record<Deal>,
        form: FormData,
    ) -> BoxFuture<'a, Result<Record<Deal>, AppError>> {
        Box::pin(async move {
            let _ = (ctx.get::<HelmOptions>().await?, form.contains("title"));
            Ok(record)
        })
    }

    fn delete<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _record: Record<Deal>,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async { Ok(()) })
    }
}

/// `m.resource::<R>()` and `m.helm_page(..)`.
fn wiring(m: &mut ModuleBuilder) {
    m.resource::<DealResource>().helm_page(
        HelmPage::new("pipeline", "Pipeline board")
            .group("Sales")
            .icon("kanban")
            .template("admin/pipeline")
            .can("read", "Deal")
            .data(|ctx: PageContext| async move {
                let _ = (ctx.uow(), ctx.user_id(), ctx.get::<HelmOptions>().await?);
                Ok(json!({ "columns": [] }))
            }),
    );
}

#[test]
fn the_documented_api_exists() {
    // The module: from_config and for_root with options.
    let _ = HelmModule::from_config();
    let _ = HelmModule::for_root(HelmOptions::default());
    let _ = wiring;
    let _ = EveryItem;

    // The `helm` section and its defaults.
    assert_eq!(HelmOptions::KEY, "helm");
    let HelmOptions {
        path,
        title,
        logo,
        accent,
        per_page,
        metrics_cache_secs,
        dashboard,
        session_minutes,
    } = HelmOptions::default();
    assert_eq!(path, "/admin");
    assert_eq!(title, "Admin");
    assert_eq!(logo, None);
    assert_eq!(accent, "#3dff8f");
    assert_eq!(per_page, 25);
    assert_eq!(metrics_cache_secs, 60);
    assert!(dashboard);
    assert_eq!(session_minutes, 480);

    // The subject, its permission and the cookie.
    assert_eq!(Helm::SUBJECT, HELM_SUBJECT);
    assert_eq!(Helm::PERMISSION_PREFIX, "helm");
    assert_eq!(VIEW, "view");
    assert_eq!(SESSION_COOKIE, "navree_helm");
    assert_eq!(Move::NAME, "move");

    // Defaults of the trait's constants.
    assert_eq!(<CompanyResource as Resource>::SINGULAR, "Company");
    assert_eq!(<DealResource as Resource>::SLUG, "");
    assert_eq!(<DealResource as Resource>::PER_PAGE, 0);
    assert_eq!(<DealResource as Resource>::ORDER, 0);

    // The model of a resource, and its records.
    let _: Option<ModelOf<DealResource>> = None::<Deal>;
    let _: Option<RecordOf<DealResource>> = None::<Record<Deal>>;

    // 10 MiB per file by default.
    assert_eq!(DEFAULT_MAX_FILE_BYTES, 10 * 1024 * 1024);

    // Metric ranges: 7, 30 (the default), 90 days, this month, this year.
    assert_eq!(MetricRange::default(), MetricRange::Days30);
    for name in ["7", "30", "90", "month", "year"] {
        assert_eq!(MetricRange::parse(name).unwrap().as_str(), name);
    }
    assert_eq!(
        Metric::<Deal>::value("Open pipeline").slug(),
        "open-pipeline"
    );

    // Queued actions are `helm.action` jobs.
    assert_eq!(HelmActionJob::NAME, "helm.action");
    let HelmActionJob {
        resource,
        action,
        ids,
        input,
        user_id,
    } = HelmActionJob {
        resource: "deals".into(),
        action: "export".into(),
        ids: vec![],
        input: Default::default(),
        user_id: uuid::Uuid::nil(),
    };
    let _ = (resource, action, ids, input, user_id);

    // Results serialize with their type.
    assert_eq!(
        serde_json::to_value(ActionResult::message("Done.")).unwrap()["type"],
        "message"
    );
}

#[tokio::test]
async fn the_documented_routes_answer() {
    let app = support::app().await;
    let (_, admin) = support::user(&app, support::ADMIN).await;
    let deal = support::deal(&app, support::DealSeed::default()).await;
    let id = deal.id.to_string();
    for path in [
        "/admin/api/resources".to_owned(),
        "/admin/api/resources/deals".to_owned(),
        format!("/admin/api/resources/deals/{id}"),
        "/admin/api/resources/companies/options".to_owned(),
        "/admin/api/resources/deals/metrics/open-pipeline".to_owned(),
    ] {
        admin.get(&path).send().await.assert_status(200);
    }
    admin
        .post("/admin/api/resources/deals")
        .json(&json!({ "title": "New", "amount_cents": 100 }))
        .send()
        .await
        .assert_status(201);
    admin
        .patch(&format!("/admin/api/resources/deals/{id}"))
        .json(&json!({ "title": "Renamed" }))
        .send()
        .await
        .assert_status(200);
    admin
        .post("/admin/api/resources/deals/actions/mark-won")
        .json(&json!({ "ids": [id] }))
        .send()
        .await
        .assert_status(200);
    admin
        .delete(&format!("/admin/api/resources/deals/{id}"))
        .send()
        .await
        .assert_status(204);
    // The sign-in screen is public.
    app.client()
        .get("/admin/login")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
}
