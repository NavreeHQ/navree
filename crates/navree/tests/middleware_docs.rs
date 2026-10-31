//! The examples of `docs/basics/middleware` through the facade: a global
//! layer in a slot, and a layer on one hand-written route.

// The page's examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use http::{HeaderValue, Method};
use navree::axum::extract::Request;
use navree::axum::middleware::{self, Next};
use navree::http::handler::Endpoint;
use navree::http::response::Response;
use navree::http::{Controller, GlobalLayer, LayerSlot, RouteDef};
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
