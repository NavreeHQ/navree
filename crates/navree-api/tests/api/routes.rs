//! `GET {api_prefix}/openapi.json` and the docs UI at `GET {api_prefix}/docs`.

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
async fn docs_ui_serves_the_scalar_page() {
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
    assert!(
        html.contains(r#"<script src="/api/docs/assets/scalar-api-reference-1.74.0.js"></script>"#),
        "{html}"
    );
    assert!(html.contains(r#"data-url="/api/openapi.json""#), "{html}");
    assert!(
        html.contains("<title>CRM &lt;API&gt;</title>"),
        "title escaped: {html}"
    );

    let csp = response.header("content-security-policy").unwrap();
    assert!(csp.contains("script-src 'self';"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(
        !csp.contains("jsdelivr") && !csp.contains("scalar.com"),
        "{csp}"
    );
}

/// Every `src` and `href` of the page names a path on this origin.
fn references(html: &str) -> Vec<String> {
    let mut found = Vec::new();
    for attribute in ["src=\"", "href=\""] {
        for part in html.split(attribute).skip(1) {
            found.push(part.split('"').next().unwrap().to_owned());
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
        assert_eq!(
            asset.header("cache-control"),
            Some("public, max-age=31536000, immutable"),
            "{reference}"
        );
        assert_eq!(
            asset.header("content-type"),
            Some("text/javascript; charset=utf-8")
        );
        assert!(asset.body.len() > 100_000, "the Scalar bundle is embedded");
    }
    // The license travels with the script, and nothing else is served.
    let license = get(
        &app,
        "/api/docs/assets/scalar-api-reference-1.74.0.LICENSE.txt",
    )
    .await;
    assert_eq!(license.status, StatusCode::OK);
    assert!(license.text().contains("MIT License"));
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
    assert_eq!(
        get(&app, "/api/docs/assets/scalar-api-reference-1.74.0.js")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
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
