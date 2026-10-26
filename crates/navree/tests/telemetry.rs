//! Telemetry through the facade, as the docs show it (the "Telemetry
//! events" section of `digging-deeper/health-and-metrics` and the testing
//! section of `microservices/tracing`): a sink registered with
//! `m.telemetry_sink::<S>()`, `TelemetryRecorder` in a test, and
//! `SpanRecorder` with an incoming `traceparent` and an outgoing call.

use navree::axum::http::Method;
use navree::http_client::{FakeResponse, FakeServer, HttpClient};
use navree::observe::testing::SpanRecorder;
use navree::prelude::*;
use navree::telemetry::{
    EventKind, Interest, Telemetry, TelemetryEvent, TelemetryRecord, TelemetryRecorder,
    TelemetrySink,
};
use navree::testing::TestApp;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// A sink, as in health-and-metrics.mdx.

/// Counts failed job attempts by queue and job type, on `/metrics`.
#[injectable]
pub struct JobFailureMetrics;

impl TelemetrySink for JobFailureMetrics {
    fn interest(&self) -> Interest {
        Interest::only([EventKind::JobFailed])
    }

    fn record(&self, record: &TelemetryRecord) {
        if let TelemetryEvent::JobFailed(job) = &record.event {
            metrics::counter!(
                "job_failures_total",
                "queue" => job.queue.clone(),
                "job" => job.job_type.clone(),
            )
            .increment(1);
        }
    }
}

/// Registers the app's own telemetry sinks.
#[module(register = Self::wire)]
pub struct MonitoringModule;

impl MonitoringModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.telemetry_sink::<JobFailureMetrics>();
    }
}

// ---------------------------------------------------------------------------
// A route that calls another service.

/// `GET /api/v1/rates/{currency}` asks the rates provider.
#[injectable]
pub struct RatesController {
    http: Arc<HttpClient>,
}

#[controller("/rates", version = "1")]
impl RatesController {
    #[get("/{currency}")]
    #[public]
    async fn show(&self, Path(currency): Path<String>) -> Result<Json<Value>> {
        let response = self
            .http
            .get(&format!(
                "https://rates.example.com/v1/{currency}?key=secret"
            ))
            .send()
            .await
            .map_err(AppError::internal)?;
        Ok(Json(
            json!({ "provider_status": response.status().as_u16() }),
        ))
    }
}

/// The rates routes.
#[module(controllers = [RatesController])]
pub struct RatesModule;

const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

async fn rates_app(sink: impl Module) -> (TestApp, FakeServer) {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://rates.example.com/*",
        FakeResponse::new(200),
    );
    let app = TestApp::builder()
        .config("observe.metrics", false)
        .config("http.per_ip_rate_limit", Value::Null)
        .module(ObserveModule::from_config())
        .module(HttpClientModule::from_config())
        .module(RatesModule)
        .module(sink)
        .override_value(Arc::new(client))
        .build()
        .await;
    (app, server)
}

#[tokio::test]
async fn a_sink_receives_only_the_kinds_it_wants() {
    let app = TestApp::builder().module(MonitoringModule).build().await;
    let telemetry = Telemetry::from_app(app.ctx()).await;
    assert!(telemetry.enabled(EventKind::JobFailed));
    assert!(!telemetry.enabled(EventKind::RequestHandled));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn without_a_sink_telemetry_is_disabled() {
    let app = TestApp::builder().build().await;
    let telemetry = Telemetry::from_app(app.ctx()).await;
    assert!(telemetry.is_disabled());
    assert!(!telemetry.enabled(EventKind::RequestHandled));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_recorder_sees_the_request_and_the_outgoing_call() {
    let recorder = TelemetryRecorder::new();
    let (app, _server) = rates_app(recorder.module()).await;

    app.client()
        .get("/api/v1/rates/eur")
        .send()
        .await
        .assert_status(200);

    let request = &recorder.requests()[0];
    assert_eq!(request.route.as_deref(), Some("/api/v1/rates/{currency}"));
    assert_eq!(request.status, 200);
    let call = &recorder.http_client_requests()[0];
    assert_eq!(
        call.url, "https://rates.example.com/v1/eur",
        "no query string"
    );
    assert_eq!(call.status, Some(200));
    let record = &recorder.of_kind(EventKind::HttpClientRequest)[0];
    assert_eq!(
        record.context.request_id.as_deref(),
        Some(request.request_id.as_str()),
        "the call happened in the request"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_outgoing_call_joins_the_callers_trace() {
    let spans = SpanRecorder::new();
    let (app, server) = rates_app(spans.module()).await;

    app.client()
        .get("/api/v1/rates/eur")
        .header("traceparent", TRACEPARENT)
        .send()
        .await
        .assert_status(200);

    let request = spans.find("GET /api/v1/rates/{currency}");
    let call = spans.find("GET");
    assert_eq!(
        request.trace_id.to_string(),
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
    assert_eq!(call.trace_id, request.trace_id);
    assert_eq!(call.parent_span_id, Some(request.span_id));
    assert_eq!(
        call.attributes["url.full"],
        json!("https://rates.example.com/v1/eur")
    );
    // The provider received the call's span as its parent.
    let sent = &server.recorded()[0];
    let traceparent = sent.header("traceparent").expect("a traceparent");
    assert!(
        traceparent.contains(&call.span_id.to_string()),
        "{traceparent}"
    );
    app.shutdown().await.unwrap();
}
