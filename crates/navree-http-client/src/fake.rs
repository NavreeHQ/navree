//! Test fakes: [`FakeServer`], [`FakeResponse`] and [`RecordedRequest`].

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use navree_glob::Glob;
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::display_url;
use crate::request::PreparedRequest;
use crate::{HttpClientError, Response};

/// The other half of [`HttpClient::fake`](crate::HttpClient::fake): stubs
/// responses and records every request. A fake client never touches the
/// network.
#[derive(Clone, Debug)]
pub struct FakeServer {
    state: Arc<FakeState>,
}

/// Stubs and recorded requests, shared by the fake client and its server.
#[derive(Debug, Default)]
pub(crate) struct FakeState {
    stubs: Mutex<Vec<Stub>>,
    recorded: Mutex<Vec<RecordedRequest>>,
}

/// One `on(...)` / `on_sequence(...)` registration.
#[derive(Debug)]
struct Stub {
    method: Method,
    pattern: Glob,
    /// Served front to back; the last one repeats.
    responses: VecDeque<FakeResponse>,
}

impl Stub {
    /// The next response: the front one, removed unless it is the last.
    fn next(&mut self) -> Option<FakeResponse> {
        if self.responses.len() > 1 {
            self.responses.pop_front()
        } else {
            self.responses.front().cloned()
        }
    }
}

impl FakeServer {
    /// A server with no stubs.
    pub(crate) fn new(state: Arc<FakeState>) -> Self {
        Self { state }
    }

    /// Answers `method` requests whose full URL (query string included)
    /// matches `url_glob` (`*` any characters, `?` one character) with
    /// `response`. Stubs are tried in registration order.
    pub fn on(&self, method: Method, url_glob: &str, response: FakeResponse) -> &Self {
        self.on_sequence(method, url_glob, [response])
    }

    /// Like [`on`](Self::on), but answers with `responses` in turn (one per
    /// attempt, retries included); the last one then repeats. An empty
    /// sequence never matches.
    pub fn on_sequence(
        &self,
        method: Method,
        url_glob: &str,
        responses: impl IntoIterator<Item = FakeResponse>,
    ) -> &Self {
        lock(&self.state.stubs).push(Stub {
            method,
            pattern: Glob::new(url_glob),
            responses: responses.into_iter().collect(),
        });
        self
    }

    /// Every request sent so far, oldest first.
    pub fn recorded(&self) -> Vec<RecordedRequest> {
        lock(&self.state.recorded).clone()
    }

    /// Panics unless at least one recorded request satisfies `predicate`.
    #[track_caller]
    pub fn assert_sent(&self, predicate: impl Fn(&RecordedRequest) -> bool) {
        let recorded = self.recorded();
        if !recorded.iter().any(predicate) {
            panic!(
                "no recorded request matches the predicate; recorded: {}",
                summary(&recorded)
            );
        }
    }

    /// Panics if any recorded request satisfies `predicate`.
    #[track_caller]
    pub fn assert_not_sent(&self, predicate: impl Fn(&RecordedRequest) -> bool) {
        let recorded = self.recorded();
        if recorded.iter().any(predicate) {
            panic!(
                "a recorded request matches the predicate; recorded: {}",
                summary(&recorded)
            );
        }
    }

    /// Panics unless exactly `count` requests were sent (retries count).
    #[track_caller]
    pub fn assert_sent_count(&self, count: usize) {
        let recorded = self.recorded();
        if recorded.len() != count {
            panic!(
                "expected {count} requests, {} were sent: {}",
                recorded.len(),
                summary(&recorded)
            );
        }
    }

    /// Panics if any request was sent.
    #[track_caller]
    pub fn assert_nothing_sent(&self) {
        self.assert_sent_count(0);
    }
}

impl FakeState {
    /// Records `request`, then answers it from the first matching stub.
    pub(crate) async fn execute(
        &self,
        request: &PreparedRequest,
    ) -> Result<Response, HttpClientError> {
        lock(&self.recorded).push(RecordedRequest::from(request));
        let response = lock(&self.stubs)
            .iter_mut()
            .filter(|stub| {
                stub.method == request.method && stub.pattern.matches(request.url.as_str())
            })
            .find_map(Stub::next);
        let Some(response) = response else {
            return Err(HttpClientError::NoFakeResponse {
                method: request.method.clone(),
                url: display_url(&request.url),
            });
        };
        if let Some(delay) = response.delay {
            tokio::time::sleep(delay).await;
        }
        if response.connection_failed {
            return Err(HttpClientError::Connect {
                url: display_url(&request.url),
                source: Box::new(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "connection refused (fake)",
                )),
            });
        }
        Ok(Response::new(
            response.status,
            response.headers,
            response.body,
            request.url.clone(),
        ))
    }
}

/// A canned response for [`FakeServer::on`].
#[derive(Clone, Debug)]
pub struct FakeResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
    connection_failed: bool,
    delay: Option<Duration>,
}

impl FakeResponse {
    /// An empty response with status `status`.
    ///
    /// # Panics
    ///
    /// If `status` is not between 100 and 999.
    pub fn new(status: u16) -> Self {
        Self {
            status: StatusCode::from_u16(status).expect("a valid HTTP status code"),
            headers: HeaderMap::new(),
            body: Bytes::new(),
            connection_failed: false,
            delay: None,
        }
    }

    /// Waits `delay` before answering (to exercise timeouts; use
    /// `tokio::time::pause` to keep tests fast).
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// No response at all: the attempt fails with
    /// [`HttpClientError::Connect`], as if the connection were refused.
    pub fn connection_failed() -> Self {
        Self {
            connection_failed: true,
            ..Self::new(503)
        }
    }

    /// Adds a response header.
    ///
    /// # Panics
    ///
    /// If `name` or `value` is not a valid header name or value.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.append(
            HeaderName::try_from(name).expect("a valid header name"),
            HeaderValue::try_from(value).expect("a valid header value"),
        );
        self
    }

    /// Sets a text body (and `Content-Type: text/plain; charset=utf-8`).
    pub fn text(mut self, body: impl Into<String>) -> Self {
        self.body = Bytes::from(body.into());
        self.headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        self
    }

    /// Sets a raw body (no `Content-Type`).
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// Sets a JSON body (and `Content-Type: application/json`).
    ///
    /// # Panics
    ///
    /// If `body` cannot be serialized.
    pub fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        self.body = Bytes::from(serde_json::to_vec(body).expect("a serializable JSON body"));
        self.headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        self
    }
}

/// A request a fake client received.
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Bytes,
}

impl From<&PreparedRequest> for RecordedRequest {
    fn from(request: &PreparedRequest) -> Self {
        Self {
            method: request.method.clone(),
            url: request.url.clone(),
            headers: request.headers.clone(),
            body: request.body.clone().unwrap_or_default(),
        }
    }
}

impl RecordedRequest {
    /// The method.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// The full URL, query string included.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The first value of header `name` (case-insensitive), if present and
    /// valid UTF-8.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }

    /// Every request header (sensitive values, such as the bearer token, are
    /// hidden from `Debug`).
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// The raw body (empty when none was sent).
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The body as text (invalid UTF-8 sequences become `U+FFFD`).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The body parsed as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, HttpClientError> {
        serde_json::from_slice(&self.body).map_err(HttpClientError::Decode)
    }
}

/// `METHOD url` per request, for assertion messages.
fn summary(recorded: &[RecordedRequest]) -> String {
    if recorded.is_empty() {
        return "none".to_owned();
    }
    recorded
        .iter()
        .map(|r| format!("{} {}", r.method, display_url(&r.url)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Locks `mutex`, ignoring poisoning (a panicking test must not hide others).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
