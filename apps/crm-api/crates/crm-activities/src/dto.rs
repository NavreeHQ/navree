//! Request bodies and queries of `/api/v1/activities`, with their
//! validation rules.

use chrono::{DateTime, Utc};
use crm_core::{ActivityKind, SubjectType};
use garde::Validate;
use serde::{Deserialize, Deserializer};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::due::Due;

/// The body of `POST /api/v1/activities`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = ActivityInput)]
pub struct CreateActivity {
    /// What the activity records.
    #[garde(skip)]
    pub kind: ActivityKind,
    /// The one-line summary (trimmed).
    #[serde(deserialize_with = "trimmed")]
    #[garde(length(min = 1, max = 255))]
    pub subject: String,
    /// Details.
    #[serde(default)]
    #[garde(length(max = 10000))]
    pub body: Option<String>,
    /// When it is due (RFC 3339; stored in UTC).
    #[serde(default)]
    #[garde(skip)]
    pub due_at: Option<DateTime<Utc>>,
    /// The kind of record it is attached to.
    #[garde(skip)]
    pub subject_type: SubjectType,
    /// The record it is attached to (must exist).
    #[garde(skip)]
    pub subject_id: Uuid,
    /// Who should do it (an existing user).
    #[serde(default)]
    #[garde(skip)]
    pub assignee_id: Option<Uuid>,
}

/// The body of `PATCH /api/v1/activities/{id}`: only the fields present
/// change. `body`, `due_at` and `assignee_id` are cleared with `null`; the
/// other fields cannot be `null`.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = ActivityUpdateInput)]
pub struct UpdateActivity {
    /// What the activity records.
    #[serde(default, deserialize_with = "present")]
    #[schema(nullable = false)]
    #[garde(skip)]
    pub kind: Option<ActivityKind>,
    /// The one-line summary (trimmed).
    #[serde(default, deserialize_with = "present_trimmed")]
    #[schema(nullable = false)]
    #[garde(length(min = 1, max = 255))]
    pub subject: Option<String>,
    /// Details; `null` clears them.
    #[serde(default, deserialize_with = "nullable")]
    #[garde(length(max = 10000))]
    #[schema(value_type = Option<String>)]
    pub body: Option<Option<String>>,
    /// When it is due; `null` clears it.
    #[serde(default, deserialize_with = "nullable")]
    #[garde(skip)]
    #[schema(value_type = Option<DateTime<Utc>>)]
    pub due_at: Option<Option<DateTime<Utc>>>,
    /// The kind of record it is attached to.
    #[serde(default, deserialize_with = "present")]
    #[schema(nullable = false)]
    #[garde(skip)]
    pub subject_type: Option<SubjectType>,
    /// The record it is attached to (must exist).
    #[serde(default, deserialize_with = "present")]
    #[schema(nullable = false)]
    #[garde(skip)]
    pub subject_id: Option<Uuid>,
    /// Who should do it; `null` unassigns it.
    #[serde(default, deserialize_with = "nullable")]
    #[garde(skip)]
    #[schema(value_type = Option<Uuid>)]
    pub assignee_id: Option<Option<Uuid>>,
}

/// A field that may be absent but not `null` (absent is `None`, thanks to
/// `#[serde(default)]`; `null` is a "should not be empty" error).
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// [`present`], trimmed.
fn present_trimmed<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    trimmed(deserializer).map(Some)
}

/// A field that may be absent (`None`), `null` (`Some(None)`) or set.
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// The query of `GET /api/v1/activities` (besides `page` and `per_page`).
#[derive(Debug, Default, Deserialize, Validate)]
pub struct ActivityFilter {
    /// Only activities attached to this kind of record.
    #[garde(skip)]
    pub subject_type: Option<SubjectType>,
    /// Only activities attached to this record.
    #[garde(skip)]
    pub subject_id: Option<Uuid>,
    /// Only activities of this kind.
    #[garde(skip)]
    pub kind: Option<ActivityKind>,
    /// Only activities assigned to this user.
    #[garde(skip)]
    pub assignee_id: Option<Uuid>,
    /// Only activities due in this window of UTC days (spec §9.5); sorts
    /// by due date, earliest first.
    #[garde(skip)]
    pub due: Option<Due>,
    /// Only completed (`true`) or open (`false`) activities; `true` sorts
    /// by completion, most recent first.
    #[garde(skip)]
    pub completed: Option<bool>,
}

/// A string without surrounding whitespace.
fn trimmed<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    String::deserialize(deserializer).map(|value| value.trim().to_owned())
}
