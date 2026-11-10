//! Custom pages and view fields: app templates inside the panel's layout.

use std::sync::Arc;

use navree_helm::{Field, HelmPage, ModuleBuilderHelmExt, Resource};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::Model;
use navree_testing::principal;
use navree_view::{ModuleBuilderViewExt, ViewModule};
use serde_json::json;
use uuid::Uuid;

use crate::support::*;

/// Counts deals for the pipeline page.
pub struct PipelineStats;

impl Injectable for PipelineStats {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(PipelineStats) })
    }
}

/// Deals shown with a view field.
pub struct BoardResource;

impl Injectable for BoardResource {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(BoardResource) })
    }
}

impl Resource for BoardResource {
    type Model = Deal;

    const LABEL: &'static str = "Board";
    const SLUG: &'static str = "board";
    const TITLE: &'static str = "title";

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title"), Field::view("card", "admin/deal-card")]
    }
}

struct PagesModule;

impl Module for PagesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.provider::<PipelineStats>()
            .templates(&[
                (
                    "admin/pipeline.html",
                    "<p class=\"who\">{{ user.name }}</p><p class=\"count\">{{ data.deals }} deals</p><form><input name=\"_csrf\" value=\"{{ csrf_token }}\"></form><a href=\"{{ path }}\">{{ title }}</a>",
                ),
                ("admin/deal-card.html", "<strong>{{ record.title }}</strong> ({{ field }})"),
            ])
            .resource::<BoardResource>()
            .helm_page(
                HelmPage::new("pipeline", "Pipeline board")
                    .group("Sales")
                    .icon("kanban")
                    .template("admin/pipeline")
                    .can("read", "Deal")
                    .data(|ctx| async move {
                        // Services resolve with this module's visibility.
                        let _stats: Arc<PipelineStats> = ctx.get().await?;
                        let deals = Deal::query().count(ctx.uow()).await?;
                        Ok(json!({ "deals": deals }))
                    }),
            );
    }
}

#[tokio::test]
async fn custom_pages_render_app_templates_in_the_layout() {
    let app = builder()
        .module(ViewModule::for_root(navree_view::ViewOptions::default()))
        .module(PagesModule)
        .build()
        .await;
    let (_, rep) = user(&app, "rep").await;
    deal(&app, DealSeed::default()).await;
    deal(&app, DealSeed::default()).await;
    let html = rep
        .get("/admin/pages/pipeline")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(
        html.contains("<h1 data-slot=\"page-title\">Pipeline board</h1>"),
        "{html}"
    );
    assert!(html.contains("<p class=\"count\">2 deals</p>"), "{html}");
    assert!(html.contains("<p class=\"who\">Signed in</p>"), "{html}");
    assert!(
        html.contains("<a href=\"/admin\">Pipeline board</a>"),
        "{html}"
    );
    let menu = rep.get("/admin/api/resources").send().await.json_value();
    assert_eq!(menu["pages"][0]["url"], "/admin/pages/pipeline");
    assert_eq!(menu["pages"][0]["group"], "Sales");

    // The page's check hides it from users who fail it.
    let none = Uuid::now_v7();
    give_role(&app, none, "nothing", &["helm.view"]).await;
    let outsider = app.acting_as(principal(none));
    outsider
        .get("/admin/pages/pipeline")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(403);
    let menu = outsider
        .get("/admin/api/resources")
        .send()
        .await
        .json_value();
    assert_eq!(menu["pages"], json!([]));
    rep.get("/admin/pages/missing")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn view_fields_render_a_template_with_the_record() {
    let app = builder()
        .module(ViewModule::for_root(navree_view::ViewOptions::default()))
        .module(PagesModule)
        .build()
        .await;
    let (_, rep) = user(&app, "rep").await;
    deal(
        &app,
        DealSeed {
            title: "Fish & Chips",
            ..DealSeed::default()
        },
    )
    .await;
    let html = rep
        .get("/admin/resources/board")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(
        html.contains("<strong>Fish &amp; Chips</strong> (card)"),
        "{html}"
    );
}

#[tokio::test]
async fn a_view_field_sees_only_what_the_user_may_read() {
    let app = builder()
        .module(ViewModule::for_root(navree_view::ViewOptions::default()))
        .module(PagesModule)
        .build()
        .await;
    let d = deal(
        &app,
        DealSeed {
            title: "TITLESECRET",
            ..DealSeed::default()
        },
    )
    .await;
    let (_, hides) = user(&app, "no-title").await;
    let html = hides
        .get(&format!("/admin/resources/board/{}", d.id))
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(!html.contains("TITLESECRET"), "{html}");
    // Someone the rule doesn't bind sees it through the template.
    let (_, rep) = user(&app, "rep").await;
    let html = rep
        .get(&format!("/admin/resources/board/{}", d.id))
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(html.contains("<strong>TITLESECRET</strong>"), "{html}");
}
