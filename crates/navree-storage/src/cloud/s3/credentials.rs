//! Ways an S3 store signs in, in the order an `s3` disk has always tried
//! them:
//!
//! 1. Static keys configured on the disk (`access_key` + `secret_key`, and
//!    optional `session_token`); when set, nothing is read from the
//!    environment.
//! 2. Static keys in the environment (`AWS_ACCESS_KEY_ID` +
//!    `AWS_SECRET_ACCESS_KEY`, and optional `AWS_SESSION_TOKEN`).
//! 3. Web identity (`AWS_WEB_IDENTITY_TOKEN_FILE` + `AWS_ROLE_ARN`,
//!    optional `AWS_ROLE_SESSION_NAME`, exchanged through STS
//!    `AssumeRoleWithWebIdentity` at `AWS_ENDPOINT_URL_STS` or
//!    `https://sts.{region}.amazonaws.com`).
//! 4. Container credentials on ECS and EKS
//!    (`AWS_CONTAINER_CREDENTIALS_RELATIVE_URI` on `http://169.254.170.2`,
//!    or `AWS_CONTAINER_CREDENTIALS_FULL_URI`, with an optional bearer
//!    token in `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE` or
//!    `AWS_CONTAINER_AUTHORIZATION_TOKEN`).
//! 5. Instance metadata (`AWS_METADATA_ENDPOINT`, default
//!    `http://169.254.169.254`): IMDSv2 session token first, falling back
//!    to IMDSv1 only when `AWS_IMDSV1_FALLBACK` is enabled and the token
//!    endpoint answers `403`, `404` or `405`.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use ::http::{Method, StatusCode};
use chrono::DateTime;
use navree_kernel::BoxFuture;
use serde::Deserialize;
use url::{Host, Url};

use super::enrich_error;
use super::sigv4::Credentials;
use crate::S3Config;
use crate::cloud::credentials::{Credential, Provider, Source};
use crate::cloud::http::{Client, Request};
use crate::cloud::keys::encode_component;
use crate::cloud::retry::{Replay, RetryPolicy, with_backoff};
use crate::cloud::{Error, ErrorKind, Result, xml};

/// Default EC2 instance metadata endpoint.
const DEFAULT_METADATA_ENDPOINT: &str = "http://169.254.169.254";

/// ECS task credentials link-local host.
const ECS_CREDENTIALS_HOST: Ipv4Addr = Ipv4Addr::new(169, 254, 170, 2);

/// EKS Pod Identity IPv4 link-local host.
const EKS_POD_IDENTITY_IPV4: Ipv4Addr = Ipv4Addr::new(169, 254, 170, 23);

/// EKS Pod Identity IPv6 link-local host (`fd00:ec2::23`).
const EKS_POD_IDENTITY_IPV6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0023);

/// Maximum size accepted for STS, container or metadata credential payloads.
const CREDENTIAL_RESPONSE_LIMIT: usize = 64 * 1024;

/// Maximum size accepted for an IMDSv2 token or role name.
const METADATA_SMALL_LIMIT: usize = 4 * 1024;

/// Builds the credential [`Provider`] for `config` in `region` using `env`
/// and `clock`.
pub(super) fn build_provider(
    config: &S3Config,
    region: &str,
    env: &BTreeMap<String, String>,
    retry: RetryPolicy,
    clock: impl Fn() -> SystemTime + Send + Sync + 'static,
) -> Result<Provider<Credentials>> {
    // 1. Static keys on the disk itself. When present, environment
    //    credentials (including AWS_SESSION_TOKEN) are untouched.
    if config.access_key.is_some() || config.secret_key.is_some() {
        let access_key = config.access_key.as_deref().filter(|s| !s.is_empty());
        let secret_key = config
            .secret_key
            .as_ref()
            .map(|s| s.expose().as_str())
            .filter(|s| !s.is_empty());
        let (Some(access_key), Some(secret_key)) = (access_key, secret_key) else {
            return Err(Error::new(
                ErrorKind::Credential,
                "set both access_key and secret_key, or neither",
            ));
        };
        let session_token = config
            .session_token
            .as_ref()
            .map(|token| token.expose().clone())
            .filter(|token| !token.is_empty());
        return Ok(Provider::fixed(
            Credentials::new(access_key, secret_key).with_session_token(session_token),
        ));
    }

    // 2. Static keys in the environment.
    let env_access = env.get("AWS_ACCESS_KEY_ID").filter(|s| !s.is_empty());
    let env_secret = env.get("AWS_SECRET_ACCESS_KEY").filter(|s| !s.is_empty());
    match (env_access, env_secret) {
        (Some(access_key), Some(secret_key)) => {
            let session_token = env
                .get("AWS_SESSION_TOKEN")
                .cloned()
                .filter(|token| !token.is_empty());
            return Ok(Provider::fixed(
                Credentials::new(access_key, secret_key).with_session_token(session_token),
            ));
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err(Error::new(
                ErrorKind::Credential,
                "set both AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or neither",
            ));
        }
        (None, None) => {}
    }

    // 3. Web identity token exchanged with STS AssumeRoleWithWebIdentity.
    let web_token_file = trimmed_env(env, "AWS_WEB_IDENTITY_TOKEN_FILE");
    let role_arn = trimmed_env(env, "AWS_ROLE_ARN");
    if let (Some(token_file), Some(role_arn)) = (web_token_file, role_arn) {
        let session_name = trimmed_env(env, "AWS_ROLE_SESSION_NAME")
            .unwrap_or("WebIdentitySession")
            .to_owned();
        let sts_endpoint = trimmed_env(env, "AWS_ENDPOINT_URL_STS")
            .map(str::to_owned)
            .unwrap_or_else(|| format!("https://sts.{region}.amazonaws.com"));
        let url = parse_endpoint_url(
            &sts_endpoint,
            "the STS endpoint must be an http or https URL",
        )?;
        let client = Client::for_endpoint(Some(&sts_endpoint))?;
        let source = WebIdentitySource {
            client,
            url,
            token_file: PathBuf::from(token_file),
            role_arn: role_arn.to_owned(),
            session_name,
            retry,
        };
        return Ok(Provider::with_clock(source, clock));
    }

    // 4. ECS or EKS container credentials.
    if let Some(relative) = trimmed_env(env, "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI") {
        let url = parse_ecs_relative_uri(relative)?;
        let source = container_source(url, env, retry)?;
        return Ok(Provider::with_clock(source, clock));
    }
    if let Some(full_uri) = trimmed_env(env, "AWS_CONTAINER_CREDENTIALS_FULL_URI") {
        let url = parse_container_full_uri(full_uri)?;
        let source = container_source(url, env, retry)?;
        return Ok(Provider::with_clock(source, clock));
    }

    // 5. EC2 instance metadata (IMDSv2 first; IMDSv1 only when opted in).
    let metadata_endpoint =
        trimmed_env(env, "AWS_METADATA_ENDPOINT").unwrap_or(DEFAULT_METADATA_ENDPOINT);
    let base_url = parse_endpoint_url(
        metadata_endpoint,
        "AWS_METADATA_ENDPOINT must be an http or https URL",
    )?;
    let imdsv1_fallback = parse_bool_env(env.get("AWS_IMDSV1_FALLBACK"), "AWS_IMDSV1_FALLBACK")?;
    let client = if base_url.scheme() == "https" {
        Client::for_endpoint(None)?
    } else {
        Client::for_metadata()?
    };
    let source = ImdsSource {
        client,
        base_url,
        imdsv1_fallback,
        retry,
    };
    Ok(Provider::with_clock(source, clock))
}

fn trimmed_env<'a>(env: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    env.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

fn parse_bool_env(raw: Option<&String>, name: &'static str) -> Result<bool> {
    let Some(value) = raw.map(|v| v.trim()).filter(|v| !v.is_empty()) else {
        return Ok(false);
    };
    if value.eq_ignore_ascii_case("true") || value == "1" || value.eq_ignore_ascii_case("yes") {
        Ok(true)
    } else if value.eq_ignore_ascii_case("false")
        || value == "0"
        || value.eq_ignore_ascii_case("no")
    {
        Ok(false)
    } else {
        Err(Error::new(
            ErrorKind::Refused,
            format!("{name} must be true or false"),
        ))
    }
}

fn parse_endpoint_url(endpoint: &str, message: &'static str) -> Result<Url> {
    let refused = || Error::new(ErrorKind::Refused, message);
    let mut url = Url::parse(endpoint.trim_end_matches('/')).map_err(|_| refused())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(refused());
    }
    if url.path().is_empty() {
        url.set_path("/");
    }
    Ok(url)
}

fn parse_ecs_relative_uri(relative: &str) -> Result<Url> {
    let refused = || {
        Error::new(
            ErrorKind::Refused,
            "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI must be a relative path starting with `/`",
        )
    };
    if !relative.starts_with('/')
        || relative.starts_with("//")
        || relative.chars().any(char::is_control)
    {
        return Err(refused());
    }
    let url =
        Url::parse(&format!("http://{ECS_CREDENTIALS_HOST}{relative}")).map_err(|_| refused())?;
    if url.scheme() != "http"
        || url.host() != Some(Host::Ipv4(ECS_CREDENTIALS_HOST))
        || url.port_or_known_default() != Some(80)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(refused());
    }
    Ok(url)
}

fn parse_container_full_uri(full_uri: &str) -> Result<Url> {
    let refused = || {
        Error::new(
            ErrorKind::Refused,
            "AWS_CONTAINER_CREDENTIALS_FULL_URI must use https, loopback or the container metadata host",
        )
    };
    if full_uri.chars().any(char::is_control) {
        return Err(refused());
    }
    let url = Url::parse(full_uri).map_err(|_| refused())?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(refused());
    }
    let allowed = match (url.scheme(), url.host()) {
        ("https", Some(_)) => true,
        ("http", Some(Host::Ipv4(ip))) => {
            ip.is_loopback() || ip == ECS_CREDENTIALS_HOST || ip == EKS_POD_IDENTITY_IPV4
        }
        ("http", Some(Host::Ipv6(ip))) => ip.is_loopback() || ip == EKS_POD_IDENTITY_IPV6,
        ("http", Some(Host::Domain(domain))) => domain.eq_ignore_ascii_case("localhost"),
        _ => false,
    };
    if !allowed {
        return Err(refused());
    }
    Ok(url)
}

fn container_source(
    url: Url,
    env: &BTreeMap<String, String>,
    retry: RetryPolicy,
) -> Result<ContainerSource> {
    let client = if url.scheme() == "https" {
        Client::for_endpoint(None)?
    } else {
        Client::for_metadata()?
    };
    let token_file = trimmed_env(env, "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE").map(PathBuf::from);
    let inline_token = env
        .get("AWS_CONTAINER_AUTHORIZATION_TOKEN")
        .cloned()
        .filter(|t| !t.is_empty());
    Ok(ContainerSource {
        client,
        url,
        token_file,
        inline_token,
        retry,
    })
}

/// Reads a token file on Tokio's blocking pool so rotated service-account
/// tokens are picked up on every refresh without stalling async tasks.
async fn read_token_file(path: PathBuf, label: &'static str) -> Result<String> {
    let content = tokio::task::spawn_blocking(move || std::fs::read_to_string(path))
        .await
        .map_err(|_| {
            Error::new(
                ErrorKind::Credential,
                format!("the {label} could not be read"),
            )
        })?
        .map_err(|io_err| {
            Error::new(
                ErrorKind::Credential,
                format!("the {label} could not be read"),
            )
            .with_source(io_err)
        })?;
    let trimmed = content.trim_end_matches(['\r', '\n']).trim();
    if trimmed.is_empty() {
        return Err(Error::new(
            ErrorKind::Credential,
            format!("the {label} is empty"),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Parses an RFC 3339 expiration timestamp into a [`SystemTime`].
fn parse_expiration(text: &str) -> Result<SystemTime> {
    let parsed = DateTime::parse_from_rfc3339(text.trim()).map_err(|_| {
        Error::new(
            ErrorKind::Answer,
            "the credential expiration timestamp is not RFC 3339",
        )
    })?;
    let secs = parsed.timestamp();
    let nanos = parsed.timestamp_subsec_nanos();
    if secs >= 0 {
        SystemTime::UNIX_EPOCH
            .checked_add(Duration::new(secs as u64, nanos))
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Answer,
                    "the credential expiration timestamp is out of range",
                )
            })
    } else {
        Ok(SystemTime::UNIX_EPOCH)
    }
}

/// STS `AssumeRoleWithWebIdentity` credential source.
struct WebIdentitySource {
    client: Client,
    url: Url,
    token_file: PathBuf,
    role_arn: String,
    session_name: String,
    retry: RetryPolicy,
}

impl Source<Credentials> for WebIdentitySource {
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<Credentials>>> {
        Box::pin(async move {
            let web_token =
                read_token_file(self.token_file.clone(), "web identity token file").await?;
            let form = format!(
                "Action=AssumeRoleWithWebIdentity&DurationSeconds=3600&RoleArn={}&RoleSessionName={}&Version=2011-06-15&WebIdentityToken={}",
                encode_component(&self.role_arn),
                encode_component(&self.session_name),
                encode_component(&web_token),
            );
            let answer = with_backoff(&self.retry, Replay::Safe, || async {
                let request = Request::new(Method::POST, self.url.clone())
                    .header("content-type", "application/x-www-form-urlencoded")?
                    .with_body(form.clone());
                self.client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(enrich_error)
            })
            .await?;

            let body = answer.bytes_up_to(CREDENTIAL_RESPONSE_LIMIT).await?;
            let root = xml::parse(&body)?;
            let creds = root
                .find(&["AssumeRoleWithWebIdentityResult", "Credentials"])
                .or_else(|| root.child("Credentials"))
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::Answer,
                        "the STS answer has no Credentials element",
                    )
                })?;
            let access_key_id = required_xml_field(creds, "AccessKeyId")?;
            let secret_access_key = required_xml_field(creds, "SecretAccessKey")?;
            let session_token = required_xml_field(creds, "SessionToken")?;
            let expiration = required_xml_field(creds, "Expiration")?;
            let expires_at = parse_expiration(expiration)?;

            Ok(Credential::until(
                Credentials::new(access_key_id, secret_access_key)
                    .with_session_token(Some(session_token.to_owned())),
                expires_at,
            ))
        })
    }
}

fn required_xml_field<'a>(element: &'a xml::Element, name: &str) -> Result<&'a str> {
    element
        .value(name)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Answer,
                format!("the STS answer is missing `{name}`"),
            )
        })
}

/// JSON credential payload returned by ECS, EKS Pod Identity and EC2 IMDS.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JsonSessionCredentials {
    access_key_id: String,
    secret_access_key: String,
    #[serde(default)]
    token: Option<String>,
    expiration: String,
}

fn parse_json_credentials(
    body: &[u8],
    source_name: &'static str,
) -> Result<Credential<Credentials>> {
    let parsed: JsonSessionCredentials = serde_json::from_slice(body).map_err(|_| {
        Error::new(
            ErrorKind::Answer,
            format!("the {source_name} credential answer is not valid JSON"),
        )
    })?;
    if parsed.access_key_id.is_empty() || parsed.secret_access_key.is_empty() {
        return Err(Error::new(
            ErrorKind::Answer,
            format!("the {source_name} credential answer is missing keys"),
        ));
    }
    let expires_at = parse_expiration(&parsed.expiration)?;
    Ok(Credential::until(
        Credentials::new(parsed.access_key_id, parsed.secret_access_key)
            .with_session_token(parsed.token),
        expires_at,
    ))
}

/// ECS task / EKS Pod Identity HTTP credential source.
struct ContainerSource {
    client: Client,
    url: Url,
    token_file: Option<PathBuf>,
    inline_token: Option<String>,
    retry: RetryPolicy,
}

impl Source<Credentials> for ContainerSource {
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<Credentials>>> {
        Box::pin(async move {
            let bearer = match &self.token_file {
                Some(path) => {
                    Some(read_token_file(path.clone(), "container authorization token file").await?)
                }
                None => self.inline_token.clone(),
            };
            let answer = with_backoff(&self.retry, Replay::Safe, || {
                let bearer = bearer.clone();
                async move {
                    let mut request = Request::new(Method::GET, self.url.clone());
                    if let Some(token) = &bearer {
                        request = request.secret("authorization", token)?;
                    }
                    self.client.send(request).await?.success().await
                }
            })
            .await?;

            let body = answer.bytes_up_to(CREDENTIAL_RESPONSE_LIMIT).await?;
            parse_json_credentials(&body, "container")
        })
    }
}

/// EC2 instance metadata credential source (IMDSv2, with optional IMDSv1
/// fallback).
struct ImdsSource {
    client: Client,
    base_url: Url,
    imdsv1_fallback: bool,
    retry: RetryPolicy,
}

impl ImdsSource {
    fn endpoint_url(&self, suffix: &str) -> Result<Url> {
        let base_path = self.base_url.path().trim_end_matches('/');
        let path = format!("{base_path}/{}", suffix.trim_start_matches('/'));
        let mut url = self.base_url.clone();
        url.set_path(&path);
        Ok(url)
    }

    async fn fetch_imdsv2_token(&self) -> Result<Option<String>> {
        let token_url = self.endpoint_url("latest/api/token")?;
        let token_result = with_backoff(&self.retry, Replay::Safe, || async {
            let request = Request::new(Method::PUT, token_url.clone())
                .header("x-aws-ec2-metadata-token-ttl-seconds", "21600")?;
            self.client.send(request).await?.success().await
        })
        .await;

        match token_result {
            Ok(answer) => {
                let bytes = answer.bytes_up_to(METADATA_SMALL_LIMIT).await?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| Error::new(ErrorKind::Answer, "the IMDSv2 token is not UTF-8"))?;
                let token = text.trim();
                if token.is_empty() {
                    return Err(Error::new(ErrorKind::Answer, "the IMDSv2 token is empty"));
                }
                Ok(Some(token.to_owned()))
            }
            Err(error)
                if self.imdsv1_fallback
                    && matches!(
                        error.status(),
                        Some(
                            StatusCode::FORBIDDEN
                                | StatusCode::NOT_FOUND
                                | StatusCode::METHOD_NOT_ALLOWED
                        )
                    ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

impl Source<Credentials> for ImdsSource {
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<Credentials>>> {
        Box::pin(async move {
            let token = self.fetch_imdsv2_token().await?;

            let role_list_url = self.endpoint_url("latest/meta-data/iam/security-credentials/")?;
            let role_answer = with_backoff(&self.retry, Replay::Safe, || {
                let token = token.clone();
                let url = role_list_url.clone();
                async move {
                    let mut request = Request::new(Method::GET, url);
                    if let Some(token) = &token {
                        request = request.secret("x-aws-ec2-metadata-token", token)?;
                    }
                    self.client.send(request).await?.success().await
                }
            })
            .await?;

            let role_bytes = role_answer.bytes_up_to(METADATA_SMALL_LIMIT).await?;
            let role_text = std::str::from_utf8(&role_bytes)
                .map_err(|_| Error::new(ErrorKind::Answer, "the IMDS role name is not UTF-8"))?;
            let role = role_text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .ok_or_else(|| Error::new(ErrorKind::Answer, "IMDS returned no IAM role"))?;
            if role == "."
                || role == ".."
                || role.contains(['/', '\\', '?', '#', '%'])
                || role.chars().any(|ch| ch.is_control() || ch.is_whitespace())
            {
                return Err(Error::new(
                    ErrorKind::Answer,
                    "IMDS returned an invalid IAM role name",
                ));
            }

            let creds_url = self.endpoint_url(&format!(
                "latest/meta-data/iam/security-credentials/{}",
                encode_component(role)
            ))?;
            let creds_answer = with_backoff(&self.retry, Replay::Safe, || {
                let token = token.clone();
                let url = creds_url.clone();
                async move {
                    let mut request = Request::new(Method::GET, url);
                    if let Some(token) = &token {
                        request = request.secret("x-aws-ec2-metadata-token", token)?;
                    }
                    self.client.send(request).await?.success().await
                }
            })
            .await?;

            let creds_bytes = creds_answer.bytes_up_to(CREDENTIAL_RESPONSE_LIMIT).await?;
            parse_json_credentials(&creds_bytes, "IMDS")
        })
    }
}
