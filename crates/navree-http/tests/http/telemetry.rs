//! Telemetry: `RequestHandled` and `ExceptionReported` events, the request's
//! `TelemetryContext`, and W3C trace context on incoming requests.

use http::Method;
use navree_config::ConfigSources;
use navree_http::body::Body;
use navree_http::{AppError, RouteDef};
use navree_telemetry::{EventKind, TelemetryContext, TelemetryRecorder, current_trace_id};
use serde_json::json;

use crate::support::{Setup, capture_logs};

const ALICE: &str = "0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23";
const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const PARENT: &str = "00f067aa0ba902b7";

/// A route that answers with the current trace id (empty without one).
fn trace_route() -> RouteDef {
    RouteDef::on(Method::GET, "/trace", || async {
        current_trace_id()
            .map(|id| id.to_string())
            .unwrap_or_default()
    })
    .public()
}

fn get_with(path: &str, headers: &[(&str, &str)]) -> http::Request<Body> {
    let mut request = http::Request::get(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn every_request_is_reported_with_route_status_user_and_request_id() {
    let recorder = TelemetryRecorder::new();
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/deals/{id}", || async { "deal" }).authenticated(),
    ])
    .authenticated()
    .module(recorder.module())
    .boot()
    .await;

    let response = app
        .send(get_with(
            "/api/deals/42?expand=owner",
            &[("x-user-id", ALICE)],
        ))
        .await;
    assert_eq!(response.status, 200);
    app.get("/api/unknown").await;

    let requests = recorder.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    let request = &requests[0];
    assert_eq!(request.method, "GET");
    assert_eq!(request.route.as_deref(), Some("/api/deals/{id}"));
    assert_eq!(request.path, "/api/deals/42", "no query string");
    assert_eq!(request.status, 200);
    assert_eq!(request.user_id.as_deref(), Some(ALICE));
    assert_eq!(request.workspace_id, None);
    assert_eq!(
        Some(request.request_id.as_str()),
        response.header("x-request-id")
    );
    assert_eq!(request.client_ip.as_deref(), Some("0.0.0.0"));

    let unmatched = &requests[1];
    assert_eq!(unmatched.route, None);
    assert_eq!(unmatched.status, 404);
    assert_eq!(unmatched.user_id, None);

    // The record carries the request's context too.
    let record = &recorder.of_kind(EventKind::RequestHandled)[0];
    assert_eq!(
        record.context.request_id.as_deref(),
        Some(request.request_id.as_str())
    );
    assert_eq!(record.context.user_id.as_deref(), Some(ALICE));
    assert!(record.context.trace.is_some());
}

#[tokio::test]
async fn a_request_whose_handler_panicked_is_reported_with_the_route_that_panicked() {
    let recorder = TelemetryRecorder::new();
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/deals/{id}", || async {
            if true {
                panic!("stage out of range");
            }
            "never"
        })
        .public(),
    ])
    .module(recorder.module())
    .boot()
    .await;

    let response = app.get("/api/deals/42").await;
    assert_eq!(response.status, 500);
    let requests = recorder.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].status, 500);
    assert_eq!(
        requests[0].route.as_deref(),
        Some("/api/deals/{id}"),
        "the route is known although the handler never answered"
    );
}

#[tokio::test]
async fn handlers_run_inside_the_request_context() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/context", || async {
            let context = TelemetryContext::current().expect("a request context");
            json!({
                "request_id": context.request_id(),
                "user_id": context.user_id(),
            })
            .to_string()
        })
        .authenticated(),
    ])
    .authenticated()
    .boot()
    .await;

    let response = app
        .send(get_with("/api/context", &[("x-user-id", ALICE)]))
        .await;
    let body = response.json();
    assert_eq!(body["request_id"], response.header("x-request-id").unwrap());
    assert_eq!(body["user_id"], ALICE, "set once authenticated");
}

#[tokio::test]
async fn an_incoming_traceparent_is_continued() {
    let recorder = TelemetryRecorder::new();
    let app = Setup::new(vec![trace_route()])
        .module(recorder.module())
        .boot()
        .await;
    let traceparent = format!("00-{TRACE}-{PARENT}-01");
    let response = app
        .send(get_with(
            "/api/trace",
            &[
                ("traceparent", &traceparent),
                ("tracestate", "congo=t61rcWkgMzE"),
            ],
        ))
        .await;
    assert_eq!(response.text(), TRACE);

    let record = &recorder.of_kind(EventKind::RequestHandled)[0];
    let trace = record.context.trace.as_ref().expect("a trace");
    assert_eq!(trace.trace_id.to_string(), TRACE);
    assert_eq!(
        trace.parent_span_id.map(|id| id.to_string()).as_deref(),
        Some(PARENT)
    );
    assert!(trace.sampled);
    assert_eq!(trace.trace_state.as_deref(), Some("congo=t61rcWkgMzE"));
}

#[tokio::test]
async fn a_request_without_traceparent_starts_a_trace() {
    let app = Setup::new(vec![trace_route()]).boot().await;
    let first = app.get("/api/trace").await.text();
    let second = app.get("/api/trace").await.text();
    assert_eq!(first.len(), 32, "{first:?}");
    assert_ne!(first, second, "one trace per request");
    let invalid = app
        .send(get_with("/api/trace", &[("traceparent", "garbage")]))
        .await
        .text();
    assert_eq!(invalid.len(), 32, "an invalid header starts a new trace");
}

#[tokio::test]
async fn untrusted_callers_get_a_new_trace_with_a_link() {
    let recorder = TelemetryRecorder::new();
    let app = Setup::new(vec![trace_route()])
        .configured_by(
            ConfigSources::new()
                .set("observe.tracing.trust_incoming", json!(false))
                .set("http.per_ip_rate_limit", serde_json::Value::Null),
        )
        .module(recorder.module())
        .boot()
        .await;
    let traceparent = format!("00-{TRACE}-{PARENT}-01");
    let response = app
        .send(get_with("/api/trace", &[("traceparent", &traceparent)]))
        .await;
    assert_ne!(response.text(), TRACE);
    let record = &recorder.of_kind(EventKind::RequestHandled)[0];
    let link = record.context.trace.as_ref().unwrap().link.expect("a link");
    assert_eq!(link.trace_id.to_string(), TRACE);
    assert_eq!(link.span_id.to_string(), PARENT);
}

#[tokio::test]
async fn tracing_can_be_turned_off() {
    let app = Setup::new(vec![trace_route()])
        .configured_by(ConfigSources::new().set("observe.tracing.enabled", json!(false)))
        .boot()
        .await;
    let traceparent = format!("00-{TRACE}-{PARENT}-01");
    let response = app
        .send(get_with("/api/trace", &[("traceparent", &traceparent)]))
        .await;
    assert_eq!(response.status, 200);
    assert_eq!(response.text(), "", "no trace at all");
}

#[tokio::test]
async fn the_sample_ratio_applies_to_new_traces_only() {
    let recorder = TelemetryRecorder::new();
    let app = Setup::new(vec![trace_route()])
        .configured_by(ConfigSources::new().set("observe.tracing.sample_ratio", json!(0.0)))
        .module(recorder.module())
        .boot()
        .await;
    app.get("/api/trace").await;
    let traceparent = format!("00-{TRACE}-{PARENT}-01");
    app.send(get_with("/api/trace", &[("traceparent", &traceparent)]))
        .await;
    let records = recorder.of_kind(EventKind::RequestHandled);
    assert!(!records[0].context.trace.as_ref().unwrap().sampled);
    assert!(
        records[1].context.trace.as_ref().unwrap().sampled,
        "the caller's decision holds"
    );
}

#[tokio::test]
async fn an_invalid_sample_ratio_fails_the_router() {
    let setup = Setup::new(vec![trace_route()])
        .configured_by(ConfigSources::new().set("observe.tracing.sample_ratio", json!("half")));
    let error = setup.router_error().await;
    assert!(
        error.contains("`observe.tracing`") && error.contains("`sample_ratio`"),
        "{error}"
    );
}

#[tokio::test]
async fn server_errors_are_reported_as_exceptions() {
    let recorder = TelemetryRecorder::new();
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/broken", || async {
            Err::<&str, AppError>(AppError::internal(std::io::Error::other("disk full")))
        })
        .public(),
        RouteDef::on(Method::GET, "/missing", || async {
            Err::<&str, AppError>(AppError::not_found())
        })
        .public(),
    ])
    .module(recorder.module())
    .boot()
    .await;

    app.get("/api/broken").await.assert_error(500);
    app.get("/api/missing").await.assert_error(404);

    let exceptions = recorder.exceptions();
    assert_eq!(exceptions.len(), 1, "4xx responses are not exceptions");
    let exception = &exceptions[0];
    assert_eq!(exception.message, "disk full");
    assert_eq!(exception.chain, ["disk full"]);
    assert!(!exception.panic);
    let request = exception.request.as_ref().expect("the request");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/broken");
    assert_eq!(request.route.as_deref(), Some("/api/broken"));
    assert_eq!(request.status, 500);
    let record = &recorder.of_kind(EventKind::ExceptionReported)[0];
    assert!(record.context.request_id.is_some());
}

#[tokio::test]
async fn panics_are_reported_with_their_location() {
    let recorder = TelemetryRecorder::new();
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/panics", || async {
            if std::hint::black_box(true) {
                panic!("boom");
            }
            "unreachable"
        })
        .public(),
    ])
    .module(recorder.module())
    .boot()
    .await;

    app.get("/api/panics").await.assert_error(500);

    let exceptions = recorder.exceptions();
    assert_eq!(exceptions.len(), 1, "{exceptions:?}");
    let exception = &exceptions[0];
    assert!(exception.panic);
    assert_eq!(exception.message, "boom");
    let location = exception.location.as_deref().expect("a location");
    assert!(location.contains("telemetry.rs"), "{location}");
    assert_eq!(exception.request.as_ref().unwrap().status, 500);
}

#[tokio::test]
async fn an_app_without_sinks_reports_nothing_and_still_traces() {
    let app = Setup::new(vec![trace_route()]).boot().await;
    let traceparent = format!("00-{TRACE}-{PARENT}-01");
    let response = app
        .send(get_with("/api/trace", &[("traceparent", &traceparent)]))
        .await;
    assert_eq!(response.text(), TRACE, "propagation needs no sink");
}

/// Records, for each `navree::request` line, the trace id current when it
/// was written.
#[derive(Clone, Default)]
struct RequestLines(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for RequestLines {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if event.metadata().target() == "navree::request" {
            let trace = current_trace_id()
                .map(|id| id.to_string())
                .unwrap_or_default();
            self.0.lock().unwrap().push(trace);
        }
    }
}

// `#[tokio::test]` runs on one thread, so a thread-local default subscriber
// sees every event of the request.
#[tokio::test]
async fn the_request_log_line_is_written_in_the_requests_trace() {
    use tracing_subscriber::layer::SubscriberExt;

    let lines = RequestLines::default();
    let _guard = capture_logs(tracing_subscriber::registry().with(lines.clone()));
    let app = Setup::new(vec![trace_route()]).boot().await;
    let traceparent = format!("00-{TRACE}-{PARENT}-01");
    app.send(get_with("/api/trace", &[("traceparent", &traceparent)]))
        .await;
    assert_eq!(*lines.0.lock().unwrap(), [TRACE.to_owned()]);
}
