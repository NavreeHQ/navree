//! A tiny feature module with a hand-written controller, used by every test.
//!
//! Routes (under the default `/api` prefix):
//!
//! - `POST /api/demo/echo` (public): `{"received": <json body>}`
//! - `GET /api/demo/visits` (public): counts visits in a `visits` cookie
//! - `POST /api/demo/forget` (public): deletes the `visits` cookie
//! - `GET|POST /api/demo/headers` (public): the request headers as JSON
//! - `GET /sanctum/csrf-cookie` (public): 204 and an `XSRF-TOKEN` cookie
//! - `GET /api/demo/items` (public): a fixed list
//! - `POST /api/demo/raw` (public): `{"content_type", "body"}` of the request
//! - `GET|POST|PUT|PATCH|DELETE /api/demo/method` (public): `{"method"}`
//! - `GET /api/demo/ip` (public): `{"ip"}`, the client IP navree-http sees
//! - `GET /api/demo/query` (public): `{"query"}`, the raw query string
//! - `POST /api/demo/invalid` (public): a 400 validation error for `email`
//!   and `name`
//!
//! and, in [`ProfileModule`], `GET /api/profile/me` (authenticated): the principal.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use http::header::{COOKIE, SET_COOKIE};
use http::{HeaderMap, Method, StatusCode};
use navree_http::response::{IntoResponse, Response};
use navree_http::{
    AppError, ClientIp, Controller, CurrentPrincipal, Json, ModuleBuilderHttpExt, RouteDef,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use serde_json::{Value, json};

/// The raw (percent-encoded) value of the `XSRF-TOKEN` cookie the demo
/// controller sets.
pub const XSRF_RAW: &str = "tok%2Ben%3D";

/// The demo controller.
pub struct DemoController;

impl Injectable for DemoController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DemoController) })
    }
}

impl Controller for DemoController {
    fn prefix(&self) -> &'static str {
        "/demo"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "/echo", echo).public(),
            RouteDef::on(Method::GET, "/visits", visits).public(),
            RouteDef::on(Method::POST, "/forget", forget).public(),
            RouteDef::on(Method::GET, "/headers", headers).public(),
            RouteDef::on(Method::POST, "/headers", headers).public(),
            RouteDef::on(Method::GET, "!/sanctum/csrf-cookie", csrf_cookie).public(),
            RouteDef::on(Method::GET, "/items", items).public(),
            RouteDef::on(Method::POST, "/raw", raw).public(),
            RouteDef::on(Method::GET, "/ip", ip).public(),
            RouteDef::on(Method::GET, "/query", query).public(),
            RouteDef::on(Method::GET, "/method", method).public(),
            RouteDef::on(Method::POST, "/method", method).public(),
            RouteDef::on(Method::PUT, "/method", method).public(),
            RouteDef::on(Method::PATCH, "/method", method).public(),
            RouteDef::on(Method::DELETE, "/method", method).public(),
            RouteDef::on(Method::POST, "/invalid", invalid).public(),
        ]
    }
}

/// A controller with one authenticated route, `GET /api/profile/me`.
pub struct ProfileController;

impl Injectable for ProfileController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ProfileController) })
    }
}

impl Controller for ProfileController {
    fn prefix(&self) -> &'static str {
        "/profile"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(Method::GET, "/me", me).authenticated()]
    }
}

/// The feature module serving [`ProfileController`] (needs an authenticator).
#[derive(Clone, Copy, Debug, Default)]
pub struct ProfileModule;

impl Module for ProfileModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ProfileController>();
    }
}

/// The feature module serving [`DemoController`].
#[derive(Clone, Copy, Debug, Default)]
pub struct DemoModule;

impl Module for DemoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DemoController>();
    }
}

async fn echo(Json(body): Json<Value>) -> Json<Value> {
    Json(json!({ "received": body }))
}

/// The value of cookie `name` in the request's `Cookie` headers.
pub fn request_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(n, _)| *n == name)
        .map(|(_, v)| v.to_owned())
}

async fn visits(headers: HeaderMap) -> Response {
    let seen: u64 = request_cookie(&headers, "visits")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut response = Json(json!({ "visits": seen })).into_response();
    response.headers_mut().append(
        SET_COOKIE,
        format!("visits={}; Path=/; HttpOnly", seen + 1)
            .parse()
            .unwrap(),
    );
    response
}

async fn forget() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append(SET_COOKIE, "visits=; Path=/; Max-Age=0".parse().unwrap());
    response
}

async fn headers(headers: HeaderMap) -> Json<Value> {
    let map: BTreeMap<String, String> = headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or("<binary>").to_owned(),
            )
        })
        .collect();
    Json(json!(map))
}

async fn csrf_cookie() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        SET_COOKIE,
        format!("XSRF-TOKEN={XSRF_RAW}; Path=/; SameSite=Lax")
            .parse()
            .unwrap(),
    );
    response
}

async fn me(CurrentPrincipal(principal): CurrentPrincipal) -> Json<Value> {
    Json(json!({
        "user_id": principal.user_id,
        "via": format!("{:?}", principal.via),
        "token_abilities": principal.token_abilities,
    }))
}

async fn items() -> Json<Value> {
    Json(json!({
        "data": [
            { "name": "Alice", "tags": ["admin", "sales"] },
            { "name": "Bob", "tags": [] }
        ],
        "meta": { "total": 2 }
    }))
}

async fn raw(headers: HeaderMap, body: String) -> Json<Value> {
    Json(json!({
        "content_type": headers.get(http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
        "body": body,
    }))
}

async fn method(method: Method) -> Json<Value> {
    Json(json!({ "method": method.as_str() }))
}

async fn ip(ClientIp(ip): ClientIp) -> Json<Value> {
    Json(json!({ "ip": ip.to_string() }))
}

async fn query(uri: http::Uri) -> Json<Value> {
    Json(json!({ "query": uri.query() }))
}

async fn invalid() -> AppError {
    AppError::validation(BTreeMap::from([
        (
            "email".to_owned(),
            vec!["email must be a valid email address".to_owned()],
        ),
        (
            "name".to_owned(),
            vec!["name should not be empty".to_owned()],
        ),
    ]))
}
