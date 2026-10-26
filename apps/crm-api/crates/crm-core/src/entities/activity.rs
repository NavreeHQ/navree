//! `activities`: notes, calls, emails, meetings and tasks attached to a
//! company, contact or deal (`subject_type` + `subject_id`, no foreign
//! key). Tasks have a due date and can be completed.

use sea_orm::entity::prelude::*;
use serde::Serialize;

use crate::enums::{ActivityKind, SubjectType};

/// An activity.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "activities")]
pub struct Model {
    /// A UUID v7.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// What it records.
    pub kind: ActivityKind,
    /// The one-line summary (required).
    pub subject: String,
    /// Details.
    #[sea_orm(column_type = "Text", nullable)]
    pub body: Option<String>,
    /// When it is due (tasks).
    pub due_at: Option<DateTimeUtc>,
    /// When it was completed (tasks).
    pub completed_at: Option<DateTimeUtc>,
    /// The kind of record it is attached to.
    pub subject_type: SubjectType,
    /// The record it is attached to.
    pub subject_id: Uuid,
    /// The user who created it.
    pub owner_id: Uuid,
    /// The user who should do it (tasks).
    pub assignee_id: Option<Uuid>,
    /// When it was created.
    pub created_at: DateTimeUtc,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
}

/// The owner and the assignee are users.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The owner (`owner_id` → `users.id`).
    #[sea_orm(
        belongs_to = "navis::auth::user::Entity",
        from = "Column::OwnerId",
        to = "navis::auth::user::Column::Id"
    )]
    Owner,
    /// The assignee (`assignee_id` → `users.id`).
    #[sea_orm(
        belongs_to = "navis::auth::user::Entity",
        from = "Column::AssigneeId",
        to = "navis::auth::user::Column::Id"
    )]
    Assignee,
}

impl ActiveModelBehavior for ActiveModel {}
