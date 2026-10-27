//! [`IntegrationConnection`]: the module's table as a navree-orm model.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_orm::Model;
use serde::Serialize;
use uuid::Uuid;

/// A row of `integration_connections`: one per owner, provider and scope
/// set (unique).
///
/// `owner_type` is `app`, `workspace` or `organization`; `owner_id` is the
/// workspace or organization id, the nil UUID for the app. `credentials`
/// and `access_token` hold navree-crypto ciphertexts, never plain secrets,
/// and are left out of serialized records and `Debug`.
///
/// Read it freely (`IntegrationConnection::query()`, typed columns such as
/// `IntegrationConnection::STATUS`); change connections through
/// [`Integrations`](crate::Integrations), which encrypts the credentials,
/// emits the events and writes the audit entries.
#[derive(Model, Clone, Default, PartialEq, Eq, Serialize)]
#[model(table = "integration_connections")]
pub struct IntegrationConnection {
    /// The id (a UUID v7, generated on insert).
    pub id: Uuid,
    /// `app`, `workspace` or `organization`.
    pub owner_type: String,
    /// The workspace or organization (the nil UUID for the app).
    pub owner_id: Uuid,
    /// The provider key.
    pub provider: String,
    /// The scope set.
    pub scope_set: String,
    /// `oauth`, `app_only`, `service_account` or `api_key`.
    pub kind: String,
    /// The tenant at the provider (Microsoft tenant id, Slack team id,
    /// Google Workspace domain).
    pub external_id: Option<String>,
    /// The tenant's name, when known.
    pub display_name: Option<String>,
    /// The encrypted credentials. Never serialized.
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub credentials: String,
    /// The encrypted cached access token. Never serialized.
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub access_token: Option<String>,
    /// When the cached access token expires.
    pub token_expires_at: Option<DateTime<Utc>>,
    /// The granted scopes, space-separated.
    pub scopes: Option<String>,
    /// `healthy` or `unhealthy`.
    pub status: String,
    /// Consecutive failed checks.
    pub failures: i32,
    /// The last failure.
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

/// The credentials and token are redacted.
impl fmt::Debug for IntegrationConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntegrationConnection")
            .field("id", &self.id)
            .field("owner_type", &self.owner_type)
            .field("owner_id", &self.owner_id)
            .field("provider", &self.provider)
            .field("scope_set", &self.scope_set)
            .field("kind", &self.kind)
            .field("external_id", &self.external_id)
            .field("status", &self.status)
            .field("has_token", &self.access_token.is_some())
            .field("token_expires_at", &self.token_expires_at)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_stay_out_of_debug_and_json() {
        let row = IntegrationConnection {
            credentials: "nvc1.secret-ciphertext".into(),
            access_token: Some("nvc1.token-ciphertext".into()),
            provider: "slack".into(),
            ..IntegrationConnection::default()
        };
        let debug = format!("{row:?}");
        assert!(!debug.contains("ciphertext"), "{debug}");
        assert!(debug.contains("has_token: true"));
        let json = serde_json::to_value(&row).unwrap();
        assert!(json.get("credentials").is_none());
        assert!(json.get("access_token").is_none());
        assert_eq!(json["provider"], "slack");
        assert_eq!(IntegrationConnection::meta().table(), "integration_connections");
        assert!(
            IntegrationConnection::meta()
                .hidden_names()
                .contains(&"credentials")
        );
    }
}
