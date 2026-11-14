//! `UiModule` in a real app: the files, and each reply answering the way the
//! request asked.

use http::{Method, StatusCode};
use navree_http::response::IntoResponse;
use navree_http::{Controller, HttpModule, HttpOptions, ModuleBuilderHttpExt, RouteDef};
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
            RouteDef::on(Method::GET, "!/quiet", || async {
                View::new("Quiet").body("x".repeat(64 * 1024))
            })
            .public()
            .no_compression(),
            RouteDef::on(Method::GET, "!/as-is", || async {
                let mut response = View::new("As is")
                    .body("x".repeat(64 * 1024))
                    .into_response();
                response.headers_mut().insert(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static("private, no-transform"),
                );
                response
            })
            .public(),
            RouteDef::on(Method::POST, "!/act", || async {
                Reply::redirect("/next").toast(ToastTone::Success, "Done")
            })
            .public(),
            RouteDef::on(Method::POST, "!/act-long", || async { long_reply() }).public(),
            RouteDef::on(Method::POST, "!/act-as-is", || async {
                let mut response = long_reply().into_response();
                response.headers_mut().insert(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static("private, no-transform"),
                );
                response
            })
            .public(),
        ]
    }
}

/// A reply whose ops are long enough to be worth compressing.
fn long_reply() -> Reply {
    Reply::redirect("/next").toast(ToastTone::Success, "Saved. ".repeat(400))
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

/// The coding a page at `path` goes out in to a client that takes `accept`.
async fn coding(app: &TestApp, path: &str, accept: &str) -> Option<String> {
    let response = app
        .client()
        .get(path)
        .header("accept-encoding", accept)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    response.header("content-encoding").map(str::to_owned)
}

#[tokio::test]
async fn a_route_without_compression_is_not_compressed_by_the_toolkit_either() {
    let app = app().await;
    for accept in ["br", "gzip, br"] {
        assert_eq!(
            coding(&app, "/big", accept).await.as_deref(),
            Some("br"),
            "{accept}"
        );
        assert_eq!(coding(&app, "/quiet", accept).await, None, "{accept}");
    }
}

#[tokio::test]
async fn a_page_that_says_no_transform_is_not_compressed_by_the_toolkit_either() {
    let app = app().await;
    for accept in ["br", "gzip, br"] {
        assert_eq!(
            coding(&app, "/big", accept).await.as_deref(),
            Some("br"),
            "{accept}"
        );
        assert_eq!(coding(&app, "/as-is", accept).await, None, "{accept}");
    }
}

/// A reply posted at `path` by the runtime, or by a native client with `native`, to a
/// client that takes `accept`.
async fn posted(
    app: &TestApp,
    path: &str,
    native: bool,
    accept: &str,
) -> navree_testing::TestResponse {
    const PHONE: &str =
        "platform=android; os=16; form=phone; app=0.1.0; vocab=1; locale=en-IN; tz=Asia/Kolkata";
    let request = app.client().post(path).header("accept-encoding", accept);
    let request = if native {
        request
            .header("x-navree-native", "1")
            .header("x-navree-client", PHONE)
    } else {
        request.header("x-navree-ui", "1")
    };
    let response = request.send().await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    response
}

#[tokio::test]
async fn a_reply_that_says_no_transform_keeps_it_as_ops_and_is_not_compressed() {
    let app = app().await;
    for accept in ["br", "gzip, br"] {
        // Without it, ops this long are squeezed.
        let squeezed = posted(&app, "/act-long", false, "br").await;
        assert_eq!(squeezed.header("content-encoding"), Some("br"));
        let kept = posted(&app, "/act-as-is", false, accept).await;
        assert_eq!(
            kept.header("content-type"),
            Some("application/vnd.navree.ops+json"),
            "{accept}"
        );
        assert_eq!(
            kept.header("cache-control"),
            Some("no-store, no-transform"),
            "{accept}"
        );
        assert_eq!(kept.header("content-encoding"), None, "{accept}");
        assert!(kept.text().contains("Saved."), "{accept}");
    }
}

#[tokio::test]
async fn a_reply_that_says_no_transform_keeps_it_for_a_native_client_and_is_not_compressed() {
    let app = app().await;
    for accept in ["br", "gzip, br"] {
        let squeezed = posted(&app, "/act-long", true, "br").await;
        assert_eq!(squeezed.header("content-encoding"), Some("br"));
        let kept = posted(&app, "/act-as-is", true, accept).await;
        assert_eq!(
            kept.header("content-type"),
            Some("application/vnd.navree.native+json"),
            "{accept}"
        );
        assert_eq!(
            kept.header("cache-control"),
            Some("no-store, no-transform"),
            "{accept}"
        );
        assert_eq!(kept.header("content-encoding"), None, "{accept}");
        assert!(kept.text().contains("Saved."), "{accept}");
    }
}

#[tokio::test]
async fn with_compression_off_the_toolkit_compresses_no_page() {
    let app = TestApp::builder()
        .module(HttpModule::for_root(HttpOptions {
            compression: false,
            ..HttpOptions::default()
        }))
        .module(UiModule)
        .module(App)
        .build()
        .await;
    for (path, accept) in [
        ("/big", "br"),
        ("/big", "gzip, br"),
        ("/small", "br"),
        ("/small", "gzip, br"),
    ] {
        assert_eq!(coding(&app, path, accept).await, None, "{path} {accept}");
    }
}

#[tokio::test]
async fn a_runtime_request_is_reported_as_a_ui_operation_and_nothing_else_is() {
    use navree_telemetry::{TelemetryRecorder, UiOperationKind};

    let recorder = TelemetryRecorder::new();
    let app = TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App)
        .module(recorder.module())
        .build()
        .await;
    let client = app.client();

    client
        .post("/act")
        .header("x-navree-ui", "1")
        .header("content-type", "application/x-www-form-urlencoded")
        .send()
        .await;
    client.get("/small").header("x-navree-ui", "1").send().await;
    client
        .get("/small")
        .header("x-navree-ui", "1")
        .header("x-navree-region", "kpis")
        .send()
        .await;
    // Not from the runtime, and a fetch ahead of a click: neither is an operation.
    client.post("/act").send().await;
    client
        .get("/small")
        .header("x-navree-ui", "1")
        .header("x-navree-prefetch", "1")
        .send()
        .await;

    let operations = recorder.ui_operations();
    let seen: Vec<(&str, UiOperationKind, u16)> = operations
        .iter()
        .map(|op| (op.name.as_str(), op.kind, op.status))
        .collect();
    assert_eq!(
        seen,
        [
            ("/act", UiOperationKind::FormPost, 200),
            ("/small", UiOperationKind::Fragment, 200),
            ("/small", UiOperationKind::LiveRegion, 200),
        ]
    );
    assert!(operations[0].ops >= 1, "the toast and the redirect are ops");
    assert_eq!(operations[1].ops, 0, "a page is not ops");
    app.shutdown().await.unwrap();
}
