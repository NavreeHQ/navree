//! The OAuth server's tables as SeaORM entities. Scopes, grants and
//! redirect URIs are stored space-separated; tokens, codes and secrets as
//! SHA-256 digests.
//!
//! - `oauth_clients(id, name, secret_hash, redirect_uris, scopes, grants,
//!   owner_id, trusted, introspect, workspace_id, logo_url, website,
//!   created_at, updated_at, revoked_at)`
//! - `oauth_authorizations(id, user_id, client_id, scopes, created_at,
//!   updated_at)`: remembered consent, one row per user and client
//! - `oauth_authorization_requests(id, client_id, redirect_uri, scopes,
//!   state, code_challenge, nonce, user_id, status, expires_at,
//!   created_at)`
//! - `oauth_codes(id, code_hash, client_id, user_id, redirect_uri, scopes,
//!   code_challenge, nonce, expires_at, used_at, created_at)`
//! - `oauth_access_tokens(id, token_hash, client_id, user_id, scopes,
//!   grant, code_id, family_id, workspace_id, expires_at, revoked_at,
//!   created_at)`
//! - `oauth_refresh_tokens(id, token_hash, client_id, user_id, scopes,
//!   family_id, code_id, access_token_id, workspace_id, expires_at,
//!   used_at, revoked_at, created_at)`
//! - `oauth_device_codes(id, device_code_hash, user_code, client_id,
//!   scopes, user_id, status, interval_secs, last_polled_at, expires_at,
//!   used_at, created_at)`
//! - `personal_access_tokens(id, user_id, name, token_hash, scopes,
//!   expires_at, last_used_at, revoked_at, created_at)`

use crate::scope::parse_scope;

/// `oauth_clients`: the apps that ask for tokens.
pub mod client {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A registered client. `secret_hash` is `None` for public clients and
    /// never serialized.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "oauth_clients")]
    pub struct Model {
        /// The client id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The name shown on the consent page.
        pub name: String,
        /// The SHA-256 digest of the secret; `None` for public clients.
        #[serde(skip_serializing)]
        pub secret_hash: Option<String>,
        /// The allowed redirect URIs, space-separated.
        pub redirect_uris: String,
        /// The scopes the client may ask for, space-separated.
        pub scopes: String,
        /// The grants it may use, space-separated.
        pub grants: String,
        /// The user a client credentials client acts as, or the developer
        /// who registered it.
        pub owner_id: Option<Uuid>,
        /// A first-party client: no consent screen.
        pub trusted: bool,
        /// Whether it may call the introspection endpoint.
        pub introspect: bool,
        /// The workspace its tokens are limited to.
        pub workspace_id: Option<Uuid>,
        /// A logo for the consent page.
        pub logo_url: Option<String>,
        /// The client's website.
        pub website: Option<String>,
        /// When it was created.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
        /// When it was revoked.
        pub revoked_at: Option<DateTimeUtc>,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `oauth_authorizations`: the scopes a user approved for a client.
pub mod authorization {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A remembered approval.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "oauth_authorizations")]
    pub struct Model {
        /// The id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The client.
        pub client_id: Uuid,
        /// The approved scopes, space-separated.
        pub scopes: String,
        /// When the user first approved the client.
        pub created_at: DateTimeUtc,
        /// When the approval last changed.
        pub updated_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `oauth_authorization_requests`: `GET /oauth/authorize` requests waiting
/// for the user's decision.
pub mod authorization_request {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A pending (or decided) authorization request.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "oauth_authorization_requests")]
    pub struct Model {
        /// The id (random, UUID v4): the `request` parameter of the
        /// consent page.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The client.
        pub client_id: Uuid,
        /// Where the browser goes back to.
        pub redirect_uri: String,
        /// The requested scopes, space-separated.
        pub scopes: String,
        /// The client's `state`.
        pub state: Option<String>,
        /// The PKCE S256 challenge.
        pub code_challenge: String,
        /// The OpenID Connect `nonce`.
        pub nonce: Option<String>,
        /// Who decided.
        pub user_id: Option<Uuid>,
        /// `pending`, `approved` or `denied`.
        pub status: String,
        /// When it expires.
        pub expires_at: DateTimeUtc,
        /// When it was made.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `oauth_codes`: authorization codes (single use).
pub mod code {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// An authorization code.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "oauth_codes")]
    pub struct Model {
        /// The id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The SHA-256 digest of the code.
        #[serde(skip_serializing)]
        #[sea_orm(unique)]
        pub code_hash: String,
        /// The client.
        pub client_id: Uuid,
        /// The user who approved.
        pub user_id: Uuid,
        /// The redirect URI of the request.
        pub redirect_uri: String,
        /// The approved scopes, space-separated.
        pub scopes: String,
        /// The PKCE S256 challenge.
        pub code_challenge: String,
        /// The OpenID Connect `nonce`.
        pub nonce: Option<String>,
        /// When it expires.
        pub expires_at: DateTimeUtc,
        /// When it was exchanged.
        pub used_at: Option<DateTimeUtc>,
        /// When it was issued.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `oauth_access_tokens`: opaque `nva_` access tokens.
pub mod access_token {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// An access token.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "oauth_access_tokens")]
    pub struct Model {
        /// The id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The SHA-256 digest of the token.
        #[serde(skip_serializing)]
        #[sea_orm(unique)]
        pub token_hash: String,
        /// The client.
        pub client_id: Uuid,
        /// The user the token acts as.
        pub user_id: Uuid,
        /// The granted scopes, space-separated.
        pub scopes: String,
        /// How it was obtained (`authorization_code`, `client_credentials`,
        /// `device_code`, `refresh_token`).
        pub grant: String,
        /// The authorization code it came from.
        pub code_id: Option<Uuid>,
        /// The refresh token family it belongs to.
        pub family_id: Option<Uuid>,
        /// The workspace it is limited to.
        pub workspace_id: Option<Uuid>,
        /// When it expires.
        pub expires_at: DateTimeUtc,
        /// When it was revoked.
        pub revoked_at: Option<DateTimeUtc>,
        /// When it was issued.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `oauth_refresh_tokens`: opaque `nvr_` refresh tokens, single use and
/// rotated; a family shares one grant.
pub mod refresh_token {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A refresh token.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "oauth_refresh_tokens")]
    pub struct Model {
        /// The id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The SHA-256 digest of the token.
        #[serde(skip_serializing)]
        #[sea_orm(unique)]
        pub token_hash: String,
        /// The client.
        pub client_id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The scopes of the original grant, space-separated.
        pub scopes: String,
        /// The family (every rotation of one grant).
        pub family_id: Uuid,
        /// The authorization code the family came from.
        pub code_id: Option<Uuid>,
        /// The access token issued with it.
        pub access_token_id: Option<Uuid>,
        /// The workspace its tokens are limited to.
        pub workspace_id: Option<Uuid>,
        /// When it expires.
        pub expires_at: DateTimeUtc,
        /// When it was rotated.
        pub used_at: Option<DateTimeUtc>,
        /// When it was revoked.
        pub revoked_at: Option<DateTimeUtc>,
        /// When it was issued.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `oauth_device_codes`: device authorization requests.
pub mod device_code {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A device authorization.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "oauth_device_codes")]
    pub struct Model {
        /// The id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The SHA-256 digest of the device code.
        #[serde(skip_serializing)]
        #[sea_orm(unique)]
        pub device_code_hash: String,
        /// The user code, normalized (`WDJBMJHT`).
        #[sea_orm(unique)]
        pub user_code: String,
        /// The client.
        pub client_id: Uuid,
        /// The requested scopes, space-separated.
        pub scopes: String,
        /// Who decided.
        pub user_id: Option<Uuid>,
        /// `pending`, `approved` or `denied`.
        pub status: String,
        /// The polling interval, in seconds.
        pub interval_secs: i64,
        /// When the device last polled.
        pub last_polled_at: Option<DateTimeUtc>,
        /// When it expires.
        pub expires_at: DateTimeUtc,
        /// When the device received its tokens.
        pub used_at: Option<DateTimeUtc>,
        /// When it was made.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `personal_access_tokens`: `nvp_` tokens users create for scripts.
pub mod personal_access_token {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A personal access token (never with its secret).
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "personal_access_tokens")]
    pub struct Model {
        /// The id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The owner.
        pub user_id: Uuid,
        /// The name the user gave it.
        pub name: String,
        /// The SHA-256 digest of the token.
        #[serde(skip_serializing)]
        #[sea_orm(unique)]
        pub token_hash: String,
        /// Its scopes, space-separated.
        pub scopes: String,
        /// When it expires; `None` never.
        pub expires_at: Option<DateTimeUtc>,
        /// When it was last used (updated at most once a minute).
        pub last_used_at: Option<DateTimeUtc>,
        /// When it was revoked.
        pub revoked_at: Option<DateTimeUtc>,
        /// When it was created.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// A registered client.
pub type Client = client::Model;
/// A personal access token.
pub type PersonalAccessToken = personal_access_token::Model;
/// A remembered approval.
pub type Authorization = authorization::Model;

impl client::Model {
    /// Whether this is a public client (no secret; PKCE only).
    pub fn is_public(&self) -> bool {
        self.secret_hash.is_none()
    }

    /// Whether the client was revoked.
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

    /// The allowed redirect URIs.
    pub fn redirect_uri_list(&self) -> Vec<String> {
        split(&self.redirect_uris)
    }

    /// The scopes it may ask for.
    pub fn scope_list(&self) -> Vec<String> {
        parse_scope(&self.scopes)
    }

    /// The grants it may use (`authorization_code`, `client_credentials`,
    /// `device_code`).
    pub fn grant_list(&self) -> Vec<String> {
        split(&self.grants)
    }

    /// Whether it may use `grant`.
    pub fn allows(&self, grant: &str) -> bool {
        self.grant_list().iter().any(|g| g == grant)
    }
}

impl personal_access_token::Model {
    /// Its scopes.
    pub fn scope_list(&self) -> Vec<String> {
        parse_scope(&self.scopes)
    }
}

impl authorization::Model {
    /// The approved scopes.
    pub fn scope_list(&self) -> Vec<String> {
        parse_scope(&self.scopes)
    }
}

fn split(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_owned).collect()
}
