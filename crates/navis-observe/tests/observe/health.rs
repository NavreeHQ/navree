//! `/health/live` and `/health/ready`.

use std::sync::Arc;
use std::time::Duration;

use http::StatusCode;
use navis_observe::{CheckState, Health, ObserveOptions};
use serde_json::json;
use tokio::time::Instant;

use crate::support::{Behaviour, ChecksModule, FakeCheck, TestApp, app_with};

#[tokio::test]
async fn live_is_always_up() {
    let down = FakeCheck::new("database", Behaviour::Down("connection refused"));
    let app = TestApp::boot(app_with(ObserveOptions::default(), vec![down.clone()])).await;

    let response = app.get("/health/live").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    assert_eq!(response.json(), json!({"status": "up"}));
    assert_eq!(down.runs(), 0, "liveness does not run readiness checks");
}

#[tokio::test]
async fn ready_is_up_when_every_check_is_up() {
    let database = FakeCheck::new("database", Behaviour::Up);
    let cache = FakeCheck::new("cache", Behaviour::Up);
    let app = TestApp::boot(app_with(
        ObserveOptions::default(),
        vec![database.clone(), cache.clone()],
    ))
    .await;

    let response = app.get("/health/ready").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    let body = response.json();
    assert_eq!(body["status"], "up");
    for name in ["database", "cache"] {
        assert_eq!(body["checks"][name]["status"], "up", "{body}");
        assert!(body["checks"][name]["latency_ms"].is_u64(), "{body}");
        assert!(body["checks"][name].get("error").is_none(), "{body}");
    }
    assert_eq!((database.runs(), cache.runs()), (1, 1));
}

#[tokio::test]
async fn ready_aggregates_checks_and_returns_503_when_one_down() {
    let app = TestApp::boot(app_with(
        ObserveOptions::default(),
        vec![
            FakeCheck::new("database", Behaviour::Up),
            FakeCheck::new("cache", Behaviour::Down("connection refused")),
        ],
    ))
    .await;

    let response = app.get("/health/ready").await;

    assert_eq!(
        response.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        response.text()
    );
    assert_eq!(response.header("content-type"), Some("application/json"),);
    let body = response.json();
    assert!(
        body.get("statusCode").is_none(),
        "a health report, not an error body: {body}"
    );
    assert_eq!(body["status"], "down");
    assert_eq!(body["checks"]["database"]["status"], "up");
    assert_eq!(body["checks"]["cache"]["status"], "down");
    assert_eq!(body["checks"]["cache"]["error"], "connection refused");
    assert!(body["checks"]["cache"]["latency_ms"].is_u64(), "{body}");
}

#[tokio::test]
async fn ready_without_checks_is_up() {
    let app = TestApp::boot(app_with(ObserveOptions::default(), vec![])).await;

    let response = app.get("/health/ready").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    assert_eq!(response.json(), json!({"status": "up", "checks": {}}));
}

#[tokio::test(start_paused = true)]
async fn slow_check_times_out_as_down() {
    let slow = FakeCheck::new("search", Behaviour::Slow(Duration::from_secs(10)));
    let app = TestApp::boot(app_with(
        ObserveOptions::default(),
        vec![FakeCheck::new("database", Behaviour::Up), slow.clone()],
    ))
    .await;

    let start = Instant::now();
    let response = app.get("/health/ready").await;

    assert_eq!(
        response.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        response.text()
    );
    assert_eq!(
        start.elapsed(),
        Duration::from_secs(2),
        "the check is abandoned after 2 s"
    );
    let body = response.json();
    assert_eq!(body["status"], "down");
    assert_eq!(body["checks"]["database"]["status"], "up");
    assert_eq!(body["checks"]["search"]["status"], "down");
    assert_eq!(body["checks"]["search"]["latency_ms"], 2000);
    assert_eq!(body["checks"]["search"]["error"], "timed out after 2000 ms");
    assert_eq!(slow.runs(), 1);
}

#[tokio::test(start_paused = true)]
async fn checks_run_concurrently() {
    let slow = || FakeCheck::new("a", Behaviour::Slow(Duration::from_millis(1500)));
    let (a, mut b) = (slow(), slow());
    Arc::get_mut(&mut b).unwrap().name = "b";
    let app = TestApp::boot(app_with(ObserveOptions::default(), vec![a, b])).await;

    let start = Instant::now();
    let response = app.get("/health/ready").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    assert_eq!(start.elapsed(), Duration::from_millis(1500));
    let body = response.json();
    assert_eq!(body["checks"]["a"]["latency_ms"], 1500);
    assert_eq!(body["checks"]["b"]["latency_ms"], 1500);
}

#[tokio::test]
async fn panicking_check_is_reported_down() {
    let app = TestApp::boot(app_with(
        ObserveOptions::default(),
        vec![
            FakeCheck::new("database", Behaviour::Up),
            FakeCheck::new("broken", Behaviour::Panic),
        ],
    ))
    .await;

    let response = app.get("/health/ready").await;

    assert_eq!(
        response.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        response.text()
    );
    let body = response.json();
    assert_eq!(body["status"], "down");
    assert_eq!(body["checks"]["database"]["status"], "up");
    assert_eq!(body["checks"]["broken"]["status"], "down");
    assert_eq!(body["checks"]["broken"]["error"], "the check panicked");
}

#[tokio::test]
async fn checks_with_the_same_name_are_all_reported() {
    let app = TestApp::boot(app_with(
        ObserveOptions::default(),
        vec![
            FakeCheck::new("database", Behaviour::Up),
            FakeCheck::new("database", Behaviour::Down("replica lagging")),
        ],
    ))
    .await;

    let response = app.get("/health/ready").await;

    assert_eq!(response.status, StatusCode::SERVICE_UNAVAILABLE);
    let body = response.json();
    assert_eq!(body["checks"]["database"]["status"], "up", "{body}");
    assert_eq!(body["checks"]["database#2"]["status"], "down", "{body}");
    assert_eq!(body["checks"]["database#2"]["error"], "replica lagging");
}

#[tokio::test]
async fn health_responses_are_not_cached() {
    let app = TestApp::boot(app_with(
        ObserveOptions::default(),
        vec![FakeCheck::new("cache", Behaviour::Down("unreachable"))],
    ))
    .await;

    for path in ["/health/live", "/health/ready"] {
        let response = app.get(path).await;
        assert_eq!(response.header("cache-control"), Some("no-store"), "{path}");
    }
}

#[tokio::test]
async fn health_provider_reports_without_http_module() {
    let app = navis_kernel::Navis::app()
        .module(navis_observe::ObserveModule::for_root(
            ObserveOptions::default(),
        ))
        .module(ChecksModule(vec![FakeCheck::new(
            "queue",
            Behaviour::Down("no workers"),
        )]))
        .boot()
        .await
        .expect("boots without HttpModule");

    let health = app.ctx().get::<Health>().await.unwrap();
    let report = health.check().await;

    assert!(!report.is_up());
    assert_eq!(report.status, CheckState::Down);
    assert_eq!(report.checks["queue"].status, CheckState::Down);
    assert_eq!(report.checks["queue"].error.as_deref(), Some("no workers"));
}
