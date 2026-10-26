//! Helpers shared by the navree-http integration tests.

#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, Bytes};
use http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{AuthVia, Principal};
use navree_http::{
    Authenticator, Controller, HttpModule, HttpOptions, ModuleBuilderHttpExt, RouteDef,
    build_router,
};
use navree_kernel::{
    App, AppBuilder, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree,
    ProviderKey, Resolver,
};
use tower::ServiceExt;
use uuid::Uuid;

/// The origin the test apps allow for CORS.
pub const ORIGIN: &str = "http://localhost:3000";

/// Options for tests: no per-IP rate limit (tests send many requests), one
/// allowed CORS origin.
pub fn options() -> HttpOptions {
    let mut options = HttpOptions {
        per_ip_rate_limit: None,
        ..HttpOptions::default()
    };
    options.cors.allowed_origins = vec![ORIGIN.to_owned()];
    options
}

/// The routes served by [`TestController`].
#[derive(Clone)]
pub struct RouteList {
    pub prefix: &'static str,
    pub version: Option<&'static str>,
    pub routes: Vec<RouteDef>,
}

/// A hand-written controller whose routes come from a [`RouteList`] value.
pub struct TestController {
    list: Arc<RouteList>,
}

impl Injectable for TestController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<RouteList>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                list: r.get::<RouteList>().await?,
            })
        })
    }
}

impl Controller for TestController {
    fn prefix(&self) -> &'static str {
        self.list.prefix
    }

    fn version(&self) -> Option<&'static str> {
        self.list.version
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        self.list.routes.clone()
    }
}

type Extra = Arc<dyn Fn(&mut ModuleBuilder) + Send + Sync>;

/// A feature module serving a [`RouteList`] through [`TestController`].
pub struct TestModule {
    list: RouteList,
    extra: Option<Extra>,
}

impl Module for TestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(self.list.clone()).controller::<TestController>();
        if let Some(extra) = &self.extra {
            extra(m);
        }
    }
}

/// Authenticates requests carrying `x-user-id: <uuid>`; everyone else is a guest.
#[derive(Debug, Default)]
pub struct HeaderAuthenticator {
    pub calls: AtomicUsize,
}

impl Authenticator for HeaderAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut http::request::Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, navree_http::AppError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(parts
                .headers
                .get("x-user-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| Uuid::parse_str(v).ok())
                .map(|user_id| Principal {
                    user_id,
                    via: AuthVia::Test,
                    token_abilities: None,
                    session_id: None,
                }))
        })
    }
}

/// Builds a test app: `HttpModule::for_root(options)`, a [`TestModule`] with
/// the routes, optional extra registrations and extra root modules.
pub struct Setup {
    options: HttpOptions,
    list: RouteList,
    extra: Option<Extra>,
    authenticator: Option<Arc<HeaderAuthenticator>>,
    config: Option<ConfigSources>,
    builder: AppBuilder,
}

impl Setup {
    pub fn new(routes: Vec<RouteDef>) -> Self {
        Self {
            options: options(),
            list: RouteList {
                prefix: "",
                version: None,
                routes,
            },
            extra: None,
            authenticator: None,
            config: None,
            builder: Navree::app(),
        }
    }

    pub fn options(mut self, options: HttpOptions) -> Self {
        self.options = options;
        self
    }

    pub fn prefix(mut self, prefix: &'static str) -> Self {
        self.list.prefix = prefix;
        self
    }

    pub fn version(mut self, version: &'static str) -> Self {
        self.list.version = Some(version);
        self
    }

    /// Extra registrations on the test module.
    pub fn register(mut self, f: impl Fn(&mut ModuleBuilder) + Send + Sync + 'static) -> Self {
        self.extra = Some(Arc::new(f));
        self
    }

    /// Registers a [`HeaderAuthenticator`] as `dyn Authenticator`.
    pub fn authenticated(mut self) -> Self {
        self.authenticator = Some(Arc::new(HeaderAuthenticator::default()));
        self
    }

    /// Uses `ConfigModule::for_root(sources)` and `HttpModule::from_config()`
    /// instead of `HttpModule::for_root(options)`.
    pub fn configured_by(mut self, sources: ConfigSources) -> Self {
        self.config = Some(sources);
        self
    }

    /// Adds another root module.
    pub fn module(mut self, m: impl Module) -> Self {
        self.builder = self.builder.module(m);
        self
    }

    fn app_builder(self) -> AppBuilder {
        let builder = match self.config {
            Some(sources) => self
                .builder
                .module(ConfigModule::for_root(sources))
                .module(HttpModule::from_config()),
            None => self.builder.module(HttpModule::for_root(self.options)),
        };
        let mut builder = builder.module(TestModule {
            list: self.list,
            extra: self.extra,
        });
        if let Some(authenticator) = self.authenticator {
            builder = builder.override_value::<dyn Authenticator>(authenticator);
        }
        builder
    }

    pub async fn boot(self) -> TestApp {
        let app = self.app_builder().boot().await.expect("app boots");
        let router = build_router(app.ctx()).await.expect("router builds");
        TestApp { app, router }
    }

    /// Boots and returns the boot error message.
    pub async fn boot_error(self) -> String {
        match self.app_builder().boot().await {
            Ok(_) => panic!("boot must fail"),
            Err(errors) => errors.to_string(),
        }
    }
}

/// A booted app and its router.
pub struct TestApp {
    pub app: App,
    pub router: Router,
}

impl TestApp {
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

    /// Asserts a JSON error response (`{"statusCode", "message",
    /// "error"}`, `application/json`) with `status` and an `X-Request-Id`
    /// header, and returns the body.
    pub fn assert_error(&self, status: u16) -> serde_json::Value {
        assert_eq!(self.status.as_u16(), status, "body: {}", self.text());
        assert_eq!(
            self.header("content-type"),
            Some("application/json"),
            "content type of a {status} response"
        );
        let body = self.json();
        assert_eq!(body["statusCode"], status, "statusCode: {body}");
        assert!(
            body["message"].is_string() || body["message"].is_array(),
            "message: {body}"
        );
        assert!(body["error"].is_string(), "error: {body}");
        assert!(body.get("request_id").is_none(), "no request_id: {body}");
        assert!(
            self.header("x-request-id").is_some(),
            "x-request-id header on a {status} response"
        );
        body
    }
}

/// Counts handler calls.
#[derive(Default, Debug)]
pub struct Counter(AtomicUsize);

impl Counter {
    pub fn hit(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// A request with an `Origin` header.
pub fn from_origin(method: &str, path: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("origin", origin)
        .body(Body::empty())
        .unwrap()
}

/// A JSON request body.
pub fn json_request(method: &str, path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub fn bytes(n: usize) -> Bytes {
    Bytes::from(vec![b'x'; n])
}
