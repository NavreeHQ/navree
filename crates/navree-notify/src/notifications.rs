//! The `notifications` table as a SeaORM entity (the database channel's
//! storage), for queries the [`Notifier`](crate::Notifier) does not offer,
//! such as pruning old notifications.

use sea_orm::entity::prelude::*;

/// One stored notification. `notifiable_type` and `notifiable_id` name its
/// owner (`user` and the user's UUID); `read_at` is `None` while unread.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "notifications")]
pub struct Model {
    /// A UUID v7, shared with the notification's broadcast and mail.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The owner type, e.g. `user`.
    pub notifiable_type: String,
    /// The owner id, e.g. the user's UUID as a string.
    pub notifiable_id: String,
    /// The notification type ([`Notification::TYPE`](crate::Notification::TYPE)),
    /// column `type`.
    #[sea_orm(column_name = "type")]
    pub notification_type: String,
    /// What [`Notification::to_database`](crate::Notification::to_database) returned.
    #[sea_orm(column_type = "JsonBinary")]
    pub data: Json,
    /// When the owner read it.
    pub read_at: Option<DateTimeUtc>,
    /// When it was sent.
    pub created_at: DateTimeUtc,
}

/// `notifications` has no relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
