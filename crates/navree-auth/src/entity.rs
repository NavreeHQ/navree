//! The auth module's tables, as navree-orm models.
//!
//! Each table's `Model` is a navree-orm model: rows are read and written
//! with the model API and its builders (`User::query()`, typed columns such
//! as `User::EMAIL`, `RefreshToken::insert`). Secrets (password hashes,
//! token digests, two-factor data, passkeys' keys) are never serialized, as
//! records or as plain models, and so never reach a change event.
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

pub mod user;

/// `refresh_tokens`: opaque refresh tokens, stored as SHA-256 digests.
///
/// Every login starts a family (a session); every refresh revokes the
/// presented token (setting `replaced_by`) and issues the next one in the
/// same family, which keeps the family's `authenticated_at`,
/// `impersonator_id` and `audience`. Presenting a revoked token revokes the
/// whole family (reuse detection).
pub mod refresh_token {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A refresh token. Its digest is never serialized.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "refresh_tokens")]
    pub struct Model {
        /// The id: the part of the token before the `.`.
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The login session the token belongs to.
        pub family_id: Uuid,
        /// SHA-256 of the whole token, as lower-case hex. Never serialized.
        #[serde(skip_serializing)]
        pub token_hash: String,
        /// When the token expires.
        pub expires_at: DateTime<Utc>,
        /// When the token was used, revoked or its family was revoked.
        pub revoked_at: Option<DateTime<Utc>>,
        /// The token issued when this one was used.
        pub replaced_by: Option<Uuid>,
        /// The client IP address the token was issued to.
        pub ip: Option<String>,
        /// The client user agent the token was issued to (truncated).
        pub user_agent: Option<String>,
        /// When the token was issued.
        pub created_at: DateTime<Utc>,
        /// When the user last authenticated in this session (signing in or
        /// `POST /auth/confirm`): the `auth_time` of its access tokens.
        pub authenticated_at: Option<DateTime<Utc>>,
        /// For an impersonation session: the impersonating user.
        pub impersonator_id: Option<Uuid>,
        /// `"platform"` for a platform session; `None` for the app.
        pub audience: Option<String>,
    }
}

/// `password_reset_tokens`: one pending reset per email address, stored as
/// a SHA-256 digest (used by the password reset flow).
pub mod password_reset_token {
    use chrono::{DateTime, Utc};
    use serde::Serialize;

    /// A pending password reset, keyed by email address. Its digest is
    /// never serialized.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "password_reset_tokens", primary_key = "email")]
    pub struct Model {
        /// The lower-cased email address.
        pub email: String,
        /// SHA-256 of the reset token, as lower-case hex. Never serialized.
        #[serde(skip_serializing)]
        pub token_hash: String,
        /// When the reset was requested.
        pub created_at: DateTime<Utc>,
    }
}

/// `login_tokens`: pending passwordless sign-ins, stored as digests: magic
/// links (`kind = "magic_link"`, the SHA-256 of the token) and emailed
/// sign-in codes (`kind = "otp"`, an HMAC of the code). Each works once,
/// until `expires_at`; a code is burned after too many wrong guesses.
pub mod login_token {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A pending passwordless sign-in. Its digest is never serialized.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "login_tokens")]
    pub struct Model {
        /// The id (the part of a magic link token before the `.`).
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// `magic_link` or `otp`.
        pub kind: String,
        /// The digest of the token or code, lower-case hex. Never
        /// serialized.
        #[serde(skip_serializing)]
        pub token_hash: String,
        /// Wrong guesses so far (codes).
        pub attempts: i32,
        /// When the token expires.
        pub expires_at: DateTime<Utc>,
        /// When the token was used (or burned).
        pub used_at: Option<DateTime<Utc>>,
        /// When the token was issued.
        pub created_at: DateTime<Utc>,
    }
}

/// `passkeys`: WebAuthn credentials (passkeys) registered by users, with
/// the public key that verifies their sign-ins.
pub mod passkey {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A registered passkey. The public key is never serialized.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "passkeys")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The credential id, base64url without padding.
        pub credential_id: String,
        /// SHA-256 of the raw credential id, lower-case hex (the lookup
        /// key; unique).
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
        pub created_at: DateTime<Utc>,
        /// When it last signed the user in.
        pub last_used_at: Option<DateTime<Utc>>,
    }
}

/// A user account (`users` row).
pub type User = user::Model;

/// A registered passkey (`passkeys` row).
pub type Passkey = passkey::Model;
