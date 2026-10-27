//! The `audit_logs` table: a navree-orm model, and a SeaORM entity.
//!
//! [`Model`] (exported as [`AuditRecord`](crate::AuditRecord)) is the one
//! definition of a row. The ORM is how the module writes and lists entries
//! and how apps query them (`AuditRecord::query()`, or
//! [`AuditFilter::query`](crate::AuditFilter::query)); the SeaORM entity
//! (`Entity`, `Column`, `ActiveModel`) stays for code built on SeaORM's
//! query API, such as admin panels generated from entities.
//!
//! ```no_run
//! use navree_audit::AuditRecord;
//! use navree_db::Db;
//! use navree_orm::Model as _;
//! # async fn demo(db: Db, user: uuid::Uuid) -> navree_orm::Result<()> {
//! let sign_ins = AuditRecord::query()
//!     .where_(AuditRecord::ACTOR_ID.eq(user))
//!     .where_(AuditRecord::ACTION.eq("auth.login"))
//!     .latest_by(AuditRecord::CREATED_AT)
//!     .limit(20)
//!     .get(&db)
//!     .await?;
//! # Ok(()) }
//! ```

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One audit trail entry as stored in `audit_logs` (also the JSON shape of
/// `GET /api/audit` items). `kind` is `data`, `auth`, `access`,
/// `security` or `custom`.
///
/// Entries are written by [`AuditLog`](crate::AuditLog), which stamps their
/// id and time: an entry inserted otherwise needs both.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, navree_orm::Model, Serialize, Deserialize)]
#[sea_orm(table_name = "audit_logs")]
#[model(table = "audit_logs")]
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
    /// The user impersonating the actor when the entry was written (an
    /// impersonation session), if any.
    pub impersonator_id: Option<Uuid>,
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
