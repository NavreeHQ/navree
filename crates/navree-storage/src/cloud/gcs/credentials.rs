//! Google Cloud Storage credentials: service account JSON (inline or file),
//! Application Default Credentials (`service_account` or `authorized_user`),
//! the Compute Engine / GKE / Cloud Run metadata server, and IAM `signBlob`.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ::http::Method;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use url::Url;

use super::token::{
    DEFAULT_SCOPE, DEFAULT_TOKEN_URI, RsaSigningKey, attach_error_code,
    credential_token_client_and_url, exchange_jwt, exchange_refresh_token,
    override_token_client_and_url, parse_token_answer,
};
use crate::GcsConfig;
use crate::cloud::credentials::{Credential, Provider, Source};
use crate::cloud::http::{Client, Request};
use crate::cloud::retry::{Replay, RetryPolicy, with_backoff};
use crate::cloud::{Error, ErrorKind, Result};

/// The default instance metadata server of Compute Engine, GKE and Cloud Run.
pub const DEFAULT_METADATA_ENDPOINT: &str = "http://metadata.google.internal";

/// The default IAM Service Account Credentials API endpoint for `signBlob`.
pub const DEFAULT_IAM_ENDPOINT: &str = "https://iamcredentials.googleapis.com";

const METADATA_TOKEN_PATH: &str = "/computeMetadata/v1/instance/service-accounts/default/token";
const METADATA_EMAIL_PATH: &str = "/computeMetadata/v1/instance/service-accounts/default/email";
const METADATA_TOKEN_LIMIT: usize = 64 * 1024;
const METADATA_EMAIL_LIMIT: usize = 1024;
const SIGN_BLOB_ANSWER_LIMIT: usize = 64 * 1024;
const CREDENTIAL_FILE_LIMIT: usize = 1024 * 1024;

pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// Optional endpoint and scope overrides set through [`super::GcsStoreBuilder`].
#[derive(Clone, Default)]
pub(crate) struct CredentialOverrides {
    pub(crate) metadata_endpoint: Option<String>,
    pub(crate) iam_endpoint: Option<String>,
    pub(crate) token_endpoint: Option<String>,
    pub(crate) scopes: Vec<String>,
}

/// The resolved token provider and V4 URL signer of a [`super::GcsStore`].
pub(crate) struct ResolvedCredentials {
    pub(crate) tokens: Arc<Provider<String>>,
    pub(crate) signer: UrlSigner,
}

/// How V4 signed URLs obtain their RSA signature and service account email.
pub(crate) enum UrlSigner {
    /// Signs locally with the service account's private key.
    LocalKey {
        client_email: String,
        key: RsaSigningKey,
    },
    /// Signs through IAM Credentials `signBlob` for the metadata server's
    /// default service account.
    SignBlob {
        metadata_client: Client,
        metadata_base: Url,
        metadata_retry: RetryPolicy,
        email: RwLock<Option<String>>,
        iam_client: Client,
        iam_base: Url,
        iam_retry: RetryPolicy,
    },
    /// Credentials that carry no service account (such as `authorized_user`).
    Unsupported(&'static str),
}

impl UrlSigner {
    /// The `client_email` of the service account signing the URL.
    pub(crate) async fn signer_email(&self) -> Result<String> {
        match self {
            Self::LocalKey { client_email, .. } => Ok(client_email.clone()),
            Self::SignBlob {
                metadata_client,
                metadata_base,
                metadata_retry,
                email,
                ..
            } => {
                if let Some(cached) = email.read().await.as_ref() {
                    return Ok(cached.clone());
                }
                let mut guard = email.write().await;
                if let Some(cached) = guard.as_ref() {
                    return Ok(cached.clone());
                }
                let fetched =
                    fetch_metadata_email(metadata_client, metadata_base, metadata_retry).await?;
                *guard = Some(fetched.clone());
                Ok(fetched)
            }
            Self::Unsupported(reason) => Err(Error::new(ErrorKind::Credential, *reason)),
        }
    }

    /// Signs `message` locally with the service account key or remotely via
    /// IAM `signBlob`.
    pub(crate) async fn sign(&self, tokens: &Provider<String>, message: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::LocalKey { key, .. } => key.sign(message),
            Self::SignBlob {
                iam_client,
                iam_base,
                iam_retry,
                ..
            } => {
                let account_email = self.signer_email().await?;
                sign_blob(
                    iam_client,
                    iam_base,
                    iam_retry,
                    tokens,
                    &account_email,
                    message,
                )
                .await
            }
            Self::Unsupported(reason) => Err(Error::new(ErrorKind::Credential, *reason)),
        }
    }
}

/// Resolves credentials in the disk's documented precedence order:
///
/// 1. `config.service_account_path` or `config.service_account_key`;
/// 2. `GOOGLE_SERVICE_ACCOUNT_KEY`;
/// 3. `GOOGLE_SERVICE_ACCOUNT` or `GOOGLE_SERVICE_ACCOUNT_PATH`;
/// 4. `GOOGLE_APPLICATION_CREDENTIALS` (`service_account` or
///    `authorized_user`);
/// 5. the instance metadata server (`GCE_METADATA_HOST`, else
///    `http://metadata.google.internal`).
pub(crate) fn resolve(
    config: &GcsConfig,
    env: &BTreeMap<String, String>,
    overrides: &CredentialOverrides,
    allow_loopback_token_http: bool,
    retry: &RetryPolicy,
    clock: Clock,
) -> Result<ResolvedCredentials> {
    let scopes = if overrides.scopes.is_empty() {
        vec![DEFAULT_SCOPE.to_owned()]
    } else {
        overrides.scopes.clone()
    };
    let env_value = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| env.get(*key).filter(|val| !val.trim().is_empty()))
    };
    let token_override = overrides.token_endpoint.as_deref();

    match (&config.service_account_path, &config.service_account_key) {
        (Some(_), Some(_)) => Err(Error::new(
            ErrorKind::Credential,
            "set service_account_path or service_account_key, not both",
        )),
        (Some(path), None) => {
            let contents = read_credential_file(path, "service account key file")?;
            from_service_account_json(
                &contents,
                scopes,
                token_override,
                allow_loopback_token_http,
                retry,
                clock,
            )
        }
        (None, Some(key)) => from_service_account_json(
            key.expose(),
            scopes,
            token_override,
            allow_loopback_token_http,
            retry,
            clock,
        ),
        (None, None) => {
            if let Some(inline) = env_value(&["GOOGLE_SERVICE_ACCOUNT_KEY"]) {
                from_service_account_json(
                    inline,
                    scopes,
                    token_override,
                    allow_loopback_token_http,
                    retry,
                    clock,
                )
            } else if let Some(path) =
                env_value(&["GOOGLE_SERVICE_ACCOUNT", "GOOGLE_SERVICE_ACCOUNT_PATH"])
            {
                let contents = read_credential_file(Path::new(path), "service account key file")?;
                from_service_account_json(
                    &contents,
                    scopes,
                    token_override,
                    allow_loopback_token_http,
                    retry,
                    clock,
                )
            } else if let Some(path) = env_value(&["GOOGLE_APPLICATION_CREDENTIALS"]) {
                let contents =
                    read_credential_file(Path::new(path), "application credentials file")?;
                from_application_credentials_json(
                    &contents,
                    scopes,
                    token_override,
                    allow_loopback_token_http,
                    retry,
                    clock,
                )
            } else {
                from_metadata_server(env, overrides, retry, clock)
            }
        }
    }
}

fn read_credential_file(path: &Path, label: &'static str) -> Result<String> {
    let file = std::fs::File::open(path).map_err(|error| {
        Error::new(
            ErrorKind::Credential,
            format!("could not read the {label}: {}", error.kind()),
        )
    })?;
    let mut raw = Vec::new();
    file.take((CREDENTIAL_FILE_LIMIT + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|error| {
            Error::new(
                ErrorKind::Credential,
                format!("could not read the {label}: {}", error.kind()),
            )
        })?;
    if raw.len() > CREDENTIAL_FILE_LIMIT {
        return Err(Error::new(
            ErrorKind::Credential,
            format!("the {label} is larger than {CREDENTIAL_FILE_LIMIT} bytes"),
        ));
    }
    String::from_utf8(raw).map_err(|_| {
        Error::new(
            ErrorKind::Credential,
            format!("the {label} is not valid UTF-8"),
        )
    })
}

#[derive(Deserialize)]
struct ServiceAccountJson {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    private_key: String,
    #[serde(default)]
    private_key_id: Option<String>,
    client_email: String,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Deserialize)]
struct AuthorizedUserJson {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Deserialize)]
struct CredentialKindProbe {
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

fn resolve_token_endpoint(
    json_token_uri: Option<String>,
    token_override: Option<&str>,
    allow_loopback_http: bool,
) -> Result<(Client, Url)> {
    if let Some(override_uri) = token_override {
        return override_token_client_and_url(override_uri);
    }
    let token_uri = json_token_uri
        .map(|uri| uri.trim().to_owned())
        .filter(|uri| !uri.is_empty())
        .unwrap_or_else(|| DEFAULT_TOKEN_URI.to_owned());
    credential_token_client_and_url(&token_uri, allow_loopback_http)
}

fn from_service_account_json(
    raw: &str,
    scopes: Vec<String>,
    token_override: Option<&str>,
    allow_loopback_http: bool,
    retry: &RetryPolicy,
    clock: Clock,
) -> Result<ResolvedCredentials> {
    let parsed: ServiceAccountJson = serde_json::from_str(raw).map_err(|_| {
        Error::new(
            ErrorKind::Credential,
            "the service account key is not valid JSON",
        )
    })?;
    if let Some(kind) = parsed.kind.as_deref()
        && kind != "service_account"
    {
        return Err(Error::new(
            ErrorKind::Credential,
            "the service account key does not have type `service_account`",
        ));
    }
    let email = parsed.client_email.trim().to_owned();
    if !is_safe_email(&email) {
        return Err(Error::new(
            ErrorKind::Credential,
            "the service account key has an invalid client_email",
        ));
    }
    let mut key = RsaSigningKey::from_pem(&parsed.private_key)?;
    if let Some(kid) = parsed.private_key_id {
        key = key.with_kid(kid);
    }
    let (token_client, token_url) =
        resolve_token_endpoint(parsed.token_uri, token_override, allow_loopback_http)?;

    let source = ServiceAccountSource {
        client: token_client,
        token_url,
        key: key.clone(),
        client_email: email.clone(),
        scopes,
        retry: retry.clone(),
        clock: clock.clone(),
    };
    let provider_clock = clock;
    let tokens = Arc::new(Provider::with_clock(source, move || (provider_clock)()));
    Ok(ResolvedCredentials {
        tokens,
        signer: UrlSigner::LocalKey {
            client_email: email,
            key,
        },
    })
}

fn from_application_credentials_json(
    raw: &str,
    scopes: Vec<String>,
    token_override: Option<&str>,
    allow_loopback_http: bool,
    retry: &RetryPolicy,
    clock: Clock,
) -> Result<ResolvedCredentials> {
    let probe: CredentialKindProbe = serde_json::from_str(raw).map_err(|_| {
        Error::new(
            ErrorKind::Credential,
            "the application credentials file is not valid JSON",
        )
    })?;
    match probe.kind.as_deref() {
        None | Some("service_account") => from_service_account_json(
            raw,
            scopes,
            token_override,
            allow_loopback_http,
            retry,
            clock,
        ),
        Some("authorized_user") => {
            from_authorized_user_json(raw, token_override, allow_loopback_http, retry, clock)
        }
        Some(_) => Err(Error::new(
            ErrorKind::Credential,
            "unsupported credential type in GOOGLE_APPLICATION_CREDENTIALS",
        )),
    }
}

fn from_authorized_user_json(
    raw: &str,
    token_override: Option<&str>,
    allow_loopback_http: bool,
    retry: &RetryPolicy,
    clock: Clock,
) -> Result<ResolvedCredentials> {
    let parsed: AuthorizedUserJson = serde_json::from_str(raw).map_err(|_| {
        Error::new(
            ErrorKind::Credential,
            "the authorized_user credentials file is not valid JSON",
        )
    })?;
    if parsed.client_id.trim().is_empty()
        || parsed.client_secret.trim().is_empty()
        || parsed.refresh_token.trim().is_empty()
    {
        return Err(Error::new(
            ErrorKind::Credential,
            "the authorized_user credentials file is missing required fields",
        ));
    }
    let (token_client, token_url) =
        resolve_token_endpoint(parsed.token_uri, token_override, allow_loopback_http)?;

    let source = AuthorizedUserSource {
        client: token_client,
        token_url,
        client_id: parsed.client_id,
        client_secret: parsed.client_secret,
        refresh_token: parsed.refresh_token,
        retry: retry.clone(),
        clock: clock.clone(),
    };
    let provider_clock = clock;
    let tokens = Arc::new(Provider::with_clock(source, move || (provider_clock)()));
    Ok(ResolvedCredentials {
        tokens,
        signer: UrlSigner::Unsupported(
            "authorized_user credentials have no service account to sign links with",
        ),
    })
}

fn from_metadata_server(
    env: &BTreeMap<String, String>,
    overrides: &CredentialOverrides,
    retry: &RetryPolicy,
    clock: Clock,
) -> Result<ResolvedCredentials> {
    let raw_metadata = overrides
        .metadata_endpoint
        .clone()
        .or_else(|| {
            env.get("GCE_METADATA_HOST")
                .filter(|val| !val.trim().is_empty())
                .map(|host| {
                    let trimmed = host.trim();
                    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                        trimmed.to_owned()
                    } else {
                        format!("http://{trimmed}")
                    }
                })
        })
        .unwrap_or_else(|| DEFAULT_METADATA_ENDPOINT.to_owned());

    let metadata_base = parse_clean_base_url(
        &raw_metadata,
        "the metadata server endpoint must be an http:// or https:// URL",
    )?;
    let metadata_client = Client::for_metadata()?;

    let (iam_client, iam_base) = match &overrides.iam_endpoint {
        Some(iam) => {
            let base = parse_clean_base_url(
                iam,
                "the IAM credentials endpoint must be an http:// or https:// URL",
            )?;
            let client = Client::for_endpoint(Some(iam))?;
            (client, base)
        }
        None => {
            let base = parse_clean_base_url(
                DEFAULT_IAM_ENDPOINT,
                "the IAM credentials endpoint must be an https:// URL",
            )?;
            let client = Client::for_endpoint(None)?;
            (client, base)
        }
    };

    // Keep metadata retries brief so a missing metadata server fails fast.
    let metadata_retry = RetryPolicy {
        max_retries: retry.max_retries.min(2),
        first_delay: retry.first_delay.min(Duration::from_millis(50)),
        max_delay: retry.max_delay.min(Duration::from_millis(250)),
        retry_timeout: retry.retry_timeout.min(Duration::from_secs(5)),
        factor: retry.factor,
    };

    let source = MetadataTokenSource {
        client: metadata_client.clone(),
        token_url: join_base_path(&metadata_base, METADATA_TOKEN_PATH)?,
        retry: metadata_retry.clone(),
        clock: clock.clone(),
    };
    let provider_clock = clock;
    let tokens = Arc::new(Provider::with_clock(source, move || (provider_clock)()));
    Ok(ResolvedCredentials {
        tokens,
        signer: UrlSigner::SignBlob {
            metadata_client,
            metadata_base,
            metadata_retry,
            email: RwLock::new(None),
            iam_client,
            iam_base,
            iam_retry: retry.clone(),
        },
    })
}

struct ServiceAccountSource {
    client: Client,
    token_url: Url,
    key: RsaSigningKey,
    client_email: String,
    scopes: Vec<String>,
    retry: RetryPolicy,
    clock: Clock,
}

impl Source<String> for ServiceAccountSource {
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<String>>> {
        Box::pin(async move {
            exchange_jwt(
                &self.client,
                &self.token_url,
                &self.retry,
                &self.key,
                &self.client_email,
                &self.scopes,
                &*self.clock,
            )
            .await
        })
    }
}

struct AuthorizedUserSource {
    client: Client,
    token_url: Url,
    client_id: String,
    client_secret: String,
    refresh_token: String,
    retry: RetryPolicy,
    clock: Clock,
}

impl Source<String> for AuthorizedUserSource {
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<String>>> {
        Box::pin(async move {
            exchange_refresh_token(
                &self.client,
                &self.token_url,
                &self.retry,
                &self.client_id,
                &self.client_secret,
                &self.refresh_token,
                &*self.clock,
            )
            .await
        })
    }
}

struct MetadataTokenSource {
    client: Client,
    token_url: Url,
    retry: RetryPolicy,
    clock: Clock,
}

impl Source<String> for MetadataTokenSource {
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<String>>> {
        Box::pin(async move {
            with_backoff(&self.retry, Replay::Safe, || async {
                let request = Request::new(Method::GET, self.token_url.clone())
                    .header("metadata-flavor", "Google")?;
                let answer = self
                    .client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(attach_error_code)?;
                let body = answer.bytes_up_to(METADATA_TOKEN_LIMIT).await?;
                parse_token_answer(&body, (self.clock)())
            })
            .await
        })
    }
}

async fn fetch_metadata_email(
    client: &Client,
    metadata_base: &Url,
    retry: &RetryPolicy,
) -> Result<String> {
    let url = join_base_path(metadata_base, METADATA_EMAIL_PATH)?;
    let answer = with_backoff(retry, Replay::Safe, || async {
        let request = Request::new(Method::GET, url.clone()).header("metadata-flavor", "Google")?;
        client
            .send(request)
            .await?
            .success()
            .await
            .map_err(attach_error_code)
    })
    .await?;
    let bytes = answer.bytes_up_to(METADATA_EMAIL_LIMIT).await?;
    let email = std::str::from_utf8(&bytes)
        .map_err(|_| {
            Error::new(
                ErrorKind::Answer,
                "the metadata server returned a non-UTF-8 email",
            )
        })?
        .trim()
        .to_owned();
    if !is_safe_email(&email) {
        return Err(Error::new(
            ErrorKind::Answer,
            "the metadata server returned an invalid service account email",
        ));
    }
    Ok(email)
}

#[derive(Serialize)]
struct SignBlobRequest {
    payload: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignBlobAnswer {
    signed_blob: String,
}

async fn sign_blob(
    iam_client: &Client,
    iam_base: &Url,
    retry: &RetryPolicy,
    tokens: &Provider<String>,
    email: &str,
    message: &[u8],
) -> Result<Vec<u8>> {
    if !is_safe_email(email) {
        return Err(Error::new(
            ErrorKind::Credential,
            "invalid service account email for signBlob",
        ));
    }
    let path = format!("/v1/projects/-/serviceAccounts/{email}:signBlob");
    let url = join_base_path(iam_base, &path)?;
    let body_json = serde_json::to_vec(&SignBlobRequest {
        payload: STANDARD.encode(message),
    })
    .map_err(|_| Error::new(ErrorKind::Other, "could not encode the signBlob request"))?;

    let answer = with_backoff(retry, Replay::Safe, || async {
        let token = tokens.credential().await?;
        let request = Request::new(Method::POST, url.clone())
            .secret("authorization", &format!("Bearer {}", token.secret))?
            .header("content-type", "application/json")?
            .with_body(body_json.clone());
        iam_client
            .send(request)
            .await?
            .success()
            .await
            .map_err(attach_error_code)
    })
    .await?;

    let bytes = answer.bytes_up_to(SIGN_BLOB_ANSWER_LIMIT).await?;
    let parsed: SignBlobAnswer = serde_json::from_slice(&bytes).map_err(|_| {
        Error::new(
            ErrorKind::Answer,
            "the IAM signBlob answer is not a valid JSON object",
        )
    })?;
    let trimmed = parsed.signed_blob.trim_ascii();
    let decoded = STANDARD
        .decode(trimmed)
        .or_else(|_| STANDARD_NO_PAD.decode(trimmed.trim_end_matches('=')))
        .map_err(|_| {
            Error::new(
                ErrorKind::Answer,
                "the IAM signBlob answer did not carry valid base64 bytes",
            )
        })?;
    if decoded.is_empty() {
        return Err(Error::new(
            ErrorKind::Answer,
            "the IAM signBlob answer carried an empty signature",
        ));
    }
    Ok(decoded)
}

/// Validates that `email` contains only safe ASCII service-account characters
/// and `@`, preventing path traversal or URL injection when interpolated into
/// `/v1/projects/-/serviceAccounts/{email}:signBlob`.
pub(crate) fn is_safe_email(email: &str) -> bool {
    !email.is_empty()
        && email.len() <= 254
        && email.contains('@')
        && !email.starts_with('@')
        && !email.ends_with('@')
        && email.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+' | b'@')
        })
}

pub(crate) fn parse_clean_base_url(raw: &str, reason: &'static str) -> Result<Url> {
    let refused = || Error::new(ErrorKind::Refused, reason);
    let url = Url::parse(raw.trim_end_matches('/')).map_err(|_| refused())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(refused());
    }
    Ok(url)
}

pub(crate) fn join_base_path(base: &Url, suffix_path: &str) -> Result<Url> {
    let prefix = base.path().trim_end_matches('/');
    let suffix = if suffix_path.starts_with('/') {
        suffix_path.to_owned()
    } else {
        format!("/{suffix_path}")
    };
    let full_path = format!("{prefix}{suffix}");
    let combined = format!("{}{full_path}", base.origin().ascii_serialization());
    let url = Url::parse(&combined)
        .map_err(|_| Error::new(ErrorKind::Refused, "could not build endpoint URL"))?;
    if url.origin() != base.origin() || url.path() != full_path {
        return Err(Error::new(
            ErrorKind::Refused,
            "endpoint path escaped its origin",
        ));
    }
    Ok(url)
}
