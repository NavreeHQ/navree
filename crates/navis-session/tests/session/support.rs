//! Helpers shared by the navis-session integration tests: a probe controller
//! that reads and writes the session, and app builders.

#![allow(dead_code)]

use std::sync::Arc;

use axum::Json;
use axum::extract::Path;
use axum::http::Method;
use navis_crypto::{CryptoModule, generate_key};
use navis_http::{Controller, ModuleBuilderHttpExt, RouteDef};
use navis_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navis_session::{Session, SessionModule, SessionOptions};
use navis_testing::{TestApp, TestAppBuilder, TestClient};
use serde::Deserialize;
use serde_json::{Value, json};

/// The default session cookie name.
pub const COOKIE: &str = "navis_session";

/// The cookie JavaScript clients read the CSRF token from.
pub const XSRF: &str = "XSRF-TOKEN";

/// Reads and writes the session over HTTP (every route is public).
pub struct ProbeController;

impl Injectable for ProbeController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ProbeController) })
    }
}

#[derive(Deserialize)]
struct Put {
    key: String,
    value: Value,
}

async fn handle_put(session: Session, Json(input): Json<Put>) -> navis_http::Result<Json<Value>> {
    session.insert(&input.key, &input.value)?;
    Ok(Json(json!({ "id": session.id() })))
}

async fn handle_get(session: Session, Path(key): Path<String>) -> Json<Value> {
    Json(json!({ "id": session.id(), "value": session.get::<Value>(&key) }))
}

async fn remove(session: Session, Path(key): Path<String>) -> Json<Value> {
    Json(json!({ "id": session.id(), "removed": session.remove(&key) }))
}

async fn regenerate(session: Session) -> Json<Value> {
    let old = session.id();
    session.regenerate();
    Json(json!({ "old": old, "new": session.id() }))
}

async fn invalidate(session: Session) -> Json<Value> {
    let old = session.id();
    session.invalidate();
    Json(json!({ "old": old, "new": session.id() }))
}

async fn flush(session: Session) -> Json<Value> {
    session.flush();
    Json(json!({ "id": session.id() }))
}

async fn token(session: Session) -> Json<Value> {
    Json(json!({ "id": session.id(), "token": session.csrf_token() }))
}

async fn put_then_fail(session: Session) -> navis_http::Result<Json<Value>> {
    let attempts = session.get::<u32>("attempts").unwrap_or(0) + 1;
    session.insert("attempts", &attempts)?;
    Err(navis_http::AppError::bad_request("rejected"))
}

async fn write() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn untouched() -> Json<Value> {
    Json(json!({ "ok": true }))
}

impl Controller for ProbeController {
    fn prefix(&self) -> &'static str {
        "/probe"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "/put", handle_put).public(),
            RouteDef::on(Method::GET, "/get/{key}", handle_get).public(),
            RouteDef::on(Method::POST, "/remove/{key}", remove).public(),
            RouteDef::on(Method::POST, "/regenerate", regenerate).public(),
            RouteDef::on(Method::POST, "/invalidate", invalidate).public(),
            RouteDef::on(Method::POST, "/flush", flush).public(),
            RouteDef::on(Method::GET, "/token", token).public(),
            RouteDef::on(Method::GET, "/untouched", untouched).public(),
            RouteDef::on(Method::POST, "/fail", put_then_fail).public(),
            RouteDef::on(Method::POST, "/write", write).public(),
            RouteDef::on(Method::PUT, "/write", write).public(),
            RouteDef::on(Method::PATCH, "/write", write).public(),
            RouteDef::on(Method::DELETE, "/write", write).public(),
            RouteDef::on(Method::POST, "!/webhooks/incoming", write).public(),
        ]
    }
}

/// Mounts [`ProbeController`] under `/api/probe`.
pub struct ProbeModule;

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ProbeController>();
    }
}

/// A test app builder with an app key, `CryptoModule`, the probe routes
/// and no per-IP rate limit; the caller adds the session module.
pub fn base() -> TestAppBuilder {
    TestApp::builder()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule)
        .module(ProbeModule)
}

/// An app with `SessionModule::for_root(options)`.
pub async fn app_with(options: SessionOptions) -> TestApp {
    base()
        .module(SessionModule::for_root(options))
        .build()
        .await
}

/// An app with the default session options (memory store).
pub async fn app() -> TestApp {
    app_with(SessionOptions::default()).await
}

/// Stores `key = value` in the client's session (fetching the XSRF cookie
/// first when the client has none) and returns the session id.
pub async fn put(client: &TestClient, key: &str, value: Value) -> String {
    if client.cookie(XSRF).is_none() {
        client.csrf_cookie().await;
    }
    let response = client
        .post("/api/probe/put")
        .xsrf()
        .json(&json!({ "key": key, "value": value }))
        .send()
        .await
        .assert_status(200);
    response.json_value()["id"]
        .as_str()
        .expect("the probe returns the session id")
        .to_owned()
}

/// Reads `key` from the client's session: `(session id, value)`.
pub async fn get(client: &TestClient, key: &str) -> (String, Value) {
    let body = client
        .get(&format!("/api/probe/get/{key}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    (
        body["id"].as_str().expect("session id").to_owned(),
        body["value"].clone(),
    )
}
