//! [`DatabaseNotification`], the read-state API of [`Notifier`] and
//! [`NotificationList`], the list query of `GET /api/notifications`.

use chrono::{DateTime, Utc};
use navree_db::{Page, PageQuery};
use navree_orm::list::{Field, ListQuery, ListQueryExt as _, ListResource, ListSpec, Op};
use navree_orm::{Builder, Executor, Model as _, OrmError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::notifications::Model;
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

/// The list query of `GET /api/notifications`, in the list-query grammar
/// that every Navree list reads:
///
/// - `filter[unread]=true` keeps the notifications not read yet;
///   `filter[unread]=false` (or no filter) keeps all.
/// - `filter[type]`: the notification type
///   ([`Notification::TYPE`](crate::Notification::TYPE)); also `ne`, `in`
///   and `not_in`.
/// - `filter[created_at][gte]`, `[gt]`, `[lte]`, `[lt]`: a date (the whole
///   day in UTC) or a date and time.
/// - `page` and `per_page` (default 15, at most 100).
///
/// Notifications are always newest first. An unknown filter, a value of the
/// wrong kind or a `per_page` over 100 is a 400 that names the parameter.
///
/// ```
/// use navree_http::list::ListResource;
/// use navree_notify::NotificationList;
///
/// let query = NotificationList::SPEC
///     .parse("filter[unread]=true&filter[type][in]=deal.won,deal.lost")
///     .unwrap();
/// assert_eq!(query.eq("unread"), Some("true"));
/// assert!(NotificationList::SPEC.parse("filter[unread]=maybe").is_err());
/// ```
pub struct NotificationList;

impl ListResource for NotificationList {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::boolean("unread").ops(&[Op::Eq]).by_app(),
        Field::text("type").ops(&[Op::Eq, Op::Ne, Op::In, Op::NotIn]),
        Field::datetime("created_at").ops(&[Op::Gt, Op::Gte, Op::Lt, Op::Lte]),
    ])
    .default_sort(&["-created_at"]);
}

/// The read state of stored notifications (the database channel). Every
/// method only sees the notifications of `notifiable`, and takes the
/// executor to use: in a handler, `uow.db()` for reads and `&uow` for
/// writes (any [`navree_orm::Executor`]: a [`Db`](navree_db::Db), a
/// [`Uow`](navree_db::Uow)).
impl Notifier {
    /// A page of `notifiable`'s notifications, newest first; only unread
    /// ones when `unread_only`.
    pub async fn notifications<E: Executor + ?Sized>(
        &self,
        exec: &E,
        notifiable: &dyn Notifiable,
        unread_only: bool,
        page: PageQuery,
    ) -> Result<Page<DatabaseNotification>, OrmError> {
        let mut query = owned_by(notifiable);
        if unread_only {
            query = query.where_null(Model::READ_AT);
        }
        let page = query
            .order_by_desc(Model::CREATED_AT)
            .order_by_desc(Model::ID)
            .paginate(exec, page)
            .await?;
        Ok(page.map(|record| DatabaseNotification::from(record.into_inner())))
    }

    /// One page of `notifiable`'s notifications that a list query (read
    /// against [`NotificationList`]) asks for, newest first: what
    /// `GET /api/notifications` answers.
    pub(crate) async fn notification_list<E: Executor + ?Sized>(
        &self,
        exec: &E,
        notifiable: &dyn Notifiable,
        list: &ListQuery,
    ) -> Result<Page<DatabaseNotification>, OrmError> {
        let spec = &NotificationList::SPEC;
        let mut query = owned_by(notifiable);
        if list.eq("unread") == Some("true") {
            query = query.where_null(Model::READ_AT);
        }
        let page = query
            .list(list, spec)?
            .paginate(exec, list.page_request(spec))
            .await?;
        Ok(page.map(|record| DatabaseNotification::from(record.into_inner())))
    }

    /// How many of `notifiable`'s notifications are unread.
    pub async fn unread_count<E: Executor + ?Sized>(
        &self,
        exec: &E,
        notifiable: &dyn Notifiable,
    ) -> Result<u64, OrmError> {
        owned_by(notifiable)
            .where_null(Model::READ_AT)
            .count(exec)
            .await
    }

    /// Marks notification `id` of `notifiable` read (keeping the first
    /// read time when it already is). `false` when `notifiable` has no
    /// notification `id`.
    pub async fn mark_as_read<E: Executor + ?Sized>(
        &self,
        exec: &E,
        notifiable: &dyn Notifiable,
        id: Uuid,
    ) -> Result<bool, OrmError> {
        let marked = owned_by(notifiable)
            .where_(Model::ID.eq(id))
            .where_null(Model::READ_AT)
            .update(exec, [Model::READ_AT.set(Some(self.now().await))])
            .await?;
        if marked > 0 {
            return Ok(true);
        }
        Ok(owned_by(notifiable)
            .where_(Model::ID.eq(id))
            .count(exec)
            .await?
            > 0)
    }

    /// Marks every unread notification of `notifiable` read; returns how
    /// many were unread.
    pub async fn mark_all_as_read<E: Executor + ?Sized>(
        &self,
        exec: &E,
        notifiable: &dyn Notifiable,
    ) -> Result<u64, OrmError> {
        owned_by(notifiable)
            .where_null(Model::READ_AT)
            .update(exec, [Model::READ_AT.set(Some(self.now().await))])
            .await
    }
}

/// `notifiable`'s notifications: `notifiable_type = ? AND notifiable_id = ?`.
fn owned_by(notifiable: &dyn Notifiable) -> Builder<Model> {
    Model::query()
        .where_(Model::NOTIFIABLE_TYPE.eq(notifiable.notifiable_type()))
        .where_(Model::NOTIFIABLE_ID.eq(notifiable.notifiable_id()))
}
