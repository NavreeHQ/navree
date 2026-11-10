//! OTLP export (feature `otel`) to a real OpenTelemetry Collector: a request
//! traced by the app reaches the Collector over OTLP/HTTP and OTLP/gRPC, which
//! decodes it with its own protobuf code and writes what it received to a file
//! (the Collector's `file` exporter, as OTLP JSON). The test reads the file
//! back and checks the span.
//!
//! These run in the integration workflow, which starts the Collector with
//! `scripts/integration-services.sh start infra`; they are `#[ignore]`d in the
//! other suites. Run them with `-- --ignored`. The variables:
//!
//! - `NAVREE_TEST_OTEL_HTTP_URL`: the Collector's OTLP/HTTP receiver;
//! - `NAVREE_TEST_OTEL_GRPC_URL`: its OTLP/gRPC receiver;
//! - `NAVREE_TEST_OTEL_FILE`: the file its exporter appends to.
//!
//! A missing variable fails the test when `NAVREE_TEST_REQUIRE_SERVICES=1` and
//! prints `SKIPPED` otherwise.
//!
//! Unlike `otlp.rs`, which decodes what the app sends with the protobuf types
//! the OpenTelemetry SDK itself uses, this shows the app's export is accepted
//! and understood by the reference implementation of the receiving side.
//! Vendor endpoints (Grafana Cloud, Honeycomb, Datadog) and TLS to a collector
//! stay untried: they need accounts or certificates this job does not have.

use std::time::{Duration, Instant};

use http::Request;
use navree_config::{ConfigModule, ConfigSources};
use navree_http::body::Body;
use navree_http::{HttpModule, build_router};
use navree_kernel::Navree;
use navree_observe::ObserveModule;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::otlp::PingModule;

const PARENT: &str = "00f067aa0ba902b7";

/// The value of `variable`; `None` after a notice, or a panic when
/// `NAVREE_TEST_REQUIRE_SERVICES=1`.
fn service(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: {variable} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// A trace id no earlier run used (the Collector's file keeps every run).
fn trace_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock")
        .as_nanos();
    format!("{nanos:032x}")
}

/// Boots an app exporting to `endpoint` over `protocol`, sends one request
/// that continues trace `trace`, and shuts down (which flushes).
async fn export_one_request(endpoint: &str, protocol: &str, trace: &str) {
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new()
                .set("observe.metrics", false)
                .set("http.per_ip_rate_limit", Value::Null)
                .set(
                    "observe.tracing",
                    json!({
                        "endpoint": endpoint,
                        "protocol": protocol,
                        "service_name": "crm-api",
                        "resource": { "deployment.environment": "test" },
                        "timeout_ms": 5000
                    }),
                ),
        ))
        .module(HttpModule::from_config())
        .module(ObserveModule::from_config())
        .module(PingModule)
        .boot()
        .await
        .expect("boots");
    let router = build_router(app.ctx()).await.unwrap();
    let response = router
        .oneshot(
            Request::get("/api/ping")
                .header("traceparent", format!("00-{trace}-{PARENT}-01"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    app.shutdown().await.expect("shutdown flushes the queue");
}

/// The value of attribute `key` among OTLP JSON `attributes`.
fn attribute(attributes: &Value, key: &str) -> Option<String> {
    attributes
        .as_array()?
        .iter()
        .find(|pair| pair["key"] == key)
        .and_then(|pair| pair["value"]["stringValue"].as_str())
        .map(str::to_owned)
}

/// The span of trace `trace` and the resource it came from, once the
/// Collector has written the export to its file.
async fn exported(file: &str, trace: &str) -> (Value, Value) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        // One OTLP JSON export per line.
        for line in text.lines().filter(|line| line.contains(trace)) {
            let export: Value = serde_json::from_str(line).expect("a line of OTLP JSON");
            for resource_spans in export["resourceSpans"].as_array().into_iter().flatten() {
                for scope in resource_spans["scopeSpans"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    for span in scope["spans"].as_array().into_iter().flatten() {
                        if span["traceId"] == trace {
                            return (span.clone(), resource_spans["resource"].clone());
                        }
                    }
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "the Collector never wrote trace {trace} to {file}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn check(protocol: &str, endpoint_variable: &str, test: &str) {
    let (Some(endpoint), Some(file)) = (
        service(endpoint_variable, test),
        service("NAVREE_TEST_OTEL_FILE", test),
    ) else {
        return;
    };
    let trace = trace_id();

    export_one_request(&endpoint, protocol, &trace).await;

    let (span, resource) = exported(&file, &trace).await;
    assert_eq!(span["name"], "GET /api/ping", "{span}");
    assert_eq!(span["parentSpanId"], PARENT, "{span}");
    // SPAN_KIND_SERVER is 2 (the JSON may spell it).
    assert!(
        span["kind"] == 2 || span["kind"] == "SPAN_KIND_SERVER",
        "{span}"
    );
    assert_eq!(
        attribute(&span["attributes"], "http.route").as_deref(),
        Some("/api/ping"),
        "{span}"
    );
    assert_eq!(
        attribute(&resource["attributes"], "service.name").as_deref(),
        Some("crm-api"),
        "{resource}"
    );
    assert_eq!(
        attribute(&resource["attributes"], "deployment.environment").as_deref(),
        Some("test"),
        "{resource}"
    );
    assert!(
        attribute(&resource["attributes"], "service.instance.id").is_some(),
        "{resource}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn spans_reach_a_collector_over_otlp_http() {
    check(
        "http",
        "NAVREE_TEST_OTEL_HTTP_URL",
        "spans_reach_a_collector_over_otlp_http",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn spans_reach_a_collector_over_otlp_grpc() {
    check(
        "grpc",
        "NAVREE_TEST_OTEL_GRPC_URL",
        "spans_reach_a_collector_over_otlp_grpc",
    )
    .await;
}
