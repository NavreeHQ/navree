//! `GET {api_prefix}/openapi.json` and the reference page at `GET {api_prefix}/docs`.

use http::StatusCode;
use navree_api::ApiOptions;

use crate::support::{app, boot, document, get};

#[tokio::test]
async fn openapi_json_route_serves_the_document() {
    let app = boot().await;
    let response = get(&app, "/api/openapi.json").await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.json(), document(&app).await);
}

#[tokio::test]
async fn docs_ui_serves_the_reference_page() {
    let app = app(ApiOptions {
        title: "CRM <API>".to_owned(),
        ..ApiOptions::default()
    })
    .boot()
    .await
    .unwrap();
    let response = get(&app, "/api/docs").await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(
        response
            .header("content-type")
            .is_some_and(|v| v.starts_with("text/html")),
        "{:?}",
        response.headers
    );
    let html = response.text();
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        html.contains(&format!(
            r#"<script src="/api/docs/assets/reference-{version}.js" defer></script>"#
        )),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="raw" href="/api/openapi.json">"#),
        "{html}"
    );
    assert!(
        html.contains("<title>CRM &lt;API&gt;</title>"),
        "title escaped: {html}"
    );

    let csp = response.header("content-security-policy").unwrap();
    assert!(csp.contains("script-src 'self';"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(csp.contains("style-src 'self';"), "{csp}");
    assert!(!csp.contains("unsafe"), "{csp}");
    assert!(!csp.contains("http"), "{csp}");
}

/// Every `src` and `href` of the page, but the fragments of the page itself.
fn references(html: &str) -> Vec<String> {
    let mut found = Vec::new();
    for attribute in ["src=\"", "href=\""] {
        for part in html.split(attribute).skip(1) {
            let reference = part.split('"').next().unwrap();
            if !reference.starts_with('#') {
                found.push(reference.to_owned());
            }
        }
    }
    found
}

#[tokio::test]
async fn the_docs_page_references_only_local_assets_and_the_app_serves_them() {
    let app = boot().await;
    let page = get(&app, "/api/docs").await.text();
    let references = references(&page);
    assert!(!references.is_empty(), "{page}");
    for reference in &references {
        assert!(
            reference.starts_with('/') && !reference.starts_with("//"),
            "{reference} is not a local path in {page}"
        );
        let asset = get(&app, reference).await;
        assert_eq!(asset.status, StatusCode::OK, "{reference}");
        if reference.ends_with("openapi.json") {
            continue;
        }
        assert_eq!(
            asset.header("cache-control"),
            Some("public, max-age=31536000, immutable"),
            "{reference}"
        );
        let content_type = asset.header("content-type").unwrap();
        assert!(
            content_type == "text/javascript; charset=utf-8"
                || content_type == "text/css; charset=utf-8",
            "{content_type}"
        );
        assert!(!asset.body.is_empty());
    }
    assert_eq!(
        get(&app, "/api/docs/assets/missing.js").await.status,
        StatusCode::NOT_FOUND
    );
    // An encoded slash in the file name never reaches the route.
    assert_eq!(
        get(&app, "/api/docs/assets/..%2FCargo.toml").await.status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn the_assets_go_with_the_docs_page() {
    let app = app(ApiOptions {
        docs_ui: false,
        ..ApiOptions::default()
    })
    .boot()
    .await
    .unwrap();
    let script = format!(
        "/api/docs/assets/reference-{}.js",
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(get(&app, &script).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn docs_ui_can_be_turned_off() {
    let app = app(ApiOptions {
        docs_ui: false,
        ..ApiOptions::default()
    })
    .boot()
    .await
    .unwrap();
    assert_eq!(get(&app, "/api/docs").await.status, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, "/api/openapi.json").await.status, StatusCode::OK);
}
