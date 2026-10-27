//! Providers: the built-in kinds, the [`SocialProvider`] trait for your own,
//! and each configured provider's endpoints.

use std::sync::Arc;

use navree_crypto::jose::{Algorithm, SigningKey};
use navree_kernel::BoxFuture;
use serde::Deserialize;
use tokio::sync::OnceCell;

use crate::{AuthorizeUrl, ProviderConfig, ProviderToken, SocialError, SocialUser};

/// Reads Sign in with Apple's `.p8` key: a P-256 private key, which signs
/// the ES256 client secret. Literal `\n` sequences (the key in one
/// environment variable) are accepted.
pub(crate) fn apple_key(pem: &str) -> Option<SigningKey> {
    SigningKey::from_pem(pem)
        .ok()
        .filter(|key| key.algorithm() == Algorithm::ES256)
}

/// A provider that is not built in: Navree does the state, PKCE, code
/// exchange (at `social.providers.<key>.token_url`), linking and token
/// issuing; the provider names its authorization endpoint and scopes, and
/// reads the user with the access token.
///
/// Register it with
/// [`SocialModule::for_feature::<P>()`](crate::SocialModule::for_feature)
/// and configure it as `[social.providers.<key>]` with `type = "custom"`,
/// where `<key>` is what [`key`](SocialProvider::key) returns.
pub trait SocialProvider: Send + Sync + 'static {
    /// The provider key: the name of its `social.providers` table and of
    /// its routes (`zoho` gives `/api/auth/social/zoho/redirect`).
    fn key(&self) -> &'static str;

    /// Adds the authorization endpoint, the provider's scopes and any
    /// parameters it needs to `url`, which already has the client id, the
    /// redirect URI, the state, the PKCE challenge, the configured scopes
    /// and the configured parameters.
    fn authorize_url(&self, config: &ProviderConfig, url: AuthorizeUrl) -> AuthorizeUrl;

    /// Reads the user with `token`, the result of the code exchange. Give
    /// the user the token (`.token(token)`) so `social.store_tokens` can
    /// keep it; Navree sets it otherwise.
    fn user<'a>(
        &'a self,
        config: &'a ProviderConfig,
        token: ProviderToken,
    ) -> BoxFuture<'a, Result<SocialUser, SocialError>>;
}

/// A built-in provider type, or `oidc` or `custom`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Google,
    GitHub,
    Apple,
    Microsoft,
    GitLab,
    Slack,
    LinkedIn,
    Discord,
    X,
    Oidc,
    Custom,
}

/// How a provider's profile is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProfileSource {
    /// The verified ID token's claims (plus the userinfo endpoint for
    /// missing claims).
    IdToken,
    /// The OpenID Connect userinfo endpoint, with the access token.
    UserInfo,
    /// GitHub's user and emails APIs.
    GitHub,
    /// Discord's `users/@me`.
    Discord,
    /// X's `users/me`.
    X,
    /// The custom provider's `user`.
    Custom,
}

impl Kind {
    /// Every type name, for error messages.
    pub(crate) const NAMES: [&'static str; 11] = [
        "google",
        "github",
        "apple",
        "microsoft",
        "gitlab",
        "slack",
        "linkedin",
        "discord",
        "x",
        "oidc",
        "custom",
    ];

    pub(crate) fn parse(name: &str) -> Option<Kind> {
        Some(match name {
            "google" => Kind::Google,
            "github" => Kind::GitHub,
            "apple" => Kind::Apple,
            "microsoft" => Kind::Microsoft,
            "gitlab" => Kind::GitLab,
            "slack" => Kind::Slack,
            "linkedin" => Kind::LinkedIn,
            "discord" => Kind::Discord,
            "x" => Kind::X,
            "oidc" => Kind::Oidc,
            "custom" => Kind::Custom,
            _ => return None,
        })
    }

    /// The name shown in messages.
    pub(crate) fn display(self) -> Option<&'static str> {
        Some(match self {
            Kind::Google => "Google",
            Kind::GitHub => "GitHub",
            Kind::Apple => "Apple",
            Kind::Microsoft => "Microsoft",
            Kind::GitLab => "GitLab",
            Kind::Slack => "Slack",
            Kind::LinkedIn => "LinkedIn",
            Kind::Discord => "Discord",
            Kind::X => "X",
            Kind::Oidc | Kind::Custom => return None,
        })
    }

    /// The scopes always requested.
    pub(crate) fn default_scopes(self) -> &'static [&'static str] {
        match self {
            Kind::Google | Kind::Microsoft | Kind::Oidc => &["openid", "email", "profile"],
            Kind::GitLab | Kind::LinkedIn => &["openid", "profile", "email"],
            Kind::Slack => &["openid", "email", "profile"],
            Kind::GitHub => &["read:user", "user:email"],
            Kind::Apple => &["name", "email"],
            Kind::Discord => &["identify", "email"],
            Kind::X => &["tweet.read", "users.read"],
            Kind::Custom => &[],
        }
    }

    pub(crate) fn profile_source(self) -> ProfileSource {
        match self {
            Kind::Google | Kind::Apple | Kind::Microsoft | Kind::Oidc => ProfileSource::IdToken,
            Kind::GitLab | Kind::Slack | Kind::LinkedIn => ProfileSource::UserInfo,
            Kind::GitHub => ProfileSource::GitHub,
            Kind::Discord => ProfileSource::Discord,
            Kind::X => ProfileSource::X,
            Kind::Custom => ProfileSource::Custom,
        }
    }
}

/// Which `iss` values an ID token may carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Issuer {
    /// One of these.
    Exact(Vec<String>),
    /// Microsoft: `{base}/{tid}/v2.0`, with the token's own tenant id.
    MicrosoftTenant(String),
}

/// A provider's endpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Endpoints {
    pub(crate) authorize: String,
    pub(crate) token: String,
    pub(crate) userinfo: Option<String>,
    pub(crate) jwks: Option<String>,
    pub(crate) issuer: Issuer,
    /// Send the client secret with HTTP Basic authentication (else in the
    /// form).
    pub(crate) basic_auth: bool,
}

/// An OpenID Connect discovery document (the members Navree reads).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Discovery {
    pub(crate) issuer: String,
    pub(crate) authorization_endpoint: String,
    pub(crate) token_endpoint: String,
    #[serde(default)]
    pub(crate) userinfo_endpoint: Option<String>,
    pub(crate) jwks_uri: String,
    #[serde(default)]
    pub(crate) token_endpoint_auth_methods_supported: Vec<String>,
}

/// The Microsoft tenant id of personal accounts.
pub(crate) const MICROSOFT_CONSUMERS_TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

/// A configured provider, ready to use.
pub(crate) struct ProviderEntry {
    pub(crate) key: String,
    pub(crate) kind: Kind,
    pub(crate) name: String,
    pub(crate) config: ProviderConfig,
    pub(crate) custom: Option<Arc<dyn SocialProvider>>,
    /// Apple's key, with the configured `key_id` as its `kid`.
    pub(crate) apple_key: Option<SigningKey>,
    pub(crate) discovery: OnceCell<Discovery>,
}

fn trim(url: &str) -> &str {
    url.trim_end_matches('/')
}

impl ProviderEntry {
    /// Builds the entry of `config` (already validated).
    pub(crate) fn new(
        config: ProviderConfig,
        custom: Option<Arc<dyn SocialProvider>>,
    ) -> Result<Self, String> {
        let kind = Kind::parse(config.kind())
            .ok_or_else(|| format!("social.providers.{}.type is unknown", config.key))?;
        let name = config
            .name
            .clone()
            .or_else(|| kind.display().map(str::to_owned))
            .unwrap_or_else(|| config.key.clone());
        let apple_key = match (kind, &config.private_key) {
            (Kind::Apple, Some(pem)) => {
                let key = apple_key(pem.expose()).ok_or_else(|| {
                    format!(
                        "social.providers.{}.private_key is not a PKCS#8 P-256 private key",
                        config.key
                    )
                })?;
                Some(key.with_kid(config.key_id.clone().unwrap_or_default()))
            }
            _ => None,
        };
        if kind == Kind::Custom && custom.is_none() {
            return Err(format!(
                "social.providers.{key}: no SocialProvider is registered under `{key}` \
                 (register it with SocialModule::for_feature)",
                key = config.key
            ));
        }
        Ok(Self {
            key: config.key.clone(),
            kind,
            name,
            config,
            custom,
            apple_key,
            discovery: OnceCell::new(),
        })
    }

    /// Whether the flow sends a nonce and checks it in the ID token.
    pub(crate) fn uses_nonce(&self) -> bool {
        self.kind.profile_source() == ProfileSource::IdToken
    }

    /// The endpoints of a provider other than `oidc` (which needs its
    /// discovery document, see [`endpoints_from`](Self::endpoints_from)).
    pub(crate) fn static_endpoints(&self) -> Endpoints {
        let c = &self.config;
        let or = |value: &Option<String>, default: String| value.clone().unwrap_or(default);
        let exact = |default: &[&str]| {
            Issuer::Exact(match &c.issuer {
                Some(issuer) => vec![issuer.clone()],
                None => default.iter().map(|s| (*s).to_owned()).collect(),
            })
        };
        match self.kind {
            Kind::Google => Endpoints {
                authorize: or(
                    &c.authorize_url,
                    "https://accounts.google.com/o/oauth2/v2/auth".into(),
                ),
                token: or(&c.token_url, "https://oauth2.googleapis.com/token".into()),
                userinfo: Some(or(
                    &c.userinfo_url,
                    "https://openidconnect.googleapis.com/v1/userinfo".into(),
                )),
                jwks: Some(or(
                    &c.jwks_url,
                    "https://www.googleapis.com/oauth2/v3/certs".into(),
                )),
                issuer: exact(&["https://accounts.google.com", "accounts.google.com"]),
                basic_auth: false,
            },
            Kind::GitHub => {
                let base = c.url.as_deref().map_or("https://github.com", trim);
                let api = match (&c.api_url, &c.url) {
                    (Some(api), _) => trim(api).to_owned(),
                    (None, Some(url)) => format!("{}/api/v3", trim(url)),
                    (None, None) => "https://api.github.com".to_owned(),
                };
                Endpoints {
                    authorize: or(&c.authorize_url, format!("{base}/login/oauth/authorize")),
                    token: or(&c.token_url, format!("{base}/login/oauth/access_token")),
                    userinfo: Some(or(&c.userinfo_url, format!("{api}/user"))),
                    jwks: None,
                    issuer: Issuer::Exact(Vec::new()),
                    basic_auth: false,
                }
            }
            Kind::Apple => {
                let base = c.url.as_deref().map_or("https://appleid.apple.com", trim);
                Endpoints {
                    authorize: or(&c.authorize_url, format!("{base}/auth/authorize")),
                    token: or(&c.token_url, format!("{base}/auth/token")),
                    userinfo: None,
                    jwks: Some(or(&c.jwks_url, format!("{base}/auth/keys"))),
                    issuer: Issuer::Exact(vec![c.issuer.clone().unwrap_or(base.to_owned())]),
                    basic_auth: false,
                }
            }
            Kind::Microsoft => {
                let base = c
                    .url
                    .as_deref()
                    .map_or("https://login.microsoftonline.com", trim);
                let tenant = c.tenant.as_deref().unwrap_or("common");
                Endpoints {
                    authorize: or(
                        &c.authorize_url,
                        format!("{base}/{tenant}/oauth2/v2.0/authorize"),
                    ),
                    token: or(&c.token_url, format!("{base}/{tenant}/oauth2/v2.0/token")),
                    userinfo: None,
                    jwks: Some(or(
                        &c.jwks_url,
                        format!("{base}/{tenant}/discovery/v2.0/keys"),
                    )),
                    issuer: match &c.issuer {
                        Some(issuer) => Issuer::Exact(vec![issuer.clone()]),
                        None => Issuer::MicrosoftTenant(base.to_owned()),
                    },
                    basic_auth: false,
                }
            }
            Kind::GitLab => {
                let base = c.url.as_deref().map_or("https://gitlab.com", trim);
                Endpoints {
                    authorize: or(&c.authorize_url, format!("{base}/oauth/authorize")),
                    token: or(&c.token_url, format!("{base}/oauth/token")),
                    userinfo: Some(or(&c.userinfo_url, format!("{base}/oauth/userinfo"))),
                    jwks: None,
                    issuer: exact(&[base]),
                    basic_auth: false,
                }
            }
            Kind::Slack => {
                let base = c.url.as_deref().map_or("https://slack.com", trim);
                Endpoints {
                    authorize: or(&c.authorize_url, format!("{base}/openid/connect/authorize")),
                    token: or(&c.token_url, format!("{base}/api/openid.connect.token")),
                    userinfo: Some(or(
                        &c.userinfo_url,
                        format!("{base}/api/openid.connect.userInfo"),
                    )),
                    jwks: None,
                    issuer: exact(&[base]),
                    basic_auth: false,
                }
            }
            Kind::LinkedIn => Endpoints {
                authorize: or(
                    &c.authorize_url,
                    "https://www.linkedin.com/oauth/v2/authorization".into(),
                ),
                token: or(
                    &c.token_url,
                    "https://www.linkedin.com/oauth/v2/accessToken".into(),
                ),
                userinfo: Some(or(
                    &c.userinfo_url,
                    "https://api.linkedin.com/v2/userinfo".into(),
                )),
                jwks: None,
                issuer: exact(&["https://www.linkedin.com/oauth"]),
                basic_auth: false,
            },
            Kind::Discord => {
                let base = c.url.as_deref().map_or("https://discord.com", trim);
                Endpoints {
                    authorize: or(&c.authorize_url, format!("{base}/oauth2/authorize")),
                    token: or(&c.token_url, format!("{base}/api/oauth2/token")),
                    userinfo: Some(or(&c.userinfo_url, format!("{base}/api/users/@me"))),
                    jwks: None,
                    issuer: Issuer::Exact(Vec::new()),
                    basic_auth: false,
                }
            }
            Kind::X => Endpoints {
                authorize: or(&c.authorize_url, "https://x.com/i/oauth2/authorize".into()),
                token: or(&c.token_url, "https://api.x.com/2/oauth2/token".into()),
                userinfo: Some(or(&c.userinfo_url, "https://api.x.com/2/users/me".into())),
                jwks: None,
                issuer: Issuer::Exact(Vec::new()),
                // X authenticates confidential clients with HTTP Basic.
                basic_auth: true,
            },
            Kind::Oidc | Kind::Custom => {
                let issuer = c.issuer.clone().unwrap_or_default();
                Endpoints {
                    authorize: or(&c.authorize_url, format!("{}/authorize", trim(&issuer))),
                    token: c.token_url.clone().unwrap_or_default(),
                    userinfo: c.userinfo_url.clone(),
                    jwks: c.jwks_url.clone(),
                    issuer: Issuer::Exact(vec![issuer]),
                    basic_auth: false,
                }
            }
        }
    }

    /// The endpoints of an `oidc` provider from its discovery document,
    /// with the configured overrides.
    pub(crate) fn endpoints_from(&self, discovery: &Discovery) -> Endpoints {
        let c = &self.config;
        let methods = &discovery.token_endpoint_auth_methods_supported;
        Endpoints {
            authorize: c
                .authorize_url
                .clone()
                .unwrap_or_else(|| discovery.authorization_endpoint.clone()),
            token: c
                .token_url
                .clone()
                .unwrap_or_else(|| discovery.token_endpoint.clone()),
            userinfo: c
                .userinfo_url
                .clone()
                .or_else(|| discovery.userinfo_endpoint.clone()),
            jwks: Some(
                c.jwks_url
                    .clone()
                    .unwrap_or_else(|| discovery.jwks_uri.clone()),
            ),
            issuer: Issuer::Exact(vec![discovery.issuer.clone()]),
            // RFC 8414: client_secret_basic is the default; use the form
            // only when the provider does not list Basic.
            basic_auth: methods.is_empty() || methods.iter().any(|m| m == "client_secret_basic"),
        }
    }

    /// The discovery document URL of an `oidc` provider.
    pub(crate) fn discovery_url(&self) -> String {
        format!(
            "{}/.well-known/openid-configuration",
            trim(self.config.issuer.as_deref().unwrap_or_default())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPLE_P8: &str = include_str!("../tests/fixtures/apple-authkey.p8");
    const RSA_PEM: &str = include_str!("../tests/fixtures/rsa-2048.pem");

    #[test]
    fn apple_keys_are_p256_pkcs8_keys() {
        let key = apple_key(APPLE_P8).unwrap();
        assert_eq!(key.algorithm(), Algorithm::ES256);
        // The same key in one line with `\n` escapes (an environment variable).
        let escaped = APPLE_P8.trim().replace('\n', "\\n");
        assert!(apple_key(&escaped).is_some());
        assert!(apple_key("not a key").is_none());
        assert!(apple_key(RSA_PEM).is_none(), "an RSA key is refused");
        let ed25519 = SigningKey::generate_ed25519().to_pkcs8_pem().unwrap();
        assert!(apple_key(&ed25519).is_none(), "an Ed25519 key is refused");
    }

    #[test]
    fn the_apple_key_signs_with_the_configured_key_id() {
        let apple = entry("apple", |c| {
            c.team_id = Some("TEAMID".to_owned());
            c.key_id = Some("KEYID".to_owned());
            c.private_key = Some(navree_contracts::Secret::new(APPLE_P8.to_owned()));
        });
        let key = apple.apple_key.as_ref().unwrap();
        assert_eq!(key.kid(), Some("KEYID"));
    }

    fn entry(kind: &str, f: impl FnOnce(&mut ProviderConfig)) -> ProviderEntry {
        let mut config = ProviderConfig {
            key: kind.to_owned(),
            client_id: "id".to_owned(),
            ..ProviderConfig::default()
        };
        f(&mut config);
        ProviderEntry::new(config, None).unwrap()
    }

    #[test]
    fn display_names_default_to_the_provider_or_the_key() {
        assert_eq!(entry("github", |_| {}).name, "GitHub");
        assert_eq!(entry("x", |_| {}).name, "X");
        let okta = entry("okta", |c| {
            c.kind = Some("oidc".to_owned());
            c.issuer = Some("https://acme.okta.com".to_owned());
        });
        assert_eq!(okta.name, "okta");
        let named = entry("okta", |c| {
            c.kind = Some("oidc".to_owned());
            c.name = Some("Okta".to_owned());
        });
        assert_eq!(named.name, "Okta");
    }

    #[test]
    fn built_in_endpoints() {
        let github = entry("github", |_| {}).static_endpoints();
        assert_eq!(github.authorize, "https://github.com/login/oauth/authorize");
        assert_eq!(
            github.userinfo.as_deref(),
            Some("https://api.github.com/user")
        );
        let enterprise = entry("github", |c| c.url = Some("https://git.acme.test/".into()));
        assert_eq!(
            enterprise.static_endpoints().userinfo.as_deref(),
            Some("https://git.acme.test/api/v3/user")
        );
        let gitlab = entry("gitlab", |c| {
            c.url = Some("https://gitlab.acme.test".into())
        });
        assert_eq!(
            gitlab.static_endpoints().token,
            "https://gitlab.acme.test/oauth/token"
        );
        let microsoft = entry("microsoft", |c| c.tenant = Some("organizations".into()));
        let endpoints = microsoft.static_endpoints();
        assert_eq!(
            endpoints.authorize,
            "https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize"
        );
        assert_eq!(
            endpoints.issuer,
            Issuer::MicrosoftTenant("https://login.microsoftonline.com".into())
        );
        assert!(entry("x", |_| {}).static_endpoints().basic_auth);
        let google = entry("google", |_| {}).static_endpoints();
        assert_eq!(
            google.issuer,
            Issuer::Exact(vec![
                "https://accounts.google.com".into(),
                "accounts.google.com".into()
            ])
        );
    }

    #[test]
    fn a_custom_type_needs_a_registered_provider() {
        let config = ProviderConfig {
            key: "zoho".to_owned(),
            kind: Some("custom".to_owned()),
            client_id: "id".to_owned(),
            ..ProviderConfig::default()
        };
        let error = ProviderEntry::new(config, None).err().unwrap();
        assert!(error.contains("SocialModule::for_feature"), "{error}");
    }

    #[test]
    fn discovery_prefers_basic_authentication() {
        let okta = entry("okta", |c| {
            c.kind = Some("oidc".to_owned());
            c.issuer = Some("https://idp.test/".into());
        });
        assert_eq!(
            okta.discovery_url(),
            "https://idp.test/.well-known/openid-configuration"
        );
        let mut discovery = Discovery {
            issuer: "https://idp.test".into(),
            authorization_endpoint: "https://idp.test/a".into(),
            token_endpoint: "https://idp.test/t".into(),
            userinfo_endpoint: None,
            jwks_uri: "https://idp.test/k".into(),
            token_endpoint_auth_methods_supported: vec![],
        };
        assert!(okta.endpoints_from(&discovery).basic_auth);
        discovery.token_endpoint_auth_methods_supported = vec!["client_secret_post".into()];
        assert!(!okta.endpoints_from(&discovery).basic_auth);
    }
}
