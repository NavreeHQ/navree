//! `GET {api_prefix}/openapi.json` and the docs UI at `GET {api_prefix}/docs`.

use axum::http::StatusCode;
use keel_api::ApiOptions;

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
    assert!(html.contains("@scalar/api-reference@1"), "{html}");
    assert!(html.contains(r#"data-url="/api/openapi.json""#), "{html}");
    assert!(
        html.contains("<title>CRM &lt;API&gt;</title>"),
        "title escaped: {html}"
    );

    let csp = response.header("content-security-policy").unwrap();
    assert!(csp.contains("script-src https://cdn.jsdelivr.net"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
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
