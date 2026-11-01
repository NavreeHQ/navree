//! [`IntegrationsOptions`]: the `integrations` configuration section.

use std::collections::BTreeMap;
use std::fmt;

use navree_config::ConfigSection;
use navree_openapi::ToSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Options of the integrations module (configuration section
/// `integrations`).
///
/// ```toml
/// [integrations]
/// redirect_base = "https://app.acme.test/settings/integrations/callback"
/// level = "workspace"         # workspace | organization | app
/// check_interval_mins = 60    # 0: no scheduled checks
///
/// [integrations.providers.microsoft]
/// client_id = "0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f"
/// # client_secret: APP_INTEGRATIONS__PROVIDERS__MICROSOFT__CLIENT_SECRET
///
/// [integrations.providers.google]
/// # service-account keys are uploaded per connection
///
/// [integrations.providers.slack]
/// client_id = "4129031.6713"
/// scopes = ["chat:write", "users:read"]
/// ```
///
/// Every key also comes from the environment:
/// `APP_INTEGRATIONS__CHECK_INTERVAL_MINS=30`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct IntegrationsOptions {
    /// The front end's callback page: a provider sends the browser back to
    /// `{redirect_base}/{provider}`. Required when a provider connects
    /// through a consent screen (every type but `api_key`, and `google`
    /// without a `client_id`).
    pub redirect_base: Option<String>,
    /// Mount the `/api/integrations` routes (default true; permission
    /// `integrations.manage`).
    pub routes: bool,
    /// Whose connections the routes and the [`Connection`](crate::Connection)
    /// extractor use: the request's workspace (default), its organization,
    /// or the app's own. Without `WorkspaceModule` in the app, `workspace`
    /// means the app's own.
    pub level: Level,
    /// Lifetime of a sealed connect state, in seconds (default 600).
    pub state_ttl_secs: u64,
    /// `integrations:check --due` (scheduled every five minutes) checks a
    /// connection when its last check is older than this many minutes
    /// (default 60; 0 turns scheduled checks off).
    pub check_interval_mins: u64,
    /// Consecutive transient failures (a provider that could not be
    /// reached or answered 5xx) before a connection is marked unhealthy
    /// (default 3). A refused credential is unhealthy at once.
    pub unhealthy_after: u32,
    /// How long a token refresh waits for another refresh of the same
    /// connection to finish, in seconds (default 15).
    pub lock_timeout_secs: u64,
    /// The providers, by key (`microsoft`, `google`, `slack`, or your own
    /// key with a `type`).
    pub providers: BTreeMap<String, ProviderConfig>,
    /// Watched mailboxes (crate feature `mail`): where Microsoft Graph and
    /// Gmail send their notifications.
    #[cfg(feature = "mail")]
    pub mailboxes: crate::MailboxesOptions,
}

impl Default for IntegrationsOptions {
    fn default() -> Self {
        Self {
            redirect_base: None,
            routes: true,
            level: Level::Workspace,
            state_ttl_secs: 600,
            check_interval_mins: 60,
            unhealthy_after: 3,
            lock_timeout_secs: 15,
            providers: BTreeMap::new(),
            #[cfg(feature = "mail")]
            mailboxes: crate::MailboxesOptions::default(),
        }
    }
}

impl IntegrationsOptions {
    /// The configuration of provider `key`, if any.
    pub fn provider(&self, key: &str) -> Option<&ProviderConfig> {
        self.providers.get(key)
    }

    /// The same options with provider `key` configured as `config`.
    pub fn with_provider(mut self, key: impl Into<String>, config: ProviderConfig) -> Self {
        self.providers.insert(key.into(), config);
        self
    }

    /// Every problem with these options, each naming its key.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if let Some(base) = &self.redirect_base {
            match url::Url::parse(base) {
                Ok(url) if matches!(url.scheme(), "http" | "https") => {}
                _ => errors.push(format!(
                    "integrations.redirect_base must be an absolute http(s) URL, got `{base}`"
                )),
            }
        }
        if self.state_ttl_secs == 0 {
            errors.push("integrations.state_ttl_secs must be at least 1".to_owned());
        }
        if self.unhealthy_after == 0 {
            errors.push("integrations.unhealthy_after must be at least 1".to_owned());
        }
        if self.lock_timeout_secs == 0 {
            errors.push("integrations.lock_timeout_secs must be at least 1".to_owned());
        }
        for (key, provider) in &self.providers {
            provider.check(key, self.redirect_base.is_some(), &mut errors);
        }
        #[cfg(feature = "mail")]
        self.mailboxes.check(&mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl ConfigSection for IntegrationsOptions {
    const KEY: &'static str = "integrations";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// Whose connections the routes and extractors use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// The request's workspace (`X-Workspace`, a path, a subdomain or a
    /// claim). A workspace without its own connection uses its
    /// organization's.
    #[default]
    Workspace,
    /// The request's organization (`X-Organization` and the other
    /// organization resolvers).
    Organization,
    /// The app's own connections (no tenant).
    App,
}

/// What a provider is: how it connects and how its API is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    /// Microsoft Graph (Microsoft 365, Entra ID): admin consent and
    /// app-only tokens, or a signed-in admin's delegated tokens.
    Microsoft,
    /// Google Workspace: service-account keys with domain-wide delegation,
    /// or OAuth consent when a `client_id` is configured.
    Google,
    /// Slack: OAuth v2 app installs (bot tokens, with token rotation).
    Slack,
    /// Any OAuth 2.0 API: authorization code with PKCE and refresh tokens.
    #[serde(rename = "oauth2")]
    OAuth2,
    /// An API key or token pasted by an admin, sent in a header.
    ApiKey,
}

impl ProviderType {
    /// The configuration name (`"oauth2"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            ProviderType::Microsoft => "microsoft",
            ProviderType::Google => "google",
            ProviderType::Slack => "slack",
            ProviderType::OAuth2 => "oauth2",
            ProviderType::ApiKey => "api_key",
        }
    }

    /// The type a provider key names on its own (`microsoft`, `google`,
    /// `slack`).
    pub fn of_key(key: &str) -> Option<ProviderType> {
        match key {
            "microsoft" => Some(ProviderType::Microsoft),
            "google" => Some(ProviderType::Google),
            "slack" => Some(ProviderType::Slack),
            _ => None,
        }
    }

    /// The display name of the built-in types.
    pub(crate) const fn default_name(self) -> Option<&'static str> {
        match self {
            ProviderType::Microsoft => Some("Microsoft 365"),
            ProviderType::Google => Some("Google Workspace"),
            ProviderType::Slack => Some("Slack"),
            ProviderType::OAuth2 | ProviderType::ApiKey => None,
        }
    }
}

impl fmt::Display for ProviderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a Microsoft provider connects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MicrosoftMode {
    /// A tenant admin grants consent once; tokens are app-only (client
    /// credentials) for that tenant.
    #[default]
    App,
    /// A signed-in admin's own delegated tokens (authorization code,
    /// `offline_access`).
    Delegated,
}

/// How the client authenticates at an OAuth 2.0 token endpoint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuth {
    /// `client_id` and `client_secret` in the form body (default).
    #[default]
    Post,
    /// HTTP Basic authentication.
    Basic,
}

/// A separate app registration for one scope set
/// (`integrations.providers.<key>.apps.<scope set>`): Microsoft app-only
/// tokens carry every permission the registration holds, so a scope set
/// that must stay apart (mail send, directory read) gets its own.
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AppCredentials {
    /// The registration's client id.
    pub client_id: String,
    /// Its secret (from `.env`).
    pub client_secret: Option<String>,
}

impl fmt::Debug for AppCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppCredentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "***"))
            .finish()
    }
}

/// One provider (`integrations.providers.<key>`).
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// `microsoft`, `google`, `slack`, `oauth2` or `api_key`; defaults to
    /// the key for the three built-in keys.
    #[serde(rename = "type")]
    pub kind: Option<ProviderType>,
    /// The display name (default: the provider's, else the key).
    pub name: Option<String>,
    /// The OAuth client id.
    pub client_id: Option<String>,
    /// The OAuth client secret (from `.env`).
    pub client_secret: Option<String>,
    /// Microsoft: the tenant whose admins may consent (`organizations`,
    /// `common` or a tenant id; default `organizations`).
    pub tenant: Option<String>,
    /// Microsoft: `app` (admin consent, app-only tokens; default) or
    /// `delegated`.
    pub mode: MicrosoftMode,
    /// Scopes of the default scope set (a list, or a comma-separated
    /// string).
    #[serde(deserialize_with = "list_or_csv")]
    pub scopes: Vec<String>,
    /// Extra authorization URL parameters.
    pub params: BTreeMap<String, String>,
    /// Send a PKCE challenge (S256). Default: on, except for Slack.
    pub pkce: Option<bool>,
    /// How the client authenticates at the token endpoint (`post` or
    /// `basic`).
    pub client_auth: ClientAuth,
    /// The authorization endpoint (`oauth2`; overrides the built-in ones).
    pub authorize_url: Option<String>,
    /// The token endpoint (`oauth2`; overrides the built-in ones, and the
    /// `token_uri` of uploaded Google keys).
    pub token_url: Option<String>,
    /// The API root that typed clients resolve paths against.
    pub api_url: Option<String>,
    /// An RFC 7009 token revocation endpoint, called on disconnect.
    pub revoke_url: Option<String>,
    /// A path (or URL) under the API root that `integrations:check` gets;
    /// any 2xx answer is healthy.
    pub health_url: Option<String>,
    /// Microsoft: the sign-in host (default
    /// `https://login.microsoftonline.com`).
    pub authority_url: Option<String>,
    /// `api_key`: the header the key goes in (default `Authorization`).
    pub header: Option<String>,
    /// `api_key`: the scheme before the key (default `Bearer`; `""` sends
    /// the key alone).
    pub scheme: Option<String>,
    /// Separate app registrations per scope set.
    pub apps: BTreeMap<String, AppCredentials>,
}

impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("type", &self.kind)
            .field("name", &self.name)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "***"))
            .field("tenant", &self.tenant)
            .field("mode", &self.mode)
            .field("scopes", &self.scopes)
            .field("api_url", &self.api_url)
            .field("apps", &self.apps)
            .finish_non_exhaustive()
    }
}

impl ProviderConfig {
    /// A configuration of type `kind`.
    pub fn new(kind: ProviderType) -> Self {
        Self {
            kind: Some(kind),
            ..Self::default()
        }
    }

    /// Sets the OAuth client.
    pub fn client(mut self, id: impl Into<String>, secret: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self.client_secret = Some(secret.into());
        self
    }

    /// Sets the scopes of the default scope set.
    pub fn scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// The type: `type`, else the key's built-in type.
    pub fn resolved_type(&self, key: &str) -> Option<ProviderType> {
        self.kind.or_else(|| ProviderType::of_key(key))
    }

    /// The display name: `name`, else the type's, else the key.
    pub fn display_name(&self, key: &str) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.resolved_type(key)
                .and_then(ProviderType::default_name)
                .map_or_else(|| key.to_owned(), str::to_owned)
        })
    }

    /// Whether this provider connects through a consent screen (and so
    /// needs `redirect_base`).
    pub(crate) fn uses_consent(&self, kind: ProviderType) -> bool {
        match kind {
            ProviderType::ApiKey => false,
            ProviderType::Google => self.client_id.is_some(),
            _ => true,
        }
    }

    fn check(&self, key: &str, has_redirect_base: bool, errors: &mut Vec<String>) {
        let at = format!("integrations.providers.{key}");
        if !is_provider_key(key) {
            errors.push(format!(
                "{at}: provider keys are lower-case letters, digits, `-` and `_` (and not `connections` or `providers`)"
            ));
        }
        let Some(kind) = self.resolved_type(key) else {
            errors.push(format!(
                "{at}.type is required: microsoft, google, slack, oauth2 or api_key"
            ));
            return;
        };
        let missing = |field: &str, errors: &mut Vec<String>| {
            errors.push(format!("{at}.{field} is required for a {kind} provider"));
        };
        match kind {
            ProviderType::Microsoft | ProviderType::Slack | ProviderType::OAuth2 => {
                if self
                    .client_id
                    .as_deref()
                    .is_none_or(|id| id.trim().is_empty())
                {
                    missing("client_id", errors);
                }
            }
            ProviderType::Google | ProviderType::ApiKey => {}
        }
        if kind == ProviderType::OAuth2 {
            for (field, value) in [
                ("authorize_url", &self.authorize_url),
                ("token_url", &self.token_url),
                ("api_url", &self.api_url),
            ] {
                if value.is_none() {
                    missing(field, errors);
                }
            }
        }
        for (field, value) in [
            ("authorize_url", &self.authorize_url),
            ("token_url", &self.token_url),
            ("api_url", &self.api_url),
            ("revoke_url", &self.revoke_url),
            ("authority_url", &self.authority_url),
        ] {
            if let Some(value) = value
                && !is_http_url(value)
            {
                errors.push(format!(
                    "{at}.{field} must be an absolute http(s) URL, got `{value}`"
                ));
            }
        }
        if self.uses_consent(kind) && !has_redirect_base {
            errors.push(format!(
                "{at}: integrations.redirect_base is required for providers that connect through a consent screen"
            ));
        }
        for (scope_set, app) in &self.apps {
            if app.client_id.trim().is_empty() {
                errors.push(format!("{at}.apps.{scope_set}.client_id is required"));
            }
        }
    }
}

/// Provider keys: lower-case letters, digits, `-` and `_`, and not a word
/// the routes use.
pub(crate) fn is_provider_key(key: &str) -> bool {
    !key.is_empty()
        && !matches!(key, "connections" | "providers")
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn is_http_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
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

    fn options(value: serde_json::Value) -> Result<IntegrationsOptions, String> {
        let config = Config::load(&ConfigSources::new().set("integrations", value))
            .map_err(|error| error.to_string())?;
        config
            .typed::<IntegrationsOptions>()
            .map_err(|error| error.to_string())
    }

    #[test]
    fn defaults_match_the_reference() {
        let options = IntegrationsOptions::default();
        assert_eq!(options.redirect_base, None);
        assert!(options.routes);
        assert_eq!(options.level, Level::Workspace);
        assert_eq!(options.state_ttl_secs, 600);
        assert_eq!(options.check_interval_mins, 60);
        assert_eq!(options.unhealthy_after, 3);
        assert_eq!(options.lock_timeout_secs, 15);
        assert!(options.check().is_ok());
    }

    #[test]
    fn built_in_keys_need_no_type_and_others_do() {
        let options = options(json!({
            "redirect_base": "https://app.acme.test/integrations/callback",
            "providers": {
                "microsoft": { "client_id": "a" },
                "google": {},
                "slack": { "client_id": "b", "scopes": "chat:write, users:read" },
                "hubspot": {
                    "type": "oauth2",
                    "client_id": "c",
                    "authorize_url": "https://app.hubspot.com/oauth/authorize",
                    "token_url": "https://api.hubapi.com/oauth/v1/token",
                    "api_url": "https://api.hubapi.com"
                },
                "edr": { "type": "api_key", "api_url": "https://edr.example.com/api" }
            }
        }))
        .unwrap();
        let slack = options.provider("slack").unwrap();
        assert_eq!(slack.scopes, vec!["chat:write", "users:read"]);
        assert_eq!(slack.display_name("slack"), "Slack");
        assert_eq!(
            options
                .provider("microsoft")
                .unwrap()
                .display_name("microsoft"),
            "Microsoft 365"
        );
        assert_eq!(
            options.provider("google").unwrap().resolved_type("google"),
            Some(ProviderType::Google)
        );
        assert_eq!(options.provider("edr").unwrap().display_name("edr"), "edr");
    }

    #[test]
    fn invalid_providers_fail_naming_the_key() {
        let error = options(json!({
            "providers": {
                "crm": { "client_id": "x" },
                "slack": {},
                "acme": { "type": "oauth2", "client_id": "a" },
                "Bad Key": { "type": "api_key" },
                "providers": { "type": "api_key" },
                "microsoft": { "client_id": "m", "apps": { "mail_send": {} } }
            }
        }))
        .unwrap_err();
        assert!(
            error.contains("integrations.providers.crm.type is required"),
            "{error}"
        );
        assert!(
            error.contains("integrations.providers.slack.client_id is required"),
            "{error}"
        );
        assert!(
            error.contains("integrations.providers.acme.token_url is required"),
            "{error}"
        );
        assert!(
            error.contains("integrations.providers.Bad Key: provider keys"),
            "{error}"
        );
        assert!(
            error.contains("integrations.providers.providers: provider keys"),
            "{error}"
        );
        assert!(
            error.contains("integrations.redirect_base is required"),
            "{error}"
        );
        assert!(
            error.contains("apps.mail_send.client_id is required"),
            "{error}"
        );
    }

    #[test]
    fn api_keys_and_service_accounts_need_no_redirect_base() {
        let options = options(json!({
            "providers": { "google": {}, "edr": { "type": "api_key" } }
        }))
        .unwrap();
        assert!(options.check().is_ok());
        let error = options
            .with_provider(
                "google",
                ProviderConfig {
                    client_id: Some("g".into()),
                    ..ProviderConfig::default()
                },
            )
            .check()
            .unwrap_err()
            .join("; ");
        assert!(error.contains("redirect_base is required"), "{error}");
    }

    #[test]
    fn secrets_never_appear_in_debug_output() {
        let config = ProviderConfig::new(ProviderType::Slack).client("id", "super-secret");
        let mut apps = BTreeMap::new();
        apps.insert(
            "mail".to_owned(),
            AppCredentials {
                client_id: "m".into(),
                client_secret: Some("other-secret".into()),
            },
        );
        let config = ProviderConfig { apps, ..config };
        let debug = format!("{config:?}");
        assert!(!debug.contains("super-secret") && !debug.contains("other-secret"));
        assert!(debug.contains("***"));
    }

    #[test]
    fn bad_urls_are_refused() {
        let error = options(json!({
            "redirect_base": "/relative",
            "providers": { "edr": { "type": "api_key", "api_url": "ftp://x" } }
        }))
        .unwrap_err();
        assert!(
            error.contains("redirect_base must be an absolute"),
            "{error}"
        );
        assert!(error.contains("edr.api_url must be an absolute"), "{error}");
    }
}
