//! Spans: the telemetry events of a request, its outgoing calls, queries
//! and jobs as OpenTelemetry-shaped spans, recorded with `SpanRecorder`.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use http::{Method, Request};
use navree_config::{ConfigModule, ConfigSources};
use navree_http::AppRouter;
use navree_http::body::Body;
use navree_http::{
    AppError, Controller, Extension, HttpModule, ModuleBuilderHttpExt, RouteDef, build_router,
};
use navree_http_client::{FakeResponse, FakeServer, HttpClient};
use navree_kernel::{
    App, AppContext, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver,
};
use navree_observe::testing::SpanRecorder;
use navree_observe::{ObserveModule, SpanKind, SpanStatus};
use navree_telemetry::{
    EventContext, QueryExecuted, TelemetryEvent, TelemetryRecord, TelemetrySink, TraceContext,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const PARENT: &str = "00f067aa0ba902b7";

struct RoutesController;

impl Injectable for RoutesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(RoutesController) })
    }
}

impl Controller for RoutesController {
    fn prefix(&self) -> &'static str {
        "/deals"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/{id}", || async { "deal" }).public(),
            RouteDef::on(Method::GET, "/{id}/broken", || async {
                Err::<&str, AppError>(AppError::internal(std::io::Error::other("disk full")))
            })
            .public(),
            RouteDef::on(Method::GET, "/{id}/missing", || async {
                Err::<&str, AppError>(AppError::not_found())
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "/{id}/rates",
                |Extension(ctx): Extension<AppContext>| async move {
                    let client = ctx.get::<HttpClient>().await.map_err(AppError::internal)?;
                    client
                        .get("https://rates.example.com/v1/eur?key=secret")
                        .send()
                        .await
                        .map_err(AppError::internal)?;
                    Ok::<_, AppError>("ok")
                },
            )
            .public(),
        ]
    }
}

struct RoutesModule;

impl Module for RoutesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<RoutesController>();
    }
}

struct Setup {
    app: App,
    router: AppRouter,
    spans: SpanRecorder,
    server: FakeServer,
}

async fn setup(config: ConfigSources) -> Setup {
    let spans = SpanRecorder::new();
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));
    let app = Navree::app()
        .module(ConfigModule::for_root(
            config
                .set("observe.metrics", false)
                .set("http.per_ip_rate_limit", Value::Null),
        ))
        .module(HttpModule::from_config())
        .module(ObserveModule::from_config())
        .module(RoutesModule)
        .module(spans.module())
        .override_value(Arc::new(client))
        .boot()
        .await
        .expect("boots");
    let router = build_router(app.ctx()).await.expect("router");
    Setup {
        app,
        router,
        spans,
        server,
    }
}

impl Setup {
    async fn get(&self, path: &str, traceparent: Option<&str>) -> u16 {
        let mut request = Request::get(path);
        if let Some(traceparent) = traceparent {
            request = request.header("traceparent", traceparent);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        response.status().as_u16()
    }
}

#[tokio::test]
async fn a_request_is_a_server_span_in_the_callers_trace() {
    let s = setup(ConfigSources::new()).await;
    let traceparent = format!("00-{TRACE}-{PARENT}-01");
    assert_eq!(s.get("/api/deals/7", Some(&traceparent)).await, 200);

    let span = s.spans.find("GET /api/deals/{id}");
    assert_eq!(span.kind, SpanKind::Server);
    assert_eq!(span.trace_id.to_string(), TRACE);
    assert_eq!(
        span.parent_span_id.map(|id| id.to_string()).as_deref(),
        Some(PARENT)
    );
    assert_eq!(span.status, SpanStatus::Unset);
    assert_eq!(span.attributes["http.request.method"], json!("GET"));
    assert_eq!(span.attributes["http.route"], json!("/api/deals/{id}"));
    assert_eq!(span.attributes["http.response.status_code"], json!(200));
    assert!(span.attributes.contains_key("navree.request_id"));
    assert!(span.end >= span.start);
    s.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn server_errors_mark_the_span_and_client_errors_do_not() {
    let s = setup(ConfigSources::new()).await;
    assert_eq!(s.get("/api/deals/7/broken", None).await, 500);
    assert_eq!(s.get("/api/deals/7/missing", None).await, 404);
    let broken = s.spans.find("GET /api/deals/{id}/broken");
    assert!(
        matches!(broken.status, SpanStatus::Error { .. }),
        "{broken:?}"
    );
    let missing = s.spans.find("GET /api/deals/{id}/missing");
    assert_eq!(missing.status, SpanStatus::Unset);
    s.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_outgoing_call_is_a_client_span_under_the_request() {
    let s = setup(ConfigSources::new()).await;
    assert_eq!(s.get("/api/deals/7/rates", None).await, 200);

    let request = s.spans.find("GET /api/deals/{id}/rates");
    let call = s.spans.find("GET");
    assert_eq!(call.kind, SpanKind::Client);
    assert_eq!(call.trace_id, request.trace_id);
    assert_eq!(call.parent_span_id, Some(request.span_id));
    assert_eq!(
        call.attributes["server.address"],
        json!("rates.example.com")
    );
    assert_eq!(
        call.attributes["url.full"],
        json!("https://rates.example.com/v1/eur"),
        "no query string"
    );
    assert_eq!(call.attributes["http.response.status_code"], json!(200));
    let sent = s.server.recorded()[0]
        .header("traceparent")
        .unwrap()
        .to_owned();
    assert!(sent.contains(&call.span_id.to_string()), "{sent}");
    s.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn unsampled_traces_are_not_recorded() {
    let s = setup(ConfigSources::new().set("observe.tracing.sample_ratio", 0.0)).await;
    s.get("/api/deals/7", None).await;
    assert!(s.spans.all().is_empty(), "{:?}", s.spans.all());
    s.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_untrusted_caller_is_kept_as_a_link() {
    let s = setup(ConfigSources::new().set("observe.tracing.trust_incoming", false)).await;
    let traceparent = format!("00-{TRACE}-{PARENT}-01");
    s.get("/api/deals/7", Some(&traceparent)).await;
    let span = s.spans.find("GET /api/deals/{id}");
    assert_ne!(span.trace_id.to_string(), TRACE);
    assert_eq!(span.parent_span_id, None);
    assert_eq!(span.links.len(), 1);
    assert_eq!(span.links[0].span_id.to_string(), PARENT);
    s.app.shutdown().await.unwrap();
}

fn record(event: TelemetryEvent, trace: &TraceContext) -> TelemetryRecord {
    TelemetryRecord {
        at: Utc::now(),
        context: EventContext {
            trace: Some(trace.clone()),
            ..EventContext::default()
        },
        event,
    }
}

#[test]
fn queries_are_named_by_operation_and_table() {
    let spans = SpanRecorder::new();
    let trace = TraceContext::root(true);
    for sql in [
        r#"SELECT "deals"."id" FROM "deals" WHERE "deals"."id" = $1"#,
        "INSERT INTO notes (id) VALUES (?)",
        "UPDATE `deals` SET title = ?",
        "DELETE FROM deals WHERE id = ?",
        "PRAGMA foreign_keys = ON",
    ] {
        spans.record(&record(
            TelemetryEvent::QueryExecuted(QueryExecuted {
                sql: sql.to_owned(),
                duration: Duration::from_millis(2),
                rows: None,
                connection: "postgres://app:***@db/crm".to_owned(),
                failed: sql.starts_with("DELETE"),
            }),
            &trace,
        ));
    }
    let names: Vec<String> = spans.all().into_iter().map(|span| span.name).collect();
    assert_eq!(
        names,
        [
            "SELECT deals",
            "INSERT notes",
            "UPDATE deals",
            "DELETE deals",
            "PRAGMA"
        ]
    );
    let select = spans.find("SELECT deals");
    assert_eq!(select.kind, SpanKind::Client);
    assert_eq!(select.parent_span_id, Some(trace.span_id));
    assert_eq!(select.attributes["db.system.name"], json!("postgresql"));
    assert_eq!(select.attributes["db.operation.name"], json!("SELECT"));
    assert_eq!(select.attributes["db.collection.name"], json!("deals"));
    assert!(
        select.attributes["db.query.text"]
            .as_str()
            .unwrap()
            .contains("$1")
    );
    assert!(matches!(
        spans.find("DELETE deals").status,
        SpanStatus::Error { .. }
    ));
}

#[test]
#[should_panic(expected = "no span named `nothing`")]
fn find_fails_the_test_without_a_match() {
    SpanRecorder::new().find("nothing");
}
