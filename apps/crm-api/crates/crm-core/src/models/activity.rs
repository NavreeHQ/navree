//! [`Activity`]: notes, calls, emails, meetings and tasks attached to a
//! company, contact or deal (`activities`, the CRM's timeline).

use chrono::{DateTime, Utc};
use navree::orm::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use super::{Company, Contact, Deal, User};
use crate::enums::{ActivityKind, SubjectType};
use crate::factories::ActivityFactory;

/// An activity. The record it is attached to is polymorphic:
/// `subject_type` holds its model's morph name (`company`, `contact` or
/// `deal`, as [`SubjectType`] spells them) and `subject_id` its key, with
/// no foreign key. Tasks have a due date and can be completed.
///
/// The `activities` table is the CRM's timeline, written once and read by
/// time, so the migrations declare it a time series: a TimescaleDB
/// hypertable on `created_at` (a regular table with an index on
/// `created_at` elsewhere). Its key on TimescaleDB is `(id, created_at)`;
/// ids stay unique (UUID v7), and records are found by `id` as usual.
///
/// `Activity::SUBJECT` is the constant of the `subject` column (the
/// summary); the access subject's name is `<Activity as Subject>::SUBJECT`
/// (`"Activity"`, also [`subjects::ACTIVITY`](crate::subjects::ACTIVITY)).
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(
    table = "activities",
    timestamps,
    subject,
    factory = ActivityFactory
)]
pub struct Activity {
    /// A UUID v7, generated when the activity is created.
    pub id: Uuid,
    /// What it records.
    #[model(cast = "string_enum")]
    pub kind: ActivityKind,
    /// The one-line summary (required).
    pub subject: String,
    /// Details.
    pub body: Option<String>,
    /// When it is due (tasks).
    pub due_at: Option<DateTime<Utc>>,
    /// When it was completed (tasks).
    pub completed_at: Option<DateTime<Utc>>,
    /// The kind of record it is attached to.
    #[model(cast = "string_enum")]
    pub subject_type: SubjectType,
    /// The record it is attached to.
    pub subject_id: Uuid,
    /// The user who created it.
    pub owner_id: Uuid,
    /// The user who should do it (tasks).
    pub assignee_id: Option<Uuid>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl Activity {
    /// The record it is attached to: the company, contact or deal named by
    /// `subject_type` and `subject_id`, deleted companies and contacts
    /// included (they keep their names); `None` once a deal is gone.
    pub fn subject_record(&self) -> MorphTo<Activity> {
        self.morph_to("subject")
            .targets::<(Company, Contact, Deal)>()
            .with_trashed()
    }

    /// The user who created it.
    pub fn owner(&self) -> BelongsTo<Activity, User> {
        self.belongs_to(Activity::OWNER_ID)
    }

    /// The user who should do it.
    pub fn assignee(&self) -> BelongsTo<Activity, User> {
        self.belongs_to(Activity::ASSIGNEE_ID)
    }
}

#[scopes]
impl Activity {
    /// Tasks not completed yet.
    #[scope]
    pub fn open_tasks(query: Builder<Self>) -> Builder<Self> {
        query
            .where_(Activity::KIND.eq(ActivityKind::Task))
            .where_null(Activity::COMPLETED_AT)
    }

    /// The activities that are `user`'s to do: assigned to them, or owned
    /// by them when nobody is assigned.
    #[scope]
    pub fn for_user(query: Builder<Self>, user: Uuid) -> Builder<Self> {
        query.where_(
            Activity::ASSIGNEE_ID.eq(user).or(Activity::ASSIGNEE_ID
                .is_null()
                .and(Activity::OWNER_ID.eq(user))),
        )
    }

    /// The activities attached to the record `subject_type` / `id`.
    #[scope]
    pub fn attached_to(query: Builder<Self>, subject_type: SubjectType, id: Uuid) -> Builder<Self> {
        query
            .where_(Activity::SUBJECT_TYPE.eq(subject_type))
            .where_(Activity::SUBJECT_ID.eq(id))
    }

    /// Newest first (by id among activities created at the same moment).
    #[scope]
    pub fn newest_first(query: Builder<Self>) -> Builder<Self> {
        query
            .order_by_desc(Activity::CREATED_AT)
            .order_by_desc(Activity::ID)
    }
}
