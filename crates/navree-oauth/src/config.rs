//! [`OAuthOptions`]: the `oauth` configuration section.

use std::collections::BTreeMap;
use std::fmt;

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::scope::{BUILT_IN_SCOPES, check_permission, check_scope_name};

/// A scope the app defines under `oauth.scopes.<name>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScopeOptions {
    /// What the scope allows, as the consent page shows it ("Read your
    /// deals"). Required.
    pub description: String,
    /// The permission strings the scope grants (`deals.read`, `deals.*`,
    /// or `*` for everything the user may do). A list, or a comma-separated
    /// string (from an environment variable).
    #[serde(deserialize_with = "list_or_csv")]
    pub permissions: Vec<String>,
}

impl ScopeOptions {
    /// A scope described as `description`, granting `permissions`.
    pub fn new<I, S>(description: impl Into<String>, permissions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            description: description.into(),
            permissions: permissions.into_iter().map(Into::into).collect(),
        }
    }
}

/// Personal access token settings (`oauth.personal_access_tokens`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonalAccessTokenOptions {
    /// Whether the module mounts `/api/oauth/personal-access-tokens`
    /// (default true). The `PersonalAccessTokens` service works either way.
    pub enabled: bool,
    /// The longest lifetime, in days (default 365); `0` allows tokens that
    /// never expire.
    pub max_days: u32,
}

impl Default for PersonalAccessTokenOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            max_days: 365,
        }
    }
}

/// The limits of one protocol endpoint (`oauth.rate_limits.<endpoint>`),
/// in requests per minute; `0` turns a limit off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EndpointLimit {
    /// Requests per minute of one client, counted once it has
    /// authenticated (so requests naming a client with a wrong secret
    /// can't use up its budget). Public clients have no secret, so anyone
    /// can name one: they are limited per IP only.
    pub per_client: u32,
    /// Requests per minute from one client IP address, counted before
    /// anything is checked (failed client authentications included).
    pub per_ip: u32,
}

impl EndpointLimit {
    /// `per_client` and `per_ip` requests per minute.
    pub const fn new(per_client: u32, per_ip: u32) -> Self {
        Self { per_client, per_ip }
    }
}

impl Default for EndpointLimit {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// Rate limits of the protocol endpoints that take client credentials
/// (`oauth.rate_limits`). Each instance of the app counts on its own. A
/// request over a limit gets 429 with `Retry-After` and the OAuth error
/// `too_many_requests`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitOptions {
    /// `POST {path}/token`: 300 per client, 120 per IP.
    pub token: EndpointLimit,
    /// `POST {path}/introspect`: 1200 per client, 600 per IP (resource
    /// servers introspect on every request they don't cache).
    pub introspect: EndpointLimit,
    /// `POST {path}/revoke`: 300 per client, 60 per IP.
    pub revoke: EndpointLimit,
    /// `POST {path}/device_authorization`: 60 per client, 30 per IP.
    pub device_authorization: EndpointLimit,
}

impl Default for RateLimitOptions {
    fn default() -> Self {
        Self {
            token: EndpointLimit::new(300, 120),
            introspect: EndpointLimit::new(1200, 600),
            revoke: EndpointLimit::new(300, 60),
            device_authorization: EndpointLimit::new(60, 30),
        }
    }
}

/// Options of the OAuth server (configuration section `oauth`).
///
/// ```toml
/// [oauth]
/// issuer = "https://api.acme.test"                    # defaults to app.url
/// path = "/oauth"                                     # protocol endpoints
/// consent_url = "https://app.acme.test/oauth/consent" # your consent page
/// device_url = "https://app.acme.test/device"         # your device page
/// access_token_ttl_secs = 3600
/// refresh_token_ttl_secs = 2592000                    # 30 days, sliding
/// request_ttl_secs = 600                              # requests and device codes
/// code_ttl_secs = 60
/// token_cache_secs = 10
/// # signing_key = "ed25519:MC4CAQAwBQYDK2VwBCIEI..."  # navree oauth:keys
/// # previous_signing_keys = ["ed25519:..."]
/// dynamic_registration = false                        # POST {path}/register
///
/// [oauth.personal_access_tokens]
/// enabled = true
/// max_days = 365                                      # 0 = no limit
///
/// [oauth.rate_limits]                                 # requests per minute; 0 = off
/// token = { per_client = 300, per_ip = 120 }
/// introspect = { per_client = 1200, per_ip = 600 }
/// revoke = { per_client = 300, per_ip = 60 }
/// device_authorization = { per_client = 60, per_ip = 30 }
///
/// [oauth.scopes."deals:read"]
/// description = "Read your deals"
/// permissions = ["deals.read"]
/// ```
///
/// Environment variables map as usual: `APP_OAUTH__CONSENT_URL` →
/// `oauth.consent_url`, `APP_OAUTH__ACCESS_TOKEN_TTL_SECS=1800`,
/// `APP_OAUTH__RATE_LIMITS__TOKEN__PER_IP=600`; lists also accept a
/// comma-separated string.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct OAuthOptions {
    /// The `iss` of ID tokens and the base of the discovery URLs. Defaults
    /// to `app.url`.
    pub issuer: Option<String>,
    /// Where the protocol endpoints are mounted (outside
    /// `http.api_prefix`).
    pub path: String,
    /// The front end's consent page; required for the authorization code
    /// flow. `GET {path}/authorize` redirects to `{consent_url}?request=<id>`.
    pub consent_url: Option<String>,
    /// The front end's device page; required for the device grant. It is
    /// the `verification_uri` shown on the device.
    pub device_url: Option<String>,
    /// The scopes the app defines, by name. `openid`, `profile`, `email`
    /// and `offline_access` are built in.
    pub scopes: BTreeMap<String, ScopeOptions>,
    /// Access token lifetime, in seconds.
    pub access_token_ttl_secs: u64,
    /// Refresh token lifetime, in seconds; every refresh starts it again.
    pub refresh_token_ttl_secs: u64,
    /// Authorization request and device code lifetime, in seconds.
    pub request_ttl_secs: u64,
    /// Authorization code lifetime, in seconds.
    pub code_ttl_secs: u64,
    /// How long each instance caches a token lookup, in seconds (`0`
    /// turns the cache off).
    pub token_cache_secs: u64,
    /// The Ed25519 key that signs ID tokens (`ed25519:<base64 PKCS#8 or
    /// seed>`, from `navree oauth:keys`). Derived from `app.key` when unset.
    pub signing_key: Option<Secret<String>>,
    /// Older signing keys, still published in the JWKS so ID tokens issued
    /// before a rotation verify. A list, or a comma-separated string.
    #[serde(deserialize_with = "list_or_csv")]
    pub previous_signing_keys: Vec<Secret<String>>,
    /// Personal access tokens.
    pub personal_access_tokens: PersonalAccessTokenOptions,
    /// Whether clients may register themselves (`POST {path}/register`,
    /// RFC 7591). Off by default.
    pub dynamic_registration: bool,
    /// Rate limits of the token, introspection, revocation and device
    /// authorization endpoints, per client and per client IP.
    pub rate_limits: RateLimitOptions,
}

impl Default for OAuthOptions {
    fn default() -> Self {
        Self {
            issuer: None,
            path: "/oauth".to_owned(),
            consent_url: None,
            device_url: None,
            scopes: BTreeMap::new(),
            access_token_ttl_secs: 3600,
            refresh_token_ttl_secs: 30 * 24 * 60 * 60,
            request_ttl_secs: 600,
            code_ttl_secs: 60,
            token_cache_secs: 10,
            signing_key: None,
            previous_signing_keys: Vec::new(),
            personal_access_tokens: PersonalAccessTokenOptions::default(),
            dynamic_registration: false,
            rate_limits: RateLimitOptions::default(),
        }
    }
}

impl OAuthOptions {
    /// Adds the scope `name` (builder style, for `for_root`).
    pub fn scope(mut self, name: impl Into<String>, scope: ScopeOptions) -> Self {
        self.scopes.insert(name.into(), scope);
        self
    }

    /// Every problem with the options, naming the configuration keys.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if let Some(issuer) = &self.issuer
            && !is_http_url(issuer)
        {
            errors.push("oauth.issuer must be an absolute http(s) URL".to_owned());
        }
        if !self.path.starts_with('/') || self.path.trim_matches('/').is_empty() {
            errors.push("oauth.path must start with `/` and name a segment, e.g. `/oauth`".into());
        }
        if self.path.contains(['{', '}', '?', '#', ' ']) {
            errors.push("oauth.path must be a plain path".to_owned());
        }
        for (key, url) in [
            ("consent_url", &self.consent_url),
            ("device_url", &self.device_url),
        ] {
            if let Some(url) = url
                && !is_http_url(url)
            {
                errors.push(format!("oauth.{key} must be an absolute http(s) URL"));
            }
        }
        for (name, scope) in &self.scopes {
            if BUILT_IN_SCOPES.iter().any(|(built_in, _)| built_in == name) {
                errors.push(format!(
                    "oauth.scopes: `{name}` is built in and can't be redefined"
                ));
                continue;
            }
            if let Err(reason) = check_scope_name(name) {
                errors.push(format!("oauth.scopes: `{name}` {reason}"));
            }
            if scope.description.trim().is_empty() {
                errors.push(format!("oauth.scopes.{name}.description is required"));
            }
            for permission in &scope.permissions {
                if let Err(reason) = check_permission(permission) {
                    errors.push(format!(
                        "oauth.scopes.{name}.permissions: `{permission}` {reason}"
                    ));
                }
            }
        }
        for (key, value) in [
            ("access_token_ttl_secs", self.access_token_ttl_secs),
            ("refresh_token_ttl_secs", self.refresh_token_ttl_secs),
            ("request_ttl_secs", self.request_ttl_secs),
            ("code_ttl_secs", self.code_ttl_secs),
        ] {
            if value == 0 {
                errors.push(format!("oauth.{key} must be at least 1"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The protocol path without a trailing slash (`/oauth`).
    pub fn base_path(&self) -> &str {
        self.path.trim_end_matches('/')
    }
}

impl ConfigSection for OAuthOptions {
    const KEY: &'static str = "oauth";

    fn validate(&self) -> Result<(), String> {
        self.check()
            .map_err(|errors| format!("invalid oauth options: {}", errors.join("; ")))
    }
}

/// Whether `url` is an absolute `http://` or `https://` URL with a host.
pub(crate) fn is_http_url(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .is_some_and(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.has_host())
}

/// Deserializes a list from a sequence or from a comma-separated string.
fn list_or_csv<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct ListOrCsv<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for ListOrCsv<T> {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list or a comma-separated string")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Vec<T>, E> {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| T::deserialize(de::value::StrDeserializer::<E>::new(item)))
                .collect()
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element()? {
                items.push(item);
            }
            Ok(items)
        }
    }

    deserializer.deserialize_any(ListOrCsv(std::marker::PhantomData))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(description: &str, permissions: &[&str]) -> ScopeOptions {
        ScopeOptions::new(description, permissions.iter().copied())
    }

    #[test]
    fn defaults_match_the_documentation() {
        let options = OAuthOptions::default();
        assert_eq!(options.path, "/oauth");
        assert_eq!(options.access_token_ttl_secs, 3600);
        assert_eq!(options.refresh_token_ttl_secs, 2_592_000);
        assert_eq!(options.request_ttl_secs, 600);
        assert_eq!(options.code_ttl_secs, 60);
        assert_eq!(options.token_cache_secs, 10);
        assert!(options.personal_access_tokens.enabled);
        assert_eq!(options.personal_access_tokens.max_days, 365);
        assert!(!options.dynamic_registration);
        assert!(options.check().is_ok());
    }

    #[test]
    fn invalid_options_name_their_keys() {
        let options = OAuthOptions {
            issuer: Some("api.acme.test".into()),
            path: "oauth".into(),
            consent_url: Some("/oauth/consent".into()),
            device_url: Some("ftp://x".into()),
            access_token_ttl_secs: 0,
            ..OAuthOptions::default()
        }
        .scope("deals:read", scope("", &["deals.read"]))
        .scope("deals:write", scope("Write deals", &["deals"]))
        .scope("openid", scope("Mine", &[]))
        .scope("bad scope", scope("Spaces", &[]));
        let errors = options.check().unwrap_err().join("\n");
        for needle in [
            "oauth.issuer",
            "oauth.path",
            "oauth.consent_url",
            "oauth.device_url",
            "oauth.access_token_ttl_secs",
            "oauth.scopes.deals:read.description is required",
            "oauth.scopes.deals:write.permissions: `deals`",
            "`openid` is built in",
            "`bad scope`",
        ] {
            assert!(errors.contains(needle), "{needle} missing in:\n{errors}");
        }
    }

    #[test]
    fn lists_come_from_arrays_or_comma_separated_strings() {
        let options: OAuthOptions = serde_json::from_value(serde_json::json!({
            "previous_signing_keys": "ed25519:a, ed25519:b",
            "scopes": {"deals:read": {"description": "Read", "permissions": "deals.read,companies.read"}}
        }))
        .unwrap();
        assert_eq!(options.previous_signing_keys.len(), 2);
        assert_eq!(options.previous_signing_keys[1].expose(), "ed25519:b");
        assert_eq!(
            options.scopes["deals:read"].permissions,
            ["deals.read", "companies.read"]
        );
    }
}
