//! OpenID Connect, the relying party's side: discovery, the authorization
//! URL (code flow with PKCE S256 and a nonce), the code exchange, ID token
//! verification against the provider's key set (on `ring` through
//! [`navree_crypto::jose`]) and the userinfo endpoint.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use navree_crypto::jose::{JwkSet, Jws};
use navree_http_client::{HttpClient, Response};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::SsoError;

/// How long a discovery document and a key set are used.
const CACHE_TTL: Duration = Duration::from_secs(3600);
/// The shortest time between two fetches of one key set (an unknown `kid`
/// refetches, but not more often than this).
const JWKS_MIN_REFETCH: Duration = Duration::from_secs(30);

/// A provider's discovery document (the members Navree uses).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Discovery {
    pub(crate) issuer: String,
    pub(crate) authorization_endpoint: String,
    pub(crate) token_endpoint: String,
    pub(crate) jwks_uri: String,
    #[serde(default)]
    pub(crate) userinfo_endpoint: Option<String>,
    #[serde(default)]
    pub(crate) token_endpoint_auth_methods_supported: Vec<String>,
}

/// Fetched discovery documents and key sets, by URL.
#[derive(Default)]
pub(crate) struct OidcCache {
    discovery: Mutex<HashMap<String, (Discovery, Instant)>>,
    jwks: Mutex<HashMap<String, (JwkSet, Instant)>>,
}

fn unavailable(what: &str, detail: impl std::fmt::Display) -> SsoError {
    SsoError::ProviderUnavailable(format!("{what}: {detail}"))
}

fn same_issuer(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    http: &HttpClient,
    url: &str,
    what: &str,
) -> Result<T, SsoError> {
    let response = http
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| unavailable(what, error))?;
    if !response.is_success() {
        return Err(unavailable(
            what,
            format!("answered {}", response.status().as_u16()),
        ));
    }
    response
        .json()
        .map_err(|_| unavailable(what, "the document is not valid"))
}

/// The discovery document of `issuer`, which must name `issuer` itself.
pub(crate) async fn discover(
    http: &HttpClient,
    cache: &OidcCache,
    issuer: &str,
) -> Result<Discovery, SsoError> {
    if let Some((discovery, fetched)) = cache
        .discovery
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(issuer)
        && fetched.elapsed() < CACHE_TTL
    {
        return Ok(discovery.clone());
    }
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let discovery: Discovery = get_json(http, &url, "OpenID Connect discovery").await?;
    if !same_issuer(&discovery.issuer, issuer) {
        return Err(SsoError::Config(format!(
            "the connection's issuer is `{issuer}` but its discovery document names `{}`",
            discovery.issuer
        )));
    }
    cache
        .discovery
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(issuer.to_owned(), (discovery.clone(), Instant::now()));
    Ok(discovery)
}

/// The PKCE S256 challenge of `verifier`.
pub(crate) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// What the authorization URL carries.
pub(crate) struct Authorize<'a> {
    pub(crate) client_id: &'a str,
    pub(crate) redirect_uri: &'a str,
    pub(crate) scopes: &'a [String],
    pub(crate) state: &'a str,
    pub(crate) nonce: &'a str,
    pub(crate) verifier: &'a str,
}

/// The authorization URL (code flow, PKCE S256, nonce).
pub(crate) fn authorize_url(
    discovery: &Discovery,
    request: &Authorize<'_>,
) -> Result<String, SsoError> {
    let mut url = url::Url::parse(&discovery.authorization_endpoint).map_err(|_| {
        SsoError::Config(format!(
            "invalid authorization endpoint `{}`",
            discovery.authorization_endpoint
        ))
    })?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", request.client_id)
        .append_pair("redirect_uri", request.redirect_uri)
        .append_pair("scope", &request.scopes.join(" "))
        .append_pair("state", request.state)
        .append_pair("nonce", request.nonce)
        .append_pair("code_challenge", &challenge(request.verifier))
        .append_pair("code_challenge_method", "S256");
    Ok(url.to_string())
}

/// The tokens of a code exchange.
pub(crate) struct Tokens {
    pub(crate) id_token: String,
    pub(crate) access_token: Option<String>,
}

/// What the token request carries.
pub(crate) struct Exchange<'a> {
    pub(crate) client_id: &'a str,
    pub(crate) client_secret: Option<&'a str>,
    pub(crate) code: &'a str,
    pub(crate) redirect_uri: &'a str,
    pub(crate) verifier: &'a str,
}

/// Exchanges the code at the token endpoint (`client_secret_basic`, or
/// `client_secret_post` when that is all the provider supports).
pub(crate) async fn exchange(
    http: &HttpClient,
    discovery: &Discovery,
    request: &Exchange<'_>,
) -> Result<Tokens, SsoError> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("code", request.code.to_owned()),
        ("redirect_uri", request.redirect_uri.to_owned()),
        ("code_verifier", request.verifier.to_owned()),
    ];
    let mut builder = http
        .post(&discovery.token_endpoint)
        .header("accept", "application/json");
    let post_only = !discovery.token_endpoint_auth_methods_supported.is_empty()
        && !discovery
            .token_endpoint_auth_methods_supported
            .iter()
            .any(|m| m == "client_secret_basic");
    match request.client_secret {
        Some(secret) if !post_only => {
            let encode =
                |v: &str| form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>();
            let credentials = format!("{}:{}", encode(request.client_id), encode(secret));
            builder = builder.header(
                "authorization",
                format!("Basic {}", STANDARD.encode(credentials)),
            );
        }
        Some(secret) => {
            form.push(("client_id", request.client_id.to_owned()));
            form.push(("client_secret", secret.to_owned()));
        }
        None => form.push(("client_id", request.client_id.to_owned())),
    }
    let response = builder
        .form(&form)
        .send()
        .await
        .map_err(|error| unavailable("the token endpoint", error))?;
    let body = token_body(&response)?;
    let id_token = body
        .get("id_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| SsoError::Provider("no ID token in the token response".to_owned()))?
        .to_owned();
    Ok(Tokens {
        id_token,
        access_token: body
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn token_body(response: &Response) -> Result<Value, SsoError> {
    let status = response.status();
    if status.is_server_error() {
        return Err(unavailable(
            "the token endpoint",
            format!("answered {}", status.as_u16()),
        ));
    }
    let body: Value = response.json().unwrap_or(Value::Null);
    if let Some(error) = body.get("error").and_then(Value::as_str) {
        return Err(SsoError::Provider(error.to_owned()));
    }
    if !status.is_success() {
        return Err(SsoError::Provider(format!(
            "the token endpoint answered {}",
            status.as_u16()
        )));
    }
    Ok(body)
}

/// What an ID token is checked against.
pub(crate) struct IdTokenCheck<'a> {
    pub(crate) client_id: &'a str,
    pub(crate) nonce: &'a str,
    pub(crate) now: DateTime<Utc>,
    pub(crate) skew: Duration,
}

/// Verifies an ID token (signature with the provider's key set; issuer,
/// audience, `azp`, expiry, `iat` and nonce) and returns its claims.
pub(crate) async fn verify_id_token(
    http: &HttpClient,
    cache: &OidcCache,
    discovery: &Discovery,
    id_token: &str,
    check: &IdTokenCheck<'_>,
) -> Result<Map<String, Value>, SsoError> {
    let invalid = |reason: &str| SsoError::InvalidIdToken(reason.to_owned());
    let jws = Jws::parse(id_token).map_err(|e| SsoError::InvalidIdToken(e.to_string()))?;
    let claims: Map<String, Value> = jws
        .claims()
        .map_err(|_| invalid("the claims are not a JSON object"))?;
    let keys = key_set(
        http,
        cache,
        &discovery.jwks_uri,
        jws.header().kid.as_deref(),
    )
    .await?;
    // RS*, PS*, ES256, ES384 or EdDSA: a key set never verifies `none` or
    // an HMAC token.
    jws.verify_with(&keys)
        .map_err(|e| SsoError::InvalidIdToken(e.to_string()))?;

    let iss = claims
        .get("iss")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !same_issuer(iss, &discovery.issuer) {
        return Err(invalid("unexpected issuer"));
    }
    let audience_ok = match claims.get("aud") {
        Some(Value::String(aud)) => aud == check.client_id,
        Some(Value::Array(auds)) => {
            auds.iter().any(|a| a.as_str() == Some(check.client_id))
                && (auds.len() == 1
                    || claims.get("azp").and_then(Value::as_str) == Some(check.client_id))
        }
        _ => false,
    };
    if !audience_ok {
        return Err(invalid("the token is for another client"));
    }
    if let Some(azp) = claims.get("azp").and_then(Value::as_str)
        && azp != check.client_id
    {
        return Err(invalid("the token was issued to another party"));
    }
    let now = check.now.timestamp();
    let skew = i64::try_from(check.skew.as_secs()).unwrap_or(i64::MAX);
    let exp = claims.get("exp").and_then(Value::as_i64).unwrap_or(0);
    if exp.saturating_add(skew) <= now {
        return Err(invalid("the token has expired"));
    }
    if let Some(iat) = claims.get("iat").and_then(Value::as_i64)
        && iat.saturating_sub(skew) > now
    {
        return Err(invalid("the token was issued in the future"));
    }
    if let Some(nbf) = claims.get("nbf").and_then(Value::as_i64)
        && nbf.saturating_sub(skew) > now
    {
        return Err(invalid("the token is not valid yet"));
    }
    if claims.get("nonce").and_then(Value::as_str) != Some(check.nonce) {
        return Err(invalid("the nonce does not match"));
    }
    if claims
        .get("sub")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(invalid("the token has no subject"));
    }
    Ok(claims)
}

/// The key set at `url`, fetched when it is not cached, older than an
/// hour, or lacks `kid` (key rotation, at most every 30 seconds).
async fn key_set(
    http: &HttpClient,
    cache: &OidcCache,
    url: &str,
    kid: Option<&str>,
) -> Result<JwkSet, SsoError> {
    if let Some((set, fetched)) = cache
        .jwks
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(url)
    {
        let fresh = fetched.elapsed() < CACHE_TTL;
        let knows_kid = kid.is_none_or(|kid| set.has_kid(kid));
        if fresh && (knows_kid || fetched.elapsed() < JWKS_MIN_REFETCH) {
            return Ok(set.clone());
        }
    }
    let set: JwkSet = get_json(http, url, "the provider's key set").await?;
    cache
        .jwks
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(url.to_owned(), (set.clone(), Instant::now()));
    Ok(set)
}

/// The userinfo claims (for providers that leave the email out of the ID
/// token), merged into `claims` when they are about the same `sub`.
pub(crate) async fn merge_userinfo(
    http: &HttpClient,
    discovery: &Discovery,
    access_token: &str,
    claims: &mut Map<String, Value>,
) -> Result<(), SsoError> {
    let Some(url) = &discovery.userinfo_endpoint else {
        return Ok(());
    };
    let response = http
        .get(url)
        .bearer(access_token)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| unavailable("the userinfo endpoint", error))?;
    if !response.is_success() {
        return Err(unavailable(
            "the userinfo endpoint",
            format!("answered {}", response.status().as_u16()),
        ));
    }
    let info: Map<String, Value> = response
        .json()
        .map_err(|_| unavailable("the userinfo endpoint", "the answer is not JSON"))?;
    if info.get("sub") != claims.get("sub") {
        return Ok(());
    }
    for (key, value) in info {
        claims.entry(key).or_insert(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc_7636() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_authorization_url_carries_pkce_and_the_nonce() {
        let discovery = Discovery {
            issuer: "https://login.acme.test".to_owned(),
            authorization_endpoint: "https://login.acme.test/authorize?tenant=1".to_owned(),
            token_endpoint: "https://login.acme.test/token".to_owned(),
            jwks_uri: "https://login.acme.test/keys".to_owned(),
            userinfo_endpoint: None,
            token_endpoint_auth_methods_supported: Vec::new(),
        };
        let url = authorize_url(
            &discovery,
            &Authorize {
                client_id: "app",
                redirect_uri: "https://api.acme.test/api/sso/oidc/acme/callback",
                scopes: &["openid".to_owned(), "email".to_owned()],
                state: "st",
                nonce: "no",
                verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            },
        )
        .unwrap();
        let url = url::Url::parse(&url).unwrap();
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(query["tenant"], "1");
        assert_eq!(query["response_type"], "code");
        assert_eq!(query["scope"], "openid email");
        assert_eq!(query["nonce"], "no");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(
            query["code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
