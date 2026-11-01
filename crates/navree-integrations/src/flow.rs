//! The inputs and answers of the connect flows.

use std::collections::BTreeMap;
use std::fmt;

use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ConnectionInfo, ServiceAccountKey};

/// Options of [`Integrations::connect_url`](crate::Integrations::connect_url).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectOptions {
    /// The scope set to connect (default `default`).
    pub scope_set: Option<String>,
    /// A relative path to send the admin back to; it comes back from the
    /// callback.
    pub return_to: Option<String>,
    /// The admin starting the flow: the callback must come from the same
    /// user.
    pub user_id: Option<Uuid>,
    /// Extra authorization URL parameters (`prompt = "select_account"`).
    pub params: BTreeMap<String, String>,
}

impl ConnectOptions {
    /// Connects `scope_set`.
    pub fn scope_set(mut self, scope_set: impl Into<String>) -> Self {
        self.scope_set = Some(scope_set.into());
        self
    }

    /// Returns to `path` afterwards.
    pub fn return_to(mut self, path: impl Into<String>) -> Self {
        self.return_to = Some(path.into());
        self
    }

    /// Binds the flow to the admin `user_id`.
    pub fn user(mut self, user_id: Uuid) -> Self {
        self.user_id = Some(user_id);
        self
    }
}

/// Where to send the admin: the provider's consent screen, and the sealed
/// state (keep it in a short-lived cookie to compare on the callback).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ConnectRedirect {
    /// The consent URL.
    pub url: String,
    /// The sealed state (`nvi1.`...).
    pub state: String,
}

/// What the provider sent back to `{redirect_base}/{provider}`, forwarded
/// unchanged: `code` from an OAuth consent, `tenant` and `admin_consent`
/// from Microsoft's admin consent, `error` and `error_description` when the
/// admin declined.
#[derive(Clone, Default, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ConnectCallback {
    /// The sealed state.
    pub state: String,
    /// The authorization code.
    #[serde(default)]
    pub code: Option<String>,
    /// Microsoft: the tenant that consented.
    #[serde(default)]
    pub tenant: Option<String>,
    /// Microsoft: `True` when the admin consented.
    #[serde(default)]
    pub admin_consent: Option<String>,
    /// The provider's error (`access_denied`).
    #[serde(default)]
    pub error: Option<String>,
    /// Its description.
    #[serde(default)]
    pub error_description: Option<String>,
}

impl fmt::Debug for ConnectCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectCallback")
            .field("has_code", &self.code.is_some())
            .field("tenant", &self.tenant)
            .field("admin_consent", &self.admin_consent)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl ConnectCallback {
    /// A callback with a code.
    pub fn code(state: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            code: Some(code.into()),
            ..Self::default()
        }
    }

    /// Microsoft's admin-consent callback for `tenant`.
    pub fn admin_consent(state: impl Into<String>, tenant: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            tenant: Some(tenant.into()),
            admin_consent: Some("True".to_owned()),
            ..Self::default()
        }
    }
}

/// A finished connect flow: the connection, and where to send the admin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CompletedConnection {
    /// The connection.
    #[serde(flatten)]
    pub connection: ConnectionInfo,
    /// The `return_to` of the flow.
    pub return_to: Option<String>,
}

/// A service-account connection
/// ([`Integrations::connect_service_account`](crate::Integrations::connect_service_account)).
#[derive(Clone, Debug)]
pub struct NewServiceAccount {
    /// The scope set (default `default`).
    pub scope_set: String,
    /// The key file.
    pub key: ServiceAccountKey,
    /// The user it acts as (domain-wide delegation): a Google Workspace
    /// admin for directory reads.
    pub subject: Option<String>,
    /// The admin connecting it.
    pub connected_by: Option<Uuid>,
}

impl NewServiceAccount {
    /// `key` in the default scope set, acting as `subject`.
    pub fn new(key: ServiceAccountKey, subject: Option<String>) -> Self {
        Self {
            scope_set: crate::provider::DEFAULT_SCOPE_SET.to_owned(),
            key,
            subject,
            connected_by: None,
        }
    }

    /// In `scope_set`.
    pub fn scope_set(mut self, scope_set: impl Into<String>) -> Self {
        self.scope_set = scope_set.into();
        self
    }

    /// Connected by `user_id`.
    pub fn connected_by(mut self, user_id: Uuid) -> Self {
        self.connected_by = Some(user_id);
        self
    }
}

/// An API-key connection ([`Integrations::connect_api_key`](crate::Integrations::connect_api_key)).
#[derive(Clone)]
pub struct NewApiKey {
    /// The scope set (default `default`).
    pub scope_set: String,
    /// The key.
    pub key: String,
    /// This connection's API root, when it differs per customer (an EDR
    /// console URL).
    pub api_url: Option<String>,
    /// Extra fields kept with the key (encrypted too).
    pub fields: BTreeMap<String, String>,
    /// The admin connecting it.
    pub connected_by: Option<Uuid>,
}

impl fmt::Debug for NewApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewApiKey")
            .field("scope_set", &self.scope_set)
            .field("api_url", &self.api_url)
            .field("fields", &self.fields.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl NewApiKey {
    /// `key` in the default scope set.
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            scope_set: crate::provider::DEFAULT_SCOPE_SET.to_owned(),
            key: key.into(),
            api_url: None,
            fields: BTreeMap::new(),
            connected_by: None,
        }
    }

    /// In `scope_set`.
    pub fn scope_set(mut self, scope_set: impl Into<String>) -> Self {
        self.scope_set = scope_set.into();
        self
    }

    /// With its own API root.
    pub fn api_url(mut self, url: impl Into<String>) -> Self {
        self.api_url = Some(url.into());
        self
    }

    /// With an extra field.
    pub fn field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(name.into(), value.into());
        self
    }

    /// Connected by `user_id`.
    pub fn connected_by(mut self, user_id: Uuid) -> Self {
        self.connected_by = Some(user_id);
        self
    }
}
