//! [`DatabaseNotification`] and the read-state API of [`Notifier`].

use chrono::{DateTime, Utc};
use navree_db::{Page, PageQuery, paginate};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    Select,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::notifications::{Column, Entity, Model};
use crate::{Notifiable, Notifier};

/// A stored notification as the `/api/notifications` endpoints return it:
/// `{id, type, data, read_at, created_at}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatabaseNotification {
    /// The notification id.
    pub id: Uuid,
    /// [`Notification::TYPE`](crate::Notification::TYPE).
    #[serde(rename = "type")]
    pub notification_type: String,
    /// What [`Notification::to_database`](crate::Notification::to_database) returned.
    pub data: Value,
    /// When the owner read it; `None` while unread.
    pub read_at: Option<DateTime<Utc>>,
    /// When it was sent.
    pub created_at: DateTime<Utc>,
}

impl From<Model> for DatabaseNotification {
    fn from(model: Model) -> Self {
        Self {
            id: model.id,
            notification_type: model.notification_type,
            data: model.data,
            read_at: model.read_at,
            created_at: model.created_at,
        }
    }
}

/// The read state of stored notifications (the database channel). Every
/// method only sees the notifications of `notifiable`, and takes the
/// connection to use: in a handler, `uow.db().conn()` for reads and
/// `&*uow.tx().await?` for writes.
impl Notifier {
    /// A page of `notifiable`'s notifications, newest first; only unread
    /// ones when `unread_only`.
    pub async fn notifications<C: ConnectionTrait>(
        &self,
        conn: &C,
        notifiable: &dyn Notifiable,
        unread_only: bool,
        page: PageQuery,
    ) -> Result<Page<DatabaseNotification>, DbErr> {
        let mut select = owned_by(notifiable);
        if unread_only {
            select = select.filter(Column::ReadAt.is_null());
        }
        let select = select
            .order_by_desc(Column::CreatedAt)
            .order_by_desc(Column::Id);
        Ok(paginate(select, conn, page)
            .await?
            .map(DatabaseNotification::from))
    }

    /// How many of `notifiable`'s notifications are unread.
    pub async fn unread_count<C: ConnectionTrait>(
        &self,
        conn: &C,
        notifiable: &dyn Notifiable,
    ) -> Result<u64, DbErr> {
        owned_by(notifiable)
            .filter(Column::ReadAt.is_null())
            .count(conn)
            .await
    }

    /// Marks notification `id` of `notifiable` read (keeping the first
    /// read time when it already is). `false` when `notifiable` has no
    /// notification `id`.
    pub async fn mark_as_read<C: ConnectionTrait>(
        &self,
        conn: &C,
        notifiable: &dyn Notifiable,
        id: Uuid,
    ) -> Result<bool, DbErr> {
        let marked = Entity::update_many()
            .col_expr(Column::ReadAt, Expr::value(self.now().await))
            .filter(Column::Id.eq(id))
            .filter(owner(notifiable))
            .filter(Column::ReadAt.is_null())
            .exec(conn)
            .await?;
        if marked.rows_affected > 0 {
            return Ok(true);
        }
        Ok(owned_by(notifiable)
            .filter(Column::Id.eq(id))
            .count(conn)
            .await?
            > 0)
    }

    /// Marks every unread notification of `notifiable` read; returns how
    /// many were unread.
    pub async fn mark_all_as_read<C: ConnectionTrait>(
        &self,
        conn: &C,
        notifiable: &dyn Notifiable,
    ) -> Result<u64, DbErr> {
        let marked = Entity::update_many()
            .col_expr(Column::ReadAt, Expr::value(self.now().await))
            .filter(owner(notifiable))
            .filter(Column::ReadAt.is_null())
            .exec(conn)
            .await?;
        Ok(marked.rows_affected)
    }
}

/// `notifiable`'s notifications.
fn owned_by(notifiable: &dyn Notifiable) -> Select<Entity> {
    Entity::find().filter(owner(notifiable))
}

/// `notifiable_type = ? AND notifiable_id = ?`.
fn owner(notifiable: &dyn Notifiable) -> sea_orm::Condition {
    sea_orm::Condition::all()
        .add(Column::NotifiableType.eq(notifiable.notifiable_type()))
        .add(Column::NotifiableId.eq(notifiable.notifiable_id()))
}
