//! OTLP export (feature `otel`): spans reach in-process OTLP/HTTP and
//! OTLP/gRPC receivers, with the resource and headers, and are flushed when
//! the app shuts down. No network beyond 127.0.0.1, no Docker.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::routing::post;
use http::{HeaderMap, Method, Request, StatusCode};
use navree_config::{ConfigModule, ConfigSources};
use navree_http::body::{Body, Bytes};
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RouteDef, build_router};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver};
use navree_observe::ObserveModule;
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::any_value::Value as AnyValue;
use prost::Message;
use serde_json::{Value, json};
use tower::ServiceExt;

const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const PARENT: &str = "00f067aa0ba902b7";

/// What a receiver got: the decoded requests and the headers they came with.
#[derive(Clone, Default)]
struct Received(Arc<Mutex<Vec<(HeaderMap, ExportTraceServiceRequest)>>>);

impl Received {
    fn all(&self) -> Vec<(HeaderMap, ExportTraceServiceRequest)> {
        self.0.lock().unwrap().clone()
    }
}

/// OTLP/HTTP: `POST /v1/traces` with a protobuf body.
async fn http_export(
    State(received): State<Received>,
    headers: HeaderMap,
    body: Bytes,
) -> impl navree_http::response::IntoResponse {
    let request = ExportTraceServiceRequest::decode(body).expect("an OTLP trace request");
    received.0.lock().unwrap().push((headers, request));
    (
        [("content-type", "application/x-protobuf")],
        ExportTraceServiceResponse::default().encode_to_vec(),
    )
}

/// OTLP/gRPC: the `TraceService/Export` call over HTTP/2, with gRPC's
/// length-prefixed messages and a `grpc-status` trailer.
async fn grpc_export(
    State(received): State<Received>,
    headers: HeaderMap,
    body: Bytes,
) -> http::Response<Body> {
    assert!(body.len() >= 5, "a gRPC frame");
    let request = ExportTraceServiceRequest::decode(&body[5..]).expect("an OTLP trace request");
    received.0.lock().unwrap().push((headers, request));
    let message = ExportTraceServiceResponse::default().encode_to_vec();
    let mut frame = vec![0u8];
    frame.extend_from_slice(&(message.len() as u32).to_be_bytes());
    frame.extend_from_slice(&message);
    let mut trailers = HeaderMap::new();
    trailers.insert("grpc-status", "0".parse().unwrap());
    let frames = futures_util::stream::iter(vec![
        Ok::<_, std::convert::Infallible>(hyper::body::Frame::data(Bytes::from(frame))),
        Ok(hyper::body::Frame::trailers(trailers)),
    ]);
    http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/grpc")
        .body(Body::new(http_body_util::StreamBody::new(frames)))
        .unwrap()
}

/// Starts a receiver on 127.0.0.1; returns its base URL.
async fn receiver(received: Received) -> String {
    let router = Router::new()
        .route("/v1/traces", post(http_export))
        .route(
            "/opentelemetry.proto.collector.trace.v1.TraceService/Export",
            post(grpc_export),
        )
        .with_state(received);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        navree_http::serve(listener, router).await.unwrap();
    });
    format!("http://{address}")
}

struct PingController;

impl Injectable for PingController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(PingController) })
    }
}

impl Controller for PingController {
    fn prefix(&self) -> &'static str {
        "/ping"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(Method::GET, "/", || async { "pong" }).public()]
    }
}

struct PingModule;

impl Module for PingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<PingController>();
    }
}

/// Boots an app exporting to `endpoint` over `protocol`, sends one traced
/// request, shuts down (which flushes) and returns what the receiver got.
async fn export_one_request(protocol: &str, received: &Received) {
    let endpoint = receiver(received.clone()).await;
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
                        "headers": { "x-api-key": "k-1" },
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
                .header("traceparent", format!("00-{TRACE}-{PARENT}-01"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    app.shutdown().await.expect("shutdown flushes the queue");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn string_attribute(
    attributes: &[opentelemetry_proto::tonic::common::v1::KeyValue],
    key: &str,
) -> Option<String> {
    attributes.iter().find(|kv| kv.key == key).and_then(|kv| {
        match kv.value.as_ref()?.value.as_ref()? {
            AnyValue::StringValue(text) => Some(text.clone()),
            _ => None,
        }
    })
}

fn assert_exported(received: &Received) {
    let all = received.all();
    assert!(!all.is_empty(), "the receiver got an export");
    let (headers, request) = &all[0];
    assert_eq!(
        headers.get("x-api-key").and_then(|v| v.to_str().ok()),
        Some("k-1"),
        "configured headers are sent"
    );
    let resource_spans = &request.resource_spans[0];
    let resource = resource_spans.resource.as_ref().expect("a resource");
    assert_eq!(
        string_attribute(&resource.attributes, "service.name").as_deref(),
        Some("crm-api")
    );
    assert_eq!(
        string_attribute(&resource.attributes, "deployment.environment").as_deref(),
        Some("test")
    );
    assert!(string_attribute(&resource.attributes, "service.instance.id").is_some());
    let spans: Vec<_> = resource_spans
        .scope_spans
        .iter()
        .flat_map(|scope| scope.spans.iter())
        .collect();
    let server = spans
        .iter()
        .find(|span| span.name == "GET /api/ping")
        .expect("the request's span");
    assert_eq!(hex(&server.trace_id), TRACE);
    assert_eq!(hex(&server.parent_span_id), PARENT);
    assert_eq!(server.kind, 2, "SPAN_KIND_SERVER");
    assert_eq!(
        string_attribute(&server.attributes, "http.route").as_deref(),
        Some("/api/ping")
    );
    assert!(server.end_time_unix_nano >= server.start_time_unix_nano);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spans_are_exported_over_otlp_http() {
    let received = Received::default();
    export_one_request("http", &received).await;
    assert_exported(&received);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spans_are_exported_over_otlp_grpc() {
    let received = Received::default();
    export_one_request("grpc", &received).await;
    assert_exported(&received);
}
