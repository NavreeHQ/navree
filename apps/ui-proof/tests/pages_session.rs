//! A post that fails the CSRF check, with a real session: a page that says the page
//! expired for a browser, a toast for the runtime, and the JSON error for everyone else.

use std::sync::Arc;

use http::Method;
use navree::crypto::{CryptoModule, generate_key};
use navree::http::{Controller, HttpModule, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::prelude::*;
use navree::session::SessionModule;
use navree::testing::TestApp;
use navree::ui::{UiModule, View};

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
            RouteDef::on(Method::POST, "!/save", || async { View::new("Saved") })
                .public()
                .page(),
            RouteDef::on(Method::POST, "!/save-data", || async { View::new("Saved") })
                .public()
                .json_only(),
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
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .config("ui.debug", false)
        .module(HttpModule::for_root(Default::default()))
        .module(CryptoModule)
        .module(SessionModule::for_root(Default::default()))
        .module(UiModule)
        .module(App)
        .build()
        .await
}

const BROWSER: &str = "text/html,application/xhtml+xml,*/*;q=0.8";

#[tokio::test]
async fn a_post_without_its_token_is_a_page_that_says_the_page_expired() {
    let app = app().await;
    let client = app.client();
    client.csrf_cookie().await;
    let response = client.post("/save").header("accept", BROWSER).send().await;
    let text = response.text();
    assert_eq!(response.status(), 403, "the wire status of the check stays");
    assert!(
        response
            .header("content-type")
            .unwrap_or("")
            .starts_with("text/html")
    );
    assert!(text.contains("<title>Page expired</title>"), "{text}");
    assert!(text.contains(r#"data-kind="expired""#), "{text}");
    assert!(text.contains("This page expired"), "{text}");
    assert!(text.contains("Reload"), "{text}");
}

#[tokio::test]
async fn the_runtime_gets_a_toast_that_says_so() {
    let app = app().await;
    let client = app.client();
    client.csrf_cookie().await;
    let response = client
        .post("/save")
        .header("accept", "*/*")
        .header("x-navree-ui", "1")
        .send()
        .await;
    assert_eq!(response.status(), 403);
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.ops+json")
    );
    let body = response.json_value();
    assert_eq!(body["ops"][0]["op"], "toast");
    assert_eq!(body["ops"][0]["tone"], "warning");
    assert_eq!(
        body["ops"][0]["text"],
        "This page expired. Reload it and try again."
    );
}

#[tokio::test]
async fn a_script_gets_the_json_error_it_always_got() {
    let app = app().await;
    let client = app.client();
    client.csrf_cookie().await;
    let response = client
        .post("/save")
        .header("accept", "application/json")
        .send()
        .await;
    assert_eq!(response.status(), 403);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.json_value()["code"], "csrf_token_mismatch");
}

#[tokio::test]
async fn a_json_only_route_answers_a_refused_token_as_data_for_every_client() {
    // The CSRF check is a layer ahead of the router: the route has not run, and its mark still decides.
    let app = app().await;
    let client = app.client();
    client.csrf_cookie().await;
    for (accept, runtime) in [(BROWSER, false), ("*/*", true), ("application/json", false)] {
        let mut request = client.post("/save-data").header("accept", accept);
        if runtime {
            request = request.header("x-navree-ui", "1");
        }
        let response = request.send().await;
        assert_eq!(response.status(), 403, "{accept} {runtime}");
        assert_eq!(
            response.header("content-type"),
            Some("application/json"),
            "{accept} {runtime}: {}",
            response.text()
        );
        assert_eq!(response.json_value()["code"], "csrf_token_mismatch");
    }
}
