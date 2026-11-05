//! Request and response bodies: [`Body`], [`Bytes`], [`to_bytes`], and the
//! request body limit ([`BodyLimit`]).

use std::borrow::Cow;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

pub use bytes::Bytes;
use futures_util::{Stream, TryStream};
pub use http_body::Body as HttpBody;
use http_body::{Frame, SizeHint};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Collected, Empty, Full, LengthLimitError, Limited};
use navree_kernel::BoxError;
use pin_project_lite::pin_project;

use crate::serve::BodyReadTimeout;

type BoxedBody = UnsyncBoxBody<Bytes, BoxError>;

/// A request or response body: any [`HttpBody`] of [`Bytes`], boxed. Frames
/// pass through as they are produced, so a stream stays a stream, and an
/// error from the inner body comes out as it is. On a response the server
/// then cuts the connection off (on HTTP/2, the stream): a client never takes
/// a partial body for a whole one.
#[must_use]
#[derive(Debug)]
pub struct Body(BoxedBody);

impl Body {
    /// Boxes `body`. A `Body` is taken as it is, not boxed again.
    pub fn new<B>(body: B) -> Self
    where
        B: HttpBody<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        try_downcast(body).unwrap_or_else(|body| Self(boxed(body)))
    }

    /// A body with nothing in it.
    pub fn empty() -> Self {
        Self::new(Empty::new())
    }

    /// A body of the chunks of `stream`: each goes out as it arrives, and an
    /// error ends the body with that error.
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: TryStream + Send + 'static,
        S::Ok: Into<Bytes>,
        S::Error: Into<BoxError>,
    {
        Self::new(StreamBody { stream })
    }

    /// The body's data as a [`Stream`] of chunks. Frames that are not data
    /// (trailers) are dropped.
    pub fn into_data_stream(self) -> BodyDataStream {
        BodyDataStream { inner: self }
    }
}

fn boxed<B>(body: B) -> BoxedBody
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    try_downcast(body).unwrap_or_else(|body| body.map_err(Into::into).boxed_unsync())
}

/// `value` as a `T`, if that is what it is.
pub(crate) fn try_downcast<T: 'static, K: Send + 'static>(value: K) -> Result<T, K> {
    let mut value = Some(value);
    match <dyn std::any::Any>::downcast_mut::<Option<T>>(&mut value).and_then(Option::take) {
        Some(found) => Ok(found),
        None => Err(value.expect("a value of another type stays where it was")),
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::empty()
    }
}

impl From<()> for Body {
    fn from((): ()) -> Self {
        Self::empty()
    }
}

macro_rules! body_from {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for Body {
                fn from(value: $ty) -> Self {
                    Self::new(Full::from(value))
                }
            }
        )*
    };
}

body_from!(
    &'static [u8],
    Cow<'static, [u8]>,
    Vec<u8>,
    &'static str,
    Cow<'static, str>,
    String,
    Bytes,
);

impl HttpBody for Body {
    type Data = Bytes;
    type Error = BoxError;

    #[inline]
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        Pin::new(&mut self.0).poll_frame(cx)
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        self.0.size_hint()
    }
}

/// A [`Body`]'s data as a [`Stream`], from [`Body::into_data_stream`].
#[must_use]
#[derive(Debug)]
pub struct BodyDataStream {
    inner: Body,
}

impl Stream for BodyDataStream {
    type Item = Result<Bytes, BoxError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match ready!(Pin::new(&mut self.inner).poll_frame(cx)?) {
                Some(frame) => {
                    if let Ok(data) = frame.into_data() {
                        return Poll::Ready(Some(Ok(data)));
                    }
                }
                None => return Poll::Ready(None),
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let hint = self.inner.size_hint();
        let lower = usize::try_from(hint.lower()).unwrap_or_default();
        (lower, hint.upper().and_then(|upper| upper.try_into().ok()))
    }
}

impl HttpBody for BodyDataStream {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

pin_project! {
    /// A stream of chunks as a body.
    struct StreamBody<S> {
        #[pin]
        stream: S,
    }
}

impl<S> HttpBody for StreamBody<S>
where
    S: TryStream,
    S::Ok: Into<Bytes>,
    S::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        match ready!(self.project().stream.try_poll_next(cx)) {
            Some(Ok(chunk)) => Poll::Ready(Some(Ok(Frame::data(chunk.into())))),
            Some(Err(error)) => Poll::Ready(Some(Err(error.into()))),
            None => Poll::Ready(None),
        }
    }
}

/// Reads a whole body, up to `limit` bytes. Going past `limit` fails, and so
/// does an error from the body.
pub async fn to_bytes(body: Body, limit: usize) -> Result<Bytes, BoxError> {
    Limited::new(body, limit)
        .collect()
        .await
        .map(Collected::to_bytes)
}

/// The limit body extractors use when a request carries no [`BodyLimit`]
/// (a route outside the pipeline): 2 MiB, `http.body_limit_bytes`'s default.
pub const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;

/// The most bytes a request body may have, in the request extensions: the
/// pipeline sets the route's (`RouteDef::body_limit`) or the app's
/// (`http.body_limit_bytes`). [`Json`](crate::Json), [`Form`](crate::Form)
/// and [`read_body`] stop reading past it (413); a `Content-Length` above it
/// is refused before authentication. Such a body, on HTTP/1, is read and
/// thrown away first when it is at most 8 MiB, for at most 5 s and never past
/// the request's own timeout, so a client that sends it whole before it reads
/// still reads the 413.
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
