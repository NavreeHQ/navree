//! The `notifications` table (the database channel's storage): a
//! navree-orm model.
//!
//! [`Model`] is the one definition of a row. The database channel writes
//! rows through the ORM, and apps query them with it for what the
//! [`Notifier`](crate::Notifier) does not offer, such as pruning old
//! notifications.
//!
//! ```no_run
//! use chrono::{TimeDelta, Utc};
//! use navree_db::Db;
//! use navree_notify::notifications::Model as Stored;
//! use navree_orm::Model as _;
//! # async fn demo(db: Db) -> navree_orm::Result<()> {
//! // Read notifications older than 90 days go.
//! let pruned = Stored::query()
//!     .where_not_null(Stored::READ_AT)
//!     .where_(Stored::CREATED_AT.lt(Utc::now() - TimeDelta::days(90)))
//!     .delete(&db)
//!     .await?;
//! # Ok(()) }
//! ```

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// One stored notification. `notifiable_type` and `notifiable_id` name its
/// owner (`user` and the user's UUID); `read_at` is `None` while unread.
#[derive(Clone, Debug, PartialEq, navree_orm::Model, Serialize)]
#[model(table = "notifications")]
pub struct Model {
    /// A UUID v7, shared with the notification's broadcast and mail.
    pub id: Uuid,
    /// The owner type, e.g. `user`.
    pub notifiable_type: String,
    /// The owner id, e.g. the user's UUID as a string.
    pub notifiable_id: String,
    /// The notification type ([`Notification::TYPE`](crate::Notification::TYPE)),
    /// column `type` (serialized as `type` too).
    #[model(column = "type")]
    #[serde(rename = "type")]
    pub notification_type: String,
    /// What [`Notification::to_database`](crate::Notification::to_database) returned.
    pub data: Value,
    /// When the owner read it.
    pub read_at: Option<DateTime<Utc>>,
    /// When it was sent.
    pub created_at: DateTime<Utc>,
}
