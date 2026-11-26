//! Azure Blob Storage account, endpoint and credential resolution.
//!
//! Resolves credentials in the order Azure disks have always used:
//!
//! 1. `config.access_key` (Shared Key);
//! 2. `AZURE_STORAGE_ACCOUNT_KEY` / `AZURE_STORAGE_ACCESS_KEY` /
//!    `AZURE_STORAGE_MASTER_KEY` (Shared Key);
//! 3. Service principal with a client secret (`AZURE_TENANT_ID`,
//!    `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`);
//! 4. Workload identity (`AZURE_TENANT_ID`, `AZURE_CLIENT_ID`,
//!    `AZURE_FEDERATED_TOKEN_FILE`, read fresh on every token refresh);
//! 5. Managed identity: App Service / Azure Functions / Container Apps
//!    (`IDENTITY_ENDPOINT` and `IDENTITY_HEADER`, or `MSI_ENDPOINT` and
//!    `MSI_SECRET`), else Azure VM / AKS IMDS (`169.254.169.254`).

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read as _;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ::http::Method;
use navree_datetime::Timestamp;
use serde::Deserialize;
use url::{Host, Url};

use super::shared_key::SharedKey;
use crate::cloud::credentials::{Credential, Provider};
use crate::cloud::http::{Client, Request};
use crate::cloud::keys::{check_key, encode_component};
use crate::cloud::retry::{Replay, RetryPolicy, with_backoff};
use crate::cloud::{Error, ErrorKind, Result};
use crate::options::AzureConfig;

const PUBLIC_AUTHORITY_HOST: &str = "https://login.microsoftonline.com";
const STORAGE_SCOPE: &str = "https://storage.azure.com/.default";
const STORAGE_RESOURCE: &str = "https://storage.azure.com/";
const DEFAULT_IMDS_ENDPOINT: &str = "http://169.254.169.254";
const APP_SERVICE_API_VERSION: &str = "2019-08-01";
const LEGACY_MSI_API_VERSION: &str = "2017-09-01";
const IMDS_API_VERSION: &str = "2018-02-01";
const JWT_BEARER_ASSERTION: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";
const TOKEN_BODY_LIMIT: usize = 64 * 1024;
/// Maximum `expires_in` lifetime accepted from a token service (one year).
const MAX_TOKEN_TTL_SECS: u64 = 365 * 24 * 60 * 60;
/// `9999-12-31T23:59:59Z` as Unix seconds, keeping `SystemTime` well within
/// both Unix `i64` and Windows `FILETIME` ranges.
const MAX_EPOCH_SECS: u64 = 253_402_300_799;

pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// What an Azure request is authorized with once [`Provider`] has resolved
/// it.
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    /// A storage account Shared Key.
    SharedKey(SharedKey),
    /// An OAuth 2.0 bearer access token.
    BearerToken(String),
}

/// Which kind of credential it is; never the key or token.
impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SharedKey(key) => f.debug_tuple("SharedKey").field(key).finish(),
            Self::BearerToken(_) => f.debug_tuple("BearerToken").finish_non_exhaustive(),
        }
    }
}

/// Which way of signing in the credential chain selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthKind {
    /// Storage account access key (Shared Key).
    SharedKey,
    /// Service principal with client secret (`client_credentials`).
    ClientSecret,
    /// Workload identity federation (`AZURE_FEDERATED_TOKEN_FILE`).
    WorkloadIdentity,
    /// Managed identity on App Service, Azure Functions or Container Apps.
    AppServiceIdentity,
    /// Managed identity through the instance metadata service (IMDS).
    ImdsIdentity,
}

/// Why resolving `AzureConfig` failed before a store could be built.
///
/// Never carries `base64::DecodeError`, whose display formats secret byte
/// values, offsets and input lengths.
pub(crate) enum ResolveError {
    /// `config.access_key` is empty or not valid base64.
    InvalidDiskAccessKey,
    /// An environment access key variable is empty or not valid base64.
    InvalidEnvAccessKey(&'static str),
    /// Another configuration or client error.
    Cloud(Error),
}

impl From<Error> for ResolveError {
    fn from(error: Error) -> Self {
        Self::Cloud(error)
    }
}

impl From<ResolveError> for Error {
    fn from(error: ResolveError) -> Self {
        match error {
            ResolveError::InvalidDiskAccessKey => Error::new(
                ErrorKind::Credential,
                "the configured Azure access_key is not a valid base64 account key",
            ),
            ResolveError::InvalidEnvAccessKey(var) => Error::new(
                ErrorKind::Credential,
                format!("{var} is not a valid base64 account key"),
            ),
            ResolveError::Cloud(error) => error,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum AppServiceHeader {
    /// Modern `IDENTITY_ENDPOINT` + `IDENTITY_HEADER` (`x-identity-header`).
    IdentityHeader,
    /// Legacy `MSI_ENDPOINT` + `MSI_SECRET` (`secret`).
    LegacySecret,
}

/// A resolved credential source that can build a [`Provider`] for any clock
/// and retry policy.
#[derive(Clone)]
pub(crate) enum AuthSource {
    SharedKey(SharedKey),
    ClientSecret {
        client: Client,
        token_url: Url,
        client_id: String,
        client_secret: String,
    },
    WorkloadIdentity {
        client: Client,
        token_url: Url,
        client_id: String,
        federated_token_file: String,
    },
    AppService {
        client: Client,
        url: Url,
        header_kind: AppServiceHeader,
        header_secret: String,
    },
    Imds {
        client: Client,
        url: Url,
    },
}

impl AuthSource {
    pub(crate) fn kind(&self) -> AuthKind {
        match self {
            Self::SharedKey(_) => AuthKind::SharedKey,
            Self::ClientSecret { .. } => AuthKind::ClientSecret,
            Self::WorkloadIdentity { .. } => AuthKind::WorkloadIdentity,
            Self::AppService { .. } => AuthKind::AppServiceIdentity,
            Self::Imds { .. } => AuthKind::ImdsIdentity,
        }
    }

    pub(crate) fn shared_key(&self) -> Option<&SharedKey> {
        match self {
            Self::SharedKey(key) => Some(key),
            _ => None,
        }
    }

    pub(crate) fn into_provider(self, clock: Clock, policy: RetryPolicy) -> Provider<Auth> {
        match self {
            Self::SharedKey(key) => Provider::fixed(Auth::SharedKey(key)),
            other => {
                let fetch_clock = clock.clone();
                Provider::with_clock(
                    move || {
                        let source = other.clone();
                        let clock = fetch_clock.clone();
                        let policy = policy.clone();
                        async move { source.fetch(&clock, &policy).await }
                    },
                    move || clock(),
                )
            }
        }
    }

    async fn fetch(&self, clock: &Clock, policy: &RetryPolicy) -> Result<Credential<Auth>> {
        match self {
            Self::SharedKey(key) => Ok(Credential::fixed(Auth::SharedKey(key.clone()))),
            Self::ClientSecret {
                client,
                token_url,
                client_id,
                client_secret,
            } => {
                let form = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("grant_type", "client_credentials")
                    .append_pair("client_id", client_id)
                    .append_pair("client_secret", client_secret)
                    .append_pair("scope", STORAGE_SCOPE)
                    .finish();
                fetch_token_post(client, token_url, form, clock, policy).await
            }
            Self::WorkloadIdentity {
                client,
                token_url,
                client_id,
                federated_token_file,
            } => {
                let assertion =
                    read_federated_token_file(PathBuf::from(federated_token_file)).await?;
                let form = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("grant_type", "client_credentials")
                    .append_pair("client_id", client_id)
                    .append_pair("client_assertion_type", JWT_BEARER_ASSERTION)
                    .append_pair("client_assertion", &assertion)
                    .append_pair("scope", STORAGE_SCOPE)
                    .finish();
                fetch_token_post(client, token_url, form, clock, policy).await
            }
            Self::AppService {
                client,
                url,
                header_kind,
                header_secret,
            } => {
                let header_name = match header_kind {
                    AppServiceHeader::IdentityHeader => "x-identity-header",
                    AppServiceHeader::LegacySecret => "secret",
                };
                let answer = with_backoff(policy, Replay::Safe, || async {
                    let request = Request::new(Method::GET, url.clone())
                        .header("accept", "application/json")?
                        .secret(header_name, header_secret)?;
                    let response = client.send(request).await?.success().await?;
                    response.bytes_up_to(TOKEN_BODY_LIMIT).await
                })
                .await
                .map_err(|error| token_request_error("managed identity", error))?;
                parse_token_answer(&answer, clock)
            }
            Self::Imds { client, url } => {
                let answer = with_backoff(policy, Replay::Safe, || async {
                    let request = Request::new(Method::GET, url.clone())
                        .header("accept", "application/json")?
                        .header("metadata", "true")?;
                    let response = client.send(request).await?.success().await?;
                    response.bytes_up_to(TOKEN_BODY_LIMIT).await
                })
                .await
                .map_err(|error| token_request_error("IMDS managed identity", error))?;
                parse_token_answer(&answer, clock)
            }
        }
    }
}

async fn read_federated_token_file(path: PathBuf) -> Result<String> {
    let raw = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.take((TOKEN_BODY_LIMIT + 1) as u64)
            .read_to_end(&mut buf)?;
        Ok(buf)
    })
    .await
    .map_err(|_| {
        Error::new(
            ErrorKind::Credential,
            "the Azure federated token file could not be read",
        )
    })?
    .map_err(|_| {
        Error::new(
            ErrorKind::Credential,
            "the Azure federated token file could not be read",
        )
    })?;
    if raw.len() > TOKEN_BODY_LIMIT {
        return Err(Error::new(
            ErrorKind::Credential,
            format!("the Azure federated token file is larger than {TOKEN_BODY_LIMIT} bytes"),
        ));
    }
    let text = std::str::from_utf8(&raw).map_err(|_| {
        Error::new(
            ErrorKind::Credential,
            "the Azure federated token file is not valid UTF-8",
        )
    })?;
    let assertion = text.trim_ascii();
    if assertion.is_empty() || !assertion.bytes().all(|byte| (0x21..=0x7E).contains(&byte)) {
        return Err(Error::new(
            ErrorKind::Credential,
            "the Azure federated token file is empty or contains invalid characters",
        ));
    }
    Ok(assertion.to_owned())
}

async fn fetch_token_post(
    client: &Client,
    token_url: &Url,
    form: String,
    clock: &Clock,
    policy: &RetryPolicy,
) -> Result<Credential<Auth>> {
    let body = bytes::Bytes::from(form);
    let answer = with_backoff(policy, Replay::Safe, || async {
        let request = Request::new(Method::POST, token_url.clone())
            .header("accept", "application/json")?
            .header("content-type", "application/x-www-form-urlencoded")?
            .with_body(body.clone());
        let response = client.send(request).await?.success().await?;
        response.bytes_up_to(TOKEN_BODY_LIMIT).await
    })
    .await
    .map_err(|error| token_request_error("Entra OAuth", error))?;
    parse_token_answer(&answer, clock)
}

fn token_request_error(service: &str, error: Error) -> Error {
    match error.status() {
        Some(status) => Error::new(
            ErrorKind::Credential,
            format!("the {service} token service answered {status}"),
        ),
        None => Error::new(
            ErrorKind::Credential,
            format!("the {service} token request failed"),
        )
        .with_source(error),
    }
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    #[serde(default, deserialize_with = "deserialize_seconds")]
    expires_in: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_epoch")]
    expires_on: Option<SystemTime>,
}

fn deserialize_seconds<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Seconds {
        Number(u64),
        Text(String),
    }

    match Option::<Seconds>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Seconds::Number(n)) => Ok(Some(n)),
        Some(Seconds::Text(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<u64>()
                .map(Some)
                .map_err(|_| serde::de::Error::custom("invalid expires_in"))
        }
    }
}

fn deserialize_epoch<'de, D>(deserializer: D) -> std::result::Result<Option<SystemTime>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Epoch {
        Number(u64),
        Text(String),
    }

    match Option::<Epoch>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Epoch::Number(secs)) => epoch_to_system_time(secs)
            .map(Some)
            .map_err(serde::de::Error::custom),
        Some(Epoch::Text(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if let Ok(secs) = trimmed.parse::<u64>() {
                return epoch_to_system_time(secs)
                    .map(Some)
                    .map_err(serde::de::Error::custom);
            }
            let parsed = Timestamp::parse_rfc3339(trimmed)
                .map_err(|_| serde::de::Error::custom("invalid expires_on"))?;
            let secs = u64::try_from(parsed.as_secs())
                .map_err(|_| serde::de::Error::custom("expires_on before 1970"))?;
            if secs == 0 || secs > MAX_EPOCH_SECS {
                return Err(serde::de::Error::custom("expires_on out of range"));
            }
            let nanos = parsed.subsec_nanos();
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::new(secs, nanos))
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom("expires_on overflow"))
        }
    }
}

fn epoch_to_system_time(secs: u64) -> std::result::Result<SystemTime, &'static str> {
    if secs == 0 || secs > MAX_EPOCH_SECS {
        return Err("expires_on out of range");
    }
    SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(secs))
        .ok_or("expires_on overflow")
}

fn parse_token_answer(body: &[u8], clock: &Clock) -> Result<Credential<Auth>> {
    let parsed: TokenAnswer = serde_json::from_slice(body).map_err(|_| {
        Error::new(
            ErrorKind::Credential,
            "the token service sent an unreadable answer",
        )
    })?;
    let token = parsed.access_token.trim_ascii();
    if token.is_empty() || !token.bytes().all(|byte| (0x21..=0x7E).contains(&byte)) {
        return Err(Error::new(
            ErrorKind::Credential,
            "the token service sent an unusable access token",
        ));
    }
    let expires_at = if let Some(secs) = parsed.expires_in {
        if secs == 0 {
            return Err(Error::new(
                ErrorKind::Credential,
                "the token service sent a zero expires_in lifetime",
            ));
        }
        let capped = secs.min(MAX_TOKEN_TTL_SECS);
        clock()
            .checked_add(Duration::from_secs(capped))
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Credential,
                    "the token expiry overflows the clock",
                )
            })?
    } else if let Some(at) = parsed.expires_on {
        at
    } else {
        return Err(Error::new(
            ErrorKind::Credential,
            "the token service sent no token expiry",
        ));
    };
    Ok(Credential::until(
        Auth::BearerToken(token.to_owned()),
        expires_at,
    ))
}

/// Everything resolved from [`AzureConfig`] and the environment.
pub(crate) struct Resolved {
    pub(crate) account: String,
    pub(crate) container: String,
    pub(crate) service_url: Url,
    pub(crate) container_url: Url,
    pub(crate) client: Client,
    pub(crate) auth: AuthSource,
}

/// Resolves account, service and container URLs, HTTP client and credential
/// source from `config` and `env`.
pub(crate) fn resolve(
    config: &AzureConfig,
    env: &BTreeMap<String, String>,
) -> std::result::Result<Resolved, ResolveError> {
    let from_env = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|key| env.get(*key).map(|v| v.trim()).filter(|v| !v.is_empty()))
            .map(str::to_owned)
    };

    let explicit_endpoint = config
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let env_endpoint = if explicit_endpoint.is_none() {
        from_env(&[
            "AZURE_STORAGE_ENDPOINT",
            "AZURE_STORAGE_BLOB_ENDPOINT",
            "AZURE_ENDPOINT",
        ])
    } else {
        None
    };
    let endpoint = explicit_endpoint.clone().or_else(|| env_endpoint.clone());

    let explicit_account = config
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let account = explicit_account
        .clone()
        .or_else(|| from_env(&["AZURE_STORAGE_ACCOUNT_NAME", "AZURE_STORAGE_ACCOUNT"]))
        .or_else(|| endpoint.as_deref().and_then(account_from_endpoint))
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Credential,
                "no Azure storage account is configured",
            )
        })?;
    check_segment(&account, "account")?;

    let container = config.container.trim().to_owned();
    if container.is_empty() {
        return Err(Error::new(ErrorKind::Refused, "the Azure container is empty").into());
    }
    check_segment(&container, "container")?;

    let service_raw = match &endpoint {
        Some(url) => url.trim_end_matches('/').to_owned(),
        None => format!("https://{account}.blob.core.windows.net"),
    };
    let service_url = clean_http_url(&service_raw, "an Azure endpoint")?;

    // When `config.account` was explicitly set without `config.endpoint`, do
    // not let an environment endpoint redirect traffic to a different account
    // or downgrade to remote plain HTTP.
    if explicit_account.is_some()
        && let Some(ref raw_env_ep) = env_endpoint
    {
        if let Some(ep_account) = account_from_endpoint(raw_env_ep)
            && !ep_account.eq_ignore_ascii_case(&account)
        {
            return Err(Error::new(
                ErrorKind::Refused,
                "the Azure environment endpoint names a different storage account than configured",
            )
            .into());
        }
        if service_url.scheme() == "http" && !is_loopback_url(&service_url) {
            return Err(Error::new(
                ErrorKind::Refused,
                "an Azure environment endpoint cannot downgrade a configured account to remote plain HTTP",
            )
            .into());
        }
    }

    let client = Client::for_endpoint(endpoint.as_deref())?;
    let container_path = format!(
        "{}/{}",
        service_url.path().trim_end_matches('/'),
        encode_component(&container)
    );
    let container_url = Url::parse(&format!(
        "{}{container_path}",
        service_url.origin().ascii_serialization()
    ))
    .map_err(|_| Error::new(ErrorKind::Refused, "the Azure container URL is invalid"))?;

    let auth = resolve_auth(&account, config, env, &from_env)?;

    Ok(Resolved {
        account,
        container,
        service_url,
        container_url,
        client,
        auth,
    })
}

fn resolve_auth(
    account: &str,
    config: &AzureConfig,
    env: &BTreeMap<String, String>,
    from_env: &impl Fn(&[&str]) -> Option<String>,
) -> std::result::Result<AuthSource, ResolveError> {
    // 1. Explicit disk access_key wins over every environment variable.
    if let Some(access_key) = &config.access_key {
        let key = SharedKey::new(account, access_key.expose())
            .map_err(|_| ResolveError::InvalidDiskAccessKey)?;
        return Ok(AuthSource::SharedKey(key));
    }

    // 2. Environment Shared Key (naming the exact variable on error).
    for key_var in [
        "AZURE_STORAGE_ACCOUNT_KEY",
        "AZURE_STORAGE_ACCESS_KEY",
        "AZURE_STORAGE_MASTER_KEY",
    ] {
        if let Some(raw_val) = env.get(key_var).filter(|v| !v.trim_ascii().is_empty()) {
            let key = SharedKey::new(account, raw_val)
                .map_err(|_| ResolveError::InvalidEnvAccessKey(key_var))?;
            return Ok(AuthSource::SharedKey(key));
        }
    }

    // 3a. Storage-scoped Entra Service Principal (`AZURE_STORAGE_*`), evaluated
    //     ahead of generic `AZURE_*` and never mixed with them.
    let storage_tenant = from_env(&["AZURE_STORAGE_TENANT_ID", "AZURE_STORAGE_AUTHORITY_ID"]);
    let storage_client = from_env(&["AZURE_STORAGE_CLIENT_ID"]);
    let storage_secret = from_env(&["AZURE_STORAGE_CLIENT_SECRET"]);
    if storage_tenant.is_some() || storage_client.is_some() || storage_secret.is_some() {
        let (Some(tenant_id), Some(client_id), Some(client_secret)) =
            (storage_tenant, storage_client, storage_secret)
        else {
            return Err(Error::new(
                ErrorKind::Credential,
                "set AZURE_STORAGE_TENANT_ID, AZURE_STORAGE_CLIENT_ID and AZURE_STORAGE_CLIENT_SECRET together",
            )
            .into());
        };
        check_segment(&client_id, "client_id")?;
        let (client, token_url) = oauth_endpoint(
            &tenant_id,
            &["AZURE_STORAGE_AUTHORITY_HOST", "AZURE_AUTHORITY_HOST"],
            from_env,
        )?;
        return Ok(AuthSource::ClientSecret {
            client,
            token_url,
            client_id,
            client_secret,
        });
    }

    // 3b & 4. Generic Entra Service Principal (`AZURE_CLIENT_SECRET`) or
    //         Workload Identity (`AZURE_FEDERATED_TOKEN_FILE`). Partial tuples
    //         fail closed rather than falling through to Managed Identity.
    let tenant_id = from_env(&["AZURE_TENANT_ID", "AZURE_AUTHORITY_ID"]);
    let client_id = from_env(&["AZURE_CLIENT_ID"]);
    let client_secret = from_env(&["AZURE_CLIENT_SECRET"]);
    let federated_token_file = from_env(&["AZURE_FEDERATED_TOKEN_FILE"]);

    if let Some(ref cid) = client_id {
        check_segment(cid, "client_id")?;
    }

    if let Some(client_secret) = client_secret {
        let (Some(tenant_id), Some(client_id)) = (tenant_id, client_id) else {
            return Err(Error::new(
                ErrorKind::Credential,
                "AZURE_CLIENT_SECRET requires both AZURE_TENANT_ID and AZURE_CLIENT_ID",
            )
            .into());
        };
        let (client, token_url) = oauth_endpoint(
            &tenant_id,
            &["AZURE_AUTHORITY_HOST", "AZURE_STORAGE_AUTHORITY_HOST"],
            from_env,
        )?;
        return Ok(AuthSource::ClientSecret {
            client,
            token_url,
            client_id,
            client_secret,
        });
    }

    if let Some(federated_token_file) = federated_token_file {
        let (Some(tenant_id), Some(client_id)) = (tenant_id, client_id) else {
            return Err(Error::new(
                ErrorKind::Credential,
                "AZURE_FEDERATED_TOKEN_FILE requires both AZURE_TENANT_ID and AZURE_CLIENT_ID",
            )
            .into());
        };
        let (client, token_url) = oauth_endpoint(
            &tenant_id,
            &["AZURE_AUTHORITY_HOST", "AZURE_STORAGE_AUTHORITY_HOST"],
            from_env,
        )?;
        return Ok(AuthSource::WorkloadIdentity {
            client,
            token_url,
            client_id,
            federated_token_file,
        });
    }

    if tenant_id.is_some() {
        return Err(Error::new(
            ErrorKind::Credential,
            "AZURE_TENANT_ID requires AZURE_CLIENT_SECRET or AZURE_FEDERATED_TOKEN_FILE",
        )
        .into());
    }

    // 5a. Managed Identity on App Service / Azure Functions / Container Apps:
    //     modern `IDENTITY_ENDPOINT` + `IDENTITY_HEADER`, or legacy
    //     `MSI_ENDPOINT` + `MSI_SECRET` (with its own API version, `Secret`
    //     header and `clientid` query parameter). Partial pairs fail closed.
    match (
        from_env(&["IDENTITY_ENDPOINT"]),
        from_env(&["IDENTITY_HEADER"]),
    ) {
        (Some(endpoint), Some(header_secret)) => {
            let (client, mut url) =
                managed_identity_client_and_url(&endpoint, "the managed identity endpoint")?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs
                    .append_pair("api-version", APP_SERVICE_API_VERSION)
                    .append_pair("resource", STORAGE_RESOURCE);
                if let Some(client_id) = &client_id {
                    pairs.append_pair("client_id", client_id);
                }
            }
            return Ok(AuthSource::AppService {
                client,
                url,
                header_kind: AppServiceHeader::IdentityHeader,
                header_secret,
            });
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err(Error::new(
                ErrorKind::Credential,
                "set both IDENTITY_ENDPOINT and IDENTITY_HEADER, or neither",
            )
            .into());
        }
        (None, None) => {}
    }

    match (from_env(&["MSI_ENDPOINT"]), from_env(&["MSI_SECRET"])) {
        (Some(endpoint), Some(header_secret)) => {
            let (client, mut url) =
                managed_identity_client_and_url(&endpoint, "the MSI managed identity endpoint")?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs
                    .append_pair("api-version", LEGACY_MSI_API_VERSION)
                    .append_pair("resource", STORAGE_RESOURCE);
                if let Some(client_id) = &client_id {
                    pairs.append_pair("clientid", client_id);
                }
            }
            return Ok(AuthSource::AppService {
                client,
                url,
                header_kind: AppServiceHeader::LegacySecret,
                header_secret,
            });
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err(Error::new(
                ErrorKind::Credential,
                "set both MSI_ENDPOINT and MSI_SECRET, or neither",
            )
            .into());
        }
        (None, None) => {}
    }

    // 5b. Managed Identity on Azure VM / AKS via IMDS.
    let imds_raw = from_env(&[
        "AZURE_IMDS_ENDPOINT",
        "AZURE_METADATA_ENDPOINT",
        "AZURE_MSI_ENDPOINT",
        "AZURE_IDENTITY_ENDPOINT",
    ])
    .unwrap_or_else(|| DEFAULT_IMDS_ENDPOINT.to_owned());
    let (client, mut url) = managed_identity_client_and_url(&imds_raw, "the IMDS endpoint")?;
    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/metadata/identity/oauth2/token");
    }
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("api-version", IMDS_API_VERSION)
            .append_pair("resource", STORAGE_RESOURCE);
        if let Some(client_id) = &client_id {
            pairs.append_pair("client_id", client_id);
        }
    }
    Ok(AuthSource::Imds { client, url })
}

fn oauth_endpoint(
    tenant_id: &str,
    authority_vars: &[&str],
    from_env: &impl Fn(&[&str]) -> Option<String>,
) -> Result<(Client, Url)> {
    check_segment(tenant_id, "tenant_id")?;
    let authority = from_env(authority_vars).unwrap_or_else(|| PUBLIC_AUTHORITY_HOST.to_owned());
    let base = parse_no_query_url(&authority, "the Azure authority host")?;
    let client = match base.scheme() {
        "https" => Client::for_endpoint(None)?,
        "http" if is_loopback_url(&base) => {
            let origin = base.origin().ascii_serialization();
            Client::for_endpoint(Some(&origin))?
        }
        _ => {
            return Err(Error::new(
                ErrorKind::Refused,
                "the Azure authority host must use https:// (or loopback http://)",
            ));
        }
    };
    let path = format!(
        "{}/{}/oauth2/v2.0/token",
        base.path().trim_end_matches('/'),
        encode_component(tenant_id)
    );
    let token_url = Url::parse(&format!("{}{path}", base.origin().ascii_serialization()))
        .map_err(|_| Error::new(ErrorKind::Refused, "the Azure authority URL is invalid"))?;
    Ok((client, token_url))
}

/// Validates a Managed Identity / IMDS endpoint: rejects pre-existing query
/// parameters, allows `https://`, and restricts plain `http://` to loopback,
/// link-local or RFC 1918 / ULA private addresses.
fn managed_identity_client_and_url(raw: &str, label: &str) -> Result<(Client, Url)> {
    let url = parse_no_query_url(raw, label)?;
    let client = match url.scheme() {
        "https" => Client::for_endpoint(None)?,
        "http" if is_local_or_private_host(&url) => Client::for_metadata()?,
        _ => {
            return Err(Error::new(
                ErrorKind::Refused,
                format!("{label} over http:// must target loopback, link-local or a private IP"),
            ));
        }
    };
    Ok((client, url))
}

fn is_loopback_url(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn is_local_or_private_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => is_local_or_private_ipv4(ip),
        Some(Host::Ipv6(ip)) => is_local_or_private_ipv6(ip),
        None => false,
    }
}

fn is_local_or_private_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_link_local() || ip.is_private()
}

fn is_local_or_private_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unicast_link_local() || (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn check_segment(value: &str, label: &str) -> Result<()> {
    check_key(value).map_err(|_| {
        Error::new(
            ErrorKind::Refused,
            format!("the Azure {label} is not a valid path segment"),
        )
    })?;
    if value.contains(['/', '\\', '?', '#']) {
        return Err(Error::new(
            ErrorKind::Refused,
            format!("the Azure {label} is not a valid path segment"),
        ));
    }
    Ok(())
}

fn parse_no_query_url(raw: &str, label: &str) -> Result<Url> {
    let refused = || {
        Error::new(
            ErrorKind::Refused,
            format!("{label} must be an absolute http or https URL without query parameters"),
        )
    };
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

fn parse_http_url(raw: &str, label: &str) -> Result<Url> {
    let refused = || {
        Error::new(
            ErrorKind::Refused,
            format!("{label} must be an absolute http or https URL"),
        )
    };
    let has_authority = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with(['/', '\\']));
    if !has_authority {
        return Err(refused());
    }
    let url = Url::parse(raw).map_err(|_| refused())?;
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

fn clean_http_url(raw: &str, label: &str) -> Result<Url> {
    parse_http_url(raw, label)
}

/// Extracts the storage account from an Azurite URL
/// (`http://127.0.0.1:10000/devstoreaccount1`) or a hosted Azure Blob URL
/// (`https://agrastorage.blob.core.windows.net`).
fn account_from_endpoint(endpoint: &str) -> Option<String> {
    let url = Url::parse(endpoint).ok()?;
    if let Some(first_segment) = url
        .path_segments()
        .and_then(|mut segments| segments.next())
        .filter(|segment| !segment.is_empty())
    {
        return Some(first_segment.to_owned());
    }
    let host = url.host_str()?;
    for suffix in [".blob.core.windows.net", ".dfs.core.windows.net"] {
        if let Some(account) = host.strip_suffix(suffix)
            && !account.is_empty()
            && !account.contains('.')
        {
            return Some(account.to_owned());
        }
    }
    None
}
