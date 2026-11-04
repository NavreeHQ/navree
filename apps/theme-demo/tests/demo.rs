//! The demo, through its routes: the builder under `/demo/ui/`, framed only by its
//! own site, a save that checks and keeps nothing, and nothing else served.

use navree::http::HttpModule;
use navree::testing::TestApp;
use navree::ui::{PagesOptions, UiModule};
use theme_demo::DemoModule;

async fn app() -> TestApp {
    TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule::with_pages(PagesOptions::default()))
        .module(DemoModule)
        .build()
        .await
}

#[tokio::test]
async fn the_builder_is_served_under_the_demo_and_only_its_site_may_frame_it() {
    let app = app().await;
    let response = app
        .client()
        .get("/demo/ui/themes/builder?base=ocean&mode=dark")
        .send()
        .await;
    assert_eq!(response.status(), 200);
    let policy = response
        .header("content-security-policy")
        .unwrap_or_default();
    assert!(policy.contains("frame-ancestors 'self'"), "{policy}");
    assert!(!policy.contains("frame-ancestors 'none'"), "{policy}");
    assert!(policy.contains("script-src 'self'"), "{policy}");
    assert_eq!(response.header("x-frame-options"), Some("SAMEORIGIN"));
    let html = response.text();
    assert!(
        html.contains("data-theme=\"ocean\" data-mode=\"dark\""),
        "{html}"
    );
    for address in [
        "action=\"/demo/ui/themes/builder\"",
        "action=\"/demo/ui/themes/builder/save\"",
        "action=\"/demo/ui/themes/builder/import\"",
        "href=\"/demo/ui/themes\"",
    ] {
        assert!(html.contains(address), "{address}");
    }
}

#[tokio::test]
async fn a_change_redraws_the_builder_in_the_draft() {
    let app = app().await;
    let html = app
        .client()
        .get("/demo/ui/themes/builder?base=ocean&accent=violet&mode=light")
        .send()
        .await
        .text();
    // A draft of your own: its stylesheet, written by the toolkit from the address.
    assert!(html.contains("/_navree/ui/theme/v1-"), "{html}");
    assert!(html.contains("data-mode=\"light\""), "{html}");
    assert!(html.contains("value=\"violet\" checked"), "{html}");
}

#[tokio::test]
async fn saving_checks_the_draft_and_keeps_nothing() {
    let app = app().await;
    let client = app.client();
    let response = client
        .post("/demo/ui/themes/builder/save")
        .header("sec-fetch-site", "same-origin")
        .form(&[("draft", "base=ocean&accent=violet&id=sutlej&name=Sutlej")])
        .send()
        .await;
    assert!(response.status().is_redirection(), "{}", response.status());
    let to = response.header("location").unwrap_or_default();
    assert!(to.starts_with("/demo/ui/themes/builder?"), "{to}");
    // Another visitor sees no trace of it.
    let other = app
        .client()
        .get("/demo/ui/themes/builder")
        .send()
        .await
        .text();
    assert!(!other.contains("Sutlej\u{201d}"), "{other}");
    assert!(!other.contains("id=sutlej"), "{other}");
}

#[tokio::test]
async fn a_post_from_another_site_is_refused() {
    let app = app().await;
    for path in [
        "/demo/ui/themes/builder/save",
        "/demo/ui/themes/builder/import",
    ] {
        let response = app
            .client()
            .post(path)
            .header("sec-fetch-site", "cross-site")
            .form(&[("draft", "base=ocean")])
            .send()
            .await;
        assert_eq!(response.status(), 403, "{path}");
    }
}

#[tokio::test]
async fn an_import_that_is_not_a_theme_says_why() {
    let app = app().await;
    let response = app
        .client()
        .post("/demo/ui/themes/builder/import")
        .header("sec-fetch-site", "same-origin")
        .form(&[("json", "not a theme")])
        .send()
        .await;
    assert_eq!(response.status(), 422);
    assert!(response.text().contains("not a theme"));
}

#[tokio::test]
async fn the_demo_says_it_is_up_and_serves_nothing_else() {
    let app = app().await;
    let client = app.client();
    assert_eq!(client.get("/demo/ui/health").send().await.status(), 204);
    assert_eq!(client.get("/demo/ui/themes").send().await.status(), 200);
    assert_eq!(
        client
            .get("/demo/ui/themes/layouts/inset")
            .send()
            .await
            .status(),
        200
    );
    for path in [
        "/themes/builder",
        "/deals",
        "/demo/ui/themes/scopes",
        "/demo/ui/themes/layouts/nowhere",
    ] {
        assert_eq!(client.get(path).send().await.status(), 404, "{path}");
    }
}
