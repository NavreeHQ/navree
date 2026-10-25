//! The API's activity shape (spec §7 `Activity`) and how models become it:
//! the owner and assignee embeds and the label of the record each activity
//! is attached to, loaded with one query per kind for a whole page.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use crm_core::entities::{activity, deal};
use crm_core::{ActivityKind, CompanyRef, ContactRef, OwnerRef, SubjectType};
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// The subject type of activities (`__type`).
pub const ACTIVITY_TYPE: &str = crm_core::subjects::ACTIVITY;

/// What `subject_label` says when the record an activity is attached to is
/// gone.
pub const MISSING_SUBJECT_LABEL: &str = "Deleted record";

/// An activity as the API returns it:
/// `{__type: "Activity", id, kind, subject, body, due_at, completed_at,
/// subject_type, subject_id, subject_label, owner_id, owner, assignee_id,
/// assignee, created_at}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = Activity)]
pub struct ActivityResource {
    /// Always `"Activity"` (the CASL subject type).
    #[serde(rename = "__type")]
    #[schema(example = "Activity")]
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

/// The activities as resources, in the same order, with their embeds and
/// subject labels read through `conn`.
pub async fn present<C: ConnectionTrait>(
    conn: &C,
    activities: Vec<activity::Model>,
) -> Result<Vec<ActivityResource>, DbErr> {
    let user_ids: Vec<Uuid> = activities
        .iter()
        .flat_map(|activity| [Some(activity.owner_id), activity.assignee_id])
        .flatten()
        .collect();
    let users = OwnerRef::load(conn, user_ids).await?;
    let labels = subject_labels(conn, &activities).await?;
    let user = |id: Uuid| {
        users.get(&id).cloned().unwrap_or_else(|| OwnerRef {
            id,
            name: String::new(),
        })
    };
    Ok(activities
        .into_iter()
        .map(|activity| ActivityResource {
            resource_type: ACTIVITY_TYPE,
            subject_label: labels
                .get(&(activity.subject_type, activity.subject_id))
                .cloned()
                .unwrap_or_else(|| MISSING_SUBJECT_LABEL.to_owned()),
            owner: user(activity.owner_id),
            assignee: activity.assignee_id.map(user),
            id: activity.id,
            kind: activity.kind,
            subject: activity.subject,
            body: activity.body,
            due_at: activity.due_at,
            completed_at: activity.completed_at,
            subject_type: activity.subject_type,
            subject_id: activity.subject_id,
            owner_id: activity.owner_id,
            assignee_id: activity.assignee_id,
            created_at: activity.created_at,
        })
        .collect())
}

/// The label of every record `activities` are attached to, by type and id
/// (soft-deleted companies and contacts keep their names).
async fn subject_labels<C: ConnectionTrait>(
    conn: &C,
    activities: &[activity::Model],
) -> Result<HashMap<(SubjectType, Uuid), String>, DbErr> {
    let ids = |kind: SubjectType| -> Vec<Uuid> {
        activities
            .iter()
            .filter(|activity| activity.subject_type == kind)
            .map(|activity| activity.subject_id)
            .collect()
    };
    let mut labels = HashMap::new();
    for (id, company) in CompanyRef::load(conn, ids(SubjectType::Company)).await? {
        labels.insert((SubjectType::Company, id), company.name);
    }
    for (id, contact) in ContactRef::load(conn, ids(SubjectType::Contact)).await? {
        labels.insert((SubjectType::Contact, id), contact.full_name);
    }
    let deal_ids = ids(SubjectType::Deal);
    if !deal_ids.is_empty() {
        let titles: Vec<(Uuid, String)> = deal::Entity::find()
            .select_only()
            .column(deal::Column::Id)
            .column(deal::Column::Title)
            .filter(deal::Column::Id.is_in(deal_ids))
            .into_tuple()
            .all(conn)
            .await?;
        for (id, title) in titles {
            labels.insert((SubjectType::Deal, id), title);
        }
    }
    Ok(labels)
}
