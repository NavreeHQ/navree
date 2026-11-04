//! `ApiModule::from_config()` and plain `ApiModule`: the `api` section,
//! servers from the environment and the session cookie name.

use std::sync::Arc;

use http::StatusCode;
use navree_api::ApiModule;
use navree_config::{ConfigModule, ConfigSources};
use navree_http::{Authenticator, HttpModule, RouteAuthorizer};
use navree_kernel::{AppBuilder, Navree};
use serde_json::json;

use crate::support::{AllowAll, DealsModule, GuestAuthenticator, boot_error, document, get};

/// ConfigModule with `sources`, HttpModule and ApiModule from config, the deals module.
fn configured(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(
            sources.set("http", json!({"per_ip_rate_limit": null})),
        ))
        .module(HttpModule::from_config())
        .module(ApiModule::from_config())
        .module(DealsModule)
        .override_value::<dyn Authenticator>(Arc::new(GuestAuthenticator))
        .override_value::<dyn RouteAuthorizer>(Arc::new(AllowAll))
}

#[tokio::test]
async fn from_config_reads_the_api_section() {
    let sources = ConfigSources::new().set(
        "api",
        json!({"title": "CRM API", "version": "2.1.0", "docs_ui": false}),
    );
    let app = configured(sources).boot().await.unwrap();
    let doc = document(&app).await;
    assert_eq!(doc["info"], json!({"title": "CRM API", "version": "2.1.0"}));
    assert!(doc.get("servers").is_none(), "no servers configured");
    assert_eq!(get(&app, "/api/docs").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn servers_come_from_a_comma_separated_environment_variable() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [(
            "APP_API__SERVERS",
            "https://crm.example.com, http://localhost:8080,",
        )],
    );
    let app = configured(sources).boot().await.unwrap();
    let doc = document(&app).await;
    assert_eq!(
        doc["servers"],
        json!([{"url": "https://crm.example.com"}, {"url": "http://localhost:8080"}])
    );
}

#[tokio::test]
async fn security_scheme_uses_the_configured_session_cookie() {
    let sources = ConfigSources::new().set("session", json!({"cookie": "crm_session"}));
    let app = configured(sources).boot().await.unwrap();
    let doc = document(&app).await;
    // MOD-001: named as navree-session sets it, with the `__Host-` prefix.
    assert_eq!(
        doc["components"]["securitySchemes"]["cookie"]["name"],
        "__Host-crm_session"
    );
    for (session, name) in [
        (
            json!({"cookie": "crm_session", "secure": false}),
            "crm_session",
        ),
        (
            json!({"cookie": "crm_session", "domain": ".example.com"}),
            "crm_session",
        ),
        (
            json!({"cookie": "crm_session", "host_prefix": false}),
            "crm_session",
        ),
    ] {
        let sources = ConfigSources::new().set("session", session);
        let app = configured(sources).boot().await.unwrap();
        let doc = document(&app).await;
        assert_eq!(doc["components"]["securitySchemes"]["cookie"]["name"], name);
    }
}

#[tokio::test]
async fn invalid_api_section_fails_boot() {
    let sources = ConfigSources::new().set("api", json!({"title": " "}));
    let error = boot_error(configured(sources)).await;
    assert!(error.contains("api.title"), "{error}");
}

#[tokio::test]
async fn plain_api_module_brings_the_http_module() {
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(ApiModule)
        .boot()
        .await
        .expect("ApiModule imports HttpModule");
    let response = get(&app, "/api/openapi.json").await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json()["paths"], json!({}));
}
