//! [`HttpClient`]: the shared outgoing HTTP client.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use backon::{BackoffBuilder, ExponentialBuilder};
use bytes::BytesMut;
use http::{HeaderValue, Method, StatusCode};
use keel_config::ConfigSection;
use tokio::time::Instant;
use url::Url;

use crate::error::display_url;
use crate::fake::{FakeServer, FakeState};
use crate::request::PreparedRequest;
use crate::ssrf::{Refused, SsrfGuard, SsrfResolver};
use crate::{HttpClientError, HttpClientOptions, RequestBuilder, Response};

/// The outgoing HTTP client: cheap to clone, shared as a singleton.
#[derive(Clone)]
pub struct HttpClient {
    inner: Arc<Inner>,
}

/// The longest wait between two attempts.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

/// Options and transport, shared by every clone.
struct Inner {
    options: HttpClientOptions,
    user_agent: HeaderValue,
    guard: Arc<SsrfGuard>,
    transport: Transport,
}

/// Where requests go.
enum Transport {
    /// reqwest on rustls (ring), with the SSRF resolver and redirect policy.
    Real(reqwest::Client),
    /// Stubbed responses; never touches the network.
    Fake(Arc<FakeState>),
}

impl HttpClient {
    /// A real client: rustls with the ring provider, system proxies ignored,
    /// redirects (at most 10) and DNS answers checked against
    /// `options.ssrf`.
    pub fn new(options: HttpClientOptions) -> Result<HttpClient, HttpClientError> {
        options.validate().map_err(HttpClientError::Build)?;
        let guard = Arc::new(SsrfGuard::new(&options.ssrf));
        // reqwest is built without a default crypto provider: use ring. An
        // error only means a process-wide provider is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(options.connect_timeout_secs))
            .dns_resolver(SsrfResolver::new(guard.clone()))
            .redirect(guard.redirect_policy())
            .no_proxy()
            .build()
            .map_err(|error| {
                HttpClientError::Build(format!("cannot build the HTTP client: {error}"))
            })?;
        Self::with_transport(options, guard, Transport::Real(client))
    }

    /// A fake client with default options, and the [`FakeServer`] that stubs
    /// its responses and records its requests.
    pub fn fake() -> (HttpClient, FakeServer) {
        Self::fake_with(HttpClientOptions::default())
    }

    /// A fake client with `options`: retries, timeouts and the response size
    /// limit apply as they would for a real client; the SSRF policy applies
    /// to literal IP URLs (a fake resolves no host names).
    ///
    /// # Panics
    ///
    /// If `options` are invalid (a zero timeout or size limit, a user agent
    /// that is not a valid header value).
    pub fn fake_with(options: HttpClientOptions) -> (HttpClient, FakeServer) {
        if let Err(message) = options.validate() {
            panic!("invalid fake client options: {message}");
        }
        let state = Arc::new(FakeState::default());
        let guard = Arc::new(SsrfGuard::new(&options.ssrf));
        let client = Self::with_transport(options, guard, Transport::Fake(state.clone()))
            .expect("valid fake client options");
        (client, FakeServer::new(state))
    }

    /// A client sending through `transport`.
    fn with_transport(
        options: HttpClientOptions,
        guard: Arc<SsrfGuard>,
        transport: Transport,
    ) -> Result<HttpClient, HttpClientError> {
        let user_agent = HeaderValue::try_from(options.user_agent.as_str()).map_err(|_| {
            HttpClientError::Build("`user_agent` is not a valid header value".to_owned())
        })?;
        Ok(HttpClient {
            inner: Arc::new(Inner {
                options,
                user_agent,
                guard,
                transport,
            }),
        })
    }

    /// The options this client was built with.
    pub fn options(&self) -> &HttpClientOptions {
        &self.inner.options
    }

    /// Starts a `method` request to `url`.
    pub fn request(&self, method: Method, url: impl AsRef<str>) -> RequestBuilder {
        RequestBuilder::new(self.clone(), method, url.as_ref().to_owned())
    }

    /// Starts a `GET` request.
    pub fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    /// Starts a `POST` request.
    pub fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    /// Starts a `PUT` request.
    pub fn put(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::PUT, url)
    }

    /// Starts a `PATCH` request.
    pub fn patch(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::PATCH, url)
    }

    /// Starts a `DELETE` request.
    pub fn delete(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::DELETE, url)
    }

    /// The `User-Agent` header value.
    pub(crate) fn user_agent(&self) -> &HeaderValue {
        &self.inner.user_agent
    }

    /// Sends `request`, retrying idempotent requests on connection errors,
    /// 5xx and 429 with exponential backoff (jittered, doubling from
    /// `retry_base_ms`). After the last retry the last outcome is returned.
    pub(crate) async fn send(
        &self,
        request: PreparedRequest,
        retries: Option<u32>,
        timeout: Option<Duration>,
    ) -> Result<Response, HttpClientError> {
        self.inner.guard.check_url(&request.url)?;
        let options = &self.inner.options;
        let timeout = timeout.unwrap_or(Duration::from_secs(options.timeout_secs));
        let retries = if is_idempotent(&request.method) {
            retries.unwrap_or(options.retries)
        } else {
            0
        };
        let mut delays = ExponentialBuilder::new()
            .with_min_delay(Duration::from_millis(options.retry_base_ms))
            .with_factor(2.0)
            .with_max_delay(MAX_RETRY_DELAY)
            .with_max_times(retries as usize)
            .with_jitter()
            .build();
        let url = display_url(&request.url);
        let started = Instant::now();
        let mut attempts = 1;
        let outcome = loop {
            let outcome = tokio::time::timeout(timeout, self.execute(request.clone()))
                .await
                .unwrap_or_else(|_| Err(HttpClientError::Timeout { url: url.clone() }));
            if is_retryable(&outcome)
                && let Some(delay) = delays.next()
            {
                tracing::debug!(
                    method = %request.method,
                    url = %url,
                    attempt = attempts,
                    delay_ms = delay.as_millis() as u64,
                    "retrying outgoing request"
                );
                tokio::time::sleep(delay).await;
                attempts += 1;
                continue;
            }
            break outcome;
        };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match &outcome {
            Ok(response) => tracing::debug!(
                method = %request.method,
                url = %url,
                status = response.status().as_u16(),
                attempts,
                elapsed_ms,
                "outgoing request"
            ),
            Err(error) => tracing::debug!(
                method = %request.method,
                url = %url,
                %error,
                attempts,
                elapsed_ms,
                "outgoing request failed"
            ),
        }
        outcome
    }

    /// Sends `request` once.
    async fn execute(&self, request: PreparedRequest) -> Result<Response, HttpClientError> {
        let limit = self.inner.options.max_response_bytes;
        match &self.inner.transport {
            Transport::Real(client) => execute_real(client, request, limit).await,
            Transport::Fake(state) => {
                let response = state.execute(&request).await?;
                if response.bytes().len() as u64 > limit {
                    return Err(too_large(&request.url, limit));
                }
                Ok(response)
            }
        }
    }
}

/// [`HttpClientError::BodyTooLarge`].
fn too_large(url: &Url, limit: u64) -> HttpClientError {
    HttpClientError::BodyTooLarge {
        url: display_url(url),
        limit,
    }
}

/// Sends `request` with reqwest and reads the whole body (at most `limit`
/// bytes).
async fn execute_real(
    client: &reqwest::Client,
    request: PreparedRequest,
    limit: u64,
) -> Result<Response, HttpClientError> {
    let mut builder = client
        .request(request.method, request.url.clone())
        .headers(request.headers);
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    let mut response = builder
        .send()
        .await
        .map_err(|error| transport_error(error, &request.url))?;
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(too_large(&request.url, limit));
    }
    let mut body = BytesMut::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| transport_error(error, &request.url))?
    {
        if (body.len() + chunk.len()) as u64 > limit {
            return Err(too_large(&request.url, limit));
        }
        body.extend_from_slice(&chunk);
    }
    let status = response.status();
    let headers = response.headers().clone();
    let url = response.url().clone();
    Ok(Response::new(status, headers, body.freeze(), url))
}

/// Maps a reqwest error: an SSRF refusal anywhere in the chain is
/// [`HttpClientError::Blocked`]; the URL (and its query) is dropped from the
/// source so it cannot leak into logs.
fn transport_error(error: reqwest::Error, url: &Url) -> HttpClientError {
    if let Some(refused) = Refused::find(&error) {
        return refused.clone().into_error();
    }
    let url = display_url(url);
    let error = error.without_url();
    if error.is_connect() {
        HttpClientError::Connect {
            url,
            source: Box::new(error),
        }
    } else if error.is_timeout() {
        HttpClientError::Timeout { url }
    } else {
        HttpClientError::Request {
            url,
            source: Box::new(error),
        }
    }
}

/// Methods that are safe to repeat (RFC 9110, section 9.2.2).
fn is_idempotent(method: &Method) -> bool {
    [
        Method::GET,
        Method::HEAD,
        Method::OPTIONS,
        Method::TRACE,
        Method::PUT,
        Method::DELETE,
    ]
    .contains(method)
}

/// Connection errors, 5xx and 429 are worth another attempt.
fn is_retryable(outcome: &Result<Response, HttpClientError>) -> bool {
    match outcome {
        Ok(response) => {
            response.status().is_server_error()
                || response.status() == StatusCode::TOO_MANY_REQUESTS
        }
        Err(error) => matches!(error, HttpClientError::Connect { .. }),
    }
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let transport = match self.inner.transport {
            Transport::Real(_) => "reqwest",
            Transport::Fake(_) => "fake",
        };
        f.debug_struct("HttpClient")
            .field("transport", &transport)
            .field("options", &self.inner.options)
            .finish()
    }
}
