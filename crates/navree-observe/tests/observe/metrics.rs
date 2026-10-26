//! `/metrics`: the Prometheus exposition of the process-wide recorder.

use std::time::Duration;

use http::StatusCode;
use navree_config::ConfigSources;
use navree_contracts::Secret;
use navree_observe::{ObserveModule, ObserveOptions};
use serde_json::json;

use crate::support::{TestApp, TestResponse, app_with, configured_app};

/// The sample lines of metric `name` whose labels contain every `label`.
fn samples<'a>(body: &'a str, name: &str, labels: &[&str]) -> Vec<&'a str> {
    body.lines()
        .filter(|line| line.starts_with(&format!("{name}{{")))
        .filter(|line| labels.iter().all(|label| line.contains(label)))
        .collect()
}

fn with_token(token: &str) -> ObserveOptions {
    ObserveOptions {
        metrics_token: Some(Secret::new(token.to_owned())),
        ..ObserveOptions::default()
    }
}

fn assert_unauthorized(response: &TestResponse) {
    assert_eq!(
        response.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        response.text()
    );
    assert_eq!(response.header("content-type"), Some("application/json"));
    let body = response.json();
    assert_eq!(body["statusCode"], 401, "{body}");
    assert_eq!(body["error"], "Unauthorized", "{body}");
    assert_eq!(response.header("www-authenticate"), Some("Bearer"));
    assert!(
        !response.text().contains("http_requests_total"),
        "no metrics leak"
    );
}

#[tokio::test]
async fn metrics_endpoint_exposes_http_counters() {
    let app = TestApp::boot(app_with(ObserveOptions::default(), vec![])).await;
    assert_eq!(app.get("/health/live").await.status, StatusCode::OK);

    let response = app.get("/metrics").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    assert_eq!(
        response.header("content-type"),
        Some("text/plain; version=0.0.4; charset=utf-8")
    );
    let body = response.text();
    let counters = samples(
        &body,
        "http_requests_total",
        &[
            r#"method="GET""#,
            r#"route="/health/live""#,
            r#"status="200""#,
        ],
    );
    assert_eq!(counters.len(), 1, "{body}");
    let count: u64 = counters[0]
        .rsplit(' ')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("a counter value in `{}`", counters[0]));
    assert!(count >= 1, "{}", counters[0]);
    assert!(
        !samples(
            &body,
            "http_request_duration_seconds_bucket",
            &[r#"route="/health/live""#, r#"le="0.005""#]
        )
        .is_empty(),
        "latency is a histogram with buckets: {body}"
    );
    assert!(
        body.contains("# HELP http_requests_total"),
        "described: {body}"
    );
}

#[tokio::test]
async fn metrics_token_required_when_configured() {
    let app = TestApp::boot(app_with(with_token("s3cret-token"), vec![])).await;

    assert_unauthorized(&app.get("/metrics").await);
    assert_unauthorized(&app.get_with_auth("/metrics", "Bearer wrong-token").await);
    assert_unauthorized(
        &app.get_with_auth("/metrics", "Bearer s3cret-token-and-more")
            .await,
    );
    assert_unauthorized(&app.get_with_auth("/metrics", "Basic s3cret-token").await);
    assert_unauthorized(&app.get_with_auth("/metrics", "s3cret-token").await);

    for authorization in ["Bearer s3cret-token", "bearer s3cret-token"] {
        let response = app.get_with_auth("/metrics", authorization).await;
        assert_eq!(response.status, StatusCode::OK, "{authorization}");
        assert!(response.text().contains("http_requests_total"));
    }

    let health = app.get("/health/live").await;
    assert_eq!(health.status, StatusCode::OK, "health routes need no token");
}

#[tokio::test]
async fn metrics_route_is_absent_when_metrics_are_disabled() {
    let options = ObserveOptions {
        metrics: false,
        ..ObserveOptions::default()
    };
    let app = TestApp::boot(app_with(options, vec![])).await;

    assert_eq!(app.get("/metrics").await.status, StatusCode::NOT_FOUND);
    assert_eq!(app.get("/health/ready").await.status, StatusCode::OK);
}

#[tokio::test]
async fn recorder_is_installed_once_per_process() {
    let first = TestApp::boot(app_with(ObserveOptions::default(), vec![])).await;
    let second = TestApp::boot(app_with(ObserveOptions::default(), vec![])).await;

    second.get("/health/ready").await;

    for app in [&first, &second] {
        let body = app.get("/metrics").await.text();
        assert!(
            !samples(&body, "http_requests_total", &[r#"route="/health/ready""#]).is_empty(),
            "both apps render the shared recorder: {body}"
        );
    }
}

#[tokio::test]
async fn from_config_reads_the_observe_section() {
    let sources = || ConfigSources::new().set("observe", json!({ "metrics_token": "from-config" }));

    for app in [
        TestApp::boot(configured_app(sources(), ObserveModule::from_config())).await,
        TestApp::boot(configured_app(sources(), ObserveModule)).await,
    ] {
        assert_unauthorized(&app.get("/metrics").await);
        let response = app.get_with_auth("/metrics", "Bearer from-config").await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    }

    let disabled = ConfigSources::new().env_vars("APP_", [("APP_OBSERVE__METRICS", "false")]);
    let app = TestApp::boot(configured_app(disabled, ObserveModule::from_config())).await;
    assert_eq!(app.get("/metrics").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn empty_metrics_token_fails_boot() {
    let error = app_with(with_token("  "), vec![])
        .boot()
        .await
        .expect_err("an empty token must not boot")
        .to_string();
    assert!(error.contains("observe.metrics_token"), "{error}");

    let sources = ConfigSources::new().env_vars("APP_", [("APP_OBSERVE__METRICS_TOKEN", "")]);
    let error = configured_app(sources, ObserveModule::from_config())
        .boot()
        .await
        .expect_err("an empty token from the environment must not boot")
        .to_string();
    assert!(error.contains("observe.metrics_token"), "{error}");
}

#[tokio::test]
async fn graceful_shutdown_stops_the_upkeep_task() {
    let app = app_with(ObserveOptions::default(), vec![])
        .shutdown_timeout(Duration::from_secs(1))
        .boot()
        .await
        .expect("app boots");

    app.shutdown()
        .await
        .expect("tracked tasks drain within the shutdown timeout");
}

#[tokio::test]
async fn routes_are_named_public_and_documented() {
    let app = TestApp::boot(app_with(with_token("t0ken"), vec![])).await;

    let routes = navree_http::route_table(app.app.ctx()).await.unwrap();
    let observe: Vec<_> = routes
        .iter()
        .filter(|r| r.module == Some("ObserveModule"))
        .map(|r| {
            let api = r.api.as_ref().expect("documented");
            (
                r.method.as_str(),
                r.full_path.as_str(),
                r.name.as_deref(),
                r.access.describe(),
                api.tags.clone(),
                api.summary.is_some(),
            )
        })
        .collect();

    let tags = vec!["Observability".to_owned()];
    assert_eq!(
        observe,
        [
            (
                "GET",
                "/health/live",
                Some("health.live"),
                "public".to_owned(),
                tags.clone(),
                true
            ),
            (
                "GET",
                "/health/ready",
                Some("health.ready"),
                "public".to_owned(),
                tags.clone(),
                true
            ),
            (
                "GET",
                "/metrics",
                Some("metrics"),
                "public".to_owned(),
                tags,
                true
            ),
        ]
    );
    let metrics = routes.iter().find(|r| r.full_path == "/metrics").unwrap();
    assert_eq!(metrics.guards.len(), 1, "the bearer token guard");
    assert_eq!(metrics.api.as_ref().unwrap().responses, [401]);
}
