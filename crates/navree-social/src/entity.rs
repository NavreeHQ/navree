//! The social module's table as a SeaORM entity.
//!
//! `social_accounts(id, user_id → users ON DELETE CASCADE, provider,
//! provider_user_id, email, name, avatar_url, nickname, access_token,
//! refresh_token, token_expires_at, scopes, created_at, updated_at)`, one
//! row per linked provider account: unique per (provider,
//! provider_user_id) and per (user_id, provider). The token columns hold
//! navree-crypto ciphertexts (`social.store_tokens`), never plain tokens.

/// `social_accounts`: provider accounts linked to users.
pub mod social_account {
    use std::fmt;

    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A linked provider account.
    #[derive(Clone, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "social_accounts")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The user the account signs in.
        pub user_id: Uuid,
        /// The provider key (`github`, `okta`).
        pub provider: String,
        /// The provider's user id.
        pub provider_user_id: String,
        /// The email address the provider reported.
        pub email: Option<String>,
        /// The name the provider reported.
        pub name: Option<String>,
        /// The avatar URL the provider reported.
        pub avatar_url: Option<String>,
        /// The handle the provider reported.
        pub nickname: Option<String>,
        /// The encrypted access token. Never serialized.
        #[serde(skip_serializing)]
        pub access_token: Option<String>,
        /// The encrypted refresh token. Never serialized.
        #[serde(skip_serializing)]
        pub refresh_token: Option<String>,
        /// When the access token expires.
        pub token_expires_at: Option<DateTimeUtc>,
        /// The granted scopes, space-separated.
        pub scopes: Option<String>,
        /// When the account was linked.
        pub created_at: DateTimeUtc,
        /// When the account last changed.
        pub updated_at: DateTimeUtc,
    }

    /// The tokens are redacted.
    impl fmt::Debug for Model {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SocialAccount")
                .field("id", &self.id)
                .field("user_id", &self.user_id)
                .field("provider", &self.provider)
                .field("provider_user_id", &self.provider_user_id)
                .field("email", &self.email)
                .field("name", &self.name)
                .field("has_tokens", &self.access_token.is_some())
                .field("token_expires_at", &self.token_expires_at)
                .field("created_at", &self.created_at)
                .finish_non_exhaustive()
        }
    }

    /// No relations are declared (the user is navree-auth's).
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
