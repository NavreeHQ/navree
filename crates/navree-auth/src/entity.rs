//! The auth module's tables as SeaORM entities.
//!
//! - `users(id, name, email UNIQUE, password_hash, email_verified_at,
//!   two_factor_secret, two_factor_recovery_codes, two_factor_confirmed_at,
//!   created_at, updated_at)`
//! - `refresh_tokens(id, user_id, family_id, token_hash UNIQUE, expires_at,
//!   revoked_at, replaced_by, ip, user_agent, created_at, authenticated_at,
//!   impersonator_id, audience)`
//! - `password_reset_tokens(email PK, token_hash, created_at)`
//! - `login_tokens(id, user_id, kind, token_hash, attempts, expires_at,
//!   used_at, created_at)`: magic links and emailed sign-in codes
//! - `passkeys(id, user_id, credential_id, credential_hash UNIQUE,
//!   public_key, algorithm, sign_count, transports, name, created_at,
//!   last_used_at)`

/// `users`: the accounts. Emails are stored trimmed and lower-cased.
///
/// The password hash and the two-factor columns are never serialized (the
/// fields are skipped by `Serialize`, so a `Model` can be returned as JSON
/// and recorded in audit trails) and never appear in `Debug` output.
pub mod user {
    use std::fmt;

    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A user account.
    #[derive(Clone, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The display name.
        pub name: String,
        /// The unique, lower-cased email address.
        #[sea_orm(unique)]
        pub email: String,
        /// The Argon2id PHC string. Never serialized.
        #[serde(skip_serializing)]
        pub password_hash: String,
        /// When the email address was verified, if it was.
        pub email_verified_at: Option<DateTimeUtc>,
        /// The encrypted TOTP secret. Never serialized.
        #[serde(skip_serializing)]
        pub two_factor_secret: Option<String>,
        /// The encrypted recovery codes. Never serialized.
        #[serde(skip_serializing)]
        pub two_factor_recovery_codes: Option<String>,
        /// When two-factor authentication was confirmed. Never serialized.
        #[serde(skip_serializing)]
        pub two_factor_confirmed_at: Option<DateTimeUtc>,
        /// When the account was created.
        pub created_at: DateTimeUtc,
        /// When the account last changed.
        pub updated_at: DateTimeUtc,
    }

    /// The password hash and two-factor data are redacted.
    impl fmt::Debug for Model {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("User")
                .field("id", &self.id)
                .field("name", &self.name)
                .field("email", &self.email)
                .field("email_verified_at", &self.email_verified_at)
                .field(
                    "two_factor_enabled",
                    &self.two_factor_confirmed_at.is_some(),
                )
                .field("created_at", &self.created_at)
                .field("updated_at", &self.updated_at)
                .finish_non_exhaustive()
        }
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `refresh_tokens`: opaque refresh tokens, stored as SHA-256 digests.
///
/// Every login starts a family (a session); every refresh revokes the
/// presented token (setting `replaced_by`) and issues the next one in the
/// same family, which keeps the family's `authenticated_at`,
/// `impersonator_id` and `audience`. Presenting a revoked token revokes the
/// whole family (reuse detection).
pub mod refresh_token {
    use sea_orm::entity::prelude::*;

    /// A refresh token.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "refresh_tokens")]
    pub struct Model {
        /// The id: the part of the token before the `.`.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The login session the token belongs to.
        pub family_id: Uuid,
        /// SHA-256 of the whole token, as lower-case hex.
        #[sea_orm(unique)]
        pub token_hash: String,
        /// When the token expires.
        pub expires_at: DateTimeUtc,
        /// When the token was used, revoked or its family was revoked.
        pub revoked_at: Option<DateTimeUtc>,
        /// The token issued when this one was used.
        pub replaced_by: Option<Uuid>,
        /// The client IP address the token was issued to.
        pub ip: Option<String>,
        /// The client user agent the token was issued to (truncated).
        pub user_agent: Option<String>,
        /// When the token was issued.
        pub created_at: DateTimeUtc,
        /// When the user last authenticated in this session (signing in or
        /// `POST /auth/confirm`): the `auth_time` of its access tokens.
        pub authenticated_at: Option<DateTimeUtc>,
        /// For an impersonation session: the impersonating user.
        pub impersonator_id: Option<Uuid>,
        /// `"platform"` for a platform session; `None` for the app.
        pub audience: Option<String>,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `password_reset_tokens`: one pending reset per email address, stored as
/// a SHA-256 digest (used by the password reset flow).
pub mod password_reset_token {
    use sea_orm::entity::prelude::*;

    /// A pending password reset.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "password_reset_tokens")]
    pub struct Model {
        /// The lower-cased email address.
        #[sea_orm(primary_key, auto_increment = false)]
        pub email: String,
        /// SHA-256 of the reset token, as lower-case hex.
        pub token_hash: String,
        /// When the reset was requested.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `login_tokens`: pending passwordless sign-ins, stored as digests: magic
/// links (`kind = "magic_link"`, the SHA-256 of the token) and emailed
/// sign-in codes (`kind = "otp"`, an HMAC of the code). Each works once,
/// until `expires_at`; a code is burned after too many wrong guesses.
pub mod login_token {
    use sea_orm::entity::prelude::*;

    /// A pending passwordless sign-in.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "login_tokens")]
    pub struct Model {
        /// The id (the part of a magic link token before the `.`).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// `magic_link` or `otp`.
        pub kind: String,
        /// The digest of the token or code, lower-case hex.
        pub token_hash: String,
        /// Wrong guesses so far (codes).
        pub attempts: i32,
        /// When the token expires.
        pub expires_at: DateTimeUtc,
        /// When the token was used (or burned).
        pub used_at: Option<DateTimeUtc>,
        /// When the token was issued.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `passkeys`: WebAuthn credentials (passkeys) registered by users, with
/// the public key that verifies their sign-ins.
pub mod passkey {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A registered passkey. The public key is never serialized.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "passkeys")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The credential id, base64url without padding.
        pub credential_id: String,
        /// SHA-256 of the raw credential id, lower-case hex (the lookup
        /// key; unique).
        #[sea_orm(unique)]
        #[serde(skip_serializing)]
        pub credential_hash: String,
        /// The public key: the uncompressed P-256 point (ES256) or the 32
        /// Ed25519 bytes (EdDSA), base64url without padding.
        #[serde(skip_serializing)]
        pub public_key: String,
        /// The COSE algorithm: `-7` (ES256) or `-8` (EdDSA).
        pub algorithm: i32,
        /// The authenticator's signature counter at the last sign-in.
        pub sign_count: i64,
        /// The transports the browser reported (`internal,hybrid`), if any.
        pub transports: Option<String>,
        /// A name the user gave it.
        pub name: String,
        /// When it was registered.
        pub created_at: DateTimeUtc,
        /// When it last signed the user in.
        pub last_used_at: Option<DateTimeUtc>,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// A user account (`users` row).
pub type User = user::Model;

/// A registered passkey (`passkeys` row).
pub type Passkey = passkey::Model;
