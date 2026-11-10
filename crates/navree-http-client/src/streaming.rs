//! [`StreamingResponse`]: a response whose body is read as it arrives.

use std::fmt;
use std::pin::Pin;

use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use http::{HeaderMap, StatusCode};
use url::Url;

use crate::HttpClientError;
use crate::error::shown_url;

/// The body of a [`StreamingResponse`]: pieces of bytes as they arrive.
pub type BodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, HttpClientError>> + Send>>;

/// A response returned as soon as its headers arrive, by
/// [`RequestBuilder::send_streaming`](crate::RequestBuilder::send_streaming).
/// The body is read piece by piece, with [`chunk`](Self::chunk) or as a
/// [`Stream`] ([`into_stream`](Self::into_stream)), for server-sent events
/// and other long responses. Dropping it closes the connection.
pub struct StreamingResponse {
    status: StatusCode,
    headers: HeaderMap,
    url: Url,
    body: BodyStream,
    limit: u64,
    secret_url: bool,
}

impl StreamingResponse {
    /// Assembles a response (used by the transports).
    pub(crate) fn new(
        status: StatusCode,
        headers: HeaderMap,
        url: Url,
        body: BodyStream,
        limit: u64,
    ) -> Self {
        Self {
            status,
            headers,
            url,
            body,
            limit,
            secret_url: false,
        }
    }

    /// Marks the URL as a secret, as its request was.
    pub(crate) fn with_secret_url(mut self, secret: bool) -> Self {
        self.secret_url = secret;
        self
    }

    /// The status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// `true` for a 2xx status.
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Every response header.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// The first value of header `name` (case-insensitive), if present and
    /// valid UTF-8.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }

    /// The URL that produced the response (after redirects).
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The next piece of the body, or `None` at its end.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, HttpClientError> {
        self.body.next().await.transpose()
    }

    /// The body as a [`Stream`] of pieces.
    pub fn into_stream(self) -> BodyStream {
        self.body
    }

    /// Reads the rest of the body, at most
    /// [`HttpClientOptions::max_response_bytes`](crate::HttpClientOptions::max_response_bytes)
    /// (an error response's message, for instance).
    pub async fn bytes(mut self) -> Result<Bytes, HttpClientError> {
        let mut body = BytesMut::new();
        while let Some(chunk) = self.chunk().await? {
            if (body.len() + chunk.len()) as u64 > self.limit {
                return Err(HttpClientError::BodyTooLarge {
                    url: shown_url(&self.url, self.secret_url),
                    limit: self.limit,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body.freeze())
    }
}

impl fmt::Debug for StreamingResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingResponse")
            .field("status", &self.status)
            .field("url", &shown_url(&self.url, self.secret_url))
            .finish_non_exhaustive()
    }
}
