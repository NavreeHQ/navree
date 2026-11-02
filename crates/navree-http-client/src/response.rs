//! [`Response`]: a fully read HTTP response.

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

use crate::HttpClientError;
use crate::error::shown_url;

/// A response whose body has been read completely, so its accessors are
/// synchronous and can be called any number of times.
#[derive(Clone)]
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
    url: Url,
    secret_url: bool,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .field("url", &shown_url(&self.url, self.secret_url))
            .finish()
    }
}

impl Response {
    /// Assembles a response (used by the transports).
    pub(crate) fn new(status: StatusCode, headers: HeaderMap, body: Bytes, url: Url) -> Self {
        Self {
            status,
            headers,
            body,
            url,
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

    /// The URL that produced the response (after redirects).
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The first value of header `name` (case-insensitive), if present and
    /// valid UTF-8.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }

    /// The body parsed as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, HttpClientError> {
        serde_json::from_slice(&self.body).map_err(HttpClientError::Decode)
    }

    /// The body as text (invalid UTF-8 sequences become `U+FFFD`).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The raw body (a cheap clone).
    pub fn bytes(&self) -> Bytes {
        self.body.clone()
    }

    /// `Ok(self)` for statuses below 400, otherwise
    /// [`HttpClientError::Status`].
    pub fn error_for_status(self) -> Result<Self, HttpClientError> {
        if self.status.is_client_error() || self.status.is_server_error() {
            Err(HttpClientError::Status {
                status: self.status,
                url: shown_url(&self.url, self.secret_url),
            })
        } else {
            Ok(self)
        }
    }
}
