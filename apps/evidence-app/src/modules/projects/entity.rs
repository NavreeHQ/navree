//! The `projects` table: one row per project, owned by a workspace
//! (`workspace_id`; row isolation, enforced by PostgreSQL row-level
//! security). [`Project`] is a navree-orm model.

use chrono::{DateTime, Utc};
use navree::search::Searchable;
use navree::workspace::WorkspaceScoped;
use serde::Serialize;
use uuid::Uuid;

/// A project: the access subject `Project` (permissions `projects.<action>`).
/// Workspace-scoped: queries are filtered by the current workspace, and
/// `WorkspaceRepo` reads and writes a workspace named explicitly.
/// Searchable: every write through the model API queues a `search.sync`
/// job, and `GET /api/v1/projects/search?q=` queries the engine
/// (`search.engine`), scoped to the workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq, navree::orm::Model, Serialize, Searchable)]
#[model(
    table = "projects",
    timestamps,
    workspace,
    subject = "Project",
    permission_prefix = "projects"
)]
#[searchable(name = "Project", workspace = "workspace_id")]
pub struct Project {
    /// The id (UUID v7).
    pub id: Uuid,
    /// The workspace it belongs to.
    pub workspace_id: Uuid,
    /// The name.
    #[searchable(boost = 3)]
    pub name: String,
    /// What it is about.
    #[searchable]
    pub description: Option<String>,
    /// `active` or `archived`.
    #[searchable(filter)]
    pub status: String,
    /// The user who created it.
    pub created_by: Uuid,
    /// When it was created.
    #[searchable(sort)]
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// Every query through `WorkspaceRepo` is filtered by `workspace_id`.
impl WorkspaceScoped for Project {}
