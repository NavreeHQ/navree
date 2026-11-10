//! The `projects` table as an ORM model: one row per project, owned by a
//! workspace (`workspace_id`; row isolation, enforced by the model's
//! workspace scope and by PostgreSQL row-level security).

use chrono::{DateTime, Utc};
use navree::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// A project. Query it with `Project::query()` (the current workspace's
/// rows) or `WorkspaceRepo::<Project>::query(workspace)`; the model is the
/// access subject `Project`, with permissions `projects.<action>`.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "projects", timestamps, workspace, subject)]
pub struct Project {
    /// The id (UUID v7, generated when the project is created).
    pub id: Uuid,
    /// The workspace it belongs to.
    pub workspace_id: Uuid,
    /// The name.
    pub name: String,
    /// What it is about.
    pub description: Option<String>,
    /// `active` or `archived`.
    pub status: String,
    /// The user who created it.
    pub created_by: Uuid,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// Every query through `WorkspaceRepo` is filtered by `workspace_id`.
impl WorkspaceScoped for Project {}
