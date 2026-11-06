//! Request and response bodies of `/api/v1/settings`.

use navree::openapi::ToSchema;
use navree::validation::Validate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use navree::workspace::Workspace;

/// The body of `PATCH /api/v1/settings/workspace`: only the fields present
/// change.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct UpdateWorkspace {
    /// 1 to 100 characters.
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,
    /// Lower-case letters, digits and single dashes (`agra-inc`); the web
    /// app's URLs (`/w/<slug>/`) and the `X-Workspace` header use it.
    #[validate(length(min = 1, max = 64), pattern(r"^[a-z0-9]+(-[a-z0-9]+)*$"))]
    pub slug: Option<String>,
}

/// The workspace's settings as the API returns them.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceSettings {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub owner_id: Uuid,
}

impl From<Workspace> for WorkspaceSettings {
    fn from(workspace: Workspace) -> Self {
        Self {
            id: workspace.id,
            name: workspace.name,
            slug: workspace.slug,
            owner_id: workspace.owner_id,
        }
    }
}

/// A member's notification preferences in the current workspace.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, ToSchema)]
pub struct NotificationSettings {
    /// Mail when a project is created or archived (default on).
    pub project_activity: bool,
    /// A weekly summary of the workspace (default on).
    pub weekly_summary: bool,
    /// News about the product (default off).
    pub product_updates: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            project_activity: true,
            weekly_summary: true,
            product_updates: false,
        }
    }
}

/// The body of `PUT /api/v1/settings/notifications`: only the fields
/// present change.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct UpdateNotifications {
    #[validate(skip)]
    pub project_activity: Option<bool>,
    #[validate(skip)]
    pub weekly_summary: Option<bool>,
    #[validate(skip)]
    pub product_updates: Option<bool>,
}
