//! The API's activity shape, built from a record with
//! its relations: the owner and assignee embeds and the label of the
//! record it is attached to, each loaded with one query per relation (and
//! per kind of record) for a whole page.

use chrono::{DateTime, Utc};
use crm_core::models::{Activity, Company, Contact, Deal};
use crm_core::{ActivityKind, OwnerRef, SubjectType};
use navree::api::ApiResource;
use navree::openapi::ToSchema;
use navree::orm::Record;
use navree::orm::attributes::Resource;
use serde::Serialize;
use uuid::Uuid;

/// The subject type of activities (`__type`).
pub const ACTIVITY_TYPE: &str = crm_core::subjects::ACTIVITY;

/// What `subject_label` says when the record an activity is attached to is
/// gone.
pub const MISSING_SUBJECT_LABEL: &str = "Deleted record";

/// The schema of `__type`.
fn activity_type() -> navree::openapi::Schema {
    crm_core::subjects::type_schema(ACTIVITY_TYPE)
}

/// An activity as the API returns it:
/// `{__type: "Activity", id, kind, subject, body, due_at, completed_at,
/// subject_type, subject_id, subject_label, owner_id, owner, assignee_id,
/// assignee, created_at}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = Activity)]
pub struct ActivityResource {
    /// Always `"Activity"` (the CASL subject type).
    #[serde(rename = "__type")]
    #[schema(schema_with = activity_type)]
    pub resource_type: &'static str,
    /// The id.
    pub id: Uuid,
    /// What it records.
    pub kind: ActivityKind,
    /// The one-line summary.
    pub subject: String,
    /// Details.
    #[schema(required = true)]
    pub body: Option<String>,
    /// When it is due (tasks), UTC.
    #[schema(required = true)]
    pub due_at: Option<DateTime<Utc>>,
    /// When it was completed (tasks), UTC.
    #[schema(required = true)]
    pub completed_at: Option<DateTime<Utc>>,
    /// The kind of record it is attached to.
    pub subject_type: SubjectType,
    /// The record it is attached to.
    pub subject_id: Uuid,
    /// That record's name: the company's name, the contact's full name or
    /// the deal's title.
    pub subject_label: String,
    /// The user who created it.
    pub owner_id: Uuid,
    /// That user.
    pub owner: OwnerRef,
    /// The user who should do it.
    #[schema(required = true)]
    pub assignee_id: Option<Uuid>,
    /// That user.
    #[schema(required = true)]
    pub assignee: Option<OwnerRef>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

impl ActivityResource {
    /// The relations an activity's resource embeds: load them with the
    /// records (`Activity::query().with_all(ActivityResource::EMBEDS)`).
    pub const EMBEDS: [&'static str; 3] = ["subject_record", "owner", "assignee"];
}

impl ApiResource for ActivityResource {
    const TYPE: &'static str = ACTIVITY_TYPE;
}

impl Resource<Activity> for ActivityResource {
    /// `activity` with its loaded subject record, owner and assignee.
    fn from_record(activity: &Record<Activity>) -> Self {
        ActivityResource {
            resource_type: ACTIVITY_TYPE,
            id: activity.id,
            kind: activity.kind,
            subject: activity.subject.clone(),
            body: activity.body.clone(),
            due_at: activity.due_at,
            completed_at: activity.completed_at,
            subject_type: activity.subject_type,
            subject_id: activity.subject_id,
            subject_label: subject_label(activity),
            owner_id: activity.owner_id,
            owner: OwnerRef::loaded(activity.loaded(Activity::owner), activity.owner_id),
            assignee_id: activity.assignee_id,
            assignee: activity
                .assignee_id
                .map(|id| OwnerRef::loaded(activity.loaded(Activity::assignee), id)),
            created_at: activity.created_at,
        }
    }
}

/// The name of the record `activity` is attached to: the company's name,
/// the contact's full name or the deal's title (deleted companies and
/// contacts keep theirs); [`MISSING_SUBJECT_LABEL`] once it is gone.
fn subject_label(activity: &Record<Activity>) -> String {
    let Some(record) = activity
        .loaded(Activity::subject_record)
        .and_then(Option::as_ref)
    else {
        return MISSING_SUBJECT_LABEL.to_owned();
    };
    if let Some(company) = record.downcast_ref::<Company>() {
        company.name.clone()
    } else if let Some(contact) = record.downcast_ref::<Contact>() {
        contact.full_name()
    } else if let Some(deal) = record.downcast_ref::<Deal>() {
        deal.title.clone()
    } else {
        MISSING_SUBJECT_LABEL.to_owned()
    }
}

/// A page of activities as `GET /api/v1/activities` returns it (the
/// OpenAPI schema of a `Page<ActivityResource>`): `{data, meta}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ActivityPage {
    /// The activities.
    pub data: Vec<ActivityResource>,
    // Where the page is (no doc comment: a description would wrap the
    // inlined schema in `allOf`).
    #[schema(inline)]
    pub meta: ActivityPageMeta,
}

/// The pagination metadata of an [`ActivityPage`] (inlined in the schema,
/// so no `PageMeta` component of this crate can clash with another's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ActivityPageMeta {
    /// The page number (from 1).
    pub current_page: u64,
    /// The page size (at most 100).
    pub per_page: u64,
    /// How many activities match.
    pub total: u64,
    /// The last page number (at least 1).
    pub last_page: u64,
}
