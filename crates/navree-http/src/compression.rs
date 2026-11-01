//! [`CompressionLayer`]: gzip for responses, on `flate2`. The HTTP
//! pipeline installs it when `http.compression` is on (the default).

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::header::{
    ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    VARY,
};
use http::{HeaderMap, HeaderValue, Request, Response};
use http_body::Body as HttpBody;
use navree_kernel::BoxError;
use pin_project_lite::pin_project;
use tower::{Layer, Service};

use crate::body::Body;

mod gzip;

use gzip::GzipBody;

/// Bodies of fewer bytes than this are sent as they are.
const MIN_SIZE: u64 = 32;

/// A Tower layer that gzips responses for clients that accept it.
///
/// - **The client chooses** with `Accept-Encoding`. Its items are read as
///   `gzip` or `x-gzip` and `identity` (any case), each with an optional
///   `;q=` weight of `0` to `1` with up to three decimals; other codings
///   are ignored, and so is an item whose parameter is anything but a valid
///   weight. The response is gzipped when `gzip` has a weight above 0 that
///   is at least that of `identity`. A header value that is not visible
///   ASCII is skipped.
/// - **Some responses are never compressed**: one that already has a
///   `Content-Encoding` or a `Content-Range`, a body of fewer than 32 bytes
///   (by its exact size, or else its `Content-Length`; a body of unknown
///   size counts as large), and the content types `image/*` (but
///   `image/svg+xml`), `application/grpc*` (but `application/grpc-web*`)
///   and `text/event-stream`. Content types are compared as written.
/// - **`Vary: accept-encoding`** is added to every response that could be
///   compressed, whatever the client accepts, unless a `Vary` value already
///   names it.
/// - **A compressed response** loses its `Content-Length` and
///   `Accept-Ranges` and gets `Content-Encoding: gzip`. Its body is a gzip
///   stream at level 6 that keeps streaming, in frames of at most 4 KiB:
///   when the handler's body has nothing ready, what it sent so far is
///   flushed to the client, and the body is read no faster than the
///   client takes the frames. Its trailers follow the gzip stream, and its
///   error ends the response with that error.
///
/// ```
/// use navree_http::{AppRouter, CompressionLayer};
/// use navree_http::routing::Router;
///
/// let app = AppRouter::new(Router::new()).layer(CompressionLayer::new());
/// # let _ = app;
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct CompressionLayer {
    _private: (),
}

impl CompressionLayer {
    /// The layer.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<S> Layer<S> for CompressionLayer {
    type Service = Compression<S>;

    fn layer(&self, inner: S) -> Compression<S> {
        Compression { inner }
    }
}

/// The service [`CompressionLayer`] makes.
#[derive(Clone, Copy, Debug)]
pub struct Compression<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for Compression<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: HttpBody<Data = Bytes> + Send + 'static,
    ResBody::Error: Into<BoxError>,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let gzip = accepts_gzip(request.headers());
        ResponseFuture {
            future: self.inner.call(request),
            gzip,
        }
    }
}

pin_project! {
    /// The response future of [`Compression`].
    pub struct ResponseFuture<F> {
        #[pin]
        future: F,
        gzip: bool,
    }
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    type Output = Result<Response<Body>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let response = match this.future.poll(cx) {
            Poll::Ready(Ok(response)) => response,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        };
        Poll::Ready(Ok(compress(response, *this.gzip)))
    }
}

/// `response`, gzipped when `gzip` (the client accepts it) and the
/// response may be compressed.
fn compress<B>(response: Response<B>, gzip: bool) -> Response<Body>
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    let compressible = compressible(&response);
    let (mut parts, body) = response.into_parts();
    if compressible && !varies_with_accept_encoding(&parts.headers) {
        parts
            .headers
            .append(VARY, HeaderValue::from_static("accept-encoding"));
    }
    if !(compressible && gzip) {
        return Response::from_parts(parts, Body::new(body));
    }
    parts.headers.remove(ACCEPT_RANGES);
    parts.headers.remove(CONTENT_LENGTH);
    parts
        .headers
        .insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    Response::from_parts(parts, Body::new(GzipBody::new(body)))
}

/// Whether the response may be compressed, whatever the client accepts.
fn compressible<B: HttpBody>(response: &Response<B>) -> bool {
    let headers = response.headers();
    if headers.contains_key(CONTENT_ENCODING) || headers.contains_key(CONTENT_RANGE) {
        return false;
    }
    let size = response.body().size_hint().exact().or_else(|| {
        headers
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    });
    if size.is_some_and(|size| size < MIN_SIZE) {
        return false;
    }
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let excluded = |prefix: &str, except: Option<&str>| {
        content_type.starts_with(prefix)
            && !except.is_some_and(|except| content_type.starts_with(except))
    };
    !(excluded("application/grpc", Some("application/grpc-web"))
        || excluded("image/", Some("image/svg+xml"))
        || excluded("text/event-stream", None))
}

/// Whether a `Vary` value of `headers` contains `accept-encoding`, in any
/// case, anywhere in it.
fn varies_with_accept_encoding(headers: &HeaderMap) -> bool {
    const NAME: &[u8] = b"accept-encoding";
    headers.get_all(VARY).iter().any(|value| {
        value
            .as_bytes()
            .windows(NAME.len())
            .any(|window| window.eq_ignore_ascii_case(NAME))
    })
}

/// A coding the layer reads in `Accept-Encoding`.
#[derive(Clone, Copy, PartialEq)]
enum Coding {
    Identity,
    Gzip,
}

/// Whether the client's `Accept-Encoding` prefers gzip (see
/// [`CompressionLayer`]).
pub(crate) fn accepts_gzip(headers: &HeaderMap) -> bool {
    let mut gzip = 0;
    let mut identity = 0;
    let items = headers
        .get_all(ACCEPT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','));
    for item in items {
        let (name, weight) = match item.split_once(';') {
            Some((name, weight)) => (name, weight_of(weight.trim())),
            None => (item, Some(1000)),
        };
        let coding = match name.trim() {
            name if name.eq_ignore_ascii_case("gzip") || name.eq_ignore_ascii_case("x-gzip") => {
                Coding::Gzip
            }
            name if name.eq_ignore_ascii_case("identity") => Coding::Identity,
            _ => continue,
        };
        let Some(weight) = weight else { continue };
        let best = match coding {
            Coding::Gzip => &mut gzip,
            Coding::Identity => &mut identity,
        };
        *best = (*best).max(weight);
    }
    // A weight of 0 refuses the coding; gzip wins a tie.
    gzip > 0 && gzip >= identity
}

/// A `q=` weight in thousandths: `q=` (any case) then `0` or `1`, then
/// optionally `.` and up to three digits, at most `1`.
fn weight_of(text: &str) -> Option<u16> {
    let rest = text
        .strip_prefix("q=")
        .or_else(|| text.strip_prefix("Q="))?;
    let mut chars = rest.chars();
    let mut weight = match chars.next()? {
        '0' => 0,
        '1' => 1000,
        _ => return None,
    };
    match chars.next() {
        None => return Some(weight),
        Some('.') => {}
        Some(_) => return None,
    }
    let mut factor = 100;
    for char in chars {
        let digit = char.to_digit(10)?;
        if factor == 0 {
            return None;
        }
        weight += factor * digit as u16;
        factor /= 10;
    }
    (weight <= 1000).then_some(weight)
}

#[cfg(test)]
mod tests;
