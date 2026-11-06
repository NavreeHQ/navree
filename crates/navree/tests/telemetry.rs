//! Telemetry through the facade, as the docs show it (the "Telemetry
//! events" section of `digging-deeper/health-and-metrics` and the testing
//! section of `microservices/tracing`): a sink registered with
//! `m.telemetry_sink::<S>()`, `TelemetryRecorder` in a test, and
//! `SpanRecorder` with an incoming `traceparent` and an outgoing call.

use http::Method;
use navree::http_client::{FakeResponse, FakeServer, HttpClient};
use navree::observe::testing::SpanRecorder;
use navree::prelude::*;
use navree::telemetry::{
    AiCall, EventKind, Interest, Outcome, Telemetry, TelemetryEvent, TelemetryRecord,
    TelemetryRecorder, TelemetrySink,
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
// A sink that hands records to a background task, as in the navree-telemetry
// skill.

/// Hands every record but log lines to a background task; never blocks.
pub struct ChannelSink(tokio::sync::mpsc::Sender<TelemetryRecord>);

impl TelemetrySink for ChannelSink {
    fn interest(&self) -> Interest {
        Interest::ALL.without(EventKind::Log)
    }

    fn record(&self, record: &TelemetryRecord) {
        // A full buffer drops the record rather than slow the request.
        let _ = self.0.try_send(record.clone());
    }
}

/// Stores telemetry records in the background (here: forwards them to the
/// test).
pub struct StoreModule(tokio::sync::mpsc::UnboundedSender<TelemetryRecord>);

impl Module for StoreModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let stored = self.0.clone();
        m.telemetry_sink_factory(Factory::<dyn TelemetrySink>::new(move |_r: Resolver| {
            let stored = stored.clone();
            async move {
                let (tx, mut rx) = tokio::sync::mpsc::channel::<TelemetryRecord>(1024);
                tokio::spawn(async move {
                    while let Some(record) = rx.recv().await {
                        let _ = stored.send(record);
                    }
                });
                Ok(Arc::new(ChannelSink(tx)) as Arc<dyn TelemetrySink>)
            }
        }));
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
            .get(format!(
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

// ---------------------------------------------------------------------------
// Reporting an error the handler handled itself, as in health-and-metrics.mdx.

/// The calendar sync failed.
#[derive(Debug)]
struct CalendarError(std::io::Error);

impl std::fmt::Display for CalendarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("calendar sync failed")
    }
}

impl std::error::Error for CalendarError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

async fn sync_calendar(_user: &str) -> std::result::Result<(), CalendarError> {
    Err(CalendarError(std::io::Error::other("connection reset")))
}

/// `POST /api/v1/calendar/sync` reports a failed sync and answers anyway.
#[injectable]
pub struct CalendarController;

#[controller("/calendar", version = "1")]
impl CalendarController {
    #[post("/sync")]
    #[public]
    async fn sync(&self) -> Result<Json<Value>> {
        let user = "u1";
        if let Err(error) = sync_calendar(user).await {
            Telemetry::current().report_exception(&error);
        }
        Ok(Json(json!({ "synced": false })))
    }
}

/// The calendar routes.
#[module(controllers = [CalendarController])]
pub struct CalendarModule;

// ---------------------------------------------------------------------------
// Emitting a platform event of the app's own, as in telemetry.mdx.

/// `POST /api/v1/summaries/draft` asks a model and reports the call.
#[injectable]
pub struct SummariesController;

#[controller("/summaries", version = "1")]
impl SummariesController {
    #[post("/draft")]
    #[public]
    async fn create(&self) -> Result<Json<Value>> {
        let started = std::time::Instant::now();
        // ... call the model ...
        Telemetry::current().ai_call(|| {
            AiCall::new("anthropic", "claude-sonnet-4-5", "generate", started.elapsed())
                .tokens(1200, 340)
        });
        Ok(Json(json!({ "summary": "Three deals closed in Ludhiana." })))
    }
}

/// The summaries routes.
#[module(controllers = [SummariesController])]
pub struct SummariesModule;

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
async fn a_channel_sink_hands_records_to_a_background_task() {
    let (stored, mut received) = tokio::sync::mpsc::unbounded_channel();
    let (app, _server) = rates_app(StoreModule(stored)).await;

    app.client()
        .get("/api/v1/rates/eur")
        .send()
        .await
        .assert_status(200);

    let mut kinds = Vec::new();
    while kinds.len() < 2 {
        let record = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
            .await
            .expect("a record in time")
            .expect("the channel is open");
        kinds.push(record.event.kind());
    }
    assert!(kinds.contains(&EventKind::RequestHandled), "{kinds:?}");
    assert!(kinds.contains(&EventKind::HttpClientRequest), "{kinds:?}");
    assert!(!kinds.contains(&EventKind::Log));
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

/// The "Testing with TelemetryRecorder" example, as written in the docs.
#[tokio::test]
async fn the_rates_call_happens_inside_the_request() {
    let recorder = TelemetryRecorder::new();
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://rates.example.com/*",
        FakeResponse::new(200),
    );
    let app = TestApp::builder()
        .module(HttpClientModule::from_config())
        .module(RatesModule)
        .module(recorder.module())
        .override_value(Arc::new(client))
        .build()
        .await;

    app.client()
        .get("/api/v1/rates/eur")
        .send()
        .await
        .assert_status(200);

    let request = &recorder.requests()[0];
    assert_eq!(request.route.as_deref(), Some("/api/v1/rates/{currency}"));
    let call = &recorder.http_client_requests()[0];
    assert_eq!(call.url, "https://rates.example.com/v1/eur");
    let record = &recorder.of_kind(EventKind::HttpClientRequest)[0];
    assert_eq!(
        record.context.request_id.as_deref(),
        Some(request.request_id.as_str())
    );

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_handler_reports_an_error_it_handled() {
    let recorder = TelemetryRecorder::only([EventKind::ExceptionReported]);
    let app = TestApp::builder()
        .module(CalendarModule)
        .module(recorder.module())
        .build()
        .await;

    app.client()
        .post("/api/v1/calendar/sync")
        .send()
        .await
        .assert_status(200);

    let records = recorder.of_kind(EventKind::ExceptionReported);
    assert_eq!(records.len(), 1);
    assert!(
        records[0].context.request_id.is_some(),
        "reported inside the request"
    );
    let exception = &recorder.exceptions()[0];
    assert_eq!(
        exception.chain,
        ["calendar sync failed", "connection reset"]
    );
    assert!(!exception.panic);
    let location = exception
        .location
        .as_deref()
        .expect("the caller's location");
    assert!(location.contains("telemetry.rs"), "{location}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn without_a_sink_reporting_does_nothing() {
    let app = TestApp::builder().module(CalendarModule).build().await;
    app.client()
        .post("/api/v1/calendar/sync")
        .send()
        .await
        .assert_status(200);
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

#[tokio::test]
async fn a_handler_reports_a_model_call_and_a_test_reads_it() {
    let recorder = TelemetryRecorder::new();
    let app = TestApp::builder()
        .module(SummariesModule)
        .module(recorder.module())
        .build()
        .await;

    app.client()
        .post("/api/v1/summaries/draft")
        .send()
        .await
        .assert_status(200);

    let call = &recorder.ai_calls()[0];
    assert_eq!(call.model, "claude-sonnet-4-5");
    assert_eq!(call.input_tokens, Some(1200));
    assert_eq!(call.output_tokens, Some(340));
    assert_eq!(call.outcome, Outcome::Success);
    let record = &recorder.of_kind(EventKind::AiCall)[0];
    assert_eq!(
        record.context.request_id.as_deref(),
        Some(recorder.requests()[0].request_id.as_str()),
        "reported inside the request"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_platform_kinds_are_in_the_facades_telemetry() {
    assert_eq!(EventKind::ALL.len(), 19);
    for name in [
        "ai_call",
        "broadcast",
        "storage_op",
        "webhook_delivery",
        "search",
        "sign_in",
        "ui_operation",
    ] {
        assert!(
            EventKind::ALL.iter().any(|kind| kind.as_str() == name),
            "{name}"
        );
    }
}
