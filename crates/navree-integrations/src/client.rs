//! Typed clients on the app's `HttpClient`: [`IntegrationClient`] and the
//! provider clients [`GraphClient`], [`GoogleClient`] and [`SlackClient`].

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::Method;
use navree_contracts::Secret;
use navree_http_client::{RequestBuilder, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::entry::ProviderEntry;
use crate::runtime::Runtime;
use crate::tokens::Want;
use crate::{ConnectionInfo, CredentialKind, IntegrationsError};

/// The most pages [`GraphClient::paginate`] and [`GoogleClient::paginate`]
/// follow.
const MAX_PAGES: usize = 1000;

/// A client of one connection: paths resolve against the provider's API
/// root, and every request carries a current token (refreshed under a lock
/// when it expires). A `401` gets one more try with a new token.
///
/// Absolute URLs are accepted only on the provider's own hosts, so a token
/// is never sent anywhere else.
///
/// ```no_run
/// # async fn run(client: navree_integrations::IntegrationClient) -> Result<(), navree_integrations::IntegrationsError> {
/// let contacts: serde_json::Value = client
///     .get("/crm/v3/objects/contacts")
///     .query(&[("limit", "10")])
///     .send_json()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct IntegrationClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    runtime: Arc<Runtime>,
    entry: Arc<ProviderEntry>,
    connection: ConnectionInfo,
    /// The connection's own API root (API-key connections).
    api_url: Option<Url>,
    /// The user a service account acts as for this client.
    subject: Option<String>,
}

impl IntegrationClient {
    pub(crate) fn new(
        runtime: Arc<Runtime>,
        entry: Arc<ProviderEntry>,
        connection: ConnectionInfo,
        api_url: Option<Url>,
    ) -> Self {
        Self {
            inner: Arc::new(ClientInner {
                runtime,
                entry,
                connection,
                api_url,
                subject: None,
            }),
        }
    }

    /// The connection.
    pub fn connection(&self) -> &ConnectionInfo {
        &self.inner.connection
    }

    /// The API root paths resolve against.
    pub fn base_url(&self) -> Option<&str> {
        self.inner
            .api_url
            .as_ref()
            .or(self.inner.entry.api_url.as_ref())
            .map(Url::as_str)
    }

    /// A current token (the API key, for API-key connections).
    pub async fn token(&self) -> Result<Secret<String>, IntegrationsError> {
        Ok(Secret::new(self.token_with(Want::Cached).await?))
    }

    async fn token_with(&self, want: Want) -> Result<String, IntegrationsError> {
        self.inner
            .runtime
            .token(
                self.inner.connection.id,
                self.inner.subject.as_deref(),
                want,
            )
            .await
    }

    /// The same client acting as `subject` (service accounts only).
    fn with_subject(&self, subject: Option<String>) -> Self {
        Self {
            inner: Arc::new(ClientInner {
                runtime: self.inner.runtime.clone(),
                entry: self.inner.entry.clone(),
                connection: self.inner.connection.clone(),
                api_url: self.inner.api_url.clone(),
                subject,
            }),
        }
    }

    /// A request: `path` is relative to the API root (`/users`), or an
    /// absolute URL on the provider's hosts.
    pub fn request(&self, method: Method, path: impl Into<String>) -> ApiRequest {
        ApiRequest {
            client: self.clone(),
            method,
            target: path.into(),
            ops: Vec::new(),
            error: None,
        }
    }

    /// A `GET` request.
    pub fn get(&self, path: impl Into<String>) -> ApiRequest {
        self.request(Method::GET, path)
    }

    /// A `POST` request.
    pub fn post(&self, path: impl Into<String>) -> ApiRequest {
        self.request(Method::POST, path)
    }

    /// A `PUT` request.
    pub fn put(&self, path: impl Into<String>) -> ApiRequest {
        self.request(Method::PUT, path)
    }

    /// A `PATCH` request.
    pub fn patch(&self, path: impl Into<String>) -> ApiRequest {
        self.request(Method::PATCH, path)
    }

    /// A `DELETE` request.
    pub fn delete(&self, path: impl Into<String>) -> ApiRequest {
        self.request(Method::DELETE, path)
    }

    fn provider(&self) -> &str {
        &self.inner.entry.name
    }

    /// The absolute URL of `target`.
    fn resolve(&self, target: &str) -> Result<Url, IntegrationsError> {
        if target.contains("://") {
            let url = Url::parse(target).map_err(|_| IntegrationsError::ForeignUrl {
                provider: self.provider().to_owned(),
                host: target.to_owned(),
            })?;
            let host = url.host_str().unwrap_or_default();
            if !self.inner.entry.allows_host(host, self.inner.api_url.as_ref()) {
                return Err(IntegrationsError::ForeignUrl {
                    provider: self.provider().to_owned(),
                    host: host.to_owned(),
                });
            }
            return Ok(url);
        }
        let base = self.base_url().ok_or_else(|| {
            IntegrationsError::Config(format!(
                "integrations.providers.{}.api_url is not set",
                self.inner.entry.key
            ))
        })?;
        let joined = format!(
            "{}/{}",
            base.trim_end_matches('/'),
            target.trim_start_matches('/')
        );
        Url::parse(&joined).map_err(|_| {
            IntegrationsError::Config(format!("`{joined}` is not a URL"))
        })
    }

    fn is_api_key(&self) -> bool {
        self.inner.connection.kind == CredentialKind::ApiKey
    }
}

impl fmt::Debug for IntegrationClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntegrationClient")
            .field("connection", &self.inner.connection.id)
            .field("provider", &self.inner.connection.provider)
            .field("base_url", &self.base_url())
            .field("subject", &self.inner.subject)
            .finish()
    }
}

type Op = Arc<dyn Fn(RequestBuilder) -> RequestBuilder + Send + Sync>;

/// A request being built by an [`IntegrationClient`]; [`send`](ApiRequest::send)
/// adds the token and sends it.
#[must_use = "a request does nothing until it is sent"]
pub struct ApiRequest {
    client: IntegrationClient,
    method: Method,
    target: String,
    ops: Vec<Op>,
    error: Option<IntegrationsError>,
}

impl ApiRequest {
    fn op(mut self, op: impl Fn(RequestBuilder) -> RequestBuilder + Send + Sync + 'static) -> Self {
        self.ops.push(Arc::new(op));
        self
    }

    fn serialized<T: Serialize + ?Sized>(&mut self, value: &T) -> Option<Value> {
        match serde_json::to_value(value) {
            Ok(value) => Some(value),
            Err(error) => {
                self.error = Some(IntegrationsError::Config(format!(
                    "the request could not be serialized: {error}"
                )));
                None
            }
        }
    }

    /// Adds a header.
    pub fn header(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let (name, value) = (name.into(), value.into());
        self.op(move |request| request.header(name.as_str(), value.as_str()))
    }

    /// Adds query parameters (a map, a struct or a list of pairs).
    pub fn query<T: Serialize + ?Sized>(mut self, query: &T) -> Self {
        match self.serialized(query).map(pairs) {
            Some(pairs) => self.op(move |request| request.query(&pairs)),
            None => self,
        }
    }

    /// Sends `body` as JSON.
    pub fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        match self.serialized(body) {
            Some(value) => self.op(move |request| request.json(&value)),
            None => self,
        }
    }

    /// Sends `form` as `application/x-www-form-urlencoded`.
    pub fn form<T: Serialize + ?Sized>(mut self, form: &T) -> Self {
        match self.serialized(form).map(pairs) {
            Some(pairs) => self.op(move |request| request.form(&pairs)),
            None => self,
        }
    }

    /// Sends raw bytes.
    pub fn body(self, body: impl Into<Bytes>) -> Self {
        let body: Bytes = body.into();
        self.op(move |request| request.body(body.clone()))
    }

    /// Limits each attempt to `timeout`.
    pub fn timeout(self, timeout: Duration) -> Self {
        self.op(move |request| request.timeout(timeout))
    }

    /// Sends the request with a current token and returns the response as
    /// it came (call `error_for_status` for 4xx and 5xx). A `401` is
    /// retried once with a new token.
    pub async fn send(self) -> Result<Response, IntegrationsError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let url = self.client.resolve(&self.target)?;
        let token = self.client.token_with(Want::Cached).await?;
        let response = self.attempt(&url, &token).await?;
        if response.status().as_u16() == 401 && !self.client.is_api_key() {
            let fresh = self.client.token_with(Want::Replace(token)).await?;
            return self.attempt(&url, &fresh).await;
        }
        Ok(response)
    }

    /// [`send`](ApiRequest::send), then the JSON body of a 2xx response.
    /// [`IntegrationsError::Http`] for any other status.
    pub async fn send_json<T: DeserializeOwned>(self) -> Result<T, IntegrationsError> {
        Ok(self.send().await?.error_for_status()?.json()?)
    }

    async fn attempt(&self, url: &Url, token: &str) -> Result<Response, IntegrationsError> {
        let runtime = &self.client.inner.runtime;
        let mut request = runtime.http.request(self.method.clone(), url.as_str());
        request = if self.client.is_api_key() {
            let (name, value) = self.client.inner.entry.api_key_header(token);
            request.header(name, value)
        } else {
            request.bearer(token)
        };
        for op in &self.ops {
            request = op(request);
        }
        request
            .send()
            .await
            .map_err(|error| IntegrationsError::Unavailable {
                provider: self.client.provider().to_owned(),
                message: error.to_string(),
            })
    }
}

/// Name-value pairs of a serialized map, struct or list of pairs (a list
/// value repeats its name; `null` leaves the pair out).
fn pairs(value: Value) -> Vec<(String, String)> {
    fn text(value: &Value) -> Option<String> {
        match value {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }
    let mut pairs = Vec::new();
    match value {
        Value::Object(map) => {
            for (name, value) in map {
                match value {
                    Value::Array(items) => {
                        pairs.extend(items.iter().filter_map(text).map(|v| (name.clone(), v)));
                    }
                    other => pairs.extend(text(&other).map(|v| (name, v))),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Value::Array(pair) = item
                    && let [name, value] = pair.as_slice()
                    && let (Some(name), Some(value)) = (text(name), text(value))
                {
                    pairs.push((name, value));
                }
            }
        }
        _ => {}
    }
    pairs
}

impl fmt::Debug for ApiRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiRequest")
            .field("method", &self.method)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

/// A Microsoft Graph client: an [`IntegrationClient`] on
/// `https://graph.microsoft.com/v1.0` with Graph's paging.
///
/// ```no_run
/// # async fn run(graph: navree_integrations::GraphClient) -> Result<(), navree_integrations::IntegrationsError> {
/// #[derive(serde::Deserialize)]
/// struct User { id: String, mail: Option<String> }
///
/// let users: Vec<User> = graph.paginate("/users?$select=id,mail").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct GraphClient(IntegrationClient);

impl GraphClient {
    /// The connected Microsoft tenant id.
    pub fn tenant_id(&self) -> Option<&str> {
        self.0.connection().external_id.as_deref()
    }

    /// Every item of a collection: `value` of each page, following
    /// `@odata.nextLink`.
    pub async fn paginate<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Vec<T>, IntegrationsError> {
        let mut items = Vec::new();
        let mut next = Some(path.to_owned());
        for _ in 0..MAX_PAGES {
            let Some(target) = next.take() else {
                break;
            };
            let page: Value = self.0.get(target).send_json().await?;
            if let Some(Value::Array(values)) = page.get("value") {
                for value in values {
                    items.push(serde_json::from_value(value.clone()).map_err(|error| {
                        IntegrationsError::Config(format!("a Graph item did not parse: {error}"))
                    })?);
                }
            }
            next = page
                .get("@odata.nextLink")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        Ok(items)
    }
}

impl From<IntegrationClient> for GraphClient {
    fn from(client: IntegrationClient) -> Self {
        GraphClient(client)
    }
}

impl Deref for GraphClient {
    type Target = IntegrationClient;

    fn deref(&self) -> &IntegrationClient {
        &self.0
    }
}

/// A Google Workspace client: an [`IntegrationClient`] on
/// `https://www.googleapis.com` (absolute URLs on other `*.googleapis.com`
/// hosts work too), with Google's paging and domain-wide delegation.
#[derive(Clone, Debug)]
pub struct GoogleClient(IntegrationClient);

impl GoogleClient {
    /// The connected Google Workspace domain.
    pub fn domain(&self) -> Option<&str> {
        self.0.connection().external_id.as_deref()
    }

    /// The same client acting as `user` (a service account with
    /// domain-wide delegation): send mail as them, read their calendar.
    /// Tokens per user are kept in memory until they expire.
    pub fn as_user(&self, user: impl Into<String>) -> GoogleClient {
        GoogleClient(self.0.with_subject(Some(user.into())))
    }

    /// Every item under `items_key` (`users`, `messages`) of each page,
    /// following `nextPageToken`.
    pub async fn paginate<T: DeserializeOwned>(
        &self,
        path: &str,
        items_key: &str,
    ) -> Result<Vec<T>, IntegrationsError> {
        let mut items = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut request = self.0.get(path);
            if let Some(token) = &page_token {
                request = request.query(&[("pageToken", token.as_str())]);
            }
            let page: Value = request.send_json().await?;
            if let Some(Value::Array(values)) = page.get(items_key) {
                for value in values {
                    items.push(serde_json::from_value(value.clone()).map_err(|error| {
                        IntegrationsError::Config(format!("a Google item did not parse: {error}"))
                    })?);
                }
            }
            page_token = page
                .get("nextPageToken")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if page_token.is_none() {
                break;
            }
        }
        Ok(items)
    }
}

impl From<IntegrationClient> for GoogleClient {
    fn from(client: IntegrationClient) -> Self {
        GoogleClient(client)
    }
}

impl Deref for GoogleClient {
    type Target = IntegrationClient;

    fn deref(&self) -> &IntegrationClient {
        &self.0
    }
}

/// A Slack client: an [`IntegrationClient`] on `https://slack.com/api`
/// whose [`call`](SlackClient::call) checks Slack's `ok` flag.
///
/// ```no_run
/// # async fn run(slack: navree_integrations::SlackClient) -> Result<(), navree_integrations::IntegrationsError> {
/// slack
///     .call("chat.postMessage", &serde_json::json!({ "channel": "C0123", "text": "Deal won" }))
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct SlackClient(IntegrationClient);

impl SlackClient {
    /// The connected Slack team (workspace) id.
    pub fn team_id(&self) -> Option<&str> {
        self.0.connection().external_id.as_deref()
    }

    /// Calls Web API method `method` with `args` as JSON and returns the
    /// body. `ok: false` is [`IntegrationsError::Api`] with Slack's error
    /// (`channel_not_found`); `ratelimited` is
    /// [`IntegrationsError::Unavailable`].
    pub async fn call<T: Serialize + ?Sized>(
        &self,
        method: &str,
        args: &T,
    ) -> Result<Value, IntegrationsError> {
        let response = self.0.post(method).json(args).send().await?;
        let status = response.status();
        if status.is_server_error() || status.as_u16() == 429 {
            return Err(IntegrationsError::Unavailable {
                provider: self.0.provider().to_owned(),
                message: format!("{method} answered {}", status.as_u16()),
            });
        }
        let body: Value = response.json()?;
        if body.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(body);
        }
        let error = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error")
            .to_owned();
        if error == "ratelimited" {
            return Err(IntegrationsError::Unavailable {
                provider: self.0.provider().to_owned(),
                message: error,
            });
        }
        Err(IntegrationsError::Api {
            provider: self.0.provider().to_owned(),
            error,
        })
    }
}

impl From<IntegrationClient> for SlackClient {
    fn from(client: IntegrationClient) -> Self {
        SlackClient(client)
    }
}

impl Deref for SlackClient {
    type Target = IntegrationClient;

    fn deref(&self) -> &IntegrationClient {
        &self.0
    }
}
