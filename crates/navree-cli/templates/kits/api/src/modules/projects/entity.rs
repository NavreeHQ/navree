//! The `projects` table: one row per project, owned by a workspace
//! (`workspace_id`; row isolation, enforced by PostgreSQL row-level
//! security).

use navree::access::Subject;
use navree::workspace::WorkspaceScoped;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A project.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "projects")]
pub struct Model {
    /// The id (UUID v7).
    #[sea_orm(primary_key, auto_increment = false)]
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
