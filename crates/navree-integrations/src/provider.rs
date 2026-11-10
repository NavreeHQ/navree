//! [`Provider`] (the typed handle of a provider) and [`ScopeSet`] (the
//! least-privilege scopes a feature declares).

use std::collections::BTreeMap;

use navree_openapi::ToSchema;
use serde::Serialize;

use crate::client::{GoogleClient, GraphClient, IntegrationClient, SlackClient};
use crate::{CredentialKind, ProviderType};

/// A provider as a type: its key under `integrations.providers` and the
/// typed client its connections give. The [`Connection`](crate::Connection)
/// extractor and [`Integrations::client_for`](crate::Integrations::client_for)
/// take one.
///
/// Navree ships [`MicrosoftGraph`], [`GoogleWorkspace`] and [`Slack`]. Any
/// other API is configured with `type = "oauth2"` or `type = "api_key"`;
/// give it a type of your own to use it in extractors:
///
/// ```
/// use navree_integrations::{IntegrationClient, Provider};
///
/// /// `[integrations.providers.hubspot]`, `type = "oauth2"`.
/// pub struct HubSpot;
///
/// impl Provider for HubSpot {
///     const KEY: &'static str = "hubspot";
///     type Client = IntegrationClient;
/// }
/// ```
pub trait Provider: Send + Sync + 'static {
    /// The provider key (`"microsoft"`).
    const KEY: &'static str;
    /// The typed client a connection gives.
    type Client: From<IntegrationClient> + Send + Sync + 'static;
}

/// Microsoft Graph (`integrations.providers.microsoft`): Microsoft 365 and
/// Entra ID. Its client is a [`GraphClient`] on
/// `https://graph.microsoft.com/v1.0`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MicrosoftGraph;

impl Provider for MicrosoftGraph {
    const KEY: &'static str = "microsoft";
    type Client = GraphClient;
}

/// Google Workspace (`integrations.providers.google`). Its client is a
/// [`GoogleClient`] on `https://www.googleapis.com` (and the other
/// `*.googleapis.com` hosts).
#[derive(Clone, Copy, Debug, Default)]
pub struct GoogleWorkspace;

impl Provider for GoogleWorkspace {
    const KEY: &'static str = "google";
    type Client = GoogleClient;
}

/// Slack (`integrations.providers.slack`). Its client is a [`SlackClient`]
/// on `https://slack.com/api`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Slack;

impl Provider for Slack {
    const KEY: &'static str = "slack";
    type Client = SlackClient;
}

/// The scopes one feature needs, per provider: least privilege, so reading
/// the directory never grants sending mail. Each scope set is connected
/// separately (one consent and one connection per provider and scope set),
/// so revoking one leaves the others working.
///
/// Register it with `IntegrationsModule::for_feature::<S>()` or
/// [`ModuleBuilderIntegrationsExt::integration_scope_set`](crate::ModuleBuilderIntegrationsExt::integration_scope_set),
/// and name it in [`Connection<P, S>`](crate::Connection).
///
/// ```
/// use navree_integrations::ScopeSet;
///
/// /// Read users and groups.
/// pub struct DirectoryRead;
///
/// impl ScopeSet for DirectoryRead {
///     const NAME: &'static str = "directory";
///     const DESCRIPTION: &'static str = "Read users and groups";
///
///     fn scopes(provider: &str) -> &'static [&'static str] {
///         match provider {
///             "microsoft" => &["https://graph.microsoft.com/User.Read.All"],
///             "google" => &["https://www.googleapis.com/auth/admin.directory.user.readonly"],
///             _ => &[],
///         }
///     }
/// }
/// ```
pub trait ScopeSet: Send + Sync + 'static {
    /// The name in URLs and rows (`directory`): lower-case letters, digits
    /// and `_`.
    const NAME: &'static str;
    /// What it allows, for a settings page.
    const DESCRIPTION: &'static str = "";
    /// The scopes it needs at `provider`; empty when the provider does not
    /// support this scope set.
    fn scopes(provider: &str) -> &'static [&'static str];
}

/// The default scope set: the scopes configured under
/// `integrations.providers.<key>.scopes`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultScopes;

impl ScopeSet for DefaultScopes {
    const NAME: &'static str = DEFAULT_SCOPE_SET;
    const DESCRIPTION: &'static str = "The provider's configured scopes";

    fn scopes(_provider: &str) -> &'static [&'static str] {
        &[]
    }
}

/// The name of the default scope set.
pub const DEFAULT_SCOPE_SET: &str = "default";

/// A scope set registered by a feature module (a contribution).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScopeSetRegistration {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) scopes: fn(&str) -> &'static [&'static str],
}

impl ScopeSetRegistration {
    pub(crate) fn of<S: ScopeSet>() -> Self {
        Self {
            name: S::NAME,
            description: S::DESCRIPTION,
            scopes: S::scopes,
        }
    }
}

/// Scope set names: lower-case letters, digits and `_`.
pub(crate) fn is_scope_set_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A configured provider, for a settings page (`GET /api/integrations/providers`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProviderInfo {
    /// The key (`microsoft`).
    pub key: String,
    /// The display name (`Microsoft 365`).
    pub name: String,
    /// The type.
    #[serde(rename = "type")]
    pub kind: ProviderType,
    /// How it connects: `GET .../connect` for `oauth` and `app_only`,
    /// `POST .../service-account` and `POST .../api-key` for the others.
    pub kinds: Vec<CredentialKind>,
    /// The scope sets it can be connected with (`default` first).
    pub scope_sets: Vec<ScopeSetInfo>,
}

/// A scope set as one provider sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ScopeSetInfo {
    /// The name (`directory`).
    pub name: String,
    /// What it allows.
    pub description: String,
    /// The scopes requested from this provider.
    pub scopes: Vec<String>,
}

/// The scope sets of every provider key: `default` from the configuration,
/// then each registered set that names scopes for the provider.
pub(crate) fn scope_sets_for(
    provider: &str,
    configured: &[String],
    registered: &BTreeMap<&'static str, ScopeSetRegistration>,
) -> Vec<ScopeSetInfo> {
    let mut sets = vec![ScopeSetInfo {
        name: DEFAULT_SCOPE_SET.to_owned(),
        description: DefaultScopes::DESCRIPTION.to_owned(),
        scopes: configured.to_vec(),
    }];
    for registration in registered.values() {
        if registration.name == DEFAULT_SCOPE_SET {
            continue;
        }
        let scopes = (registration.scopes)(provider);
        if !scopes.is_empty() {
            sets.push(ScopeSetInfo {
                name: registration.name.to_owned(),
                description: registration.description.to_owned(),
                scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            });
        }
    }
    sets
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MailSend;

    impl ScopeSet for MailSend {
        const NAME: &'static str = "mail_send";
        const DESCRIPTION: &'static str = "Send mail as any user";

        fn scopes(provider: &str) -> &'static [&'static str] {
            match provider {
                "microsoft" => &["https://graph.microsoft.com/Mail.Send"],
                _ => &[],
            }
        }
    }

    #[test]
    fn scope_sets_list_default_then_the_ones_a_provider_supports() {
        let mut registered = BTreeMap::new();
        registered.insert(MailSend::NAME, ScopeSetRegistration::of::<MailSend>());
        let microsoft = scope_sets_for("microsoft", &[], &registered);
        assert_eq!(microsoft.len(), 2);
        assert_eq!(microsoft[0].name, "default");
        assert_eq!(microsoft[1].name, "mail_send");
        assert_eq!(
            microsoft[1].scopes,
            vec!["https://graph.microsoft.com/Mail.Send"]
        );
        let slack = scope_sets_for("slack", &["chat:write".to_owned()], &registered);
        assert_eq!(slack.len(), 1);
        assert_eq!(slack[0].scopes, vec!["chat:write"]);
    }

    #[test]
    fn scope_set_names_are_snake_case() {
        assert!(is_scope_set_name("mail_send"));
        assert!(is_scope_set_name("directory2"));
        assert!(!is_scope_set_name("Mail"));
        assert!(!is_scope_set_name("mail-send"));
        assert!(!is_scope_set_name(""));
    }

    #[test]
    fn built_in_providers_have_their_keys() {
        assert_eq!(MicrosoftGraph::KEY, "microsoft");
        assert_eq!(GoogleWorkspace::KEY, "google");
        assert_eq!(Slack::KEY, "slack");
        assert_eq!(DefaultScopes::NAME, "default");
    }
}
