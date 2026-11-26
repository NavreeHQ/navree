//! [`AzureStore`]: Azure Blob Storage on Navree's shared HTTP core.
//!
//! Supports Shared Key authorization, Entra service principals (client
//! secret), workload identity federation (`AZURE_FEDERATED_TOKEN_FILE`) and
//! managed identities (App Service `IDENTITY_ENDPOINT` and IMDS
//! `169.254.169.254`), plus service SAS and user delegation SAS links.

mod blocks;
mod credentials;
mod delegation;
mod shared_key;

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ::http::{HeaderMap, Method, StatusCode};
use bytes::Bytes;
use chrono::{DateTime, Datelike, Utc};
use futures_util::TryStreamExt;
use navree_kernel::BoxFuture;
use url::Url;

pub use blocks::DEFAULT_BLOCK_SIZE;
pub use credentials::{Auth, AuthKind};
pub use shared_key::{SharedKey, VERSION, http_date};

use crate::StorageError;
use crate::azure_sas::SharedKeySas;
pub use crate::azure_sas::UserDelegationKey;
use crate::cloud::credentials::Provider;
use crate::cloud::http::{Client, Request, Response};
use crate::cloud::keys::{check_key, check_prefix, encode_component, object_url};
use crate::cloud::retry::{Replay, RetryPolicy, with_backoff};
use crate::cloud::{Error, ErrorKind, MAX_LIST_ENTRIES, MAX_LIST_PAGES, Result, xml};
use crate::options::AzureConfig;
use crate::store::{HeaderName, HeaderValue, Meta, Object, Store, StoreError, StoreResult, Upload};

use self::blocks::{BlockUpload, MAX_BLOCK_SIZE};
use self::credentials::{AuthSource, Clock, ResolveError};
use self::delegation::{DelegationKeys, MAX_DELEGATION_TTL};

pub(crate) struct AzureInner {
    pub(crate) account: String,
    pub(crate) container: String,
    pub(crate) service_url: Url,
    pub(crate) container_url: Url,
    pub(crate) client: Client,
    pub(crate) auth_source: AuthSource,
    pub(crate) credentials: Provider<Auth>,
    pub(crate) shared_key_sas: Option<SharedKeySas>,
    pub(crate) delegation_keys: DelegationKeys,
    pub(crate) policy: RetryPolicy,
    pub(crate) block_size: usize,
    pub(crate) clock: Clock,
}

impl AzureInner {
    /// Signs or attaches bearer authorization and `x-ms-date` /
    /// `x-ms-version` headers to `request`.
    pub(crate) fn authorize(&self, mut request: Request, auth: &Auth) -> Result<Request> {
        let now = (self.clock)();
        match auth {
            Auth::SharedKey(key) => key.sign(request, now),
            Auth::BearerToken(token) => {
                if !request.headers.contains_key("x-ms-date") {
                    request = request.header("x-ms-date", &http_date(now))?;
                }
                if !request.headers.contains_key("x-ms-version") {
                    request = request.header("x-ms-version", VERSION)?;
                }
                request.secret("authorization", &format!("Bearer {token}"))
            }
        }
    }

    /// Sends `request` and turns non-2xx answers into an [`Error`] carrying
    /// Azure's short error code (`x-ms-error-code` or `<Error><Code>`),
    /// never any `<Message>` or `<AuthenticationErrorDetail>`.
    pub(crate) async fn send_checked(&self, request: Request) -> Result<Response> {
        let response = self.client.send(request).await?;
        check_response(response).await
    }
}

async fn check_response(response: Response) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let header_code = response.header("x-ms-error-code").map(str::to_owned);
    let Err(mut error) = response.success().await else {
        unreachable!();
    };
    if let Some(code) = header_code {
        error = error.with_code(code.trim());
    } else if let Some(body) = error.body()
        && let Ok(root) = xml::parse(body.as_bytes())
        && root.name() == "Error"
        && let Some(code) = root.value("Code")
    {
        error = error.with_code(code.trim());
    }
    Err(error)
}

/// An Azure Blob Storage container implementing [`Store`].
#[derive(Clone)]
pub struct AzureStore {
    inner: Arc<AzureInner>,
}

impl AzureStore {
    /// Builds an Azure Blob store from `config` and the process environment.
    pub fn new(config: AzureConfig) -> Result<Self> {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        Self::from_env(config, &env)
    }

    /// Builds an Azure Blob store from `config` and `env` without reading
    /// global process variables.
    pub fn from_env(config: AzureConfig, env: &BTreeMap<String, String>) -> Result<Self> {
        Self::resolve(&config, env).map_err(Error::from)
    }

    /// Builds an Azure Blob store for disk `name`, preserving
    /// `StorageError::InvalidOptions` on malformed base64 access keys.
    pub(crate) fn build_for_disk(
        name: &str,
        config: &AzureConfig,
        env: &BTreeMap<String, String>,
    ) -> std::result::Result<Self, StorageError> {
        Self::resolve(config, env).map_err(|error| match error {
            ResolveError::InvalidDiskAccessKey => StorageError::InvalidOptions(format!(
                "storage.disks.{name}.access_key is not a base64 account key"
            )),
            ResolveError::InvalidEnvAccessKey(var) => StorageError::Backend {
                disk: name.to_owned(),
                source: Box::new(Error::from(ResolveError::InvalidEnvAccessKey(var))),
            },
            ResolveError::Cloud(error) => StorageError::Backend {
                disk: name.to_owned(),
                source: Box::new(error),
            },
        })
    }

    fn resolve(
        config: &AzureConfig,
        env: &BTreeMap<String, String>,
    ) -> std::result::Result<Self, ResolveError> {
        let resolved = credentials::resolve(config, env)?;
        let clock: Clock = Arc::new(SystemTime::now);
        let policy = RetryPolicy::default();
        let shared_key_sas = resolved
            .auth
            .shared_key()
            .map(|key| SharedKeySas::from_bytes(&resolved.account, key.key_bytes().to_vec()))
            .transpose()
            .map_err(|_| ResolveError::InvalidDiskAccessKey)?;
        let credentials = resolved
            .auth
            .clone()
            .into_provider(clock.clone(), policy.clone());

        Ok(Self {
            inner: Arc::new(AzureInner {
                account: resolved.account,
                container: resolved.container,
                service_url: resolved.service_url,
                container_url: resolved.container_url,
                client: resolved.client,
                auth_source: resolved.auth,
                credentials,
                shared_key_sas,
                delegation_keys: DelegationKeys::default(),
                policy,
                block_size: DEFAULT_BLOCK_SIZE,
                clock,
            }),
        })
    }

    /// Sets the block size threshold at which [`Store::begin_upload`]
    /// stages a block (`DEFAULT_BLOCK_SIZE` by default, capped at 4000 MiB).
    #[must_use]
    pub fn with_block_size(self, block_size: usize) -> Self {
        self.rebuild(
            self.inner.clock.clone(),
            self.inner.policy.clone(),
            block_size.clamp(1, MAX_BLOCK_SIZE),
        )
    }

    /// Sets the [`RetryPolicy`] used for storage, delegation-key and token
    /// requests.
    #[must_use]
    pub fn with_retry_policy(self, policy: RetryPolicy) -> Self {
        self.rebuild(self.inner.clock.clone(), policy, self.inner.block_size)
    }

    /// Sets the clock used for request dates, SAS windows and credential
    /// expiry checks.
    #[must_use]
    pub fn with_clock(self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.rebuild(
            Arc::new(clock),
            self.inner.policy.clone(),
            self.inner.block_size,
        )
    }

    fn rebuild(&self, clock: Clock, policy: RetryPolicy, block_size: usize) -> Self {
        let credentials = self
            .inner
            .auth_source
            .clone()
            .into_provider(clock.clone(), policy.clone());
        Self {
            inner: Arc::new(AzureInner {
                account: self.inner.account.clone(),
                container: self.inner.container.clone(),
                service_url: self.inner.service_url.clone(),
                container_url: self.inner.container_url.clone(),
                client: self.inner.client.clone(),
                auth_source: self.inner.auth_source.clone(),
                credentials,
                shared_key_sas: self.inner.shared_key_sas.clone(),
                delegation_keys: DelegationKeys::default(),
                policy,
                block_size,
                clock,
            }),
        }
    }

    /// The resolved Azure storage account name.
    pub fn account(&self) -> &str {
        &self.inner.account
    }

    /// The container name.
    pub fn container(&self) -> &str {
        &self.inner.container
    }

    /// Which way of signing in the credential chain selected.
    pub fn auth_kind(&self) -> AuthKind {
        self.inner.auth_source.kind()
    }
}

impl fmt::Display for AzureStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Azure({}/{})", self.inner.account, self.inner.container)
    }
}

/// Account, container and credential kind; never a secret.
impl fmt::Debug for AzureStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AzureStore")
            .field("account", &self.inner.account)
            .field("container", &self.inner.container)
            .field("auth_kind", &self.auth_kind())
            .field("block_size", &self.inner.block_size)
            .finish_non_exhaustive()
    }
}

impl Store for AzureStore {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let url = object_url(&self.inner.container_url, path)?;
            let inner = &self.inner;
            with_backoff(&inner.policy, Replay::Safe, || async {
                let cred = inner.credentials.credential().await?;
                let request = Request::new(Method::PUT, url.clone())
                    .header("x-ms-blob-type", "BlockBlob")?
                    .with_body(contents.clone());
                let request = inner.authorize(request, &cred.secret)?;
                inner.send_checked(request).await.map(drop)
            })
            .await
            .map_err(StoreError::from)
        })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        Box::pin(async move {
            let url = object_url(&self.inner.container_url, path)?;
            let upload = BlockUpload::new(self.inner.clone(), url, self.inner.block_size);
            Ok(Box::new(upload) as Box<dyn Upload>)
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        Box::pin(async move {
            let url = object_url(&self.inner.container_url, path)?;
            if let Some(ref r) = range
                && r.start >= r.end
            {
                self.head(path).await?;
                return Ok(Bytes::new());
            }
            let range_header = range
                .as_ref()
                .map(|r| format!("bytes={}-{}", r.start, r.end - 1));
            let inner = &self.inner;
            let bytes = with_backoff(&inner.policy, Replay::Safe, || {
                let range_header = range_header.clone();
                let span = range.clone();
                let url = url.clone();
                async move {
                    let cred = inner.credentials.credential().await?;
                    let mut request = Request::new(Method::GET, url);
                    if let Some(ref value) = range_header {
                        request = request.header("range", value)?;
                    }
                    let request = inner.authorize(request, &cred.secret)?;
                    let response = inner.client.send(request).await?;
                    if range_header.is_some()
                        && response.status() == StatusCode::RANGE_NOT_SATISFIABLE
                    {
                        ensure_blob_file(&response)?;
                        return Ok(Bytes::new());
                    }
                    let response = check_response(response).await?;
                    ensure_blob_file(&response)?;
                    match span {
                        Some(span) => response.ranged_bytes(span).await,
                        None => response.bytes().await,
                    }
                }
            })
            .await?;
            Ok(bytes)
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        Box::pin(async move {
            let url = object_url(&self.inner.container_url, path)?;
            let inner = &self.inner;
            let first = with_backoff(&inner.policy, Replay::Safe, || async {
                let cred = inner.credentials.credential().await?;
                let request = Request::new(Method::GET, url.clone());
                let request = inner.authorize(request, &cred.secret)?;
                let response = inner.send_checked(request).await?;
                ensure_blob_file(&response)?;
                Ok(response)
            })
            .await?;

            let size = first.declared_length().unwrap_or(0);
            let resume_inner = self.inner.clone();
            let resume_url = url;
            let stream = first.resuming_stream(&self.inner.policy, move |rest: HeaderMap| {
                let inner = resume_inner.clone();
                let url = resume_url.clone();
                async move {
                    let cred = inner.credentials.credential().await?;
                    let mut request = Request::new(Method::GET, url);
                    request.headers.extend(rest);
                    let request = inner.authorize(request, &cred.secret)?;
                    inner.client.send(request).await
                }
            });
            Ok(Object::new(
                size,
                Box::pin(stream.map_err(StoreError::from)),
            ))
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        Box::pin(async move {
            let url = object_url(&self.inner.container_url, path)?;
            let inner = &self.inner;
            let response = with_backoff(&inner.policy, Replay::Safe, || async {
                let cred = inner.credentials.credential().await?;
                let request = Request::new(Method::HEAD, url.clone());
                let request = inner.authorize(request, &cred.secret)?;
                let response = inner.send_checked(request).await?;
                ensure_blob_file(&response)?;
                Ok(response)
            })
            .await?;

            let size = response.declared_length().ok_or_else(|| {
                Error::new(
                    ErrorKind::Answer,
                    "the blob head answer has no Content-Length",
                )
            })?;
            let last_modified_raw = response.header("last-modified").ok_or_else(|| {
                Error::new(
                    ErrorKind::Answer,
                    "the blob head answer has no Last-Modified",
                )
            })?;
            let last_modified = parse_http_date(last_modified_raw)?;
            Ok(Meta {
                path: path.to_owned(),
                size,
                last_modified,
            })
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let url = object_url(&self.inner.container_url, path)?;
            let inner = &self.inner;
            with_backoff(&inner.policy, Replay::Safe, || async {
                let cred = inner.credentials.credential().await?;
                let request = Request::new(Method::DELETE, url.clone());
                let request = inner.authorize(request, &cred.secret)?;
                inner.send_checked(request).await.map(drop)
            })
            .await
            .map_err(StoreError::from)
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
        Box::pin(async move {
            check_prefix(prefix)?;
            let directory = if prefix.is_empty() {
                None
            } else if prefix.ends_with('/') {
                Some(prefix.to_owned())
            } else {
                Some(format!("{prefix}/"))
            };

            let mut files = Vec::new();
            let mut marker: Option<String> = None;
            let mut seen_markers = HashSet::new();
            let mut pages = 0usize;
            let xml_limit = xml::Limits::default().max_bytes;

            loop {
                pages += 1;
                if pages > MAX_LIST_PAGES {
                    return Err(Error::new(
                        ErrorKind::Answer,
                        "the blob listing exceeded the maximum page limit",
                    )
                    .into());
                }

                let mut query = String::from("restype=container&comp=list");
                if let Some(ref dir) = directory {
                    query.push_str("&prefix=");
                    query.push_str(&encode_component(dir));
                }
                if let Some(ref next) = marker {
                    query.push_str("&marker=");
                    query.push_str(&encode_component(next));
                }
                let mut url = self.inner.container_url.clone();
                url.set_query(Some(&query));

                let inner = &self.inner;
                let body = with_backoff(&inner.policy, Replay::Safe, || async {
                    let cred = inner.credentials.credential().await?;
                    let request = Request::new(Method::GET, url.clone());
                    let request = inner.authorize(request, &cred.secret)?;
                    let response = inner.send_checked(request).await?;
                    response.bytes_up_to(xml_limit).await
                })
                .await?;

                let root = xml::parse(&body)?;
                if root.name() != "EnumerationResults" {
                    return Err(Error::new(
                        ErrorKind::Answer,
                        "the blob listing does not have an EnumerationResults root",
                    )
                    .into());
                }

                if let Some(blobs) = root.child("Blobs") {
                    for blob in blobs.all("Blob") {
                        let Some(props) = blob.child("Properties") else {
                            return Err(Error::new(
                                ErrorKind::Answer,
                                "a listed blob has no Properties element",
                            )
                            .into());
                        };
                        if props
                            .value("ResourceType")
                            .is_some_and(|kind| kind.trim() == "directory")
                        {
                            continue;
                        }
                        let Some(name) = blob.value("Name") else {
                            return Err(Error::new(
                                ErrorKind::Answer,
                                "a listed blob has no Name element",
                            )
                            .into());
                        };
                        check_key(name)?;
                        if let Some(ref dir) = directory
                            && (!name.starts_with(dir.as_str()) || name.len() <= dir.len())
                        {
                            continue;
                        }
                        if files.len() >= MAX_LIST_ENTRIES {
                            return Err(Error::new(
                                ErrorKind::Answer,
                                "the blob listing exceeded the maximum object count",
                            )
                            .into());
                        }
                        let size = props
                            .value("Content-Length")
                            .and_then(|s| s.trim().parse::<u64>().ok())
                            .ok_or_else(|| {
                                Error::new(
                                    ErrorKind::Answer,
                                    "a listed blob has no valid Content-Length",
                                )
                            })?;
                        let last_modified = props
                            .value("Last-Modified")
                            .ok_or_else(|| {
                                Error::new(ErrorKind::Answer, "a listed blob has no Last-Modified")
                            })
                            .and_then(parse_http_date)?;
                        files.push(Meta {
                            path: name.to_owned(),
                            size,
                            last_modified,
                        });
                    }
                }

                match root.value("NextMarker").filter(|next| !next.is_empty()) {
                    Some(next) => {
                        if !seen_markers.insert(next.to_owned()) {
                            return Err(Error::new(
                                ErrorKind::Answer,
                                "the blob listing repeated a NextMarker",
                            )
                            .into());
                        }
                        marker = Some(next.to_owned());
                    }
                    None => break,
                }
            }

            Ok(files)
        })
    }

    fn signed_url_with<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        expires_in: Duration,
        headers: &'a [(HeaderName, HeaderValue)],
    ) -> BoxFuture<'a, StoreResult<Url>> {
        Box::pin(async move {
            let mut url = object_url(&self.inner.container_url, path)?;
            if let Some((name, _)) = headers.first() {
                return Err(StoreError::failed(format!(
                    "Azure SAS links cannot bind the header `{name}`"
                )));
            }
            let permissions = match method {
                Method::GET => "r",
                Method::PUT => "w",
                other => {
                    return Err(StoreError::failed(format!(
                        "Azure Blob Storage signs GET and PUT links, not {other}"
                    )));
                }
            };
            if expires_in < Duration::from_secs(1) {
                return Err(StoreError::failed(
                    "a signed link must stay valid for at least 1 second",
                ));
            }

            let raw_start: DateTime<Utc> = (self.inner.clock)().into();
            let start = DateTime::from_timestamp(raw_start.timestamp(), 0)
                .ok_or_else(|| StoreError::failed("the clock timestamp is out of range"))?;
            let delta = chrono::Duration::from_std(expires_in)
                .map_err(|_| StoreError::failed("the link would expire too far in the future"))?;
            let expiry = start
                .checked_add_signed(delta)
                .ok_or_else(|| StoreError::failed("the link would expire too far in the future"))?;
            if !(1970..=9999).contains(&start.year()) || !(1970..=9999).contains(&expiry.year()) {
                return Err(StoreError::failed(
                    "Azure SAS timestamps must fall within years 1970..=9999",
                ));
            }

            if let Some(ref shared_key) = self.inner.shared_key_sas {
                shared_key.sign(
                    &self.inner.container,
                    path,
                    &mut url,
                    permissions,
                    start,
                    expiry,
                );
                return Ok(url);
            }

            if expires_in > MAX_DELEGATION_TTL {
                return Err(StoreError::failed(
                    "Azure user delegation SAS links last at most 7 days",
                ));
            }
            let (delegation_key, effective_start) = self
                .inner
                .delegation_keys
                .key_for(&self.inner, start, expiry, expires_in)
                .await?;
            delegation_key.sign(
                &self.inner.account,
                &self.inner.container,
                path,
                &mut url,
                permissions,
                effective_start,
                expiry,
            );
            Ok(url)
        })
    }
}

/// Hierarchical namespace accounts report directories with
/// `x-ms-resource-type: directory`; only files are objects.
fn ensure_blob_file(response: &Response) -> Result<()> {
    if response
        .header("x-ms-resource-type")
        .is_some_and(|kind| kind.trim() != "file")
    {
        return Err(Error::answered(StatusCode::NOT_FOUND, None));
    }
    Ok(())
}

fn parse_http_date(text: &str) -> Result<DateTime<Utc>> {
    let trimmed = text.trim();
    DateTime::parse_from_rfc2822(trimmed)
        .or_else(|_| DateTime::parse_from_rfc3339(trimmed))
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| Error::new(ErrorKind::Answer, "the blob Last-Modified date is invalid"))
}
