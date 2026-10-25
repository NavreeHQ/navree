//! The `audit_logs` table as a SeaORM entity.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One audit trail entry as stored in `audit_logs` (also the JSON shape of
/// `GET /api/audit` items). `kind` is `data`, `auth`, `access`,
/// `security` or `custom`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "audit_logs")]
pub struct Model {
    /// A UUID v7 (ordered by time).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The category: `data`, `auth`, `access`, `security` or `custom`.
    pub kind: String,
    /// What happened: `deal.created`, `auth.login`, `access.denied`, ...
    pub action: String,
    /// The acting user, if any.
    pub actor_id: Option<Uuid>,
    /// The affected record type, e.g. `Deal`.
    pub subject_type: Option<String>,
    /// The affected record id.
    pub subject_id: Option<String>,
    /// Values before the change (changed fields only for updates).
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub old_values: Option<Json>,
    /// Values after the change (changed fields only for updates).
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub new_values: Option<Json>,
    /// The request id of the unit of work or request.
    pub request_id: Option<String>,
    /// The client IP address.
    pub ip: Option<String>,
    /// The client user agent.
    pub user_agent: Option<String>,
    /// Extra data (an object).
    #[sea_orm(column_type = "JsonBinary")]
    pub metadata: Json,
    /// When the entry was recorded (UTC).
    pub created_at: DateTimeUtc,
}

/// `audit_logs` has no relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
