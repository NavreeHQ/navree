//! The `user_profiles` table as an ORM model: what users say about
//! themselves (one row per user, created on the first update).

use chrono::{DateTime, Utc};
use navree::orm::Model;
use serde::Serialize;
use uuid::Uuid;

/// A user's profile. Query it with `Profile::query()`; the ORM sets the
/// timestamps when it creates or saves a record.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "user_profiles", primary_key = "user_id", timestamps)]
pub struct Profile {
    /// The user's id (`users.id`).
    pub user_id: Uuid,
    /// A short biography.
    pub bio: Option<String>,
    /// An IANA time zone, e.g. `Asia/Kolkata`.
    pub timezone: String,
    /// When the profile was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}
