//! Helpers shared by the suites: parsing shorthands and a small app harness
//! (navree-testing sits on the same layer, so the HTTP tests drive the
//! router directly).

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use http::request::Parts;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{AuthVia, Principal};
use navree_http::body::Body;
use navree_http::{AppError, Authenticator, HttpModule, HttpOptions, build_router};
use navree_kernel::{App, AppBuilder, BoxFuture, Navree};
use navree_time::{LocalDay, Zone};
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

/// An RFC 3339 instant.
pub fn utc(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap_or_else(|e| panic!("bad instant `{text}`: {e}"))
        .with_timezone(&Utc)
}

/// A `yyyy-MM-dd` day.
pub fn day(text: &str) -> LocalDay {
    text.parse()
        .unwrap_or_else(|e| panic!("bad day `{text}`: {e}"))
}

/// A zone by name.
pub fn zone(name: &str) -> Zone {
    Zone::parse(name).unwrap_or_else(|e| panic!("bad zone `{name}`: {e}"))
}

/// A wall-clock time, `yyyy-MM-ddTHH:MM`.
pub fn local(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
        .unwrap_or_else(|e| panic!("bad local time `{text}`: {e}"))
}

/// A calendar date.
pub fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

/// The header naming the signed-in user of a test request.
pub const USER_HEADER: &str = "x-test-user";

/// Signs requests in as the user in [`USER_HEADER`].
struct HeaderUser;

impl Authenticator for HeaderUser {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let user = parts
            .headers
            .get(USER_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Uuid::parse_str(value).ok());
        Box::pin(async move {
            Ok(user.map(|user_id| Principal {
                user_id,
                via: AuthVia::Test,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

/// An app with `sources`, HTTP (no per-IP limit) and the header
/// authenticator.
pub fn app_with(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::for_root(HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        }))
        .override_value::<dyn Authenticator>(Arc::new(HeaderUser))
}

/// An app with the `config` values set.
pub fn app(config: &[(&str, Value)]) -> AppBuilder {
    let mut sources = ConfigSources::new();
    for (key, value) in config {
        sources = sources.set(key, value.clone());
    }
    app_with(sources)
}

/// The boot error of `builder`, as text.
pub async fn boot_error(builder: AppBuilder) -> String {
    match builder.boot().await {
        Ok(_) => panic!("the boot must fail"),
        Err(errors) => errors.to_string(),
    }
}

/// A response: status and JSON body (`Null` when not JSON).
#[derive(Debug)]
pub struct Reply {
    pub status: StatusCode,
    pub body: Value,
    pub cache_control: Option<String>,
}

/// `GET path` as `user` (a guest when `None`) with `headers`.
pub async fn get(app: &App, path: &str, user: Option<Uuid>, headers: &[(&str, String)]) -> Reply {
    let router = build_router(app.ctx()).await.expect("the router builds");
    let mut request = Request::get(path);
    if let Some(user) = user {
        request = request.header(USER_HEADER, user.to_string());
    }
    for (name, value) in headers {
        request = request.header(*name, value.as_str());
    }
    let response = router
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .expect("the router is infallible");
    let status = response.status();
    let cache_control = response
        .headers()
        .get(http::header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body collects")
        .to_bytes();
    Reply {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cache_control,
    }
}
