//! The `notification_settings` table as an ORM model: what a member wants
//! to hear about, per workspace (one row per user and workspace, created on
//! the first change).

use chrono::{DateTime, Utc};
use navree::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// A member's notification preferences in one workspace. Rows belong to one
/// workspace: read and write them through `WorkspaceRepo::<NotificationSetting>`.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "notification_settings", timestamps, workspace)]
pub struct NotificationSetting {
    /// The id (UUID v7, generated when the row is created).
    pub id: Uuid,
    /// The workspace the preferences are for.
    pub workspace_id: Uuid,
    /// The member they belong to.
    pub user_id: Uuid,
    /// Mail when a project is created or archived.
    pub project_activity: bool,
    /// A weekly summary of the workspace.
    pub weekly_summary: bool,
    /// News about the product.
    pub product_updates: bool,
    /// When the row was created.
    pub created_at: DateTime<Utc>,
    /// When the row last changed.
    pub updated_at: DateTime<Utc>,
}

/// Reads and writes go through `WorkspaceRepo`.
impl WorkspaceScoped for NotificationSetting {}
