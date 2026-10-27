//! Helpers shared by the navree-storage integration tests.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use navree_http::{HttpModule, HttpOptions, build_router};
use navree_kernel::{App, AppBuilder};
use navree_storage::{DiskConfig, Storage, StorageOptions};
use tower::ServiceExt;

/// The key the test storages sign temporary URLs with.
pub const SIGNING_KEY: &[u8] = b"navree-storage test signing key";

/// Both test disks, by name.
pub const DISKS: [&str; 2] = ["local", "memory"];

/// Two disks: `local` (the default) rooted at `root`, and `memory`.
pub fn options(root: &Path) -> StorageOptions {
    StorageOptions {
        default: "local".to_owned(),
        disks: BTreeMap::from([
            ("local".to_owned(), DiskConfig::local(root)),
            ("memory".to_owned(), DiskConfig::Memory),
        ]),
        ..StorageOptions::default()
    }
}

/// A [`Storage`] built from [`options`] without an app.
pub fn storage(root: &Path) -> Storage {
    Storage::new(&options(root), SIGNING_KEY).expect("storage builds")
}

/// HTTP options for tests: no per-IP rate limit.
pub fn http_options() -> HttpOptions {
    HttpOptions {
        per_ip_rate_limit: None,
        ..HttpOptions::default()
    }
}

/// Boots `builder`, which must fail, and returns the error message.
pub async fn boot_error(builder: AppBuilder) -> String {
    match builder.boot().await {
        Ok(_) => panic!("boot must fail"),
        Err(errors) => errors.to_string(),
    }
}

/// A booted app and its router.
pub struct TestApp {
    pub app: App,
    pub router: Router,
}

impl TestApp {
    /// Boots `builder` plus `HttpModule::for_root(http_options())`.
    pub async fn boot(builder: AppBuilder) -> Self {
        let app = builder
            .module(HttpModule::for_root(http_options()))
            .boot()
            .await
            .expect("app boots");
        let router = build_router(app.ctx()).await.expect("router builds");
        Self { app, router }
    }

    pub async fn send(&self, request: Request<Body>) -> TestResponse {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        TestResponse {
            status,
            headers,
            body,
        }
    }

    pub async fn get(&self, uri: &str) -> TestResponse {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
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
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Asserts a JSON error response with `status` and returns its
    /// body.
    pub fn error(&self, status: u16) -> serde_json::Value {
        assert_eq!(self.status.as_u16(), status, "body: {}", self.text());
        assert_eq!(self.header("content-type"), Some("application/json"));
        let body: serde_json::Value = serde_json::from_slice(&self.body).unwrap();
        assert_eq!(body["statusCode"], status, "{body}");
        assert!(body["error"].is_string(), "{body}");
        body
    }
}

/// The URL of a test service from `NAVREE_TEST_<SERVICE>_URL` (`variable`).
/// When it is missing, `test` prints `SKIPPED <test>: set <variable>` and
/// gets `None`, unless `NAVREE_TEST_REQUIRE_SERVICES=1`, which turns a
/// missing service into a failure.
pub fn service_url(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") => {
            panic!("{test}: set {variable} (NAVREE_TEST_REQUIRE_SERVICES=1)")
        }
        _ => {
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// One disk called `name` as the default disk of an app with `HttpModule`.
pub fn single_disk(name: &str, config: DiskConfig) -> StorageOptions {
    StorageOptions {
        default: name.to_owned(),
        disks: BTreeMap::from([(name.to_owned(), config)]),
        ..StorageOptions::default()
    }
}
