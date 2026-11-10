//! Whether `/api/openapi.json` and `/api/docs` are served: an explicit
//! `api.openapi_json` or `api.docs_ui` wins; otherwise they are off in the
//! `production` environment, on in any other, and with no environment on in
//! a debug build and off in a release build.

use std::sync::Arc;

use http::StatusCode;
use navree_api::ApiModule;
use navree_config::{ConfigModule, ConfigSources};
use navree_http::{Authenticator, HttpModule, RouteAuthorizer};
use navree_kernel::{App, Navree};
use serde_json::json;

use crate::support::{AllowAll, DealsModule, GuestAuthenticator, get};

/// An app configured from `sources` (environment variables under `APP_`).
async fn boot(vars: &[(&str, &str)]) -> App {
    let sources = ConfigSources::new()
        .env_vars("APP_", vars.iter().copied())
        .set("http", json!({"per_ip_rate_limit": null}));
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(ApiModule::from_config())
        .module(DealsModule)
        .override_value::<dyn Authenticator>(Arc::new(GuestAuthenticator))
        .override_value::<dyn RouteAuthorizer>(Arc::new(AllowAll))
        .boot()
        .await
        .expect("app boots")
}

/// The status of the document and of the page.
async fn served(app: &App) -> (StatusCode, StatusCode) {
    (
        get(app, "/api/openapi.json").await.status,
        get(app, "/api/docs").await.status,
    )
}

const ON: (StatusCode, StatusCode) = (StatusCode::OK, StatusCode::OK);
const OFF: (StatusCode, StatusCode) = (StatusCode::NOT_FOUND, StatusCode::NOT_FOUND);

#[tokio::test]
async fn off_by_default_in_production() {
    let app = boot(&[("APP_ENV", "production")]).await;
    assert_eq!(served(&app).await, OFF);
}

#[tokio::test]
async fn off_by_default_in_staging() {
    let app = boot(&[("APP_ENV", "staging")]).await;
    assert_eq!(served(&app).await, OFF);
}

#[tokio::test]
async fn production_is_matched_in_any_case() {
    let app = boot(&[("APP_ENV", "Production")]).await;
    assert_eq!(served(&app).await, OFF);
}

#[tokio::test]
async fn on_by_default_in_any_other_environment() {
    for env in ["development", "local", "qa", "testing"] {
        let app = boot(&[("APP_ENV", env)]).await;
        assert_eq!(served(&app).await, ON, "env = {env}");
    }
}

#[tokio::test]
async fn the_environment_can_come_from_the_app_section() {
    let app = boot(&[("APP_APP__ENV", "production")]).await;
    assert_eq!(served(&app).await, OFF);
    let app = boot(&[("APP_APP__ENV", "development")]).await;
    assert_eq!(served(&app).await, ON);
}

#[tokio::test]
async fn the_top_level_environment_wins_over_the_app_section() {
    let app = boot(&[("APP_ENV", "development"), ("APP_APP__ENV", "production")]).await;
    assert_eq!(served(&app).await, ON);
    let app = boot(&[("APP_ENV", "production"), ("APP_APP__ENV", "development")]).await;
    assert_eq!(served(&app).await, OFF);
}

#[tokio::test]
async fn explicitly_on_in_production() {
    let app = boot(&[
        ("APP_ENV", "production"),
        ("APP_API__OPENAPI_JSON", "true"),
        ("APP_API__DOCS_UI", "true"),
    ])
    .await;
    assert_eq!(served(&app).await, ON);
}

#[tokio::test]
async fn explicitly_off_in_production() {
    let app = boot(&[
        ("APP_ENV", "production"),
        ("APP_API__OPENAPI_JSON", "false"),
        ("APP_API__DOCS_UI", "false"),
    ])
    .await;
    assert_eq!(served(&app).await, OFF);
}

#[tokio::test]
async fn explicitly_off_in_development() {
    let app = boot(&[
        ("APP_ENV", "development"),
        ("APP_API__OPENAPI_JSON", "false"),
    ])
    .await;
    assert_eq!(served(&app).await, OFF, "the page goes with the document");
    let app = boot(&[("APP_ENV", "development"), ("APP_API__DOCS_UI", "false")]).await;
    assert_eq!(served(&app).await, (StatusCode::OK, StatusCode::NOT_FOUND));
}

#[tokio::test]
async fn in_production_the_document_alone_can_be_turned_on() {
    let app = boot(&[("APP_ENV", "production"), ("APP_API__OPENAPI_JSON", "true")]).await;
    assert_eq!(
        served(&app).await,
        (StatusCode::OK, StatusCode::NOT_FOUND),
        "docs_ui is still off by default"
    );
}

#[tokio::test]
async fn in_production_the_page_needs_the_document_turned_on_too() {
    let app = boot(&[("APP_ENV", "production"), ("APP_API__DOCS_UI", "true")]).await;
    assert_eq!(served(&app).await, OFF);
}

/// Run in a debug build (`cargo test`) and a release build
/// (`cargo test --release`): with no environment, on in the first and off
/// in the second.
#[tokio::test]
async fn with_no_environment_it_follows_the_build() {
    let expected = if cfg!(debug_assertions) { ON } else { OFF };
    let app = boot(&[]).await;
    assert_eq!(served(&app).await, expected);
    let app = boot(&[("APP_ENV", " ")]).await;
    assert_eq!(
        served(&app).await,
        expected,
        "a blank environment names none"
    );
}

#[tokio::test]
async fn export_writes_the_document_in_production() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("openapi.json");
    let app = boot(&[("APP_ENV", "production")]).await;
    assert_eq!(served(&app).await, OFF);
    app.run_command("openapi:export", vec![file.display().to_string()])
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("\"openapi\"")
    );
}
