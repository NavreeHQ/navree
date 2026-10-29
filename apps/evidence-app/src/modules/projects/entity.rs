//! The `projects` table: one row per project, owned by a workspace
//! (`workspace_id`; row isolation, enforced by PostgreSQL row-level
//! security).

use navree::access::Subject;
use navree::search::Searchable;
// COMPAT (data-layer move): the SeaORM forms of the workspace repositories.
use navree::workspace::sea_compat::WorkspaceScoped;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A project. Searchable: every write through `WorkspaceRepo` queues a
/// `search.sync` job, and `GET /api/v1/projects/search?q=` queries the
/// engine (`search.engine`), scoped to the workspace.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Searchable)]
#[sea_orm(table_name = "projects")]
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
    #[searchable(sort)]
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// The access subject `Project`; permissions `projects.<action>`.
impl Subject for Entity {
    const SUBJECT: &'static str = "Project";
    const PERMISSION_PREFIX: &'static str = "projects";
}

/// Every query through `WorkspaceRepo` is filtered by `workspace_id`.
impl WorkspaceScoped for Entity {}

/// The entity, named after its subject for `#[can(Read, Project)]`.
pub type Project = Entity;
