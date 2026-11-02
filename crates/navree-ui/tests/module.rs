//! `UiModule` in a real app: the files, and each reply answering the way the
//! request asked.

use http::{Method, StatusCode};
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_testing::TestApp;
use navree_ui::{Input, Reply, ToastTone, UiModule, View, assets};
use std::sync::Arc;

struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/small", || async {
                View::new("Small").body(Input::text("x"))
            })
            .public(),
            RouteDef::on(Method::GET, "!/big", || async {
                View::new("Big").body("x".repeat(64 * 1024))
            })
            .public(),
            RouteDef::on(Method::POST, "!/act", || async {
                Reply::redirect("/next").toast(ToastTone::Success, "Done")
            })
            .public(),
        ]
    }
}

struct App;

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

async fn app() -> TestApp {
    TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App)
        .build()
        .await
}

#[tokio::test]
async fn files_are_served_by_their_hash() {
    let app = app().await;
    let response = app
        .client()
        .get(assets::CORE_URL)
        .header("accept-encoding", "br")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.header("content-encoding"), Some("br"));
    assert!(
        response
            .header("cache-control")
            .unwrap_or_default()
            .contains("immutable")
    );
    let response = app
        .client()
        .get("/_navree/ui/nothing-000000000000.js")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_reply_is_ops_for_the_runtime() {
    let app = app().await;
    let response = app
        .client()
        .post("/act")
        .header("x-navree-ui", "1")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.ops+json")
    );
    let body = response.text();
    assert!(
        body.contains(r#"{"op":"toast","tone":"success","text":"Done"}"#),
        "{body}"
    );
    assert!(
        body.contains(r#"{"op":"navigate","href":"/next"}"#),
        "{body}"
    );
    assert!(body.contains(assets::BUILD), "{body}");
}

#[tokio::test]
async fn a_reply_is_a_redirect_with_javascript_off() {
    let app = app().await;
    let response = app.client().post("/act").send().await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.header("location"), Some("/next"));
    assert!(
        response
            .header("set-cookie")
            .unwrap_or_default()
            .starts_with("navree_ui_flash=Done;")
    );
}

#[tokio::test]
async fn big_pages_go_out_in_brotli_and_small_ones_in_gzip() {
    let app = app().await;
    let big = app
        .client()
        .get("/big")
        .header("accept-encoding", "gzip, br")
        .send()
        .await;
    assert_eq!(big.header("content-encoding"), Some("br"));
    let small = app
        .client()
        .get("/small")
        .header("accept-encoding", "gzip, br")
        .send()
        .await;
    assert_eq!(small.header("content-encoding"), Some("gzip"));
    let only_br = app
        .client()
        .get("/small")
        .header("accept-encoding", "br")
        .send()
        .await;
    assert_eq!(only_br.header("content-encoding"), Some("br"));
    let none = app.client().get("/small").send().await;
    assert_eq!(none.header("content-encoding"), None);
}
