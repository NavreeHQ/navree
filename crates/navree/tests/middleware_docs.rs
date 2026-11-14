//! The examples of `docs/basics/middleware` through the facade: a global
//! layer in a slot, a layer on one hand-written route, and a layer whose
//! service can fail.

// The page's examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use std::time::Duration;

use http::{HeaderValue, Method};
use navree::http::extract::Request;
use navree::http::handler::Endpoint;
use navree::http::middleware::{self, Next, State};
use navree::http::response::Response;
use navree::http::{Controller, FallibleLayer, GlobalLayer, LayerSlot, RouteDef};
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use uuid::Uuid;

/// Adds `X-Response-Time` (milliseconds) to every response.
async fn response_time(request: Request, next: Next) -> Response {
    let started = std::time::Instant::now();
    let mut response = next.run(request).await;
    let millis = started.elapsed().as_millis().to_string();
    if let Ok(value) = HeaderValue::from_str(&millis) {
        response.headers_mut().insert("x-response-time", value);
    }
    response
}

async fn tagged(State(tag): State<&'static str>, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-tag", HeaderValue::from_static(tag));
    response
}

#[module(register = Self::wire)]
pub struct TaggedModule;

impl TaggedModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.http_layer(GlobalLayer::new(
            LayerSlot::Session,
            "tagged",
            |_ctx, router| Ok(router.layer(middleware::from_fn_with_state("docs", tagged))),
        ));
    }
}

#[module(register = Self::wire)]
pub struct TimingModule;

impl TimingModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.http_layer(GlobalLayer::new(
            LayerSlot::Session,
            "response-time",
            |_ctx, router| Ok(router.layer(middleware::from_fn(response_time))),
        ));
    }
}

/// Forbids caching the response.
async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

#[injectable]
pub struct TokensController;

impl Controller for TokensController {
    fn prefix(&self) -> &'static str {
        "/tokens"
    }

    fn version(&self) -> Option<&'static str> {
        Some("1")
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let issue =
            Endpoint::new(|| async { Json("a-new-token") }).layer(middleware::from_fn(no_store));
        vec![RouteDef::with_endpoint(Method::POST, "/", issue).authenticated()]
    }
}

#[module(controllers = [TokensController])]
pub struct TokensModule;

#[module(register = Self::wire)]
pub struct KhataTimeoutModule;

impl KhataTimeoutModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.http_layer(GlobalLayer::new(
            LayerSlot::Context,
            "timeout",
            |_ctx, router| {
                Ok(
                    router.layer(FallibleLayer::new(tower::timeout::TimeoutLayer::new(
                        Duration::from_secs(1),
                    ))),
                )
            },
        ))
        // The timeout's error answers 503 instead of 500.
        .map_error::<tower::timeout::error::Elapsed>(|_| {
            Some(AppError::service_unavailable("The khata took too long."))
        });
    }
}

/// One route that answers at once and one that takes too long.
#[injectable]
pub struct KhataController;

impl Controller for KhataController {
    fn prefix(&self) -> &'static str {
        "/khata"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/fast", || async { "on time" }).public(),
            RouteDef::on(Method::GET, "/slow", || async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "late"
            })
            .public(),
        ]
    }
}

#[module(controllers = [KhataController])]
pub struct KhataModule;

#[tokio::test]
async fn a_global_layer_wraps_every_response() {
    let app = TestApp::builder()
        .module(TimingModule)
        .module(TokensModule)
        .build()
        .await;
    let response = app
        .acting_as(principal(Uuid::now_v7()))
        .post("/api/v1/tokens")
        .send()
        .await
        .assert_status(200);
    assert!(response.header("x-response-time").is_some());
}

#[tokio::test]
async fn a_layer_with_state_gets_it_as_its_first_argument() {
    let app = TestApp::builder()
        .module(TaggedModule)
        .module(TokensModule)
        .build()
        .await;
    let response = app
        .acting_as(principal(Uuid::now_v7()))
        .post("/api/v1/tokens")
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.header("x-tag"), Some("docs"));
}

#[tokio::test]
async fn a_layer_on_one_route_wraps_its_handler() {
    let app = TestApp::builder().module(TokensModule).build().await;
    let response = app
        .acting_as(principal(Uuid::now_v7()))
        .post("/api/v1/tokens")
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.json_value(), serde_json::json!("a-new-token"));
}

#[tokio::test]
async fn a_layer_whose_service_can_fail_answers_as_the_middleware_page_shows() {
    let app = TestApp::builder()
        .module(KhataTimeoutModule)
        .module(KhataModule)
        .build()
        .await;
    app.client()
        .get("/api/khata/slow")
        .send()
        .await
        .assert_error(503, "The khata took too long.");
    let fast = app
        .client()
        .get("/api/khata/fast")
        .send()
        .await
        .assert_status(200);
    assert_eq!(fast.text(), "on time");
}
