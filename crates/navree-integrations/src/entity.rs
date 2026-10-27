//! The integrations module's table as a SeaORM entity.
//!
//! `integration_connections(id, owner_type, owner_id, provider, scope_set,
//! kind, external_id, display_name, credentials, access_token,
//! token_expires_at, scopes, status, failures, last_error,
//! last_checked_at, connected_by, created_at, updated_at)`: one row per
//! owner, provider and scope set (unique). `owner_type` is `app`,
//! `workspace` or `organization`; `owner_id` is the workspace or
//! organization id, the nil UUID for the app. `credentials` and
//! `access_token` hold navree-crypto ciphertexts, never plain secrets, and
//! are never serialized.

/// `integration_connections`: connections to third-party APIs.
pub mod integration_connection {
    use std::fmt;

    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A connection.
    #[derive(Clone, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "integration_connections")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        #[sea_orm(column_type = "Text")]
        pub credentials: String,
        /// The encrypted cached access token. Never serialized.
        #[serde(skip_serializing)]
        #[sea_orm(column_type = "Text", nullable)]
        pub access_token: Option<String>,
        /// When the cached access token expires.
        pub token_expires_at: Option<DateTimeUtc>,
        /// The granted scopes, space-separated.
        #[sea_orm(column_type = "Text", nullable)]
        pub scopes: Option<String>,
        /// `healthy` or `unhealthy`.
        pub status: String,
        /// Consecutive failed checks.
        pub failures: i32,
        /// The last failure.
        #[sea_orm(column_type = "Text", nullable)]
        pub last_error: Option<String>,
        /// When it was last checked.
        pub last_checked_at: Option<DateTimeUtc>,
        /// The user who connected it.
        pub connected_by: Option<Uuid>,
        /// When it was connected.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
    }

    /// The credentials and token are redacted.
    impl fmt::Debug for Model {
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

    /// No relations are declared (owners are navree-workspace's).
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
