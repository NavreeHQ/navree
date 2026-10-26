//! `UiModule` in an app: the asset route, the gallery route, the templates
//! it contributes to the app's `Views`, and its configuration.

use std::sync::Arc;

use axum::http::{HeaderMap, Method};
use navree_http::{Controller, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_testing::TestApp;
use navree_ui::{
    Crumb, NavGroup, NavItem, PAGE_CSP, Ui, UiModule, UiOptions, assets, render, sidebar_collapsed,
};
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions, Views};
use serde_json::json;

/// The page with `/` unescaped (MiniJinja writes `&#x2f;` in attributes).
fn decoded(html: String) -> String {
    html.replace("&#x2f;", "/")
}

fn view_module() -> impl Module {
    ViewModule::for_root(ViewOptions {
        dirs: Vec::new(),
        reload: false,
    })
}

fn options(gallery: bool) -> UiOptions {
    UiOptions {
        gallery,
        ..UiOptions::default()
    }
}

async fn app_with(options: UiOptions) -> TestApp {
    TestApp::builder()
        .module(view_module())
        .module(UiModule::for_root(options))
        .module(ToolModule)
        .build()
        .await
}

#[tokio::test]
async fn serves_versioned_assets_with_immutable_caching() {
    let app = app_with(options(false)).await;
    let ui = app.get::<Ui>().await;
    assert_eq!(
        ui.assets_url(),
        format!("/_navree/ui/{}", assets::version())
    );

    let css = app
        .client()
        .get(&format!("{}/navree-ui.css", ui.assets_url()))
        .send()
        .await
        .assert_status(200);
    assert_eq!(css.header("content-type"), Some("text/css; charset=utf-8"));
    assert_eq!(
        css.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert!(css.text().contains("--ui-ink-0: #040605"));

    let js = app
        .client()
        .get(&format!("{}/navree-ui.js", ui.assets_url()))
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        js.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );

    let font = app
        .client()
        .get(&format!("{}/fonts/geist.woff2", ui.assets_url()))
        .send()
        .await
        .assert_status(200);
    assert_eq!(font.header("content-type"), Some("font/woff2"));
    assert_eq!(
        font.bytes().as_ref(),
        assets::get("fonts/geist.woff2").unwrap().bytes
    );
}

#[tokio::test]
async fn stale_versions_are_served_uncached_and_unknown_files_are_404() {
    let app = app_with(options(false)).await;
    let stale = app
        .client()
        .get("/_navree/ui/0123456789ab/navree-ui.css")
        .send()
        .await
        .assert_status(200);
    assert_eq!(stale.header("cache-control"), Some("no-cache"));

    for path in [
        format!("/_navree/ui/{}/missing.css", assets::version()),
        format!("/_navree/ui/{}/fonts/../navree-ui.css", assets::version()),
        "/_navree/ui/navree-ui.css".to_owned(),
    ] {
        let status = app.client().get(&path).send().await.status();
        assert_eq!(status.as_u16(), 404, "{path}");
    }
}

#[tokio::test]
async fn the_mount_is_configurable() {
    let app = TestApp::builder()
        .config("ui.mount", "/admin/assets/")
        .config("ui.gallery", false)
        .module(view_module())
        .module(UiModule::from_config())
        .build()
        .await;
    let ui = app.get::<Ui>().await;
    assert_eq!(ui.mount(), "/admin/assets");
    assert!(!ui.options().gallery);
    app.client()
        .get(&format!("/admin/assets/{}/navree-ui.js", assets::version()))
        .send()
        .await
        .assert_status(200);
    app.client()
        .get(&format!("/_navree/ui/{}/navree-ui.js", assets::version()))
        .send()
        .await
        .assert_status(404);
    let shell = serde_json::to_value(ui.shell("Fleet")).unwrap();
    assert_eq!(
        shell["assets"],
        format!("/admin/assets/{}", assets::version())
    );
}

#[tokio::test]
async fn an_invalid_mount_fails_the_boot() {
    let result = TestApp::builder()
        .module(view_module())
        .module(UiModule::for_root(UiOptions {
            mount: "assets".to_owned(),
            gallery: false,
        }))
        .try_build()
        .await;
    let Err(error) = result else {
        panic!("the boot must fail");
    };
    let error = error.to_string();
    assert!(error.contains("ui.mount"), "{error}");
}

#[tokio::test]
async fn the_gallery_is_served_when_enabled() {
    let app = app_with(options(true)).await;
    for (path, marker) in [
        ("/_navree/ui/gallery", "<title>Components · Helm</title>"),
        (
            "/_navree/ui/gallery/dashboard",
            "<title>Dashboard · Helm</title>",
        ),
        (
            "/_navree/ui/gallery/sign-in",
            "<title>Sign in · Helm</title>",
        ),
    ] {
        let page = app.client().get(path).send().await.assert_status(200);
        assert_eq!(page.header("content-security-policy"), Some(PAGE_CSP));
        assert_eq!(page.header("cache-control"), Some("no-store"));
        let html = decoded(page.text());
        assert!(html.contains(marker), "{path}");
        assert!(
            html.contains(&format!("/_navree/ui/{}/navree-ui.css", assets::version())),
            "{path} links the versioned stylesheet"
        );
        // Links between gallery pages point at the routes.
        assert!(
            html.contains(r#"="/_navree/ui/gallery/dashboard""#),
            "{path}"
        );
    }

    let off = app_with(options(false)).await;
    off.client()
        .get("/_navree/ui/gallery")
        .send()
        .await
        .assert_status(404);
}

/// A tool module: its own template extends the kit's shell, rendered with
/// the app's `Views` (which `UiModule` gave the `ui/*` templates).
struct ToolModule;

impl Module for ToolModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[(
            "tool/home.html",
            r#"{% extends "ui/app.html" %}
{% import "ui/components.html" as ui %}
{% block content %}{% call ui.page_header("Queues", "Every queue of the app.") %}{{ ui.button("Pause all", icon="pause") }}{% endcall %}{{ ui.badge(count ~ " jobs", "success") }}{% endblock %}"#,
        )])
        .controller::<ToolController>();
    }
}

struct ToolController {
    views: Arc<Views>,
    ui: Arc<Ui>,
}

impl Injectable for ToolController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Views>(), ProviderKey::of::<Ui>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                views: r.get::<Views>().await?,
                ui: r.get::<Ui>().await?,
            })
        })
    }
}

impl Controller for ToolController {
    fn prefix(&self) -> &'static str {
        "!/tool"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "", move |headers: HeaderMap| {
                let this = self.clone();
                async move {
                    let shell = this
                        .ui
                        .shell("Fleet")
                        .home("/tool")
                        .group(
                            NavGroup::new("Queues")
                                .item(NavItem::new("Overview", "/tool").icon("boxes")),
                        )
                        .crumb(Crumb::page("Queues"))
                        .title("Queues")
                        .collapsed(sidebar_collapsed(&headers))
                        .current("/tool");
                    render(&this.views, "tool/home", &shell, json!({ "count": 42 }))
                        .map_err(navree_http::AppError::internal)
                }
            })
            .public(),
        ]
    }
}

#[tokio::test]
async fn tool_templates_extend_the_shell_through_the_apps_views() {
    let app = app_with(options(false)).await;
    let page = app.client().get("/tool").send().await.assert_status(200);
    assert_eq!(page.header("content-security-policy"), Some(PAGE_CSP));
    let html = decoded(page.text());
    assert!(html.contains("<title>Queues · Fleet</title>"), "{html}");
    assert!(html.contains(r#"<h1 class="ui-h1">Queues</h1>"#), "{html}");
    assert!(html.contains("42 jobs"), "{html}");
    assert!(
        html.contains(r#"<a class="ui-nav__link" href="/tool" aria-current="page""#),
        "{html}"
    );
    assert!(html.contains(r#"data-sidebar="expanded""#), "{html}");

    // The sidebar cookie the script sets renders the collapsed state.
    let client = app.client();
    client.set_cookie("navree_ui_sidebar", "collapsed");
    let html = decoded(client.get("/tool").send().await.assert_status(200).text());
    assert!(
        html.contains(r#"data-sidebar="collapsed" data-sidebar-from-cookie"#),
        "{html}"
    );
}
