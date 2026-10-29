//! The `projects` table: one row per project, owned by a workspace
//! (`workspace_id`; row isolation, enforced by PostgreSQL row-level
//! security).
//!
//! [`Project`] is a navree-orm model. The SeaORM derive kept on it
//! (`Entity`, `Column`) serves the admin panel's resource until navree-helm
//! runs on models.

use navree::access::Subject;
use navree::search::Searchable;
use navree::workspace::WorkspaceScoped;
// COMPAT (data-layer port): the SeaORM derive, for the admin panel.
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A project: the access subject `Project` (permissions `projects.<action>`).
/// Workspace-scoped: queries are filtered by the current workspace, and
/// `WorkspaceRepo` reads and writes a workspace named explicitly.
/// Searchable: every write through the model API queues a `search.sync`
/// job, and `GET /api/v1/projects/search?q=` queries the engine
/// (`search.engine`), scoped to the workspace.
// COMPAT: `DeriveEntityModel` and the `#[sea_orm]` attributes.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, DeriveEntityModel, navree::orm::Model, Serialize, Searchable,
)]
#[sea_orm(table_name = "projects")]
#[model(
    table = "projects",
    timestamps,
    workspace,
    subject = "Project",
    permission_prefix = "projects"
)]
#[searchable(name = "Project", workspace = "workspace_id")]
pub struct Model {
    /// The id (UUID v7).
    #[sea_orm(primary_key, auto_increment = false)]
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
    pub created_at: DateTimeUtc,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
}

/// The model, named after its subject for `#[can(Read, Project)]`.
pub type Project = Model;

/// Every query through `WorkspaceRepo` is filtered by `workspace_id`.
impl WorkspaceScoped for Project {}

/// COMPAT: no relations are declared (the SeaORM entity's).
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

/// COMPAT: the SeaORM entity's behaviour.
impl ActiveModelBehavior for ActiveModel {}

/// COMPAT: the SeaORM entity as the subject `Project`, for the admin
/// panel's resource.
impl Subject for Entity {
    const SUBJECT: &'static str = "Project";
    const PERMISSION_PREFIX: &'static str = "projects";
}
