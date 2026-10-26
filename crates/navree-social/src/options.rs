//! [`SocialOptions`] and [`ProviderConfig`]: the `social` configuration section.

use std::collections::BTreeMap;
use std::fmt;

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::{Deserialize, Deserializer};

use crate::provider::{Kind, apple_key};

/// Whether a provider's email address links to an existing user with the
/// same address (`social.link_by_email`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkByEmail {
    /// Link when the provider has verified the address (the default).
    #[default]
    Verified,
    /// Never link automatically: the user connects the provider from their
    /// settings.
    Never,
}

/// How a social sign-in treats users with two-factor authentication
/// (`social.two_factor`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TwoFactorPolicy {
    /// The same challenge as a password login (the default).
    #[default]
    Require,
    /// Trust the provider's own second factor.
    Skip,
}

/// Options of the social module (configuration section `social`).
///
/// ```toml
/// [social]
/// redirect_base = "https://app.acme.test/auth/callback"  # callbacks: {redirect_base}/{provider}
/// routes = true                   # /api/auth/social/...
/// # registration = true           # default: auth.registration
/// link_by_email = "verified"      # verified | never
/// allowed_email_domains = []      # e.g. ["acme.test"]; or "acme.test,acme.dev"
/// two_factor = "require"          # require | skip
/// state_ttl_secs = 600
/// store_tokens = false            # keep encrypted provider tokens for Social::token
///
/// [social.providers.github]
/// client_id = "Iv1.8a61f9b3a7aba766"
/// # client_secret from APP_SOCIAL__PROVIDERS__GITHUB__CLIENT_SECRET
/// ```
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SocialOptions {
    /// The front end's callback page: each provider's redirect URI is
    /// `{redirect_base}/{provider}`. Required; an absolute http(s) URL.
    pub redirect_base: String,
    /// Whether the module mounts the `/api/auth/social` routes.
    pub routes: bool,
    /// Whether a first sign-in creates a user; `None` follows
    /// `auth.registration`.
    pub registration: Option<bool>,
    /// Whether a verified provider email links to the user with that email.
    pub link_by_email: LinkByEmail,
    /// When not empty, only these email domains may sign in or sign up
    /// (with a verified address). A list, or a comma-separated string.
    #[serde(deserialize_with = "list_or_csv")]
    pub allowed_email_domains: Vec<String>,
    /// Two-factor authentication on social sign-ins.
    pub two_factor: TwoFactorPolicy,
    /// The lifetime of a sealed state, in seconds.
    pub state_ttl_secs: u64,
    /// Whether provider tokens are stored (encrypted) for
    /// [`Social::token`](crate::Social::token).
    pub store_tokens: bool,
    /// The providers, by key (`github`, `okta`).
    pub providers: BTreeMap<String, ProviderConfig>,
}

impl Default for SocialOptions {
    fn default() -> Self {
        Self {
            redirect_base: String::new(),
            routes: true,
            registration: None,
            link_by_email: LinkByEmail::Verified,
            allowed_email_domains: Vec::new(),
            two_factor: TwoFactorPolicy::Require,
            state_ttl_secs: 600,
            store_tokens: false,
            providers: BTreeMap::new(),
        }
    }
}

impl SocialOptions {
    /// Every problem with the options, naming the configuration keys (never
    /// a secret value).
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.redirect_base.trim().is_empty() {
            errors.push(
                "social.redirect_base is required (the front end's callback page, an absolute URL)"
                    .to_owned(),
            );
        } else if !is_http_url(&self.redirect_base) {
            errors.push("social.redirect_base must be an absolute http(s) URL".to_owned());
        }
        if self.state_ttl_secs == 0 {
            errors.push("social.state_ttl_secs must be at least 1".to_owned());
        }
        check_domains(
            "social.allowed_email_domains",
            &self.allowed_email_domains,
            &mut errors,
        );
        for (key, provider) in &self.providers {
            provider.check(key, &mut errors);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The configuration of provider `key`, with its key filled in.
    pub fn provider(&self, key: &str) -> Option<ProviderConfig> {
        self.providers.get(key).map(|config| ProviderConfig {
            key: key.to_owned(),
            ..config.clone()
        })
    }
}

impl ConfigSection for SocialOptions {
    const KEY: &'static str = "social";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// One provider (`social.providers.<key>`).
///
/// Every provider takes `client_id`, `client_secret` (from the
/// environment: `APP_SOCIAL__PROVIDERS__<KEY>__CLIENT_SECRET`), `scopes`,
/// `params` and `allowed_email_domains`. The rest depends on the type:
///
/// | Key | For |
/// |---|---|
/// | `type` | `google`, `github`, `apple`, `microsoft`, `gitlab`, `slack`, `linkedin`, `discord`, `x`, `oidc` or `custom`; default: the provider key |
/// | `name` | The name shown in messages (default: the provider's, or the key) |
/// | `hosted_domain` | Google: only this Google Workspace domain |
/// | `tenant` | Microsoft: `common` (default), `organizations`, `consumers` or a tenant id |
/// | `team` | Slack: only this workspace (team id) |
/// | `team_id`, `key_id`, `private_key` | Apple: the team, the key id and the `.p8` key (PEM) that signs the client secret |
/// | `url`, `api_url` | GitLab (`url`, default `https://gitlab.com`) and GitHub Enterprise Server (`url`, `api_url`) |
/// | `issuer` | `oidc`: the issuer whose discovery document is read |
/// | `authorize_url`, `token_url`, `userinfo_url`, `jwks_url` | Endpoint overrides (a proxy, a test server); `token_url` is required for `custom` |
/// | `pkce` | Send a PKCE challenge (default `true`) |
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// The provider key (`github`, `okta`), filled in from the table name.
    #[serde(skip)]
    pub key: String,
    /// The provider type; `None` means the key.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// The name shown in messages and the accounts list.
    pub name: Option<String>,
    /// The OAuth client id. Required.
    pub client_id: String,
    /// The OAuth client secret (keep it in `.env`). Apple's is generated
    /// from `private_key`.
    pub client_secret: Option<Secret<String>>,
    /// Extra scopes, requested in addition to the provider's defaults.
    #[serde(deserialize_with = "list_or_csv")]
    pub scopes: Vec<String>,
    /// Extra authorization URL parameters (`prompt = "consent"`).
    pub params: BTreeMap<String, String>,
    /// The issuer of an `oidc` provider (its discovery document is
    /// `{issuer}/.well-known/openid-configuration`); for the others, an
    /// override of the expected `iss` claim.
    pub issuer: Option<String>,
    /// When not empty, only these email domains may sign in or sign up with
    /// this provider.
    #[serde(deserialize_with = "list_or_csv")]
    pub allowed_email_domains: Vec<String>,
    /// Whether to send a PKCE challenge (S256).
    pub pkce: bool,
    /// Google: only accounts of this Google Workspace domain.
    pub hosted_domain: Option<String>,
    /// Microsoft: who may sign in (`common`, `organizations`, `consumers`
    /// or a tenant id).
    pub tenant: Option<String>,
    /// Slack: only members of this workspace (team id).
    pub team: Option<String>,
    /// GitLab: the instance (default `https://gitlab.com`); GitHub: a
    /// GitHub Enterprise Server.
    pub url: Option<String>,
    /// GitHub Enterprise Server: the API root (default `{url}/api/v3`).
    pub api_url: Option<String>,
    /// Apple: the team id.
    pub team_id: Option<String>,
    /// Apple: the id of the `.p8` key.
    pub key_id: Option<String>,
    /// Apple: the `.p8` private key (PEM; `\n` escapes are accepted).
    pub private_key: Option<Secret<String>>,
    /// Override of the authorization endpoint.
    pub authorize_url: Option<String>,
    /// Override of the token endpoint; required for `custom`.
    pub token_url: Option<String>,
    /// Override of the profile (userinfo) endpoint.
    pub userinfo_url: Option<String>,
    /// Override of the JSON Web Key Set the ID tokens are checked against.
    pub jwks_url: Option<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            key: String::new(),
            kind: None,
            name: None,
            client_id: String::new(),
            client_secret: None,
            scopes: Vec::new(),
            params: BTreeMap::new(),
            issuer: None,
            allowed_email_domains: Vec::new(),
            pkce: true,
            hosted_domain: None,
            tenant: None,
            team: None,
            url: None,
            api_url: None,
            team_id: None,
            key_id: None,
            private_key: None,
            authorize_url: None,
            token_url: None,
            userinfo_url: None,
            jwks_url: None,
        }
    }
}

/// Secrets are redacted.
impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("key", &self.key)
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret)
            .field("scopes", &self.scopes)
            .field("issuer", &self.issuer)
            .finish_non_exhaustive()
    }
}

/// Authorization URL parameters Navree sets itself.
pub(crate) const RESERVED_PARAMS: [&str; 7] = [
    "client_id",
    "redirect_uri",
    "state",
    "nonce",
    "code_challenge",
    "code_challenge_method",
    "response_type",
];

impl ProviderConfig {
    /// The provider type: `type`, or the key.
    pub fn kind(&self) -> &str {
        self.kind.as_deref().unwrap_or(&self.key)
    }

    /// The client secret, if one is configured.
    pub fn secret(&self) -> Option<&str> {
        self.client_secret
            .as_ref()
            .map(|secret| secret.expose().as_str())
            .filter(|secret| !secret.is_empty())
    }

    fn check(&self, key: &str, errors: &mut Vec<String>) {
        let at = format!("social.providers.{key}");
        if !is_provider_key(key) {
            errors.push(format!(
                "social.providers: `{key}` must be lower-case letters, digits, `-` or `_` (and not `accounts`)"
            ));
        }
        let kind_name = self.kind.as_deref().unwrap_or(key);
        let Some(kind) = Kind::parse(kind_name) else {
            errors.push(format!(
                "{at}.type: unknown provider type `{kind_name}` (expected one of {})",
                Kind::NAMES.join(", ")
            ));
            return;
        };
        if self.client_id.trim().is_empty() {
            errors.push(format!("{at}.client_id is required"));
        }
        for (name, value) in [
            ("issuer", &self.issuer),
            ("url", &self.url),
            ("api_url", &self.api_url),
            ("authorize_url", &self.authorize_url),
            ("token_url", &self.token_url),
            ("userinfo_url", &self.userinfo_url),
            ("jwks_url", &self.jwks_url),
        ] {
            if value.as_deref().is_some_and(|url| !is_http_url(url)) {
                errors.push(format!("{at}.{name} must be an absolute http(s) URL"));
            }
        }
        for param in self.params.keys() {
            if RESERVED_PARAMS.contains(&param.as_str()) {
                errors.push(format!("{at}.params.{param} is set by Navree"));
            }
        }
        check_domains(
            &format!("{at}.allowed_email_domains"),
            &self.allowed_email_domains,
            errors,
        );
        match kind {
            Kind::Oidc if self.issuer.is_none() => {
                errors.push(format!("{at}.issuer is required for an `oidc` provider"));
            }
            Kind::Custom if self.token_url.is_none() => {
                errors.push(format!(
                    "{at}.token_url is required for a `custom` provider"
                ));
            }
            Kind::Apple => {
                for (name, value) in [("team_id", &self.team_id), ("key_id", &self.key_id)] {
                    if value.as_deref().is_none_or(|v| v.trim().is_empty()) {
                        errors.push(format!("{at}.{name} is required for Sign in with Apple"));
                    }
                }
                match &self.private_key {
                    None => errors.push(format!(
                        "{at}.private_key is required for Sign in with Apple (the .p8 key)"
                    )),
                    Some(pem) => {
                        if apple_key(pem.expose()).is_none() {
                            errors.push(format!(
                                "{at}.private_key is not a PKCS#8 P-256 private key (the .p8 file)"
                            ));
                        }
                    }
                }
            }
            Kind::Microsoft if self.tenant.as_deref().is_some_and(|t| t.trim().is_empty()) => {
                errors.push(format!("{at}.tenant must not be empty when set"));
            }
            _ => {}
        }
    }
}

/// A provider key: lower-case letters, digits, `-` and `_`, not `accounts`
/// (which would clash with `/social/accounts`).
fn is_provider_key(key: &str) -> bool {
    !key.is_empty()
        && key != "accounts"
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn check_domains(at: &str, domains: &[String], errors: &mut Vec<String>) {
    for domain in domains {
        if domain.trim().is_empty() || domain.contains('@') || domain.contains('/') {
            errors.push(format!(
                "{at}: `{domain}` must be a domain such as `acme.test`"
            ));
        }
    }
}

/// An absolute `http` or `https` URL with a host.
pub(crate) fn is_http_url(value: &str) -> bool {
    url::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

/// A list, or one comma-separated string (what an environment variable
/// holds).
fn list_or_csv<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Items {
        List(Vec<String>),
        Joined(String),
    }
    let items = match Items::deserialize(deserializer)? {
        Items::List(items) => items,
        Items::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(items
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};
    use serde_json::json;

    use super::*;

    fn options(value: serde_json::Value) -> Result<SocialOptions, String> {
        let config = Config::load(&ConfigSources::new().set("social", value))
            .map_err(|error| error.to_string())?;
        config.typed::<SocialOptions>().map_err(|e| e.to_string())
    }

    #[test]
    fn defaults_match_the_reference() {
        let options = SocialOptions::default();
        assert!(options.routes);
        assert_eq!(options.registration, None);
        assert_eq!(options.link_by_email, LinkByEmail::Verified);
        assert_eq!(options.two_factor, TwoFactorPolicy::Require);
        assert_eq!(options.state_ttl_secs, 600);
        assert!(!options.store_tokens);
        assert!(ProviderConfig::default().pkce);
    }

    #[test]
    fn a_documented_configuration_loads() {
        let options = options(json!({
            "redirect_base": "https://app.acme.test/auth/callback",
            "link_by_email": "never",
            "two_factor": "skip",
            "allowed_email_domains": "acme.test, acme.dev",
            "providers": {
                "github": { "client_id": "Iv1.8a61f9b3a7aba766", "client_secret": "s3cret" },
                "google": {
                    "client_id": "481203-k2l9.apps.googleusercontent.com",
                    "hosted_domain": "acme.test",
                    "scopes": ["https://www.googleapis.com/auth/calendar.readonly"],
                    "params": { "access_type": "offline", "prompt": "consent" }
                },
                "okta": {
                    "type": "oidc",
                    "name": "Okta",
                    "issuer": "https://acme.okta.com",
                    "client_id": "0oa8f2kq1xYz3AbC4d5e",
                    "scopes": ["openid", "profile", "email", "groups"]
                }
            }
        }))
        .expect("valid options");
        assert_eq!(options.link_by_email, LinkByEmail::Never);
        assert_eq!(options.two_factor, TwoFactorPolicy::Skip);
        assert_eq!(options.allowed_email_domains, ["acme.test", "acme.dev"]);
        let github = options.provider("github").unwrap();
        assert_eq!(github.key, "github");
        assert_eq!(github.kind(), "github");
        assert_eq!(github.secret(), Some("s3cret"));
        assert!(!format!("{github:?}").contains("s3cret"));
        let google = options.provider("google").unwrap();
        assert_eq!(google.params["prompt"], "consent");
        assert_eq!(options.provider("okta").unwrap().kind(), "oidc");
    }

    #[test]
    fn the_boot_fails_naming_the_key() {
        let error = options(json!({ "redirect_base": "/auth/callback" })).unwrap_err();
        assert!(
            error.contains("social.redirect_base must be an absolute"),
            "{error}"
        );

        let error = options(json!({ "providers": {} })).unwrap_err();
        assert!(
            error.contains("social.redirect_base is required"),
            "{error}"
        );

        let error = options(json!({
            "redirect_base": "https://app.acme.test/cb",
            "providers": { "github": { "client_secret": "x" } }
        }))
        .unwrap_err();
        assert!(
            error.contains("social.providers.github.client_id is required"),
            "{error}"
        );
        assert!(!error.contains("\"x\""), "{error}");

        let error = options(json!({
            "redirect_base": "https://app.acme.test/cb",
            "providers": { "myspace": { "client_id": "a" } }
        }))
        .unwrap_err();
        assert!(
            error.contains("social.providers.myspace.type: unknown provider type `myspace`"),
            "{error}"
        );

        let error = options(json!({
            "redirect_base": "https://app.acme.test/cb",
            "providers": { "okta": { "type": "oidc", "client_id": "a" } }
        }))
        .unwrap_err();
        assert!(
            error.contains("social.providers.okta.issuer is required"),
            "{error}"
        );

        let error = options(json!({
            "redirect_base": "https://app.acme.test/cb",
            "providers": { "zoho": { "type": "custom", "client_id": "a" } }
        }))
        .unwrap_err();
        assert!(
            error.contains("social.providers.zoho.token_url is required"),
            "{error}"
        );

        let error = options(json!({
            "redirect_base": "https://app.acme.test/cb",
            "providers": { "apple": { "client_id": "a", "team_id": "T", "key_id": "K", "private_key": "nope" } }
        }))
        .unwrap_err();
        assert!(
            error.contains("social.providers.apple.private_key is not"),
            "{error}"
        );

        let error = options(json!({
            "redirect_base": "https://app.acme.test/cb",
            "providers": { "github": { "client_id": "a", "params": { "state": "x" } } }
        }))
        .unwrap_err();
        assert!(
            error.contains("social.providers.github.params.state is set by Navree"),
            "{error}"
        );
    }

    #[test]
    fn provider_keys_are_lower_case_and_not_accounts() {
        assert!(is_provider_key("github"));
        assert!(is_provider_key("acme-sso_2"));
        assert!(!is_provider_key("GitHub"));
        assert!(!is_provider_key("accounts"));
        assert!(!is_provider_key(""));
    }

    #[test]
    fn environment_values_are_strings() {
        let config = Config::load(&ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_SOCIAL__REDIRECT_BASE", "https://app.acme.test/cb"),
                ("APP_SOCIAL__STORE_TOKENS", "true"),
                ("APP_SOCIAL__STATE_TTL_SECS", "120"),
                ("APP_SOCIAL__PROVIDERS__GITHUB__CLIENT_ID", "abc"),
                ("APP_SOCIAL__PROVIDERS__GITHUB__CLIENT_SECRET", "s3cret"),
                ("APP_SOCIAL__PROVIDERS__GITHUB__SCOPES", "repo,gist"),
            ],
        ))
        .unwrap();
        let options = config.typed::<SocialOptions>().unwrap();
        assert!(options.store_tokens);
        assert_eq!(options.state_ttl_secs, 120);
        let github = options.provider("github").unwrap();
        assert_eq!(github.secret(), Some("s3cret"));
        assert_eq!(github.scopes, ["repo", "gist"]);
    }
}
