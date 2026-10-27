//! [`User`]: the CRM's view of navree-auth's `users`.

use chrono::{DateTime, Utc};
use navree::orm::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use super::Role;

/// A user, as the CRM reads them: the columns of navree-auth's `users`
/// table the CRM shows (never the password hash or the two-factor
/// secrets). Accounts are created through `AuthService` (which hashes the
/// password and normalises the email); the CRM renames users and lists
/// them with their roles.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "users", timestamps, subject)]
pub struct User {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// The display name.
    pub name: String,
    /// The email address (lower-cased, unique).
    pub email: String,
    /// When the email address was verified, if it was.
    pub email_verified_at: Option<DateTime<Utc>>,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
    /// When the account last changed.
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl User {
    /// The user's roles everywhere (navree-access's assignments without a
    /// team), through `user_roles`.
    pub fn roles(&self) -> BelongsToMany<User, Role> {
        self.belongs_to_many("user_roles")
            .where_pivot_null("team_id")
    }
}

#[scopes]
impl User {
    /// Users whose name or email contains `term`, ignoring case (non-ASCII
    /// letters too, on every database).
    #[scope]
    pub fn search(query: Builder<Self>, term: &str) -> Builder<Self> {
        query.where_(
            User::NAME
                .contains_ignore_case(term)
                .or(User::EMAIL.contains_ignore_case(term)),
        )
    }
}
