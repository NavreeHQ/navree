//! [`RequestBuilder`] and the request the transports send.

use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, PROXY_AUTHORIZATION, USER_AGENT};
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use navree_contracts::RequestId;
use serde::Serialize;
use url::Url;

use crate::error::shown_url;
use crate::{HttpClient, HttpClientError, Response, StreamingResponse};

/// A request being built; nothing is sent until [`send`](RequestBuilder::send).
///
/// Builder methods never fail: the first problem (an invalid URL, header or
/// body) is kept and returned by `send`.
///
/// `Debug` shows the method, the URL without query string or credentials,
/// and header names only.
#[must_use = "a request does nothing until `send` is awaited"]
pub struct RequestBuilder {
    client: HttpClient,
    method: Method,
    url: String,
    headers: HeaderMap,
    query: Vec<String>,
    body: Option<Bytes>,
    retries: Option<u32>,
    timeout: Option<Duration>,
    secret_url: bool,
    error: Option<HttpClientError>,
}

/// The header that carries the request id.
const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

impl RequestBuilder {
    /// A request for `method` `url` sent through `client`.
    pub(crate) fn new(client: HttpClient, method: Method, url: String) -> Self {
        Self {
            client,
            method,
            url,
            headers: HeaderMap::new(),
            query: Vec::new(),
            body: None,
            retries: None,
            timeout: None,
            secret_url: false,
            error: None,
        }
    }

    /// Treats the whole URL as a secret, as an incoming webhook's URL is:
    /// errors, logs, `Debug` output and the `HttpClientRequest` telemetry event
    /// show its scheme and host alone (`https://hooks.example.com/[hidden]`).
    /// Without this they show the path, leaving out only the query string and
    /// credentials.
    pub fn secret_url(mut self) -> Self {
        self.secret_url = true;
        self
    }

    /// Adds a header (repeated names are kept, not replaced). Values of
    /// `Authorization`, `Proxy-Authorization` and `Cookie` are marked
    /// sensitive, so they never show in `Debug` output.
    pub fn header<K, V>(mut self, name: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        match (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            (Ok(name), Ok(mut value)) => {
                if [AUTHORIZATION, PROXY_AUTHORIZATION, COOKIE].contains(&name) {
                    value.set_sensitive(true);
                }
                self.headers.append(name, value);
            }
            (Err(error), _) => self.fail(format!("invalid header name: {}", error.into())),
            (Ok(name), Err(_)) => self.fail(format!("invalid value for header `{name}`")),
        }
        self
    }

    /// Sets `Authorization: Bearer <token>`. The value is marked sensitive, so
    /// it never shows in `Debug` output or logs.
    pub fn bearer(mut self, token: impl fmt::Display) -> Self {
        match HeaderValue::try_from(format!("Bearer {token}")) {
            Ok(mut value) => {
                value.set_sensitive(true);
                self.headers.insert(AUTHORIZATION, value);
            }
            Err(_) => self.fail("invalid bearer token".to_owned()),
        }
        self
    }

    /// Sets `X-Request-Id`, so the remote service can tie its logs to the
    /// incoming request that caused this call.
    pub fn with_request_id(mut self, id: &RequestId) -> Self {
        match HeaderValue::try_from(id.as_str()) {
            Ok(value) => {
                self.headers.insert(X_REQUEST_ID, value);
            }
            Err(_) => self.fail("invalid request id".to_owned()),
        }
        self
    }

    /// Appends `query` (anything `serde_urlencoded` accepts: a struct, a map,
    /// or a slice of pairs) to the URL's query string.
    pub fn query<T: Serialize + ?Sized>(mut self, query: &T) -> Self {
        match serde_urlencoded::to_string(query) {
            Ok(encoded) if encoded.is_empty() => {}
            Ok(encoded) => self.query.push(encoded),
            Err(error) => self.fail(format!("cannot encode the query string: {error}")),
        }
        self
    }

    /// Sends `form` URL-encoded (`Content-Type: application/x-www-form-urlencoded`).
    pub fn form<T: Serialize + ?Sized>(mut self, form: &T) -> Self {
        match serde_urlencoded::to_string(form) {
            Ok(encoded) => {
                self.headers.insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/x-www-form-urlencoded"),
                );
                self.body = Some(Bytes::from(encoded));
            }
            Err(error) => self.fail(format!("cannot encode the form body: {error}")),
        }
        self
    }

    /// Sends a raw body (set `Content-Type` with [`header`](Self::header)).
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Retries after the first attempt for this request, instead of
    /// [`HttpClientOptions::retries`](crate::HttpClientOptions::retries).
    /// Only idempotent methods (`GET`, `HEAD`, `OPTIONS`, `TRACE`, `PUT`,
    /// `DELETE`) are ever retried: for `POST` and `PATCH` this is ignored.
    pub fn retries(mut self, retries: u32) -> Self {
        self.retries = Some(retries);
        self
    }

    /// The time allowed for each attempt, from connecting to the last body
    /// byte, instead of
    /// [`HttpClientOptions::timeout_secs`](crate::HttpClientOptions::timeout_secs).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sends `body` as JSON (`Content-Type: application/json`).
    pub fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        match serde_json::to_vec(body) {
            Ok(bytes) => {
                self.headers
                    .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                self.body = Some(Bytes::from(bytes));
            }
            Err(error) => self.fail(format!("cannot encode the JSON body: {error}")),
        }
        self
    }

    /// Sends the request, then reads the whole response body.
    pub async fn send(self) -> Result<Response, HttpClientError> {
        let (client, request, retries, timeout) = self.prepare()?;
        client.send(request, retries, timeout).await
    }

    /// Sends the request once (never retried) and returns as soon as the
    /// response headers arrive; read the body with
    /// [`StreamingResponse::chunk`] or as a stream. The SSRF policy, the
    /// `traceparent` header and the `HttpClientRequest` event (reported at
    /// the headers) apply as for [`send`](Self::send); the attempt timeout covers the wait for
    /// the headers, not the body, and the body is not limited to
    /// `max_response_bytes` (except through [`StreamingResponse::bytes`]).
    pub async fn send_streaming(self) -> Result<StreamingResponse, HttpClientError> {
        let (client, request, _, timeout) = self.prepare()?;
        client.send_streaming(request, timeout).await
    }

    /// The request ready to send (the URL with its query, the headers with a
    /// user agent), with its client, retries and timeout.
    #[allow(clippy::type_complexity)]
    fn prepare(
        self,
    ) -> Result<(HttpClient, PreparedRequest, Option<u32>, Option<Duration>), HttpClientError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut url = Url::parse(&self.url)
            .map_err(|error| HttpClientError::Build(format!("invalid URL: {error}")))?;
        if !self.query.is_empty() {
            let added = self.query.join("&");
            let query = match url.query() {
                Some(existing) if !existing.is_empty() => format!("{existing}&{added}"),
                _ => added,
            };
            url.set_query(Some(&query));
        }
        let mut headers = self.headers;
        if !headers.contains_key(USER_AGENT) {
            headers.insert(USER_AGENT, self.client.user_agent().clone());
        }
        let request = PreparedRequest {
            method: self.method,
            url,
            headers,
            body: self.body,
            secret_url: self.secret_url,
        };
        Ok((self.client, request, self.retries, self.timeout))
    }

    /// Keeps the first build error.
    fn fail(&mut self, message: String) {
        if self.error.is_none() {
            self.error = Some(HttpClientError::Build(message));
        }
    }
}

impl fmt::Debug for RequestBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let url = Url::parse(&self.url)
            .map(|url| shown_url(&url, self.secret_url))
            .unwrap_or_else(|_| "<invalid URL>".to_owned());
        f.debug_struct("RequestBuilder")
            .field("method", &self.method)
            .field("url", &url)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("body_bytes", &self.body.as_ref().map(Bytes::len))
            .field("retries", &self.retries)
            .field("timeout", &self.timeout)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// A request ready to go: what the transports send and the fake records.
#[derive(Clone, Debug)]
pub(crate) struct PreparedRequest {
    pub(crate) method: Method,
    pub(crate) url: Url,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Option<Bytes>,
    pub(crate) secret_url: bool,
}
