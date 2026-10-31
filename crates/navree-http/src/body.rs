//! Request and response bodies: [`Body`], [`Bytes`], and the request body
//! limit ([`BodyLimit`]).
//!
//! `Body` is still the previous framework's body type, re-exported so code
//! names it through navree-http.

pub use axum::body::{Body, HttpBody, to_bytes};
pub use bytes::Bytes;
use http_body_util::{BodyExt, LengthLimitError, Limited};

use crate::serve::BodyReadTimeout;

/// The limit body extractors use when a request carries no [`BodyLimit`]
/// (a route outside the pipeline): 2 MiB, `http.body_limit_bytes`'s default.
pub const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;

/// The most bytes a request body may have, in the request extensions: the
/// pipeline sets the route's (`RouteDef::body_limit`) or the app's
/// (`http.body_limit_bytes`). [`Json`](crate::Json), [`Form`](crate::Form)
/// and [`read_body`] stop reading past it (413); a `Content-Length` above it
/// is refused before authentication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyLimit(pub usize);

impl BodyLimit {
    /// The limit for a request with `extensions` (the default without one).
    pub fn of(extensions: &http::Extensions) -> usize {
        extensions
            .get::<BodyLimit>()
            .map_or(DEFAULT_BODY_LIMIT, |limit| limit.0)
    }
}

/// Reads a whole body, up to `limit` bytes.
pub async fn read_body(body: Body, limit: usize) -> Result<Bytes, BodyError> {
    match Limited::new(body, limit).collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(error) => {
            let chain = || {
                std::iter::successors(
                    Some(&*error as &(dyn std::error::Error + 'static)),
                    |error| error.source(),
                )
            };
            Err(if chain().any(|error| error.is::<LengthLimitError>()) {
                BodyError::TooLarge
            } else if chain().any(|error| error.is::<BodyReadTimeout>()) {
                BodyError::TimedOut
            } else {
                BodyError::Read(error.to_string())
            })
        }
    }
}

/// Why a body could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyError {
    /// It is longer than the limit (413).
    TooLarge,
    /// The client sent it too slowly (408): `http.server.body_read_timeout_ms`
    /// and `http.server.body_min_bytes_per_sec`.
    TimedOut,
    /// The connection failed while it was read (400).
    Read(String),
}

impl BodyError {
    /// The status of the rejection.
    pub fn status(&self) -> http::StatusCode {
        match self {
            BodyError::TooLarge => http::StatusCode::PAYLOAD_TOO_LARGE,
            BodyError::TimedOut => http::StatusCode::REQUEST_TIMEOUT,
            BodyError::Read(_) => http::StatusCode::BAD_REQUEST,
        }
    }
}

impl std::fmt::Display for BodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BodyError::TooLarge => {
                f.write_str("Failed to buffer the request body: length limit exceeded")
            }
            BodyError::TimedOut => {
                f.write_str("Failed to buffer the request body: it arrived too slowly")
            }
            BodyError::Read(error) => write!(f, "Failed to buffer the request body: {error}"),
        }
    }
}

impl std::error::Error for BodyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bodies_are_read_up_to_the_limit() {
        let body = read_body(Body::from("hello"), 5).await.unwrap();
        assert_eq!(&body[..], b"hello");
        let error = read_body(Body::from("hello!"), 5).await.unwrap_err();
        assert_eq!(error, BodyError::TooLarge);
        assert_eq!(error.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            error.to_string(),
            "Failed to buffer the request body: length limit exceeded"
        );
        // A streamed body without a length is cut off at the limit too.
        let chunks = futures_util::stream::iter(
            (0..10).map(|_| Ok::<_, std::io::Error>(Bytes::from_static(b"0123456789"))),
        );
        let error = read_body(Body::from_stream(chunks), 64).await.unwrap_err();
        assert_eq!(error, BodyError::TooLarge);
    }

    #[tokio::test]
    async fn a_body_the_client_sent_too_slowly_is_a_408() {
        let chunks = futures_util::stream::iter([Err::<Bytes, _>(BodyReadTimeout)]);
        let error = read_body(Body::from_stream(chunks), 64).await.unwrap_err();
        assert_eq!(error, BodyError::TimedOut);
        assert_eq!(error.status(), http::StatusCode::REQUEST_TIMEOUT);
    }

    #[test]
    fn the_limit_comes_from_the_extensions() {
        let mut extensions = http::Extensions::new();
        assert_eq!(BodyLimit::of(&extensions), DEFAULT_BODY_LIMIT);
        extensions.insert(BodyLimit(16));
        assert_eq!(BodyLimit::of(&extensions), 16);
    }
}
