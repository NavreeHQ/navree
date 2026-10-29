//! The `notification_settings` table: what a member wants to hear about,
//! per workspace (one row per user and workspace, created on the first
//! change).

// COMPAT (data-layer move): the SeaORM forms of the workspace repositories.
use navree::workspace::sea_compat::WorkspaceScoped;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A member's notification preferences in one workspace.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "notification_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    /// Mail when a project is created or archived.
    pub project_activity: bool,
    /// A weekly summary of the workspace.
    pub weekly_summary: bool,
    /// News about the product.
    pub product_updates: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Reads and writes go through `WorkspaceRepo`.
impl WorkspaceScoped for Entity {}
