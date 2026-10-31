//! Extractors: how a handler's arguments are read from the request.
//!
//! A handler is an `async fn` whose arguments each implement
//! [`FromRequestParts`] (they read the head: path, query, headers,
//! extensions), except the last, which may implement [`FromRequest`] and
//! read the body. An extractor that fails ends the request with its
//! rejection, rendered as an [`AppError`](crate::AppError) in the JSON error
//! format.
//!
//! | Extractor | Reads | Rejection |
//! |---|---|---|
//! | [`Path<T>`](crate::Path) | the route's parameters, by name or position | 400 `Invalid URL: ...`; 500 when `T` does not fit the route |
//! | [`Query<T>`](crate::Query), [`RawQuery`](crate::RawQuery) | the query string | 400 `Failed to deserialize query string: ...` |
//! | [`Form<T>`](crate::Form) | an `application/x-www-form-urlencoded` body (the query string for `GET` and `HEAD`) | 415, 413, 400 |
//! | [`Json<T>`](crate::Json) | an `application/json` (or `+json`) body | 415, 413, 400 |
//! | [`Multipart`] | a `multipart/form-data` body, part by part, within [`MultipartLimits`] | 400, then 413, 408 and 400 while reading ([`MultipartError`]) |
//! | [`WebSocketUpgrade`](crate::ws::WebSocketUpgrade) | a WebSocket handshake | 400, 405, 426 |
//! | [`Extension<T>`](crate::Extension) | a value the pipeline put in the request | 500 |
//! | [`Scoped<T>`](crate::Scoped) | a provider of the app, for this request | 500 |
//! | [`ClientIp`](crate::ClientIp), [`CurrentPrincipal`](crate::CurrentPrincipal), [`PathParams`](crate::routing::PathParams) | what the pipeline found | 401 for a guest (`CurrentPrincipal`) |
//! | [`Request`], [`Body`](crate::body::Body) | the whole request, or its body unread (only as the last argument) | never |
//! | `HeaderMap`, `Method`, `Uri`, `Version`, `Extensions`, `Parts` (from `http`) | a copy of that part of the request | never |
//! | `Bytes`, `String` | the body, whole, within the route's [`BodyLimit`](crate::body::BodyLimit) (only as the last argument) | 413, 408, 400; 400 for a `String` that is not UTF-8 |
//! | `Option<T>` | `T`, or `None` when it is absent (for `T` implementing [`OptionalFromRequestParts`] / [`OptionalFromRequest`]) | `T`'s, when present but invalid |
//! | `Result<T, T::Rejection>` | `T` or its rejection, for the handler to decide | never |

use std::convert::Infallible;
use std::future::Future;

use http::request::Parts;

use crate::response::IntoResponse;

mod client;
mod extension;
mod form;
mod json;
mod media;
mod multipart;
mod path;
mod plain;
mod query;
pub mod rejection;
mod scoped;
pub mod urlencoded;

pub use client::ClientIp;
pub(crate) use client::{IpNetwork, ProxyTrust};
pub use extension::Extension;
pub use form::Form;
pub use json::Json;
pub use multipart::{Field, Multipart, MultipartLimits};
pub use path::Path;
pub use query::{Query, RawQuery};
pub use rejection::MultipartError;
pub use scoped::Scoped;

/// A request, as handlers and extractors see it.
pub type Request<B = crate::body::Body> = http::Request<B>;

/// An extractor that reads the request head.
///
/// ```
/// use http::request::Parts;
/// use navree_http::AppError;
/// use navree_http::extract::FromRequestParts;
///
/// /// The `X-Tenant` header.
/// struct Tenant(String);
///
/// impl FromRequestParts for Tenant {
///     type Rejection = AppError;
///
///     async fn from_request_parts(parts: &mut Parts) -> Result<Self, AppError> {
///         parts
///             .headers
///             .get("x-tenant")
///             .and_then(|value| value.to_str().ok())
///             .map(|tenant| Tenant(tenant.to_owned()))
///             .ok_or_else(|| AppError::bad_request("The X-Tenant header is missing."))
///     }
/// }
/// ```
pub trait FromRequestParts: Sized + Send {
    /// What a failed extraction answers.
    type Rejection: IntoResponse;

    /// Reads the extractor from the request head.
    fn from_request_parts(
        parts: &mut Parts,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send;
}

/// An extractor that consumes the request (reads the body): only a handler's
/// last argument. `M` tells body extractors ([`via::ViaRequest`], the
/// default) from head extractors used last ([`via::ViaParts`]).
pub trait FromRequest<M = via::ViaRequest>: Sized + Send {
    /// What a failed extraction answers.
    type Rejection: IntoResponse;

    /// Reads the extractor from the request.
    fn from_request(request: Request)
    -> impl Future<Output = Result<Self, Self::Rejection>> + Send;
}

impl<T: FromRequestParts> FromRequest<via::ViaParts> for T {
    type Rejection = T::Rejection;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (mut parts, _body) = request.into_parts();
        T::from_request_parts(&mut parts).await
    }
}

/// A head extractor that may be absent: `Option<T>` is `None` then, and
/// still fails when the value is there but invalid.
pub trait OptionalFromRequestParts: Sized + Send {
    /// What a failed extraction answers.
    type Rejection: IntoResponse;

    /// Reads the extractor, `None` when the request does not carry it.
    fn from_request_parts(
        parts: &mut Parts,
    ) -> impl Future<Output = Result<Option<Self>, Self::Rejection>> + Send;
}

/// A body extractor that may be absent (`Option<Json<T>>` is `None` without
/// a `Content-Type`).
pub trait OptionalFromRequest<M = via::ViaRequest>: Sized + Send {
    /// What a failed extraction answers.
    type Rejection: IntoResponse;

    /// Reads the extractor, `None` when the request does not carry it.
    fn from_request(
        request: Request,
    ) -> impl Future<Output = Result<Option<Self>, Self::Rejection>> + Send;
}

impl FromRequest for Request {
    type Rejection = Infallible;

    async fn from_request(request: Request) -> Result<Self, Infallible> {
        Ok(request)
    }
}

impl FromRequest for crate::body::Body {
    type Rejection = Infallible;

    async fn from_request(request: Request) -> Result<Self, Infallible> {
        Ok(request.into_body())
    }
}

impl<T: OptionalFromRequestParts> FromRequestParts for Option<T> {
    type Rejection = T::Rejection;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        T::from_request_parts(parts).await
    }
}

impl<T: OptionalFromRequest> FromRequest for Option<T> {
    type Rejection = T::Rejection;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        T::from_request(request).await
    }
}

impl<T> FromRequestParts for Result<T, T::Rejection>
where
    T: FromRequestParts,
    T::Rejection: Send,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(T::from_request_parts(parts).await)
    }
}

impl<T> FromRequest for Result<T, T::Rejection>
where
    T: FromRequest,
    T::Rejection: Send,
{
    type Rejection = Infallible;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        Ok(T::from_request(request).await)
    }
}

/// Markers for [`FromRequest`]'s `M`; they only steer type inference.
pub mod via {
    /// A body extractor ([`FromRequest`](super::FromRequest) itself).
    #[derive(Debug)]
    pub enum ViaRequest {}

    /// A head extractor used as the last argument.
    #[derive(Debug)]
    pub enum ViaParts {}
}
