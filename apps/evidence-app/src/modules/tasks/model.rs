//! [`Task`]: a piece of work in a project (`tasks`).

use chrono::{DateTime, NaiveDate, Utc};
use navree::orm::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Where a task stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Not started (the default).
    #[default]
    Todo,
    /// In progress.
    Doing,
    /// Finished.
    Done,
}

impl TaskStatus {
    /// The stored value.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::Doing => "doing",
            TaskStatus::Done => "done",
        }
    }
}

/// A task. Workspace-scoped: `workspace_id` is filled from the request's
/// workspace and every query is filtered by it.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "tasks", timestamps, workspace, subject)]
pub struct Task {
    /// A UUID v7.
    pub id: Uuid,
    /// The workspace it belongs to.
    pub workspace_id: Uuid,
    /// The project it is part of.
    pub project_id: Uuid,
    /// What to do.
    pub title: String,
    /// `todo`, `doing` or `done`.
    pub status: String,
    /// 1 (highest) to 4.
    pub priority: i32,
    /// When it is due.
    pub due_on: Option<NaiveDate>,
    /// Who works on it.
    pub assignee_id: Option<Uuid>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}
