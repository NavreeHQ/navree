//! Service account RSA keys (`PKCS#8` PEM), `RS256` JWT assertions and
//! Google OAuth 2.0 token exchange.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ::http::Method;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::cloud::credentials::Credential;
use crate::cloud::http::{Client, Request};
use crate::cloud::keys::encode_component;
use crate::cloud::retry::{Replay, RetryPolicy, with_backoff};
use crate::cloud::{Error, ErrorKind, Result};

/// Google's OAuth 2.0 token endpoint when a credential file names none.
pub const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// Full read, write, list and delete access to Google Cloud Storage buckets
/// and objects.
pub const DEFAULT_SCOPE: &str = "https://www.googleapis.com/auth/devstorage.full_control";

/// The lifetime requested on each self-signed JWT assertion (one hour, the
/// longest Google OAuth 2.0 accepts).
const ASSERTION_TTL_SECS: u64 = 3600;

/// The largest OAuth 2.0 token answer read into memory.
const TOKEN_ANSWER_LIMIT: usize = 64 * 1024;

/// Maximum lifetime accepted from a token response (one year in seconds).
const MAX_TOKEN_TTL_SECS: u64 = 365 * 24 * 60 * 60;

/// Trusted Google OAuth 2.0 token hosts permitted inside credential JSON files.
const TRUSTED_GOOGLE_TOKEN_HOSTS: [&str; 3] = [
    "oauth2.googleapis.com",
    "www.googleapis.com",
    "accounts.google.com",
];

/// The `grant_type` of an OAuth 2.0 JWT bearer assertion request (RFC 7523).
const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// An RSA private key loaded from PEM, used to sign OAuth 2.0 JWT assertions
/// and V4 signed URLs with `RSASSA-PKCS1-v1_5` and `SHA-256`.
///
/// Cloning is cheap: the parsed key pair is shared. `Debug` prints only the
/// key identifier, never key material.
#[derive(Clone)]
pub struct RsaSigningKey {
    pair: Arc<RsaKeyPair>,
    kid: Option<String>,
}

impl RsaSigningKey {
    /// Parses a PEM-encoded RSA private key (`PRIVATE KEY` PKCS#8, or
    /// `RSA PRIVATE KEY` PKCS#1). Literal `\n` sequences from single-line
    /// environment variables are accepted.
    pub fn from_pem(pem: &str) -> Result<Self> {
        let invalid = || {
            Error::new(
                ErrorKind::Credential,
                "the service account key is not a valid PEM RSA private key",
            )
        };
        let normalized;
        let text = if !pem.contains('\n') && pem.contains("\\n") {
            normalized = pem.replace("\\n", "\n");
            normalized.as_str()
        } else {
            pem
        };
        let (label, der) = decode_pem_block(text).ok_or_else(invalid)?;
        let pair = match label {
            "PRIVATE KEY" => RsaKeyPair::from_pkcs8(&der).map_err(|_| invalid())?,
            "RSA PRIVATE KEY" => RsaKeyPair::from_der(&der).map_err(|_| invalid())?,
            _ => return Err(invalid()),
        };
        Ok(Self {
            pair: Arc::new(pair),
            kid: None,
        })
    }

    /// Attaches a non-empty `private_key_id` for the `kid` header of signed
    /// JWT assertions.
    #[must_use]
    pub fn with_kid(mut self, kid: impl Into<String>) -> Self {
        let kid = kid.into();
        self.kid = (!kid.trim().is_empty()).then_some(kid);
        self
    }

    /// The key identifier, when the service account JSON supplied one.
    pub fn kid(&self) -> Option<&str> {
        self.kid.as_deref()
    }

    /// Signs `message` with `RSASSA-PKCS1-v1_5` using `SHA-256`.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        let rng = SystemRandom::new();
        let mut signature = vec![0u8; self.pair.public().modulus_len()];
        self.pair
            .sign(&RSA_PKCS1_SHA256, &rng, message, &mut signature)
            .map_err(|_| {
                Error::new(
                    ErrorKind::Credential,
                    "the service account key could not sign",
                )
            })?;
        Ok(signature)
    }
}

impl fmt::Debug for RsaSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RsaSigningKey")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

/// Extracts the label and DER bytes of the first PEM block in `pem`.
fn decode_pem_block(pem: &str) -> Option<(&str, Vec<u8>)> {
    let (_, after_begin) = pem.split_once("-----BEGIN ")?;
    let (label, rest) = after_begin.split_once("-----")?;
    let label = label.trim();
    let end_marker = format!("-----END {label}-----");
    let (body, _) = rest.split_once(&end_marker)?;
    let cleaned: String = body
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let der = STANDARD.decode(cleaned).ok()?;
    Some((label, der))
}

#[derive(Serialize)]
struct JwtHeader<'a> {
    alg: &'static str,
    typ: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kid: Option<&'a str>,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: String,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

/// Builds and signs a compact `RS256` JWT assertion for `client_email`,
/// asking `token_uri` for `scopes` at `now`.
pub fn assertion(
    key: &RsaSigningKey,
    client_email: &str,
    token_uri: &str,
    scopes: &[&str],
    now: SystemTime,
) -> Result<String> {
    if client_email.trim().is_empty() {
        return Err(Error::new(
            ErrorKind::Credential,
            "the service account has no client_email",
        ));
    }
    if token_uri.trim().is_empty() {
        return Err(Error::new(
            ErrorKind::Credential,
            "the service account has an empty token_uri",
        ));
    }
    if scopes.is_empty() || scopes.iter().any(|scope| scope.trim().is_empty()) {
        return Err(Error::new(
            ErrorKind::Credential,
            "at least one OAuth scope is required",
        ));
    }
    let iat = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| Error::new(ErrorKind::Credential, "the clock is set before 1970"))?
        .as_secs();
    let exp = iat.saturating_add(ASSERTION_TTL_SECS);
    let header = JwtHeader {
        alg: "RS256",
        typ: "JWT",
        kid: key.kid(),
    };
    let claims = JwtClaims {
        iss: client_email,
        scope: scopes.join(" "),
        aud: token_uri,
        iat,
        exp,
    };
    let header_json = serde_json::to_vec(&header)
        .map_err(|_| Error::new(ErrorKind::Credential, "could not encode the JWT header"))?;
    let claims_json = serde_json::to_vec(&claims)
        .map_err(|_| Error::new(ErrorKind::Credential, "could not encode the JWT claims"))?;
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header_json),
        URL_SAFE_NO_PAD.encode(claims_json),
    );
    let signature = key.sign(signing_input.as_bytes())?;
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

/// Validates `token_uri` read from a `service_account` or `authorized_user`
/// credential JSON file: must be one of Google's HTTPS OAuth 2.0 hosts on
/// default port 443 with no userinfo, query or fragment (or a loopback
/// `http://` address strictly when the storage `endpoint` itself is an
/// explicit loopback `http://` emulator).
pub(crate) fn credential_token_client_and_url(
    token_uri: &str,
    allow_loopback_http: bool,
) -> Result<(Client, Url)> {
    let refused = || {
        Error::new(
            ErrorKind::Refused,
            "a credential token_uri must point to a Google OAuth 2.0 https:// host",
        )
    };
    let url = Url::parse(token_uri).map_err(|_| refused())?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(refused());
    }
    match (url.scheme(), url.host_str()) {
        ("https", Some(host))
            if url.port().is_none()
                && TRUSTED_GOOGLE_TOKEN_HOSTS
                    .iter()
                    .any(|trusted| host.eq_ignore_ascii_case(trusted)) =>
        {
            Ok((Client::for_endpoint(None)?, url))
        }
        ("http", Some(_)) if allow_loopback_http && is_loopback(&url) => {
            let origin = url.origin().ascii_serialization();
            Ok((Client::for_endpoint(Some(&origin))?, url))
        }
        _ => Err(refused()),
    }
}

/// Validates an explicit programmatic token endpoint set on
/// [`super::GcsStoreBuilder::token_endpoint`]: `https://` anywhere, or
/// loopback `http://` for local mock OAuth servers.
pub(crate) fn override_token_client_and_url(token_uri: &str) -> Result<(Client, Url)> {
    let refused = || {
        Error::new(
            ErrorKind::Refused,
            "a Google OAuth token endpoint must be an https:// URL (or loopback http://)",
        )
    };
    let url = Url::parse(token_uri).map_err(|_| refused())?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(refused());
    }
    match url.scheme() {
        "https" if url.host_str().is_some() => Ok((Client::for_endpoint(None)?, url)),
        "http" if is_loopback(&url) => {
            let origin = url.origin().ascii_serialization();
            Ok((Client::for_endpoint(Some(&origin))?, url))
        }
        _ => Err(refused()),
    }
}

/// Whether `url`'s host is the local loopback interface (`localhost`,
/// `127.0.0.0/8` or `::1`).
pub(crate) fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(addr)) => addr.is_loopback(),
        Some(url::Host::Ipv6(addr)) => addr.is_loopback(),
        None => false,
    }
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// Exchanges a freshly signed `RS256` JWT assertion for an OAuth 2.0 bearer
/// token at `token_url`.
pub(crate) async fn exchange_jwt(
    client: &Client,
    token_url: &Url,
    policy: &RetryPolicy,
    key: &RsaSigningKey,
    client_email: &str,
    scopes: &[String],
    clock: &(dyn Fn() -> SystemTime + Send + Sync),
) -> Result<Credential<String>> {
    let scope_refs: Vec<&str> = scopes.iter().map(String::as_str).collect();
    let aud = token_url.as_str();
    with_backoff(policy, Replay::Safe, || async {
        let now = clock();
        let jwt = assertion(key, client_email, aud, &scope_refs, now)?;
        let form = format!(
            "grant_type={}&assertion={}",
            encode_component(JWT_BEARER_GRANT),
            encode_component(&jwt),
        );
        post_token_form(client, token_url, form, clock).await
    })
    .await
}

/// Exchanges an authorized-user `refresh_token` for an OAuth 2.0 bearer token
/// at `token_url`.
pub(crate) async fn exchange_refresh_token(
    client: &Client,
    token_url: &Url,
    policy: &RetryPolicy,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
    clock: &(dyn Fn() -> SystemTime + Send + Sync),
) -> Result<Credential<String>> {
    with_backoff(policy, Replay::Safe, || async {
        let form = format!(
            "grant_type=refresh_token&client_id={}&client_secret={}&refresh_token={}",
            encode_component(client_id),
            encode_component(client_secret),
            encode_component(refresh_token),
        );
        post_token_form(client, token_url, form, clock).await
    })
    .await
}

async fn post_token_form(
    client: &Client,
    token_url: &Url,
    form: String,
    clock: &(dyn Fn() -> SystemTime + Send + Sync),
) -> Result<Credential<String>> {
    let request = Request::new(Method::POST, token_url.clone())
        .header("content-type", "application/x-www-form-urlencoded")?
        .with_body(form);
    let answer = client
        .send(request)
        .await?
        .success()
        .await
        .map_err(attach_error_code)?;
    let body = answer.bytes_up_to(TOKEN_ANSWER_LIMIT).await?;
    parse_token_answer(&body, clock())
}

/// Parses an OAuth 2.0 or metadata-server JSON token payload into a
/// [`Credential`].
pub(crate) fn parse_token_answer(body: &[u8], now: SystemTime) -> Result<Credential<String>> {
    let parsed: TokenAnswer = serde_json::from_slice(body).map_err(|_| {
        Error::new(
            ErrorKind::Answer,
            "the token answer is not a valid JSON object",
        )
    })?;
    let token = parsed.access_token.trim_ascii();
    if token.is_empty() || !token.bytes().all(|byte| (0x21..=0x7E).contains(&byte)) {
        return Err(Error::new(
            ErrorKind::Answer,
            "the token answer did not carry a valid access_token",
        ));
    }
    if parsed.expires_in == Some(0) {
        return Err(Error::new(
            ErrorKind::Answer,
            "the token answer had a zero expires_in lifetime",
        ));
    }
    let secs = parsed
        .expires_in
        .unwrap_or(ASSERTION_TTL_SECS)
        .min(MAX_TOKEN_TTL_SECS);
    let expires_at = now.checked_add(Duration::from_secs(secs)).ok_or_else(|| {
        Error::new(
            ErrorKind::Answer,
            "the token expiry timestamp overflows the system clock",
        )
    })?;
    Ok(Credential::until(token.to_owned(), expires_at))
}

/// Extracts Google's short error code (`error.status`, `error.errors[0].reason`
/// or OAuth `error`) onto `error` without ever exposing messages, descriptions
/// or tokens.
pub(crate) fn attach_error_code(error: Error) -> Error {
    let Some(body) = error.body() else {
        return error;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return error;
    };
    let code = match json.get("error") {
        Some(serde_json::Value::String(oauth_code)) => Some(oauth_code.as_str()),
        Some(serde_json::Value::Object(map)) => map
            .get("status")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                map.get("errors")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("reason"))
                    .and_then(serde_json::Value::as_str)
            }),
        _ => None,
    };
    match code {
        Some(code) => error.with_code(code),
        None => error,
    }
}
