//! Helpers shared by the navis-observe integration tests.

#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use navis_config::{ConfigModule, ConfigSources};
use navis_contracts::{BoxFuture, HealthCheck, HealthStatus};
use navis_http::{HttpModule, HttpOptions, build_router};
use navis_kernel::{App, AppBuilder, Factory, Module, ModuleBuilder, Navis};
use navis_observe::{ObserveModule, ObserveOptions};
use tower::ServiceExt;

/// HTTP options for tests: no per-IP rate limit.
pub fn http_options() -> HttpOptions {
    HttpOptions {
        per_ip_rate_limit: None,
        ..HttpOptions::default()
    }
}

/// What a [`FakeCheck`] does when run.
#[derive(Clone, Debug)]
pub enum Behaviour {
    Up,
    Down(&'static str),
    /// Sleeps, then reports up.
    Slow(Duration),
    Panic,
}

/// A health check with a fixed behaviour that counts its runs.
#[derive(Debug)]
pub struct FakeCheck {
    pub name: &'static str,
    pub behaviour: Behaviour,
    pub runs: AtomicUsize,
}

impl FakeCheck {
    pub fn new(name: &'static str, behaviour: Behaviour) -> Arc<Self> {
        Arc::new(Self {
            name,
            behaviour,
            runs: AtomicUsize::new(0),
        })
    }

    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

impl HealthCheck for FakeCheck {
    fn name(&self) -> &str {
        self.name
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match &self.behaviour {
                Behaviour::Up => HealthStatus::Up,
                Behaviour::Down(reason) => HealthStatus::Down((*reason).to_owned()),
                Behaviour::Slow(delay) => {
                    tokio::time::sleep(*delay).await;
                    HealthStatus::Up
                }
                Behaviour::Panic => panic!("the fake check panicked"),
            }
        })
    }
}

/// A feature module registering health checks as `dyn HealthCheck`
/// multi-bindings.
pub struct ChecksModule(pub Vec<Arc<FakeCheck>>);

impl Module for ChecksModule {
    fn register(&self, m: &mut ModuleBuilder) {
        for check in &self.0 {
            let check = check.clone();
            m.multi(Factory::<dyn HealthCheck>::new(move |_| {
                let check = check.clone();
                async move { Ok(check as Arc<dyn HealthCheck>) }
            }));
        }
    }
}

/// An app with `HttpModule`, `ObserveModule::for_root(options)` and the checks.
pub fn app_with(options: ObserveOptions, checks: Vec<Arc<FakeCheck>>) -> AppBuilder {
    Navis::app()
        .module(HttpModule::for_root(http_options()))
        .module(ObserveModule::for_root(options))
        .module(ChecksModule(checks))
}

/// An app configured from `sources`: `ConfigModule`, `HttpModule::for_root`
/// and `observe` (`ObserveModule::from_config()` or the plain module).
pub fn configured_app(sources: ConfigSources, observe: impl Module) -> AppBuilder {
    Navis::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::for_root(http_options()))
        .module(observe)
}

/// A booted app and its router.
pub struct TestApp {
    pub app: App,
    pub router: Router,
}

impl TestApp {
    pub async fn boot(builder: AppBuilder) -> Self {
        let app = builder.boot().await.expect("app boots");
        let router = build_router(app.ctx()).await.expect("router builds");
        Self { app, router }
    }

    pub async fn send(&self, request: Request<Body>) -> TestResponse {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router is infallible");
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body collects")
            .to_bytes();
        TestResponse {
            status,
            headers,
            body,
        }
    }

    pub async fn get(&self, path: &str) -> TestResponse {
        self.send(Request::get(path).body(Body::empty()).unwrap())
            .await
    }

    /// `GET path` with `Authorization: <authorization>`.
    pub async fn get_with_auth(&self, path: &str, authorization: &str) -> TestResponse {
        self.send(
            Request::get(path)
                .header("authorization", authorization)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }
}

/// A collected response.
#[derive(Debug)]
pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl TestResponse {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!(
                "body is not JSON ({e}): {}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}
