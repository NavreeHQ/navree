//! Mounting, the route table, configuration, commands and signed URLs.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Path;
use axum::routing::get;
use chrono::{TimeZone, Utc};
use http::{Method, Request, StatusCode};
use keel_config::ConfigSources;
use keel_contracts::{Clock, FixedClock, Secret};
use keel_http::{
    ClientIp, Created, HttpOptions, Json, NoContent, RouteDef, UrlSigner, ValidSignature,
    route_table,
};

use crate::support::{Setup, options};

#[tokio::test]
async fn versioned_controller_mounts_under_v1() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/{id}", |Path(id): Path<u32>| async move {
            format!("company {id}")
        })
        .public(),
        RouteDef::on(Method::GET, "/", || async { "index" }).public(),
    ])
    .prefix("/companies")
    .version("1")
    .boot()
    .await;

    assert_eq!(app.get("/api/v1/companies/42").await.text(), "company 42");
    assert_eq!(app.get("/api/v1/companies").await.text(), "index");
    app.get("/api/companies/42").await.assert_problem(404);
}

#[tokio::test]
async fn absolute_paths_skip_prefix_and_version() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "!/health/live", || async { "up" }).public(),
        RouteDef::on(Method::GET, "/inside", || async { "in" }).public(),
    ])
    .prefix("/ops")
    .version("2")
    .boot()
    .await;

    assert_eq!(app.get("/health/live").await.text(), "up");
    assert_eq!(app.get("/api/v2/ops/inside").await.text(), "in");
}

#[tokio::test]
async fn route_table_lists_full_paths_and_meta() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/{id}", || async { "x" })
            .name("deals.show")
            .public(),
        RouteDef::on(Method::POST, "/", || async { "x" })
            .name("deals.store")
            .authenticated()
            .transactional()
            .audit("deal.created"),
    ])
    .prefix("/deals")
    .version("1")
    .authenticated()
    .boot()
    .await;

    let table = route_table(app.app.ctx()).await.unwrap();
    let rows: Vec<(String, String, Option<String>, Option<&str>)> = table
        .iter()
        .map(|r| {
            (
                r.method.clone(),
                r.full_path.clone(),
                r.name.clone(),
                r.module,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "GET".to_owned(),
                "/api/v1/deals/{id}".to_owned(),
                Some("deals.show".to_owned()),
                Some("TestModule"),
            ),
            (
                "POST".to_owned(),
                "/api/v1/deals".to_owned(),
                Some("deals.store".to_owned()),
                Some("TestModule"),
            ),
        ]
    );
    assert!(table[0].access.public);
    assert!(table[1].access.authenticated && table[1].transactional);
    assert_eq!(table[1].audit_action.as_deref(), Some("deal.created"));
}

#[tokio::test]
async fn routes_command_lists_routes() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/", || async { "x" })
            .name("things.index")
            .public(),
    ])
    .boot()
    .await;

    app.app.run_command("routes", vec![]).await.unwrap();
    app.app
        .run_command("routes", vec!["--json".to_owned()])
        .await
        .unwrap();
    let names: Vec<String> = app
        .app
        .ctx()
        .commands()
        .iter()
        .map(|c| c.name.to_owned())
        .collect();
    for command in ["serve", "routes", "down", "up"] {
        assert!(
            names.contains(&command.to_owned()),
            "{command} in {names:?}"
        );
    }
}

#[tokio::test]
async fn from_config_reads_the_http_section() {
    let sources = ConfigSources::new()
        .set("http.api_prefix", "/backend")
        .set("http.per_ip_rate_limit", serde_json::Value::Null);
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/ping", || async { "pong" }).public(),
    ])
    .configured_by(sources)
    .boot()
    .await;

    assert_eq!(app.get("/backend/ping").await.text(), "pong");
    let options = app.app.ctx().get::<HttpOptions>().await.unwrap();
    assert_eq!(options.per_ip_rate_limit, None);
    assert_eq!(options.timeout_secs, 30, "unset keys keep their defaults");
}

#[tokio::test]
async fn invalid_http_section_fails_boot_naming_it() {
    let sources = ConfigSources::new().set("http.cors.allowed_origins", serde_json::json!(["*"]));
    let error = Setup::new(vec![
        RouteDef::on(Method::GET, "/ping", || async { "pong" }).public(),
    ])
    .configured_by(sources)
    .boot_error()
    .await;
    assert!(error.contains("http"), "{error}");
    assert!(error.contains("allowed_origins"), "{error}");
}

#[tokio::test]
async fn created_no_content_and_client_ip() {
    let app = Setup::new(vec![
        RouteDef::on(Method::POST, "/", || async {
            Created("/api/things/1".to_owned(), serde_json::json!({"id": 1}))
        })
        .public(),
        RouteDef::on(Method::DELETE, "/{id}", || async { NoContent }).public(),
        RouteDef::on(Method::GET, "/ip", |ClientIp(ip): ClientIp| async move {
            Json(serde_json::json!({ "ip": ip.to_string() }))
        })
        .public(),
    ])
    .prefix("/things")
    .options(HttpOptions {
        trust_proxy: true,
        ..options()
    })
    .boot()
    .await;

    let created = app
        .send(Request::post("/api/things").body(Body::empty()).unwrap())
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.header("location"), Some("/api/things/1"));
    assert_eq!(created.json()["id"], 1);

    let deleted = app
        .send(
            Request::delete("/api/things/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let ip = app
        .send(
            Request::get("/api/things/ip")
                .header("x-forwarded-for", "203.0.113.9")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(ip.json()["ip"], "203.0.113.9");
}

#[tokio::test]
async fn signed_url_guard_in_the_pipeline() {
    let now = Utc.with_ymd_and_hms(2026, 10, 25, 12, 0, 0).unwrap();
    let clock = Arc::new(FixedClock::new(now));
    let registered = clock.clone();
    let app = Setup::new(vec![
        RouteDef::new(
            Method::GET,
            "!/verify-email/{id}",
            get(|Path(id): Path<u32>| async move { format!("verified {id}") }),
        )
        .public()
        .guard(ValidSignature::guard()),
    ])
    .options(HttpOptions {
        signing_key: Some(Secret::new("test-signing-key".to_owned())),
        ..options()
    })
    .register(move |m| {
        m.value_arc::<dyn Clock>(registered.clone());
    })
    .boot()
    .await;

    let signer = app.app.ctx().get::<UrlSigner>().await.unwrap();
    let url = signer.sign(
        "/verify-email/7?hash=abc",
        now + chrono::Duration::minutes(60),
    );
    assert_eq!(
        UrlSigner::new(b"test-signing-key").sign(
            "/verify-email/7?hash=abc",
            now + chrono::Duration::minutes(60)
        ),
        url,
        "keyed by http.signing_key"
    );

    assert_eq!(app.get(&url).await.text(), "verified 7");
    let body = app
        .get(&url.replace("/verify-email/7", "/verify-email/8"))
        .await
        .assert_problem(401);
    assert_eq!(body["type"], "invalid_signature");
    app.get("/verify-email/7?hash=abc")
        .await
        .assert_problem(401);

    clock.advance(std::time::Duration::from_secs(61 * 60));
    app.get(&url).await.assert_problem(401);
}

/// The app's `UrlSigner` after booting with `sources`.
async fn signer_for(sources: ConfigSources) -> Arc<UrlSigner> {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/ping", || async { "pong" }).public(),
    ])
    .configured_by(sources)
    .boot()
    .await;
    app.app.ctx().get::<UrlSigner>().await.unwrap()
}

#[tokio::test]
async fn signed_urls_survive_a_restart_when_the_app_key_is_set() {
    let now = Utc::now();
    let expires = now + chrono::Duration::minutes(5);
    let app_key = "base64:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=";

    // Two boots with the same APP_KEY (and no http.signing_key) agree.
    let url = signer_for(ConfigSources::new().set("app.key", app_key))
        .await
        .sign("/verify-email/1", expires);
    let restarted = signer_for(ConfigSources::new().env_vars("APP_", [("APP_KEY", app_key)])).await;
    assert!(
        restarted.verify(&url.parse().unwrap(), now),
        "same key: {url}"
    );

    // The signing key is derived, never the raw app key.
    assert!(!UrlSigner::new(app_key.as_bytes()).verify(&url.parse().unwrap(), now));

    // Another app key, or no key at all (a random per-process key), rejects it.
    let other = signer_for(ConfigSources::new().set("app.key", "base64:other")).await;
    assert!(!other.verify(&url.parse().unwrap(), now));
    let random = signer_for(ConfigSources::new()).await;
    assert!(!random.verify(&url.parse().unwrap(), now));

    // An explicit http.signing_key still wins over the app key.
    let explicit = signer_for(
        ConfigSources::new()
            .set("app.key", app_key)
            .set("http.signing_key", "test-signing-key"),
    )
    .await;
    assert_eq!(
        explicit.sign("/x", expires),
        UrlSigner::new(b"test-signing-key").sign("/x", expires)
    );
}
