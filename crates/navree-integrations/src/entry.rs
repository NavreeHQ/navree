//! [`ProviderEntry`]: a configured provider with its endpoints resolved.

use std::collections::BTreeMap;

use navree_contracts::Secret;
use url::Url;

use crate::options::MicrosoftMode;
use crate::provider::{DEFAULT_SCOPE_SET, ScopeSetInfo, ScopeSetRegistration, scope_sets_for};
use crate::{CredentialKind, IntegrationsError, ProviderConfig, ProviderType};

/// Microsoft's sign-in host.
pub(crate) const MICROSOFT_AUTHORITY: &str = "https://login.microsoftonline.com";
/// Microsoft Graph's v1.0 root.
pub(crate) const GRAPH_API: &str = "https://graph.microsoft.com/v1.0";
/// The scope of app-only Graph tokens.
pub(crate) const GRAPH_DEFAULT_SCOPE: &str = "https://graph.microsoft.com/.default";
/// Google's authorization endpoint.
pub(crate) const GOOGLE_AUTHORIZE: &str = "https://accounts.google.com/o/oauth2/v2/auth";
/// Google's token endpoint (also the `aud` of service-account assertions).
pub(crate) const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";
/// Google's revocation endpoint.
pub(crate) const GOOGLE_REVOKE: &str = "https://oauth2.googleapis.com/revoke";
/// Google's API root.
pub(crate) const GOOGLE_API: &str = "https://www.googleapis.com";
/// Slack's authorization endpoint.
pub(crate) const SLACK_AUTHORIZE: &str = "https://slack.com/oauth/v2/authorize";
/// Slack's token endpoint.
pub(crate) const SLACK_TOKEN: &str = "https://slack.com/api/oauth.v2.access";
/// Slack's Web API root.
pub(crate) const SLACK_API: &str = "https://slack.com/api";

/// A health probe: what `integrations:check` asks the API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    /// `GET` or `POST`.
    pub(crate) method: http::Method,
    /// A path under the API root, or an URL on an allowed host.
    pub(crate) path: String,
}

/// A configured provider.
#[derive(Clone, Debug)]
pub(crate) struct ProviderEntry {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) kind: ProviderType,
    pub(crate) config: ProviderConfig,
    /// The API root, when the provider has one.
    pub(crate) api_url: Option<Url>,
    /// Hosts a typed client may send the token to: the API root's, plus
    /// `*.suffix` patterns.
    pub(crate) allowed_hosts: Vec<String>,
}

impl ProviderEntry {
    /// The entry of provider `key` (options already checked).
    pub(crate) fn new(key: &str, config: &ProviderConfig) -> Result<Self, IntegrationsError> {
        let kind = config.resolved_type(key).ok_or_else(|| {
            IntegrationsError::Config(format!("integrations.providers.{key}.type is required"))
        })?;
        let api = config.api_url.clone().or_else(|| {
            match kind {
                ProviderType::Microsoft => Some(GRAPH_API),
                ProviderType::Google => Some(GOOGLE_API),
                ProviderType::Slack => Some(SLACK_API),
                ProviderType::OAuth2 | ProviderType::ApiKey => None,
            }
            .map(str::to_owned)
        });
        let api_url = api
            .map(|api| {
                Url::parse(&api).map_err(|_| {
                    IntegrationsError::Config(format!(
                        "integrations.providers.{key}.api_url is not a URL"
                    ))
                })
            })
            .transpose()?;
        let mut allowed_hosts: Vec<String> = api_url
            .as_ref()
            .and_then(Url::host_str)
            .map(str::to_owned)
            .into_iter()
            .collect();
        if kind == ProviderType::Google {
            allowed_hosts.push("*.googleapis.com".to_owned());
        }
        Ok(Self {
            key: key.to_owned(),
            name: config.display_name(key),
            kind,
            config: config.clone(),
            api_url,
            allowed_hosts,
        })
    }

    /// How it connects.
    pub(crate) fn kinds(&self) -> Vec<CredentialKind> {
        match self.kind {
            ProviderType::Microsoft => vec![match self.config.mode {
                MicrosoftMode::App => CredentialKind::AppOnly,
                MicrosoftMode::Delegated => CredentialKind::OAuth,
            }],
            ProviderType::Google => {
                let mut kinds = vec![CredentialKind::ServiceAccount];
                if self.config.client_id.is_some() {
                    kinds.push(CredentialKind::OAuth);
                }
                kinds
            }
            ProviderType::Slack | ProviderType::OAuth2 => vec![CredentialKind::OAuth],
            ProviderType::ApiKey => vec![CredentialKind::ApiKey],
        }
    }

    /// The kind a consent screen gives (`None`: no consent screen).
    pub(crate) fn consent_kind(&self) -> Option<CredentialKind> {
        self.kinds()
            .into_iter()
            .find(|kind| matches!(kind, CredentialKind::OAuth | CredentialKind::AppOnly))
    }

    /// Fails unless the provider connects with `kind`.
    pub(crate) fn require(&self, kind: CredentialKind) -> Result<(), IntegrationsError> {
        if self.kinds().contains(&kind) {
            Ok(())
        } else {
            Err(IntegrationsError::Unsupported {
                provider: self.name.clone(),
                kind: kind.phrase(),
            })
        }
    }

    /// Whether the consent flow sends a PKCE challenge.
    pub(crate) fn pkce(&self) -> bool {
        if self.consent_kind() != Some(CredentialKind::OAuth) {
            return false;
        }
        self.config
            .pkce
            .unwrap_or(!matches!(self.kind, ProviderType::Slack))
    }

    /// Microsoft's sign-in host.
    pub(crate) fn authority(&self) -> String {
        self.config
            .authority_url
            .as_deref()
            .unwrap_or(MICROSOFT_AUTHORITY)
            .trim_end_matches('/')
            .to_owned()
    }

    /// The tenant whose admins may consent (Microsoft).
    pub(crate) fn consent_tenant(&self) -> &str {
        self.config.tenant.as_deref().unwrap_or("organizations")
    }

    /// The authorization (consent) endpoint.
    pub(crate) fn authorize_endpoint(&self) -> String {
        if let Some(url) = &self.config.authorize_url {
            return url.clone();
        }
        match self.kind {
            ProviderType::Microsoft => {
                let tenant = self.consent_tenant();
                match self.config.mode {
                    MicrosoftMode::App => {
                        format!("{}/{tenant}/v2.0/adminconsent", self.authority())
                    }
                    MicrosoftMode::Delegated => {
                        format!("{}/{tenant}/oauth2/v2.0/authorize", self.authority())
                    }
                }
            }
            ProviderType::Google => GOOGLE_AUTHORIZE.to_owned(),
            ProviderType::Slack => SLACK_AUTHORIZE.to_owned(),
            ProviderType::OAuth2 | ProviderType::ApiKey => String::new(),
        }
    }

    /// The token endpoint; Microsoft's is per tenant.
    pub(crate) fn token_endpoint(&self, tenant: Option<&str>) -> String {
        if let Some(url) = &self.config.token_url {
            return url.clone();
        }
        match self.kind {
            ProviderType::Microsoft => {
                let tenant = tenant.unwrap_or_else(|| self.consent_tenant());
                format!("{}/{tenant}/oauth2/v2.0/token", self.authority())
            }
            ProviderType::Google => GOOGLE_TOKEN.to_owned(),
            ProviderType::Slack => SLACK_TOKEN.to_owned(),
            ProviderType::OAuth2 | ProviderType::ApiKey => String::new(),
        }
    }

    /// The revocation endpoint, if the provider has one.
    pub(crate) fn revoke_endpoint(&self) -> Option<String> {
        if let Some(url) = &self.config.revoke_url {
            return Some(url.clone());
        }
        match self.kind {
            ProviderType::Google => Some(GOOGLE_REVOKE.to_owned()),
            ProviderType::Slack => Some(self.api_path("auth.revoke")),
            _ => None,
        }
    }

    /// `path` under the API root.
    pub(crate) fn api_path(&self, path: &str) -> String {
        match &self.api_url {
            Some(base) => format!(
                "{}/{}",
                base.as_str().trim_end_matches('/'),
                path.trim_start_matches('/')
            ),
            None => path.to_owned(),
        }
    }

    /// How scopes are joined in an authorization URL.
    pub(crate) fn scope_separator(&self) -> &'static str {
        if self.kind == ProviderType::Slack {
            ","
        } else {
            " "
        }
    }

    /// The client id and secret for `scope_set`: its own registration under
    /// `apps`, else the provider's.
    pub(crate) fn client(
        &self,
        scope_set: &str,
    ) -> Result<(String, Option<Secret<String>>), IntegrationsError> {
        if let Some(app) = self.config.apps.get(scope_set) {
            return Ok((app.client_id.clone(), app.client_secret.clone()));
        }
        let id = self.config.client_id.clone().ok_or_else(|| {
            IntegrationsError::Config(format!(
                "integrations.providers.{}.client_id is not set",
                self.key
            ))
        })?;
        Ok((id, self.config.client_secret.clone()))
    }

    /// The scopes of `scope_set` at this provider.
    /// [`IntegrationsError::UnknownScopeSet`] when it is not registered or
    /// names no scopes here.
    pub(crate) fn scopes_for(
        &self,
        scope_set: &str,
        registered: &BTreeMap<&'static str, ScopeSetRegistration>,
    ) -> Result<Vec<String>, IntegrationsError> {
        if scope_set == DEFAULT_SCOPE_SET {
            return Ok(self.config.scopes.clone());
        }
        let unknown = || IntegrationsError::UnknownScopeSet {
            provider: self.name.clone(),
            scope_set: scope_set.to_owned(),
        };
        let registration = registered.get(scope_set).ok_or_else(unknown)?;
        let scopes = (registration.scopes)(&self.key);
        if scopes.is_empty() {
            return Err(unknown());
        }
        Ok(scopes.iter().map(|s| (*s).to_owned()).collect())
    }

    /// Every scope set this provider can be connected with.
    pub(crate) fn scope_sets(
        &self,
        registered: &BTreeMap<&'static str, ScopeSetRegistration>,
    ) -> Vec<ScopeSetInfo> {
        scope_sets_for(&self.key, &self.config.scopes, registered)
    }

    /// The health probe, if any: `health_url`, else Slack's `auth.test`.
    pub(crate) fn probe(&self) -> Option<Probe> {
        if let Some(path) = &self.config.health_url {
            return Some(Probe {
                method: http::Method::GET,
                path: path.clone(),
            });
        }
        (self.kind == ProviderType::Slack).then(|| Probe {
            method: http::Method::POST,
            path: "auth.test".to_owned(),
        })
    }

    /// Whether a typed client may send the token to `host`.
    pub(crate) fn allows_host(&self, host: &str, extra: Option<&Url>) -> bool {
        let host = host.to_ascii_lowercase();
        if extra
            .and_then(Url::host_str)
            .is_some_and(|h| h.eq_ignore_ascii_case(&host))
        {
            return true;
        }
        self.allowed_hosts
            .iter()
            .any(|allowed| match allowed.strip_prefix("*.") {
                Some(suffix) => host.ends_with(&format!(".{suffix}")),
                None => allowed.eq_ignore_ascii_case(&host),
            })
    }

    /// The `api_key` header and its value for `key`.
    pub(crate) fn api_key_header(&self, key: &str) -> (String, String) {
        let header = self
            .config
            .header
            .clone()
            .unwrap_or_else(|| "Authorization".to_owned());
        let scheme = self.config.scheme.as_deref().unwrap_or("Bearer");
        let value = if scheme.is_empty() {
            key.to_owned()
        } else {
            format!("{scheme} {key}")
        };
        (header, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, f: impl FnOnce(&mut ProviderConfig)) -> ProviderEntry {
        let mut config = ProviderConfig::default();
        f(&mut config);
        ProviderEntry::new(key, &config).unwrap()
    }

    #[test]
    fn microsoft_defaults_to_admin_consent_and_app_only_tokens() {
        let ms = entry("microsoft", |c| c.client_id = Some("id".into()));
        assert_eq!(ms.kinds(), vec![CredentialKind::AppOnly]);
        assert_eq!(
            ms.authorize_endpoint(),
            "https://login.microsoftonline.com/organizations/v2.0/adminconsent"
        );
        assert_eq!(
            ms.token_endpoint(Some("t-1")),
            "https://login.microsoftonline.com/t-1/oauth2/v2.0/token"
        );
        assert!(!ms.pkce());
        assert_eq!(
            ms.api_path("/users"),
            "https://graph.microsoft.com/v1.0/users"
        );
        assert!(ms.allows_host("graph.microsoft.com", None));
        assert!(!ms.allows_host("evil.test", None));

        let delegated = entry("microsoft", |c| {
            c.client_id = Some("id".into());
            c.mode = MicrosoftMode::Delegated;
            c.tenant = Some("common".into());
        });
        assert_eq!(delegated.kinds(), vec![CredentialKind::OAuth]);
        assert!(delegated.pkce());
        assert_eq!(
            delegated.authorize_endpoint(),
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
        );
    }

    #[test]
    fn google_takes_service_accounts_and_oauth_with_a_client() {
        let google = entry("google", |_| {});
        assert_eq!(google.kinds(), vec![CredentialKind::ServiceAccount]);
        assert_eq!(google.consent_kind(), None);
        assert!(google.allows_host("admin.googleapis.com", None));
        assert!(google.allows_host("www.googleapis.com", None));
        assert!(!google.allows_host("googleapis.com.evil.test", None));
        let with_client = entry("google", |c| c.client_id = Some("g".into()));
        assert_eq!(with_client.consent_kind(), Some(CredentialKind::OAuth));
        assert_eq!(
            with_client.revoke_endpoint().as_deref(),
            Some(GOOGLE_REVOKE)
        );
    }

    #[test]
    fn slack_joins_scopes_with_commas_and_probes_auth_test() {
        let slack = entry("slack", |c| c.client_id = Some("s".into()));
        assert_eq!(slack.scope_separator(), ",");
        assert!(!slack.pkce());
        assert_eq!(
            slack.probe(),
            Some(Probe {
                method: http::Method::POST,
                path: "auth.test".into()
            })
        );
        assert_eq!(
            slack.revoke_endpoint().as_deref(),
            Some("https://slack.com/api/auth.revoke")
        );
    }

    #[test]
    fn scope_sets_may_have_their_own_app_registration() {
        let ms = entry("microsoft", |c| {
            c.client_id = Some("main".into());
            c.client_secret = Some(Secret::new("s1".to_owned()));
            c.apps.insert(
                "mail_send".into(),
                crate::AppCredentials {
                    client_id: "mail".into(),
                    client_secret: Some(Secret::new("s2".to_owned())),
                },
            );
        });
        assert_eq!(ms.client("default").unwrap().0, "main");
        assert_eq!(ms.client("directory").unwrap().0, "main");
        assert_eq!(
            ms.client("mail_send").unwrap(),
            ("mail".into(), Some(Secret::new("s2".to_owned())))
        );
    }

    #[test]
    fn api_keys_go_in_the_configured_header() {
        let plain = entry("edr", |c| c.kind = Some(ProviderType::ApiKey));
        assert_eq!(
            plain.api_key_header("k"),
            ("Authorization".into(), "Bearer k".into())
        );
        let custom = entry("edr", |c| {
            c.kind = Some(ProviderType::ApiKey);
            c.header = Some("X-Api-Key".into());
            c.scheme = Some(String::new());
        });
        assert_eq!(custom.api_key_header("k"), ("X-Api-Key".into(), "k".into()));
    }
}
