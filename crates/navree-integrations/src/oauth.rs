//! Talking to providers: authorization URLs, token requests (authorization
//! code, refresh, client credentials and service-account assertions),
//! revocation, and the [`TokenBackend`] that the fake replaces.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use navree_http_client::{HttpClient, HttpClientError, Response};
use navree_kernel::BoxFuture;
use serde_json::{Value, json};
use url::Url;

use crate::entry::{GRAPH_DEFAULT_SCOPE, ProviderEntry};
use crate::options::{ClientAuth, MicrosoftMode};
use crate::{IntegrationsError, ProviderType, ServiceAccountKey};

/// What a token endpoint gave.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Grant {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    /// Lifetime in seconds (`None`: it does not expire).
    pub(crate) expires_in: Option<i64>,
    pub(crate) scopes: Option<Vec<String>>,
    pub(crate) external_id: Option<String>,
    pub(crate) display_name: Option<String>,
}

impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grant")
            .field("has_refresh_token", &self.refresh_token.is_some())
            .field("expires_in", &self.expires_in)
            .field("scopes", &self.scopes)
            .field("external_id", &self.external_id)
            .field("display_name", &self.display_name)
            .finish_non_exhaustive()
    }
}

/// What to ask a token endpoint for.
pub(crate) enum Exchange<'a> {
    /// The code from a consent screen.
    Code {
        code: &'a str,
        verifier: Option<&'a str>,
        redirect_uri: &'a str,
        scopes: &'a [String],
    },
    /// A new access token for a refresh token.
    Refresh {
        refresh_token: &'a str,
        scopes: &'a [String],
    },
    /// An app-only token for a tenant (Microsoft client credentials).
    ClientCredentials { tenant: &'a str },
    /// A service-account token (an RS256 assertion, RFC 7523).
    JwtBearer {
        key: &'a ServiceAccountKey,
        subject: Option<&'a str>,
        scopes: &'a [String],
        now: DateTime<Utc>,
    },
}

impl Exchange<'_> {
    /// The grant's name, as a fake records it.
    pub(crate) fn grant_type(&self) -> &'static str {
        match self {
            Exchange::Code { .. } => "authorization_code",
            Exchange::Refresh { .. } => "refresh_token",
            Exchange::ClientCredentials { .. } => "client_credentials",
            Exchange::JwtBearer { .. } => "jwt_bearer",
        }
    }
}

/// Where token requests go: the providers over HTTP, or a fake.
pub(crate) trait TokenBackend: Send + Sync + 'static {
    /// Asks `entry`'s token endpoint for a grant (`scope_set` picks the app
    /// registration).
    fn grant<'a>(
        &'a self,
        entry: &'a ProviderEntry,
        scope_set: &'a str,
        exchange: Exchange<'a>,
    ) -> BoxFuture<'a, Result<Grant, IntegrationsError>>;

    /// Revokes `token` at the provider (best effort).
    fn revoke<'a>(
        &'a self,
        entry: &'a ProviderEntry,
        scope_set: &'a str,
        token: &'a str,
    ) -> BoxFuture<'a, Result<(), IntegrationsError>>;
}

/// The real backend: the providers' token endpoints over the app's HTTP
/// client.
pub(crate) struct HttpBackend {
    pub(crate) http: HttpClient,
}

impl TokenBackend for HttpBackend {
    fn grant<'a>(
        &'a self,
        entry: &'a ProviderEntry,
        scope_set: &'a str,
        exchange: Exchange<'a>,
    ) -> BoxFuture<'a, Result<Grant, IntegrationsError>> {
        Box::pin(async move { request_grant(&self.http, entry, scope_set, exchange).await })
    }

    fn revoke<'a>(
        &'a self,
        entry: &'a ProviderEntry,
        scope_set: &'a str,
        token: &'a str,
    ) -> BoxFuture<'a, Result<(), IntegrationsError>> {
        Box::pin(async move {
            let Some(endpoint) = entry.revoke_endpoint() else {
                return Ok(());
            };
            let request = if entry.kind == ProviderType::Slack {
                self.http.post(&endpoint).bearer(token)
            } else {
                let mut form = vec![("token", token.to_owned())];
                let request = self.http.post(&endpoint);
                let request = match entry.client(scope_set) {
                    Ok(client) => client_auth(entry, request, &mut form, client),
                    Err(_) => request,
                };
                request.form(&form)
            };
            let response = request.send().await.map_err(unavailable(entry))?;
            if response.status().is_server_error() {
                return Err(IntegrationsError::Unavailable {
                    provider: entry.name.clone(),
                    message: format!("revocation answered {}", response.status().as_u16()),
                });
            }
            Ok(())
        })
    }
}

fn unavailable(entry: &ProviderEntry) -> impl FnOnce(HttpClientError) -> IntegrationsError + '_ {
    move |error| IntegrationsError::Unavailable {
        provider: entry.name.clone(),
        message: error.to_string(),
    }
}

/// The consent URL of a connect flow.
pub(crate) fn authorize_url(
    entry: &ProviderEntry,
    scope_set: &str,
    scopes: &[String],
    redirect_uri: &str,
    state: &str,
    challenge: Option<&str>,
) -> Result<String, IntegrationsError> {
    let (client_id, _) = entry.client(scope_set)?;
    let mut url = Url::parse(&entry.authorize_endpoint()).map_err(|_| {
        IntegrationsError::Config(format!(
            "integrations.providers.{}.authorize_url is not a URL",
            entry.key
        ))
    })?;
    {
        let mut query = url.query_pairs_mut();
        let admin_consent =
            entry.kind == ProviderType::Microsoft && entry.config.mode == MicrosoftMode::App;
        if !admin_consent {
            query.append_pair("response_type", "code");
        }
        query.append_pair("client_id", &client_id);
        query.append_pair("redirect_uri", redirect_uri);
        let mut requested: Vec<String> = Vec::new();
        match entry.kind {
            ProviderType::Microsoft if admin_consent => {
                if scopes.is_empty() {
                    requested.push(GRAPH_DEFAULT_SCOPE.to_owned());
                }
            }
            ProviderType::Microsoft => requested.push("offline_access".to_owned()),
            _ => {}
        }
        requested.extend(scopes.iter().cloned());
        if !requested.is_empty() {
            query.append_pair("scope", &requested.join(entry.scope_separator()));
        }
        query.append_pair("state", state);
        if let Some(challenge) = challenge {
            query.append_pair("code_challenge", challenge);
            query.append_pair("code_challenge_method", "S256");
        }
        match entry.kind {
            ProviderType::Google => {
                query.append_pair("access_type", "offline");
                query.append_pair("prompt", "consent");
                query.append_pair("include_granted_scopes", "true");
            }
            ProviderType::Microsoft if !admin_consent => {
                query.append_pair("response_mode", "query");
            }
            _ => {}
        }
        for (name, value) in &entry.config.params {
            query.append_pair(name, value);
        }
    }
    Ok(url.into())
}

/// Adds the client's credentials to a token request.
fn client_auth(
    entry: &ProviderEntry,
    request: navree_http_client::RequestBuilder,
    form: &mut Vec<(&'static str, String)>,
    (client_id, secret): (String, Option<String>),
) -> navree_http_client::RequestBuilder {
    match (entry.config.client_auth, secret) {
        (ClientAuth::Basic, Some(secret)) => {
            let encode =
                |v: &str| form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>();
            let credentials = format!("{}:{}", encode(&client_id), encode(&secret));
            request.header(
                "authorization",
                format!("Basic {}", STANDARD.encode(credentials)),
            )
        }
        (_, secret) => {
            form.push(("client_id", client_id));
            if let Some(secret) = secret {
                form.push(("client_secret", secret));
            }
            request
        }
    }
}

/// One token request over HTTP.
async fn request_grant(
    http: &HttpClient,
    entry: &ProviderEntry,
    scope_set: &str,
    exchange: Exchange<'_>,
) -> Result<Grant, IntegrationsError> {
    let mut form: Vec<(&'static str, String)> = Vec::new();
    let mut endpoint = entry.token_endpoint(None);
    let mut with_client = true;
    let mut external_id = None;
    let mut display_name = None;
    match exchange {
        Exchange::Code {
            code,
            verifier,
            redirect_uri,
            scopes,
        } => {
            form.push(("grant_type", "authorization_code".to_owned()));
            form.push(("code", code.to_owned()));
            form.push(("redirect_uri", redirect_uri.to_owned()));
            if let Some(verifier) = verifier {
                form.push(("code_verifier", verifier.to_owned()));
            }
            if entry.kind == ProviderType::Microsoft {
                form.push(("scope", microsoft_scope(scopes)));
            }
        }
        Exchange::Refresh {
            refresh_token,
            scopes,
        } => {
            form.push(("grant_type", "refresh_token".to_owned()));
            form.push(("refresh_token", refresh_token.to_owned()));
            if entry.kind == ProviderType::Microsoft {
                form.push(("scope", microsoft_scope(scopes)));
            }
        }
        Exchange::ClientCredentials { tenant } => {
            endpoint = entry.token_endpoint(Some(tenant));
            form.push(("grant_type", "client_credentials".to_owned()));
            form.push(("scope", GRAPH_DEFAULT_SCOPE.to_owned()));
            external_id = Some(tenant.to_owned());
        }
        Exchange::JwtBearer {
            key,
            subject,
            scopes,
            now,
        } => {
            let assertion = service_account_assertion(&endpoint, key, subject, scopes, now)?;
            form.push((
                "grant_type",
                "urn:ietf:params:oauth:grant-type:jwt-bearer".to_owned(),
            ));
            form.push(("assertion", assertion));
            with_client = false;
            external_id = subject
                .or(Some(key.client_email.as_str()))
                .and_then(|email| email.rsplit_once('@'))
                .map(|(_, domain)| domain.to_owned());
            display_name = Some(key.client_email.clone());
        }
    }
    let mut request = http.post(&endpoint).header("accept", "application/json");
    if with_client {
        let client = entry.client(scope_set)?;
        request = client_auth(entry, request, &mut form, client);
    }
    let response = request
        .form(&form)
        .retries(0)
        .send()
        .await
        .map_err(unavailable(entry))?;
    let mut grant = if entry.kind == ProviderType::Slack {
        slack_grant(entry, &response)?
    } else {
        oauth_grant(entry, &response)?
    };
    grant.external_id = grant.external_id.or(external_id);
    grant.display_name = grant.display_name.or(display_name);
    Ok(grant)
}

/// The scope parameter of Microsoft's delegated token requests.
fn microsoft_scope(scopes: &[String]) -> String {
    let mut all = vec!["offline_access".to_owned()];
    if scopes.is_empty() {
        all.push(GRAPH_DEFAULT_SCOPE.to_owned());
    }
    all.extend(scopes.iter().cloned());
    all.join(" ")
}

/// The signed assertion of a service-account token request: RS256 over
/// `{iss, sub?, scope, aud, iat, exp}`, valid for an hour.
pub(crate) fn service_account_assertion(
    audience: &str,
    key: &ServiceAccountKey,
    subject: Option<&str>,
    scopes: &[String],
    now: DateTime<Utc>,
) -> Result<String, IntegrationsError> {
    let iat = now.timestamp();
    let mut claims = json!({
        "iss": key.client_email,
        "scope": scopes.join(" "),
        "aud": audience,
        "iat": iat,
        "exp": iat + 3600,
    });
    if let Some(subject) = subject {
        claims["sub"] = Value::String(subject.to_owned());
    }
    key.signing_key()?.sign_jwt(&claims).map_err(|_| {
        IntegrationsError::InvalidCredentials(
            "the service account's private key can't sign".to_owned(),
        )
    })
}

/// Reads a standard OAuth token response (RFC 6749 §5).
fn oauth_grant(entry: &ProviderEntry, response: &Response) -> Result<Grant, IntegrationsError> {
    let status = response.status();
    let body: Option<Value> = response.json().ok();
    if status.is_server_error() || status.as_u16() == 429 {
        return Err(IntegrationsError::Unavailable {
            provider: entry.name.clone(),
            message: format!("the token endpoint answered {}", status.as_u16()),
        });
    }
    if !status.is_success() {
        let error = body
            .as_ref()
            .and_then(|b| b.get("error"))
            .and_then(Value::as_str)
            .map_or_else(|| format!("http_{}", status.as_u16()), str::to_owned);
        let description = body
            .as_ref()
            .and_then(|b| b.get("error_description"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        return Err(IntegrationsError::Refused {
            provider: entry.name.clone(),
            error,
            description,
        });
    }
    let body = body.ok_or_else(|| IntegrationsError::Unavailable {
        provider: entry.name.clone(),
        message: "the token endpoint answered without JSON".to_owned(),
    })?;
    parse_token(entry, &body)
}

/// The fields of a token response.
pub(crate) fn parse_token(entry: &ProviderEntry, body: &Value) -> Result<Grant, IntegrationsError> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| IntegrationsError::Refused {
            provider: entry.name.clone(),
            error: "no_access_token".to_owned(),
            description: None,
        })?
        .to_owned();
    let expires_in = match body.get("expires_in") {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    };
    let scopes = body.get("scope").and_then(Value::as_str).map(|scope| {
        scope
            .split([' ', ','])
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    });
    Ok(Grant {
        access_token,
        refresh_token: body
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
        expires_in,
        scopes,
        external_id: None,
        display_name: None,
    })
}

/// Slack errors that mean "try again later", not "refused".
const SLACK_TRANSIENT: [&str; 5] = [
    "ratelimited",
    "internal_error",
    "fatal_error",
    "request_timeout",
    "service_unavailable",
];

/// Reads Slack's `oauth.v2.access` answer: `{ok, access_token, team, ...}`
/// with status 200 even for errors.
fn slack_grant(entry: &ProviderEntry, response: &Response) -> Result<Grant, IntegrationsError> {
    let status = response.status();
    if status.is_server_error() || status.as_u16() == 429 {
        return Err(IntegrationsError::Unavailable {
            provider: entry.name.clone(),
            message: format!("oauth.v2.access answered {}", status.as_u16()),
        });
    }
    let body: Value = response.json().map_err(|_| IntegrationsError::Unavailable {
        provider: entry.name.clone(),
        message: "oauth.v2.access answered without JSON".to_owned(),
    })?;
    slack_body(entry, &body)
}

/// The grant in a Slack OAuth body.
pub(crate) fn slack_body(entry: &ProviderEntry, body: &Value) -> Result<Grant, IntegrationsError> {
    if body.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error")
            .to_owned();
        if SLACK_TRANSIENT.contains(&error.as_str()) {
            return Err(IntegrationsError::Unavailable {
                provider: entry.name.clone(),
                message: error,
            });
        }
        return Err(IntegrationsError::Refused {
            provider: entry.name.clone(),
            error,
            description: None,
        });
    }
    // An install with only user scopes carries the token under `authed_user`.
    let mut token_body = body.clone();
    if token_body.get("access_token").is_none()
        && let Some(user) = body.get("authed_user")
    {
        token_body = user.clone();
    }
    let mut grant = parse_token(entry, &token_body)?;
    grant.external_id = body
        .pointer("/team/id")
        .or_else(|| body.pointer("/enterprise/id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    grant.display_name = body
        .pointer("/team/name")
        .or_else(|| body.pointer("/enterprise/name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(grant)
}

#[cfg(test)]
mod tests {
    use navree_crypto::jose::{Jws, VerifyingKey};

    use super::*;
    use crate::ProviderConfig;

    const PEM: &str = include_str!("../tests/fixtures/rsa-2048.pem");

    fn entry(key: &str, f: impl FnOnce(&mut ProviderConfig)) -> ProviderEntry {
        let mut config = ProviderConfig {
            client_id: Some("client-1".into()),
            ..ProviderConfig::default()
        };
        f(&mut config);
        ProviderEntry::new(key, &config).unwrap()
    }

    fn query(url: &str) -> std::collections::BTreeMap<String, String> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    #[test]
    fn microsoft_admin_consent_urls_ask_for_the_scope_set() {
        let ms = entry("microsoft", |_| {});
        let url = authorize_url(&ms, "default", &[], "https://app/cb/microsoft", "nvi1.s", None)
            .unwrap();
        assert!(url.starts_with(
            "https://login.microsoftonline.com/organizations/v2.0/adminconsent?"
        ));
        let q = query(&url);
        assert_eq!(q["client_id"], "client-1");
        assert_eq!(q["scope"], "https://graph.microsoft.com/.default");
        assert_eq!(q["state"], "nvi1.s");
        assert!(!q.contains_key("response_type"));
        let scoped = authorize_url(
            &ms,
            "directory",
            &["https://graph.microsoft.com/User.Read.All".to_owned()],
            "https://app/cb/microsoft",
            "s",
            None,
        )
        .unwrap();
        assert_eq!(query(&scoped)["scope"], "https://graph.microsoft.com/User.Read.All");
    }

    #[test]
    fn oauth_urls_carry_pkce_offline_access_and_extra_params() {
        let google = entry("google", |c| {
            c.params.insert("hd".into(), "acme.test".into());
        });
        let url = authorize_url(
            &google,
            "default",
            &["openid".into(), "email".into()],
            "https://app/cb/google",
            "st",
            Some("chal"),
        )
        .unwrap();
        let q = query(&url);
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["scope"], "openid email");
        assert_eq!(q["code_challenge"], "chal");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");
        assert_eq!(q["hd"], "acme.test");

        let slack = entry("slack", |_| {});
        let url = authorize_url(
            &slack,
            "default",
            &["chat:write".into(), "users:read".into()],
            "https://app/cb/slack",
            "st",
            None,
        )
        .unwrap();
        assert_eq!(query(&url)["scope"], "chat:write,users:read");
    }

    #[test]
    fn token_responses_parse_numbers_strings_and_scopes() {
        let e = entry("hub", |c| c.kind = Some(ProviderType::OAuth2));
        let grant = parse_token(
            &e,
            &json!({ "access_token": "a", "refresh_token": "r", "expires_in": "3599", "scope": "x y" }),
        )
        .unwrap();
        assert_eq!(grant.expires_in, Some(3599));
        assert_eq!(grant.scopes, Some(vec!["x".to_owned(), "y".to_owned()]));
        assert_eq!(grant.refresh_token.as_deref(), Some("r"));
        assert!(!format!("{grant:?}").contains("\"a\""));
        assert!(parse_token(&e, &json!({ "token_type": "bearer" })).is_err());
    }

    #[test]
    fn slack_bodies_give_the_team_and_tell_refusals_from_outages() {
        let slack = entry("slack", |_| {});
        let grant = slack_body(
            &slack,
            &json!({
                "ok": true, "access_token": "xoxb-1", "scope": "chat:write,users:read",
                "team": { "id": "T0123", "name": "Acme" }
            }),
        )
        .unwrap();
        assert_eq!(grant.external_id.as_deref(), Some("T0123"));
        assert_eq!(grant.display_name.as_deref(), Some("Acme"));
        assert_eq!(grant.scopes.unwrap().len(), 2);
        assert!(matches!(
            slack_body(&slack, &json!({ "ok": false, "error": "invalid_code" })),
            Err(IntegrationsError::Refused { .. })
        ));
        assert!(matches!(
            slack_body(&slack, &json!({ "ok": false, "error": "ratelimited" })),
            Err(IntegrationsError::Unavailable { .. })
        ));
        let user_only = slack_body(
            &slack,
            &json!({ "ok": true, "authed_user": { "id": "U1", "access_token": "xoxp-1" }, "team": { "id": "T1" } }),
        )
        .unwrap();
        assert_eq!(user_only.access_token, "xoxp-1");
    }

    #[test]
    fn service_account_assertions_are_rs256_jwts() {
        let key = ServiceAccountKey {
            client_email: "navree@acme-prod.iam.gserviceaccount.com".into(),
            private_key: PEM.into(),
            private_key_id: Some("kid-1".into()),
            client_id: None,
            project_id: None,
        };
        let now = DateTime::parse_from_rfc3339("2026-10-27T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let jwt = service_account_assertion(
            "https://oauth2.googleapis.com/token",
            &key,
            Some("admin@acme.test"),
            &["https://www.googleapis.com/auth/admin.directory.user.readonly".into()],
            now,
        )
        .unwrap();
        let jws = Jws::parse(&jwt).unwrap();
        assert_eq!(jws.header().kid.as_deref(), Some("kid-1"));
        let public = key.signing_key().unwrap().verifying_key();
        jws.verify(&public).unwrap();
        let _: &VerifyingKey = &public;
        let claims: Value = jws.claims().unwrap();
        assert_eq!(claims["iss"], "navree@acme-prod.iam.gserviceaccount.com");
        assert_eq!(claims["sub"], "admin@acme.test");
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(), 3600);
    }
}
