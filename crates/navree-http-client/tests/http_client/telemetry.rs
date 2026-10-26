//! Telemetry: `traceparent` on outgoing requests and `HttpClientRequest` events.

use std::sync::Arc;

use http::Method;
use navree_http_client::{FakeResponse, HttpClient, HttpClientModule, HttpClientOptions};
use navree_kernel::Navree;
use navree_telemetry::{
    EventKind, Telemetry, TelemetryContext, TelemetryRecorder, TraceContext,
};

fn recording() -> (Telemetry, TelemetryRecorder) {
    let recorder = TelemetryRecorder::new();
    (
        Telemetry::with_sinks(vec![Arc::new(recorder.clone())]),
        recorder,
    )
}

#[tokio::test]
async fn requests_inside_a_trace_carry_its_traceparent() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));
    let mut trace = TraceContext::root(true);
    trace.trace_state = Some("congo=t61rcWkgMzE".to_owned());
    let context = TelemetryContext::new(Telemetry::disabled()).with_trace(trace.clone());

    context
        .scope(async {
            client.get("https://api.example.com/ping").send().await.unwrap();
        })
        .await;

    let sent = &server.recorded()[0];
    let traceparent = sent.header("traceparent").expect("a traceparent");
    let fields: Vec<&str> = traceparent.split('-').collect();
    assert_eq!(fields[0], "00");
    assert_eq!(fields[1], trace.trace_id.to_string(), "same trace");
    assert_ne!(fields[2], trace.span_id.to_string(), "a new client span");
    assert_eq!(fields[3], "01");
    assert_eq!(sent.header("tracestate"), Some("congo=t61rcWkgMzE"));
}

#[tokio::test]
async fn an_unsampled_trace_is_propagated_unsampled() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));
    let context = TelemetryContext::new(Telemetry::disabled()).with_trace(TraceContext::root(false));
    context
        .scope(async {
            client.get("https://api.example.com/ping").send().await.unwrap();
        })
        .await;
    let traceparent = server.recorded()[0].header("traceparent").unwrap().to_owned();
    assert!(traceparent.ends_with("-00"), "{traceparent}");
}

#[tokio::test]
async fn without_a_trace_no_header_is_sent() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));
    client.get("https://api.example.com/ping").send().await.unwrap();
    assert_eq!(server.recorded()[0].header("traceparent"), None);
}

#[tokio::test]
async fn a_traceparent_set_by_the_caller_is_kept() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));
    let mine = "00-11111111111111111111111111111111-2222222222222222-01";
    TelemetryContext::new(Telemetry::disabled())
        .with_trace(TraceContext::root(true))
        .scope(async {
            client
                .get("https://api.example.com/ping")
                .header("traceparent", mine)
                .send()
                .await
                .unwrap();
        })
        .await;
    assert_eq!(server.recorded()[0].header("traceparent"), Some(mine));
}

#[tokio::test]
async fn finished_requests_are_reported_without_their_query_string() {
    let (telemetry, recorder) = recording();
    let (client, server) = HttpClient::fake();
    let client = client.with_telemetry(telemetry);
    server.on(Method::GET, "*", FakeResponse::new(200));
    let trace = TraceContext::root(true);

    TelemetryContext::new(Telemetry::disabled())
        .with_trace(trace)
        .scope(async {
            client
                .get("https://user:pass@api.example.com/v1/rates?key=secret#top")
                .send()
                .await
                .unwrap();
        })
        .await;

    let requests = recorder.http_client_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "GET");
    assert_eq!(request.url, "https://api.example.com/v1/rates");
    assert_eq!(request.status, Some(200));
    assert_eq!(request.attempts, 1);
    assert_eq!(request.error, None);
    let sent = server.recorded()[0].header("traceparent").unwrap().to_owned();
    assert_eq!(
        request.span_id.map(|id| id.to_string()).as_deref(),
        sent.split('-').nth(2),
        "the reported span is the one sent"
    );
}

#[tokio::test]
async fn failures_are_reported_with_the_error_and_attempts() {
    let (telemetry, recorder) = recording();
    let (client, server) = HttpClient::fake_with(HttpClientOptions {
        retries: 1,
        retry_base_ms: 1,
        ..HttpClientOptions::default()
    });
    let client = client.with_telemetry(telemetry);
    server.on(Method::GET, "*", FakeResponse::connection_failed());

    let result = client.get("https://api.example.com/ping").send().await;
    assert!(result.is_err());

    let request = &recorder.http_client_requests()[0];
    assert_eq!(request.status, None);
    assert_eq!(request.attempts, 2, "one retry");
    let error = request.error.as_deref().expect("an error");
    assert!(error.contains("api.example.com"), "{error}");
    assert_eq!(request.span_id, None, "no trace, no span");
}

#[tokio::test]
async fn a_client_without_telemetry_reports_to_the_current_context() {
    let (telemetry, recorder) = recording();
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(204));
    TelemetryContext::new(telemetry)
        .scope(async {
            client.get("https://api.example.com/ping").send().await.unwrap();
        })
        .await;
    assert_eq!(recorder.http_client_requests()[0].status, Some(204));
    let record = &recorder.of_kind(EventKind::HttpClientRequest)[0];
    assert_eq!(record.context.trace, None);
}

#[tokio::test]
async fn the_module_client_reports_to_the_app() {
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(HttpClientModule::for_root(HttpClientOptions::default()))
        .module(recorder.module())
        .boot()
        .await
        .unwrap();
    let client = app.ctx().get::<HttpClient>().await.unwrap();
    assert!(client.telemetry().is_active());
    app.shutdown().await.unwrap();
}
