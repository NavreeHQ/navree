//! [`GcsStore`]: Google Cloud Storage on the JSON API (`storage/v1`), with
//! service account keys, Application Default Credentials, the instance
//! metadata server, resumable multipart uploads and V4 signed URLs.

pub mod credentials;
pub mod signed_url;
pub mod token;
mod upload;

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ::http::{Method, StatusCode};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use navree_kernel::BoxFuture;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use url::Url;

pub use crate::GcsConfig;
use crate::cloud::credentials::Provider;
use crate::cloud::http::{Client, Request, redacted};
use crate::cloud::keys::{check_key, check_prefix, encode_component};
use crate::cloud::retry::{Replay, RetryPolicy, with_backoff};
use crate::cloud::{Error, ErrorKind, Result};
use crate::store::{HeaderName, HeaderValue, Meta, Object, Store, StoreError, StoreResult, Upload};
pub use upload::{DEFAULT_CHUNK_SIZE, UPLOAD_QUANTUM};

use self::credentials::{
    Clock, CredentialOverrides, UrlSigner, join_base_path, parse_clean_base_url,
};
use self::token::attach_error_code;
use self::upload::GcsUpload;

/// The default Google Cloud Storage endpoint.
pub const DEFAULT_ENDPOINT: &str = "https://storage.googleapis.com";

/// Maximum size of a single-object JSON metadata response (1 MiB).
pub(super) const OBJECT_JSON_LIMIT: usize = 1024 * 1024;

/// Maximum size of a single listing page JSON response (16 MiB).
const LIST_JSON_LIMIT: usize = 16 * 1024 * 1024;

/// Builds a [`GcsStore`] with optional overrides for environment variables,
/// credential endpoints, OAuth scopes, upload chunk size, retry policy and
/// clock.
pub struct GcsStoreBuilder {
    config: GcsConfig,
    env: Option<BTreeMap<String, String>>,
    overrides: CredentialOverrides,
    chunk_size: usize,
    retry: RetryPolicy,
    clock: Option<Clock>,
}

impl GcsStoreBuilder {
    /// Starts building a store for `config`.
    pub fn new(config: GcsConfig) -> Self {
        Self {
            config,
            env: None,
            overrides: CredentialOverrides::default(),
            chunk_size: DEFAULT_CHUNK_SIZE,
            retry: RetryPolicy::default(),
            clock: None,
        }
    }

    /// Reads fallback `GOOGLE_*` and `GCE_METADATA_HOST` variables from `env`
    /// rather than from the process environment.
    #[must_use]
    pub fn env<I, K, V>(mut self, env: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env = Some(env.into_iter().map(|(k, v)| (k.into(), v.into())).collect());
        self
    }

    /// Overrides the Compute Engine metadata server base URL.
    #[must_use]
    pub fn metadata_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.overrides.metadata_endpoint = Some(endpoint.into());
        self
    }

    /// Overrides the IAM Service Account Credentials API base URL (`signBlob`).
    #[must_use]
    pub fn iam_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.overrides.iam_endpoint = Some(endpoint.into());
        self
    }

    /// Overrides the OAuth 2.0 scopes requested on JWT assertions.
    #[must_use]
    pub fn scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.overrides.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the resumable upload part size (must be a non-zero multiple of
    /// [`UPLOAD_QUANTUM`], 256 KiB).
    #[must_use]
    pub fn chunk_size(mut self, bytes: usize) -> Self {
        self.chunk_size = bytes;
        self
    }

    /// Sets the retry policy for storage and credential requests.
    #[must_use]
    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// Reads current time from `clock` rather than [`SystemTime::now`].
    #[must_use]
    pub fn clock(mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.clock = Some(Arc::new(clock));
        self
    }

    /// Validates configuration and credentials and returns the [`GcsStore`].
    pub fn build(self) -> Result<GcsStore> {
        let bucket = self.config.bucket.trim();
        if bucket.is_empty() {
            return Err(Error::new(
                ErrorKind::Refused,
                "`bucket` is required for Google Cloud Storage",
            ));
        }
        check_key(bucket)?;
        if bucket.contains('/') {
            return Err(Error::new(
                ErrorKind::Refused,
                "a Google Cloud Storage bucket name cannot contain `/`",
            ));
        }
        if self.chunk_size == 0 || self.chunk_size % UPLOAD_QUANTUM != 0 {
            return Err(Error::new(
                ErrorKind::Refused,
                "the resumable upload chunk size must be a positive multiple of 256 KiB",
            ));
        }

        let endpoint = self
            .config
            .endpoint
            .as_deref()
            .map(|url| url.trim_end_matches('/'));
        let base_url = parse_clean_base_url(
            endpoint.unwrap_or(DEFAULT_ENDPOINT),
            "a Google Cloud Storage endpoint must be an absolute http:// or https:// URL",
        )?;
        let client = Client::for_endpoint(endpoint)?;

        let bucket_encoded = encode_component(bucket);
        let objects_base = join_base_path(&base_url, &format!("/storage/v1/b/{bucket_encoded}/o"))?;
        let upload_base = join_base_path(
            &base_url,
            &format!("/upload/storage/v1/b/{bucket_encoded}/o"),
        )?;
        let signed_bucket_base = join_base_path(&base_url, &format!("/{bucket_encoded}"))?;

        let clock: Clock = self.clock.unwrap_or_else(|| Arc::new(SystemTime::now));
        let env = match self.env {
            Some(env) => env,
            None if self.config.service_account_path.is_some()
                || self.config.service_account_key.is_some() =>
            {
                BTreeMap::new()
            }
            None => std::env::vars().collect(),
        };
        let resolved = credentials::resolve(
            &self.config,
            &env,
            &self.overrides,
            &self.retry,
            clock.clone(),
        )?;

        Ok(GcsStore {
            inner: Arc::new(Inner {
                bucket: bucket.to_owned(),
                base_url,
                objects_base,
                upload_base,
                signed_bucket_base,
                client,
                tokens: resolved.tokens,
                signer: resolved.signer,
                chunk_size: self.chunk_size,
                retry: self.retry,
                clock,
            }),
        })
    }
}

impl fmt::Debug for GcsStoreBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GcsStoreBuilder")
            .field("bucket", &self.config.bucket)
            .field("chunk_size", &self.chunk_size)
            .finish_non_exhaustive()
    }
}

/// A Google Cloud Storage bucket implementing [`Store`].
///
/// Cloning is cheap: the connection pool, cached tokens and signer are shared.
#[derive(Clone)]
pub struct GcsStore {
    inner: Arc<Inner>,
}

struct Inner {
    bucket: String,
    base_url: Url,
    objects_base: Url,
    upload_base: Url,
    signed_bucket_base: Url,
    client: Client,
    tokens: Arc<Provider<String>>,
    signer: UrlSigner,
    chunk_size: usize,
    retry: RetryPolicy,
    clock: Clock,
}

impl GcsStore {
    /// Builds a store for `config`, falling back to the process environment
    /// and the instance metadata server when `config` carries no key.
    pub fn new(config: GcsConfig) -> Result<Self> {
        GcsStoreBuilder::new(config).build()
    }

    /// Starts building a store for `config` with custom options.
    pub fn builder(config: GcsConfig) -> GcsStoreBuilder {
        GcsStoreBuilder::new(config)
    }

    /// Builds a store for `config` reading environment fallbacks from `env`.
    pub(crate) fn from_env(config: &GcsConfig, env: &BTreeMap<String, String>) -> Result<Self> {
        GcsStoreBuilder::new(config.clone())
            .env(env.clone())
            .build()
    }

    /// The URL of object `key` on the JSON API (`/storage/v1/b/{bucket}/o/{encoded}`).
    fn object_api_url(&self, key: &str) -> Result<Url> {
        check_key(key)?;
        let path = format!(
            "{}/{}",
            self.inner.objects_base.path(),
            encode_component(key)
        );
        let url = Url::parse(&format!(
            "{}{path}",
            self.inner.objects_base.origin().ascii_serialization()
        ))
        .map_err(|_| Error::new(ErrorKind::Key, "could not build object URL"))?;
        if url.origin() != self.inner.objects_base.origin() || url.path() != path {
            return Err(Error::new(
                ErrorKind::Key,
                "object key escaped its bucket path",
            ));
        }
        Ok(url)
    }

    /// The media download URL of `key` (`/storage/v1/b/{bucket}/o/{encoded}?alt=media`).
    fn media_url(&self, key: &str) -> Result<Url> {
        let mut url = self.object_api_url(key)?;
        url.set_query(Some("alt=media"));
        Ok(url)
    }
}

impl fmt::Display for GcsStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GoogleCloudStorage({})", self.inner.bucket)
    }
}

impl fmt::Debug for GcsStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GcsStore")
            .field("bucket", &self.inner.bucket)
            .field("endpoint", &redacted(&self.inner.base_url))
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct ObjectResource {
    #[serde(default)]
    name: Option<String>,
    #[serde(deserialize_with = "deserialize_u64")]
    size: u64,
    updated: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectList {
    #[serde(default)]
    items: Vec<ObjectResource>,
    #[serde(default)]
    next_page_token: Option<String>,
}

fn deserialize_u64<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrU64 {
        Text(String),
        Number(u64),
    }
    match StringOrU64::deserialize(deserializer)? {
        StringOrU64::Number(n) => Ok(n),
        StringOrU64::Text(text) => text
            .trim()
            .parse::<u64>()
            .map_err(|_| de::Error::custom("invalid object size")),
    }
}

fn to_meta(resource: ObjectResource, fallback_path: Option<&str>) -> Result<Meta> {
    let path = match resource.name {
        Some(name) if !name.is_empty() => name,
        _ => match fallback_path {
            Some(path) => path.to_owned(),
            None => {
                return Err(Error::new(ErrorKind::Answer, "a listed object has no name"));
            }
        },
    };
    let last_modified = DateTime::parse_from_rfc3339(&resource.updated)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| {
            Error::new(
                ErrorKind::Answer,
                "an object's `updated` timestamp is not RFC 3339",
            )
        })?;
    Ok(Meta {
        path,
        size: resource.size,
        last_modified,
    })
}

impl Store for GcsStore {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            check_key(path)?;
            let url = Url::parse(&format!(
                "{}?uploadType=media&name={}",
                self.inner.upload_base.as_str(),
                encode_component(path),
            ))
            .map_err(|_| StoreError::failed("could not build the upload URL"))?;

            let answer = with_backoff(&self.inner.retry, Replay::Safe, || async {
                let token = self.inner.tokens.credential().await?;
                let request = Request::new(Method::POST, url.clone())
                    .secret("authorization", &format!("Bearer {}", token.secret))?
                    .header("content-type", "application/octet-stream")?
                    .with_body(contents.clone());
                self.inner
                    .client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(attach_error_code)
            })
            .await?;

            let _ = answer.bytes_up_to(OBJECT_JSON_LIMIT).await?;
            Ok(())
        })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        Box::pin(async move {
            check_key(path)?;
            let upload = GcsUpload::start(
                self.inner.client.clone(),
                self.inner.tokens.clone(),
                self.inner.retry.clone(),
                &self.inner.base_url,
                &self.inner.upload_base,
                path,
                self.inner.chunk_size,
            )
            .await?;
            Ok(Box::new(upload) as Box<dyn Upload>)
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        Box::pin(async move {
            let url = self.media_url(path)?;
            if let Some(ref span) = range
                && span.start >= span.end
            {
                let _ = self.head(path).await?;
                return Ok(Bytes::new());
            }

            let range_header = range
                .as_ref()
                .map(|span| format!("bytes={}-{}", span.start, span.end - 1));

            let answer = with_backoff(&self.inner.retry, Replay::Safe, || async {
                let token = self.inner.tokens.credential().await?;
                let mut request = Request::new(Method::GET, url.clone())
                    .secret("authorization", &format!("Bearer {}", token.secret))?;
                if let Some(ref value) = range_header {
                    request = request.header("range", value)?;
                }
                let response = self.inner.client.send(request).await?;
                if range_header.is_some() && response.status() == StatusCode::RANGE_NOT_SATISFIABLE
                {
                    return Ok(None);
                }
                response
                    .success()
                    .await
                    .map(Some)
                    .map_err(attach_error_code)
            })
            .await?;

            let Some(answer) = answer else {
                return Ok(Bytes::new());
            };
            let status = answer.status();
            let body = answer.bytes().await?;
            // If an emulator ignored `Range` and answered `200 OK` with the
            // whole file, slice it locally so callers never receive extra bytes.
            if let Some(span) = range
                && status == StatusCode::OK
            {
                let end = usize::try_from(span.end).map_or(body.len(), |end| end.min(body.len()));
                let start = usize::try_from(span.start).map_or(end, |start| start.min(end));
                return Ok(body.slice(start..end));
            }
            Ok(body)
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        Box::pin(async move {
            let url = self.media_url(path)?;
            let first = with_backoff(&self.inner.retry, Replay::Safe, || async {
                let token = self.inner.tokens.credential().await?;
                let request = Request::new(Method::GET, url.clone())
                    .secret("authorization", &format!("Bearer {}", token.secret))?;
                self.inner
                    .client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(attach_error_code)
            })
            .await?;

            let declared = first.declared_length().or_else(|| {
                first
                    .header("x-goog-stored-content-length")
                    .and_then(|val| val.trim().parse::<u64>().ok())
            });
            let size = match declared {
                Some(size) => size,
                None => self.head(path).await?.size,
            };

            let client = self.inner.client.clone();
            let tokens = self.inner.tokens.clone();
            let stream = first.resuming_stream(&self.inner.retry, move |rest| {
                let client = client.clone();
                let tokens = tokens.clone();
                let url = url.clone();
                async move {
                    let token = tokens.credential().await?;
                    let mut request = Request::new(Method::GET, url)
                        .secret("authorization", &format!("Bearer {}", token.secret))?;
                    request.headers.extend(rest);
                    client.send(request).await
                }
            });

            Ok(Object::new(
                size,
                Box::pin(stream.map_err(|err| StoreError::from(attach_error_code(err)))),
            ))
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        Box::pin(async move {
            let url = self.object_api_url(path)?;
            let answer = with_backoff(&self.inner.retry, Replay::Safe, || async {
                let token = self.inner.tokens.credential().await?;
                let request = Request::new(Method::GET, url.clone())
                    .secret("authorization", &format!("Bearer {}", token.secret))?;
                self.inner
                    .client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(attach_error_code)
            })
            .await?;

            let body = answer.bytes_up_to(OBJECT_JSON_LIMIT).await?;
            let resource: ObjectResource = serde_json::from_slice(&body).map_err(|_| {
                Error::new(
                    ErrorKind::Answer,
                    "the object metadata answer is not valid JSON",
                )
            })?;
            Ok(to_meta(resource, Some(path))?)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let url = self.object_api_url(path)?;
            with_backoff(&self.inner.retry, Replay::Safe, || async {
                let token = self.inner.tokens.credential().await?;
                let request = Request::new(Method::DELETE, url.clone())
                    .secret("authorization", &format!("Bearer {}", token.secret))?;
                self.inner
                    .client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map(drop)
                    .map_err(attach_error_code)
            })
            .await?;
            Ok(())
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
            let mut page_token: Option<String> = None;

            loop {
                let mut query = Vec::new();
                if let Some(ref dir) = directory {
                    query.push(format!("prefix={}", encode_component(dir)));
                }
                if let Some(ref next) = page_token {
                    query.push(format!("pageToken={}", encode_component(next)));
                }
                let mut url = self.inner.objects_base.clone();
                if !query.is_empty() {
                    url.set_query(Some(&query.join("&")));
                }

                let answer = with_backoff(&self.inner.retry, Replay::Safe, || async {
                    let token = self.inner.tokens.credential().await?;
                    let request = Request::new(Method::GET, url.clone())
                        .secret("authorization", &format!("Bearer {}", token.secret))?;
                    self.inner
                        .client
                        .send(request)
                        .await?
                        .success()
                        .await
                        .map_err(attach_error_code)
                })
                .await?;

                let body = answer.bytes_up_to(LIST_JSON_LIMIT).await?;
                let page: ObjectList = serde_json::from_slice(&body).map_err(|_| {
                    Error::new(
                        ErrorKind::Answer,
                        "the bucket listing answer is not valid JSON",
                    )
                })?;

                for item in page.items {
                    let meta = to_meta(item, None)?;
                    // Keep defensive filtering even if an emulator ignores `prefix`.
                    if directory
                        .as_ref()
                        .is_none_or(|dir| meta.path.starts_with(dir))
                    {
                        files.push(meta);
                    }
                }

                match page.next_page_token.filter(|tok| !tok.is_empty()) {
                    Some(next) => {
                        if page_token.as_deref() == Some(next.as_str()) {
                            return Err(Error::new(
                                ErrorKind::Answer,
                                "the bucket listing repeated the same pageToken",
                            )
                            .into());
                        }
                        page_token = Some(next);
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
            check_key(path)?;
            let email = self.inner.signer.signer_email().await?;
            let prepared = signed_url::prepare(
                &self.inner.signed_bucket_base,
                path,
                &method,
                expires_in,
                headers,
                &email,
                (self.inner.clock)(),
            )?;
            let signature = self
                .inner
                .signer
                .sign(&self.inner.tokens, prepared.string_to_sign.as_bytes())
                .await?;
            Ok(prepared.finish(&signature)?)
        })
    }
}
