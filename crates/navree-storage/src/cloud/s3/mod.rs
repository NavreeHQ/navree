//! [`S3Store`]: Navree's S3 and S3-compatible object store (AWS S3, MinIO,
//! Cloudflare R2, DigitalOcean Spaces, Ceph and RustFS).

pub mod credentials;
pub mod list;
pub mod multipart;
pub mod sigv4;

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ::http::header::{HeaderName, HeaderValue};
use ::http::{Method, StatusCode};
use bytes::Bytes;
use chrono::{DateTime, NaiveDateTime, Utc};
use futures_util::TryStreamExt;
use navree_kernel::BoxFuture;
use url::Url;

pub use crate::S3Config;

use self::multipart::DEFAULT_PART_SIZE;
use self::sigv4::Credentials;
use crate::cloud::credentials::Provider;
use crate::cloud::http::{Client, Request, redacted};
use crate::cloud::keys::{encode_component, object_url};
use crate::cloud::retry::{Replay, RetryPolicy, with_backoff};
use crate::cloud::{Error, ErrorKind, Result, xml};
use crate::store::{Meta, Object, Store, StoreError, StoreResult, Upload};

type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// An S3 or S3-compatible bucket implementing [`Store`].
#[derive(Clone)]
pub struct S3Store {
    bucket: String,
    region: String,
    bucket_url: Url,
    client: Client,
    credentials: Arc<Provider<Credentials>>,
    retry: RetryPolicy,
    part_size: usize,
    clock: Clock,
}

impl S3Store {
    /// Builds a store for `config`, reading fallback settings and
    /// credentials from the process environment.
    pub fn new(config: S3Config) -> Result<Self> {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        Self::with_env(&config, &env)
    }

    /// Builds a store for `config`, reading fallback settings and
    /// credentials from `env`.
    pub fn with_env(config: &S3Config, env: &BTreeMap<String, String>) -> Result<Self> {
        Self::with_env_and_clock(config, env, SystemTime::now)
    }

    /// Like [`with_env`](Self::with_env), reading the current time from
    /// `clock` for both credential refreshes and request signing.
    pub fn with_env_and_clock(
        config: &S3Config,
        env: &BTreeMap<String, String>,
        clock: impl Fn() -> SystemTime + Send + Sync + 'static,
    ) -> Result<Self> {
        check_bucket_name(&config.bucket)?;
        let resolved = resolve_address(config, env)?;
        let client = Client::for_endpoint(resolved.client_endpoint.as_deref())?;
        let retry = RetryPolicy::default();
        let shared_clock: Clock = Arc::new(clock);
        let provider_clock = shared_clock.clone();
        let credentials =
            credentials::build_provider(config, &resolved.region, env, retry.clone(), move || {
                provider_clock()
            })?;
        Ok(Self {
            bucket: config.bucket.clone(),
            region: resolved.region,
            bucket_url: resolved.bucket_url,
            client,
            credentials: Arc::new(credentials),
            retry,
            part_size: DEFAULT_PART_SIZE,
            clock: shared_clock,
        })
    }

    /// Replaces the credential [`Provider`].
    #[must_use]
    pub fn with_credentials(mut self, credentials: Provider<Credentials>) -> Self {
        self.credentials = Arc::new(credentials);
        self
    }

    /// Replaces the [`RetryPolicy`] used by requests and multipart uploads.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// Sets the multipart upload part threshold in bytes (default 5 MiB).
    #[must_use]
    pub fn with_part_size(mut self, bytes: usize) -> Self {
        self.part_size = bytes.max(1);
        self
    }

    /// Current UTC timestamp from the store's clock.
    pub(super) fn now(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from((self.clock)())
    }
}

impl fmt::Display for S3Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "S3({})", self.bucket)
    }
}

impl fmt::Debug for S3Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Store")
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("bucket_url", &redacted(&self.bucket_url))
            .field("credentials", &self.credentials)
            .field("part_size", &self.part_size)
            .finish_non_exhaustive()
    }
}

impl Store for S3Store {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let url = object_url(&self.bucket_url, path)?;
            with_backoff(&self.retry, Replay::Safe, || {
                let url = url.clone();
                let contents = contents.clone();
                async move {
                    let credential = self.credentials.credential().await?;
                    let mut request = Request::new(Method::PUT, url).with_body(contents);
                    sigv4::sign_request(
                        &mut request,
                        &credential.secret,
                        &self.region,
                        self.now(),
                    )?;
                    self.client
                        .send(request)
                        .await?
                        .success()
                        .await
                        .map(drop)
                        .map_err(enrich_error)
                }
            })
            .await
            .map_err(StoreError::from)
        })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        let store = self.clone();
        Box::pin(async move {
            multipart::begin(store, path)
                .await
                .map_err(StoreError::from)
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        Box::pin(async move {
            let url = object_url(&self.bucket_url, path)?;
            if let Some(ref r) = range
                && r.start >= r.end
            {
                self.head(path).await?;
                return Ok(Bytes::new());
            }
            let range_header = range
                .as_ref()
                .map(|r| format!("bytes={}-{}", r.start, r.end - 1));

            let response = with_backoff(&self.retry, Replay::Safe, || {
                let url = url.clone();
                let range_header = range_header.clone();
                async move {
                    let credential = self.credentials.credential().await?;
                    let mut request = Request::new(Method::GET, url);
                    if let Some(ref value) = range_header {
                        request = request.header("range", value)?;
                    }
                    sigv4::sign_request(
                        &mut request,
                        &credential.secret,
                        &self.region,
                        self.now(),
                    )?;
                    let answer = self.client.send(request).await?;
                    if range_header.is_some()
                        && answer.status() == StatusCode::RANGE_NOT_SATISFIABLE
                    {
                        return Ok(None);
                    }
                    answer.success().await.map(Some).map_err(enrich_error)
                }
            })
            .await
            .map_err(StoreError::from)?;

            let Some(answer) = response else {
                // Range starts at or past EOF: verify the object exists, then
                // cut short to empty bytes as Store::read promises.
                self.head(path).await?;
                return Ok(Bytes::new());
            };
            answer.bytes().await.map_err(StoreError::from)
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        Box::pin(async move {
            let url = object_url(&self.bucket_url, path)?;
            let first = with_backoff(&self.retry, Replay::Safe, || {
                let url = url.clone();
                async move {
                    let credential = self.credentials.credential().await?;
                    let mut request = Request::new(Method::GET, url);
                    sigv4::sign_request(
                        &mut request,
                        &credential.secret,
                        &self.region,
                        self.now(),
                    )?;
                    self.client
                        .send(request)
                        .await?
                        .success()
                        .await
                        .map_err(enrich_error)
                }
            })
            .await
            .map_err(StoreError::from)?;

            let size = first.declared_length().unwrap_or(0);
            let store = self.clone();
            let body = first.resuming_stream(&self.retry, move |rest_headers| {
                let store = store.clone();
                let url = url.clone();
                async move {
                    let credential = store.credentials.credential().await?;
                    let mut request = Request::new(Method::GET, url);
                    request.headers.extend(rest_headers);
                    sigv4::sign_request(
                        &mut request,
                        &credential.secret,
                        &store.region,
                        store.now(),
                    )?;
                    store.client.send(request).await
                }
            });
            Ok(Object::new(
                size,
                Box::pin(body.map_err(|err| StoreError::from(enrich_error(err)))),
            ))
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        Box::pin(async move {
            let url = object_url(&self.bucket_url, path)?;
            let answer = with_backoff(&self.retry, Replay::Safe, || {
                let url = url.clone();
                async move {
                    let credential = self.credentials.credential().await?;
                    let mut request = Request::new(Method::HEAD, url);
                    sigv4::sign_request(
                        &mut request,
                        &credential.secret,
                        &self.region,
                        self.now(),
                    )?;
                    self.client
                        .send(request)
                        .await?
                        .success()
                        .await
                        .map_err(enrich_error)
                }
            })
            .await
            .map_err(StoreError::from)?;

            let size = answer.declared_length().unwrap_or(0);
            let last_modified = match answer.header("last-modified") {
                Some(header) => parse_http_date(header).map_err(StoreError::from)?,
                None => DateTime::UNIX_EPOCH,
            };
            Ok(Meta {
                path: path.to_owned(),
                size,
                last_modified,
            })
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            // S3 answers DELETE with 204 even when the key was never there;
            // checking HEAD first keeps Store::delete's NotFound contract.
            let url = object_url(&self.bucket_url, path)?;
            self.head(path).await?;
            with_backoff(&self.retry, Replay::Safe, || {
                let url = url.clone();
                async move {
                    let credential = self.credentials.credential().await?;
                    let mut request = Request::new(Method::DELETE, url);
                    sigv4::sign_request(
                        &mut request,
                        &credential.secret,
                        &self.region,
                        self.now(),
                    )?;
                    self.client
                        .send(request)
                        .await?
                        .success()
                        .await
                        .map(drop)
                        .map_err(enrich_error)
                }
            })
            .await
            .map_err(StoreError::from)
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
        Box::pin(async move {
            list::list_objects(self, prefix)
                .await
                .map_err(StoreError::from)
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
            let url = object_url(&self.bucket_url, path)?;
            let credential = self.credentials.credential().await?;
            sigv4::presign_url(
                &method,
                &url,
                headers,
                expires_in,
                &credential.secret,
                &self.region,
                self.now(),
            )
            .map_err(StoreError::from)
        })
    }
}

struct ResolvedAddress {
    region: String,
    bucket_url: Url,
    client_endpoint: Option<String>,
}

fn check_bucket_name(bucket: &str) -> Result<()> {
    if bucket.is_empty()
        || bucket.len() > 255
        || bucket == "."
        || bucket == ".."
        || !bucket
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(Error::new(
            ErrorKind::Refused,
            "the S3 bucket name is missing or invalid",
        ));
    }
    Ok(())
}

fn check_region(region: &str) -> Result<()> {
    if region.is_empty()
        || region.len() > 64
        || !region
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(Error::new(
            ErrorKind::Refused,
            "the S3 region name is invalid",
        ));
    }
    Ok(())
}

fn resolve_address(config: &S3Config, env: &BTreeMap<String, String>) -> Result<ResolvedAddress> {
    let from_env = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| env.get(*key).filter(|value| !value.trim().is_empty()))
            .map(|value| value.trim().to_owned())
    };
    let region = config
        .region
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| from_env(&["AWS_REGION", "AWS_DEFAULT_REGION"]))
        .unwrap_or_else(|| "us-east-1".to_owned());
    check_region(&region)?;

    let raw_endpoint = config
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| from_env(&["AWS_ENDPOINT_URL_S3", "AWS_ENDPOINT_URL", "AWS_ENDPOINT"]));
    let path_style = config.path_style.unwrap_or(raw_endpoint.is_some());

    let refused = || {
        Error::new(
            ErrorKind::Refused,
            "an S3 endpoint must be an absolute http or https URL",
        )
    };

    match raw_endpoint {
        None => {
            let url_str = if path_style {
                format!(
                    "https://s3.{region}.amazonaws.com/{}",
                    encode_component(&config.bucket)
                )
            } else {
                format!("https://{}.s3.{region}.amazonaws.com", config.bucket)
            };
            let bucket_url = Url::parse(&url_str).map_err(|_| refused())?;
            Ok(ResolvedAddress {
                region,
                bucket_url,
                client_endpoint: None,
            })
        }
        Some(endpoint) => {
            let trimmed = endpoint.trim_end_matches('/');
            let parsed = Url::parse(trimmed).map_err(|_| refused())?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(refused());
            }
            if path_style {
                let base_path = parsed.path().trim_end_matches('/');
                let full = format!(
                    "{}{base_path}/{}",
                    parsed.origin().ascii_serialization(),
                    encode_component(&config.bucket)
                );
                let bucket_url = Url::parse(&full).map_err(|_| refused())?;
                let client_endpoint = Some(parsed.origin().ascii_serialization());
                Ok(ResolvedAddress {
                    region,
                    bucket_url,
                    client_endpoint,
                })
            } else {
                let Some((scheme, rest)) = trimmed.split_once("://") else {
                    return Err(refused());
                };
                let vhost = format!("{scheme}://{}.{rest}", config.bucket);
                let bucket_url = Url::parse(&vhost).map_err(|_| refused())?;
                if bucket_url.host_str().is_none() {
                    return Err(refused());
                }
                let client_endpoint = Some(bucket_url.origin().ascii_serialization());
                Ok(ResolvedAddress {
                    region,
                    bucket_url,
                    client_endpoint,
                })
            }
        }
    }
}

/// Parses an HTTP `Last-Modified` date (RFC 2822 / IMF-fixdate / RFC 3339).
fn parse_http_date(header: &str) -> Result<DateTime<Utc>> {
    let text = header.trim();
    if let Ok(dt) = DateTime::parse_from_rfc2822(text) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(text, "%a, %d %b %Y %H:%M:%S GMT") {
        return Ok(naive.and_utc());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
        return Ok(dt.with_timezone(&Utc));
    }
    Err(Error::new(
        ErrorKind::Answer,
        "the Last-Modified header could not be parsed",
    ))
}

/// Extracts the provider error `<Code>` from an XML error body, if any, and
/// attaches it to `error` without ever exposing `<Message>` or any signed
/// text.
pub(super) fn enrich_error(error: Error) -> Error {
    let Some(body) = error.body() else {
        return error;
    };
    let Ok(root) = xml::parse(body.as_bytes()) else {
        return error;
    };
    let code = root
        .value("Code")
        .or_else(|| root.find(&["Error", "Code"]).map(xml::Element::text))
        .map(str::trim)
        .filter(|code| !code.is_empty());
    match code {
        Some(code) => error.with_code(code),
        None => error,
    }
}
