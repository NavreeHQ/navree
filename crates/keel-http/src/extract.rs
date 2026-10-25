//! Small extractors and responses: [`ClientIp`], [`Created`], [`NoContent`].

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts};
use axum::response::{IntoResponse, Response};
use http::header::LOCATION;
use http::request::Parts;
use http::{HeaderMap, HeaderValue, StatusCode};
use serde::Serialize;

use crate::AppError;

/// The client IP address. The pipeline computes it once per request (from
/// the connection, or from `X-Forwarded-For` / `X-Real-IP` when
/// `http.trust_proxy` is on) and stores it in the request extensions.
/// `0.0.0.0` when unknown (requests not served through a socket).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

impl ClientIp {
    /// The client IP of a request: proxy headers first when `trust_proxy`,
    /// then the peer address, else `0.0.0.0`.
    pub(crate) fn resolve(
        headers: &HeaderMap,
        extensions: &http::Extensions,
        trust_proxy: bool,
    ) -> Self {
        if trust_proxy && let Some(ip) = forwarded_ip(headers) {
            return Self(ip);
        }
        extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| Self(addr.ip()))
            .unwrap_or(Self(IpAddr::V4(Ipv4Addr::UNSPECIFIED)))
    }
}

/// The right-most valid `X-Forwarded-For` entry (the one our proxy added),
/// else `X-Real-IP`.
fn forwarded_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let forwarded = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|entry| entry.trim().parse::<IpAddr>().ok())
        .next_back();
    forwarded.or_else(|| {
        headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
    })
}

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(match parts.extensions.get::<ClientIp>() {
            Some(ip) => *ip,
            None => ClientIp::resolve(&parts.headers, &parts.extensions, false),
        })
    }
}

/// 201 Created with a `Location` header and a JSON body.
#[derive(Clone, Debug)]
pub struct Created<T>(pub String, pub T);

impl<T: Serialize> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        let Created(location, body) = self;
        let location = match HeaderValue::from_str(&location) {
            Ok(value) => value,
            Err(error) => {
                return AppError::internal(format!("invalid Location header: {error}"))
                    .into_response();
            }
        };
        let mut response = (StatusCode::CREATED, Json(body)).into_response();
        response.headers_mut().insert(LOCATION, location);
        response
    }
}

/// 204 No Content.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoContent;

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        StatusCode::NO_CONTENT.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_headers_are_ignored_unless_trusted() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("6.6.6.6, 10.0.0.1"),
        );
        let mut extensions = http::Extensions::new();
        extensions.insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 2], 4000))));

        let untrusted = ClientIp::resolve(&headers, &extensions, false);
        assert_eq!(untrusted.0, IpAddr::from([192, 168, 1, 2]));

        let trusted = ClientIp::resolve(&headers, &extensions, true);
        assert_eq!(trusted.0, IpAddr::from([10, 0, 0, 1]), "right-most entry");

        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("8.8.8.8"));
        assert_eq!(
            ClientIp::resolve(&headers, &extensions, true).0,
            IpAddr::from([8, 8, 8, 8])
        );
        assert_eq!(
            ClientIp::resolve(&HeaderMap::new(), &http::Extensions::new(), true).0,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
    }

    #[test]
    fn created_sets_location_and_status() {
        let response =
            Created("/api/deals/1".to_owned(), serde_json::json!({"id": 1})).into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()[LOCATION], "/api/deals/1");
        assert_eq!(NoContent.into_response().status(), StatusCode::NO_CONTENT);
    }
}
