//! [`ObserveController`]: the health and metrics routes.

use std::sync::Arc;

use axum::Json;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderName};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::PrometheusHandle;
use navree_http::{ApiMeta, Controller, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde_json::json;

use crate::metrics::{BearerToken, MetricsRecorder};
use crate::{Health, ObserveOptions};

/// The Prometheus text exposition format.
const PROMETHEUS_TEXT: &str = "text/plain; version=0.0.4; charset=utf-8";

/// The OpenAPI tag of every route.
const TAG: &str = "Observability";

/// Serves `GET /health/live`, `GET /health/ready` and, when metrics are
/// enabled, `GET /metrics`. Every path is absolute (no API prefix).
pub(crate) struct ObserveController {
    health: Arc<Health>,
    options: Arc<ObserveOptions>,
    metrics: Arc<MetricsRecorder>,
}

impl Injectable for ObserveController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Health>(),
            ProviderKey::of::<ObserveOptions>(),
            ProviderKey::of::<MetricsRecorder>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                health: r.get::<Health>().await?,
                options: r.get::<ObserveOptions>().await?,
                metrics: r.get::<MetricsRecorder>().await?,
            })
        })
    }
}

impl Controller for ObserveController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let health = self.health.clone();
        let mut routes = vec![
            RouteDef::on(Method::GET, "!/health/live", live)
                .public()
                .name("health.live")
                .api(api("Liveness: the process answers HTTP", &[])),
            RouteDef::on(Method::GET, "!/health/ready", move || {
                let health = health.clone();
                async move { ready(&health).await }
            })
            .public()
            .name("health.ready")
            .api(api(
                "Readiness: every health check; 503 when one is down",
                &[],
            )),
        ];
        if let Some(handle) = self.metrics.handle().cloned() {
            let mut route = RouteDef::on(Method::GET, "!/metrics", move || {
                let handle = handle.clone();
                async move { metrics(&handle) }
            })
            .public()
            .name("metrics");
            match &self.options.metrics_token {
                Some(token) => {
                    route = route
                        .guard(Arc::new(BearerToken(token.clone())))
                        .api(api("Prometheus metrics (bearer token)", &[401]));
                }
                None => route = route.api(api("Prometheus metrics", &[])),
            }
            routes.push(route);
        }
        routes
    }
}

fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec![TAG.to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

/// Probes must see the current state, never a cached answer.
fn no_store() -> [(HeaderName, HeaderValue); 1] {
    [(CACHE_CONTROL, HeaderValue::from_static("no-store"))]
}

/// Liveness: the process answers HTTP.
async fn live() -> Response {
    (no_store(), Json(json!({ "status": "up" }))).into_response()
}

/// Readiness: every registered health check, 503 when one is down.
async fn ready(health: &Health) -> Response {
    let report = health.check().await;
    let status = if report.is_up() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, no_store(), Json(report)).into_response()
}

/// The Prometheus exposition of every metric recorded in the process.
fn metrics(handle: &PrometheusHandle) -> Response {
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(PROMETHEUS_TEXT)),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        handle.render(),
    )
        .into_response()
}
