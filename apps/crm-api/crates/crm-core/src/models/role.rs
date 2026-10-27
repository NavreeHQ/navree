//! [`Role`]: the CRM's view of navree-access's `roles`.

use chrono::{DateTime, Utc};
use navree::orm::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// A role (`admin`, `manager`, `rep`), as a user's `roles` relation loads
/// it. Roles are created, granted and assigned through `AccessService`,
/// which keeps the abilities cache in step; the CRM only reads them.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "roles", timestamps(updated_at = false))]
pub struct Role {
    /// The id.
    pub id: Uuid,
    /// The unique name, e.g. `admin`.
    pub name: String,
    /// What the role is for.
    pub description: Option<String>,
    /// When the role was created.
    pub created_at: DateTime<Utc>,
}
