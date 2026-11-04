//! Request bodies and queries of `/api/v1/activities`, with their
//! validation rules.

use chrono::{DateTime, Utc};
use crm_core::models::Activity;
use crm_core::{ActivityKind, SubjectType};
use navree::openapi::ToSchema;
use navree::orm::list::{Field, ListResource, ListSpec, Op};
use navree::validation::Validate;
use serde::{Deserialize, Deserializer};
use uuid::Uuid;

use crate::due::Due;

/// The body of `POST /api/v1/activities`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = ActivityInput)]
pub struct CreateActivity {
    /// What the activity records.
    #[validate(skip)]
    pub kind: ActivityKind,
    /// The one-line summary (trimmed).
    #[serde(deserialize_with = "trimmed")]
    #[validate(length(min = 1, max = 255))]
    pub subject: String,
    /// Details.
    #[serde(default)]
    #[validate(length(max = 10000))]
    pub body: Option<String>,
    /// When it is due (RFC 3339; stored in UTC).
    #[serde(default)]
    #[validate(skip)]
    pub due_at: Option<DateTime<Utc>>,
    /// The kind of record it is attached to.
    #[validate(skip)]
    pub subject_type: SubjectType,
    /// The record it is attached to (must exist).
    #[validate(skip)]
    pub subject_id: Uuid,
    /// Who should do it (an existing user).
    #[serde(default)]
    #[validate(skip)]
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
    #[validate(skip)]
    pub kind: Option<ActivityKind>,
    /// The one-line summary (trimmed).
    #[serde(default, deserialize_with = "present_trimmed")]
    #[schema(nullable = false)]
    #[validate(length(min = 1, max = 255))]
    pub subject: Option<String>,
    /// Details; `null` clears them.
    #[serde(default, deserialize_with = "nullable")]
    #[validate(length(max = 10000))]
    #[schema(value_type = Option<String>)]
    pub body: Option<Option<String>>,
    /// When it is due; `null` clears it.
    #[serde(default, deserialize_with = "nullable")]
    #[validate(skip)]
    #[schema(value_type = Option<DateTime<Utc>>)]
    pub due_at: Option<Option<DateTime<Utc>>>,
    /// The kind of record it is attached to.
    #[serde(default, deserialize_with = "present")]
    #[schema(nullable = false)]
    #[validate(skip)]
    pub subject_type: Option<SubjectType>,
    /// The record it is attached to (must exist).
    #[serde(default, deserialize_with = "present")]
    #[schema(nullable = false)]
    #[validate(skip)]
    pub subject_id: Option<Uuid>,
    /// Who should do it; `null` unassigns it.
    #[serde(default, deserialize_with = "nullable")]
    #[validate(skip)]
    #[schema(value_type = Option<Uuid>)]
    pub assignee_id: Option<Option<Uuid>>,
}

impl CreateActivity {
    /// The new activity, owned by `owner_id`; a blank body is none.
    pub fn into_activity(self, owner_id: Uuid) -> Activity {
        Activity {
            kind: self.kind,
            subject: self.subject,
            body: non_blank(self.body),
            due_at: self.due_at,
            subject_type: self.subject_type,
            subject_id: self.subject_id,
            owner_id,
            assignee_id: self.assignee_id,
            ..Activity::default()
        }
    }
}

impl UpdateActivity {
    /// Changes the fields sent; a blank body clears it.
    pub fn apply_to(self, activity: &mut Activity) {
        if let Some(kind) = self.kind {
            activity.kind = kind;
        }
        if let Some(subject) = self.subject {
            activity.subject = subject;
        }
        if let Some(body) = self.body {
            activity.body = non_blank(body);
        }
        if let Some(due_at) = self.due_at {
            activity.due_at = due_at;
        }
        if let Some(subject_type) = self.subject_type {
            activity.subject_type = subject_type;
        }
        if let Some(subject_id) = self.subject_id {
            activity.subject_id = subject_id;
        }
        if let Some(assignee) = self.assignee_id {
            activity.assignee_id = assignee;
        }
    }
}

/// `None` for a missing or blank text.
fn non_blank(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
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

/// The strings of [`ActivityKind`] (`filter[kind]` takes these).
const KINDS: &[&str] = &["note", "call", "email", "meeting", "task"];

/// The strings of [`SubjectType`] (`filter[subject_type]` takes these).
const SUBJECT_TYPES: &[&str] = &["company", "contact", "deal"];

/// The list query of `GET /api/v1/activities`, in the list-query grammar:
///
/// - `filter[subject_type]` and `filter[subject_id]`: the record an
///   activity is attached to.
/// - `filter[kind]`: one kind, or several with `filter[kind][in]=call,email`.
/// - `filter[assignee_id]`: the user it is assigned to.
/// - `filter[due]`: `overdue`, `today` or `upcoming` (UTC days); sorts by due
///   date, earliest first.
/// - `filter[completed]`: `true` for completed activities, most recently
///   completed first, or `false` for open ones.
/// - `page` and `per_page` (default 15, at most 100).
///
/// Without `filter[due]` or `filter[completed]=true`, the newest come first.
pub struct ActivityList;

impl ListResource for ActivityList {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::one_of("subject_type", SUBJECT_TYPES),
        Field::uuid("subject_id"),
        Field::one_of("kind", KINDS),
        Field::uuid("assignee_id"),
        // A window of days and a state, worked out by the service, which
        // also picks the order they imply.
        Field::one_of("due", Due::WORDS).ops(&[Op::Eq]).by_app(),
        Field::boolean("completed").ops(&[Op::Eq]).by_app(),
    ]);
}

/// A string without surrounding whitespace.
pub(crate) fn trimmed<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    String::deserialize(deserializer).map(|value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use navree::orm::list::Kind;

    use super::*;

    #[test]
    fn the_word_filters_take_exactly_their_enums() {
        let kinds: Vec<&str> = ActivityKind::ALL.iter().map(|kind| kind.as_str()).collect();
        let subjects: Vec<&str> = SubjectType::ALL.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(KINDS, kinds);
        assert_eq!(SUBJECT_TYPES, subjects);
        for (name, words) in [
            ("kind", KINDS),
            ("subject_type", SUBJECT_TYPES),
            ("due", Due::WORDS),
        ] {
            let field = ActivityList::SPEC.field(name).expect(name);
            assert_eq!(field.kind, Kind::OneOf(words), "{name}");
        }
    }

    #[test]
    fn due_and_completed_are_checked_but_left_to_the_service() {
        for name in ["due", "completed"] {
            let field = ActivityList::SPEC.field(name).expect(name);
            assert!(field.by_app, "{name}");
            assert_eq!(field.allowed_ops(), [Op::Eq], "{name}");
        }
        let list = ActivityList::SPEC
            .parse("filter[due]=today&filter[completed]=false&filter[kind][in]=call,task")
            .expect("a valid query");
        assert_eq!(list.eq("due"), Some("today"));
        assert_eq!(list.eq("completed"), Some("false"));
        for query in [
            "filter[due]=later",
            "filter[due][in]=today,overdue",
            "filter[completed]=maybe",
            "filter[completed][null]=true",
            "sort=due_at",
            "search=call",
        ] {
            assert!(ActivityList::SPEC.parse(query).is_err(), "{query}");
        }
    }
}
