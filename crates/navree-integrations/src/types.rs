//! The values the service hands out: [`Owner`], [`ConnectionInfo`],
//! [`CredentialKind`], [`ServiceAccountKey`] and friends.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use navree_contracts::Secret;
use navree_crypto::jose::{Algorithm, SigningKey};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::IntegrationsError;

/// Who a connection belongs to.
///
/// A workspace without a connection of its own uses its organization's
/// (when organizations are on), so an organization connects Microsoft 365
/// once for all its workspaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Owner {
    /// The app itself (no tenant).
    App,
    /// A workspace.
    Workspace(Uuid),
    /// An organization.
    Organization(Uuid),
}

impl Owner {
    /// The owner's type.
    pub fn kind(&self) -> OwnerType {
        match self {
            Owner::App => OwnerType::App,
            Owner::Workspace(_) => OwnerType::Workspace,
            Owner::Organization(_) => OwnerType::Organization,
        }
    }

    /// The workspace or organization id (`None` for the app).
    pub fn id(&self) -> Option<Uuid> {
        match self {
            Owner::App => None,
            Owner::Workspace(id) | Owner::Organization(id) => Some(*id),
        }
    }

    /// The row's `owner_type` and `owner_id` (the nil UUID for the app).
    pub(crate) fn columns(&self) -> (&'static str, Uuid) {
        (self.kind().as_str(), self.id().unwrap_or(Uuid::nil()))
    }

    /// The owner of a row.
    pub(crate) fn from_columns(kind: &str, id: Uuid) -> Owner {
        match kind {
            "workspace" => Owner::Workspace(id),
            "organization" => Owner::Organization(id),
            _ => Owner::App,
        }
    }
}

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Owner::App => f.write_str("app"),
            Owner::Workspace(id) => write!(f, "workspace:{id}"),
            Owner::Organization(id) => write!(f, "organization:{id}"),
        }
    }
}

/// The type of an [`Owner`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OwnerType {
    /// The app itself.
    App,
    /// A workspace.
    Workspace,
    /// An organization.
    Organization,
}

impl OwnerType {
    /// The stored name (`"workspace"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            OwnerType::App => "app",
            OwnerType::Workspace => "workspace",
            OwnerType::Organization => "organization",
        }
    }
}

/// How a connection authenticates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CredentialKind {
    /// Delegated OAuth tokens (an access token and a refresh token).
    #[serde(rename = "oauth")]
    OAuth,
    /// A tenant's admin consent; tokens are app-only (client credentials).
    AppOnly,
    /// A Google service-account key with domain-wide delegation.
    ServiceAccount,
    /// An API key or token.
    ApiKey,
}

impl CredentialKind {
    /// The stored name (`"app_only"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            CredentialKind::OAuth => "oauth",
            CredentialKind::AppOnly => "app_only",
            CredentialKind::ServiceAccount => "service_account",
            CredentialKind::ApiKey => "api_key",
        }
    }

    pub(crate) fn parse(value: &str) -> CredentialKind {
        match value {
            "app_only" => CredentialKind::AppOnly,
            "service_account" => CredentialKind::ServiceAccount,
            "api_key" => CredentialKind::ApiKey,
            _ => CredentialKind::OAuth,
        }
    }

    /// A phrase for messages ("an API key").
    pub(crate) const fn phrase(self) -> &'static str {
        match self {
            CredentialKind::OAuth => "OAuth consent",
            CredentialKind::AppOnly => "admin consent",
            CredentialKind::ServiceAccount => "a service account",
            CredentialKind::ApiKey => "an API key",
        }
    }
}

impl fmt::Display for CredentialKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a connection works.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConnectionStatus {
    /// The last token or check succeeded (or none has failed yet).
    Healthy,
    /// The provider refused the credentials, or could not be reached
    /// `integrations.unhealthy_after` times in a row.
    Unhealthy,
}

impl ConnectionStatus {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ConnectionStatus::Healthy => "healthy",
            ConnectionStatus::Unhealthy => "unhealthy",
        }
    }

    pub(crate) fn parse(value: &str) -> ConnectionStatus {
        if value == "unhealthy" {
            ConnectionStatus::Unhealthy
        } else {
            ConnectionStatus::Healthy
        }
    }
}

/// A connection, without its credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ConnectionInfo {
    /// The id (UUID v7).
    pub id: Uuid,
    /// Who it belongs to.
    pub owner_type: OwnerType,
    /// The workspace or organization (`null` for the app).
    pub owner_id: Option<Uuid>,
    /// The provider key (`microsoft`).
    pub provider: String,
    /// The provider's display name (`Microsoft 365`).
    pub provider_name: String,
    /// The scope set (`default`, `directory`).
    pub scope_set: String,
    /// How it authenticates.
    pub kind: CredentialKind,
    /// The connected tenant at the provider: the Microsoft tenant id, the
    /// Slack team id, the Google Workspace domain.
    pub external_id: Option<String>,
    /// Its name, when the provider reports one (the Slack team, the
    /// service account).
    pub display_name: Option<String>,
    /// The scopes granted (or requested, when the provider does not say).
    pub scopes: Vec<String>,
    /// Whether it works.
    pub status: ConnectionStatus,
    /// Consecutive failed checks.
    pub failures: u32,
    /// The last failure, as the provider put it (never a credential).
    pub last_error: Option<String>,
    /// When it was last checked.
    pub last_checked_at: Option<DateTime<Utc>>,
    /// The user who connected it.
    pub connected_by: Option<Uuid>,
    /// When it was connected.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl ConnectionInfo {
    /// Who it belongs to.
    pub fn owner(&self) -> Owner {
        match (self.owner_type, self.owner_id) {
            (OwnerType::Workspace, Some(id)) => Owner::Workspace(id),
            (OwnerType::Organization, Some(id)) => Owner::Organization(id),
            _ => Owner::App,
        }
    }

    /// Whether the status is [`ConnectionStatus::Healthy`].
    pub fn is_healthy(&self) -> bool {
        self.status == ConnectionStatus::Healthy
    }
}

/// The result of a health check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Health {
    /// The connection.
    pub connection_id: Uuid,
    /// Its status after the check.
    pub status: ConnectionStatus,
    /// Consecutive failed checks.
    pub failures: u32,
    /// What failed, if the check did.
    pub error: Option<String>,
    /// When it ran.
    pub checked_at: DateTime<Utc>,
}

/// What `integrations:check` did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CheckSummary {
    /// Connections checked.
    pub checked: usize,
    /// Healthy after the check.
    pub healthy: usize,
    /// Unhealthy after the check.
    pub unhealthy: usize,
}

/// A Google service-account key: the JSON file Google Cloud gives you
/// (`type`, `client_email`, `private_key`, `private_key_id`, ...).
///
/// The private key is an RSA key in PKCS#8 PEM; Navree signs the RS256
/// assertions with `ring` (through `navree_crypto::jose`). The key's
/// `token_uri` is ignored: tokens always come from the provider's
/// configured token endpoint. `Debug` never shows the private key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceAccountKey {
    /// The service account (`navree@acme-prod.iam.gserviceaccount.com`).
    pub client_email: String,
    /// The private key (PEM).
    pub private_key: String,
    /// The key id, sent as the JWT `kid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_id: Option<String>,
    /// The service account's numeric client id (what an admin authorizes
    /// for domain-wide delegation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The Google Cloud project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl fmt::Debug for ServiceAccountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceAccountKey")
            .field("client_email", &self.client_email)
            .field("private_key_id", &self.private_key_id)
            .field("client_id", &self.client_id)
            .field("project_id", &self.project_id)
            .finish_non_exhaustive()
    }
}

impl ServiceAccountKey {
    /// Reads a key file's JSON.
    /// [`IntegrationsError::InvalidCredentials`] when it is not a service
    /// account key with an RSA private key.
    pub fn from_json(json: &str) -> Result<Self, IntegrationsError> {
        let value: Value = serde_json::from_str(json).map_err(|_| {
            IntegrationsError::InvalidCredentials("the service account key is not JSON".to_owned())
        })?;
        Self::from_value(&value)
    }

    /// Reads a key file's parsed JSON.
    pub fn from_value(value: &Value) -> Result<Self, IntegrationsError> {
        let invalid = |message: &str| IntegrationsError::InvalidCredentials(message.to_owned());
        if let Some(kind) = value.get("type").and_then(Value::as_str)
            && kind != "service_account"
        {
            return Err(invalid("the key file is not a service account key"));
        }
        let key: ServiceAccountKey = serde_json::from_value(value.clone()).map_err(|_| {
            invalid("the service account key needs `client_email` and `private_key`")
        })?;
        if key.client_email.trim().is_empty() || !key.client_email.contains('@') {
            return Err(invalid("the service account key has no `client_email`"));
        }
        key.signing_key()?;
        Ok(key)
    }

    /// The RS256 signing key.
    pub(crate) fn signing_key(&self) -> Result<SigningKey, IntegrationsError> {
        let key = SigningKey::from_pem(&self.private_key).map_err(|_| {
            IntegrationsError::InvalidCredentials(
                "the service account's private key can't be read".to_owned(),
            )
        })?;
        if key.algorithm() != Algorithm::RS256 {
            return Err(IntegrationsError::InvalidCredentials(
                "the service account's private key is not an RSA key".to_owned(),
            ));
        }
        Ok(match &self.private_key_id {
            Some(kid) => key.with_kid(kid.clone()),
            None => key,
        })
    }
}

/// An API-key connection's credentials, for code that signs requests
/// itself (an AWS SigV4 transport, for example).
#[derive(Clone, Debug)]
pub struct ApiKeyCredentials {
    /// The key.
    pub key: Secret<String>,
    /// The connection's own API root, when it has one.
    pub api_url: Option<String>,
    /// Extra fields stored with the key (an access key id, a region).
    pub fields: BTreeMap<String, String>,
}

/// What `integration_connections.credentials` holds, encrypted with the
/// application key.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum StoredCredentials {
    /// Delegated tokens: the refresh token (the access token is the row's
    /// cached token).
    #[serde(rename = "oauth")]
    OAuth {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
    },
    /// A tenant's admin consent.
    AppOnly { tenant: String },
    /// A service-account key and the user it acts as.
    ServiceAccount {
        key: ServiceAccountKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
    },
    /// An API key.
    ApiKey {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_url: Option<String>,
        #[serde(default)]
        fields: BTreeMap<String, String>,
    },
}

impl StoredCredentials {
    pub(crate) fn kind(&self) -> CredentialKind {
        match self {
            StoredCredentials::OAuth { .. } => CredentialKind::OAuth,
            StoredCredentials::AppOnly { .. } => CredentialKind::AppOnly,
            StoredCredentials::ServiceAccount { .. } => CredentialKind::ServiceAccount,
            StoredCredentials::ApiKey { .. } => CredentialKind::ApiKey,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    pub(crate) const PEM: &str = include_str!("../tests/fixtures/rsa-2048.pem");

    #[test]
    fn owners_serialize_with_type_and_id() {
        let id = Uuid::from_u128(7);
        assert_eq!(
            serde_json::to_value(Owner::Workspace(id)).unwrap(),
            json!({ "type": "workspace", "id": id })
        );
        assert_eq!(
            serde_json::to_value(Owner::App).unwrap(),
            json!({ "type": "app" })
        );
        assert_eq!(Owner::App.columns(), ("app", Uuid::nil()));
        assert_eq!(
            Owner::from_columns("organization", id),
            Owner::Organization(id)
        );
        assert_eq!(Owner::Workspace(id).to_string(), format!("workspace:{id}"));
    }

    #[test]
    fn service_account_keys_are_read_from_googles_json() {
        let key = ServiceAccountKey::from_value(&json!({
            "type": "service_account",
            "project_id": "acme-prod",
            "private_key_id": "4f2a",
            "private_key": PEM,
            "client_email": "navree@acme-prod.iam.gserviceaccount.com",
            "client_id": "1093",
            "token_uri": "https://oauth2.googleapis.com/token"
        }))
        .unwrap();
        assert_eq!(key.client_email, "navree@acme-prod.iam.gserviceaccount.com");
        assert_eq!(key.signing_key().unwrap().kid(), Some("4f2a"));
        let debug = format!("{key:?}");
        assert!(!debug.contains("PRIVATE KEY"), "{debug}");
    }

    #[test]
    fn bad_service_account_keys_are_refused() {
        for bad in [
            json!({ "type": "authorized_user", "client_email": "a@b", "private_key": PEM }),
            json!({ "type": "service_account", "private_key": PEM }),
            json!({ "type": "service_account", "client_email": "a@b", "private_key": "nope" }),
        ] {
            assert!(matches!(
                ServiceAccountKey::from_value(&bad),
                Err(IntegrationsError::InvalidCredentials(_))
            ));
        }
        assert!(ServiceAccountKey::from_json("{").is_err());
        // An EC key is not an RSA key.
        let ec = SigningKey::generate_es256().to_pkcs8_pem().unwrap();
        assert!(
            ServiceAccountKey::from_value(&json!({ "client_email": "a@b", "private_key": ec }))
                .is_err()
        );
    }

    #[test]
    fn stored_credentials_are_tagged_by_kind() {
        let stored = StoredCredentials::AppOnly {
            tenant: "t-1".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(&stored).unwrap(),
            json!({ "kind": "app_only", "tenant": "t-1" })
        );
        assert_eq!(stored.kind(), CredentialKind::AppOnly);
        let oauth: StoredCredentials = serde_json::from_value(json!({ "kind": "oauth" })).unwrap();
        assert_eq!(oauth.kind(), CredentialKind::OAuth);
    }
}
