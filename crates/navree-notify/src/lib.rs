//! # navree-notify
//!
//! `NotifyModule`: notifications delivered over mail, database and broadcast
//! channels, with read state (Laravel's notifications).
//!
//! - A [`Notification`] lists its [`Channel`]s per recipient
//!   ([`via`](Notification::via)) and renders itself for each one:
//!   [`to_mail`](Notification::to_mail) (a navree-mail
//!   [`TemplateMail`](navree_mail::TemplateMail)),
//!   [`to_database`](Notification::to_database) (the stored `data`) and
//!   [`to_broadcast`](Notification::to_broadcast). A [`Notifiable`] (usually
//!   a user) names the owner of stored notifications and routes mail and
//!   broadcasts.
//! - [`NotifyModule`] is global: it provides and exports [`Notifier`]
//!   ([`NotifyOptions`], section `notify`), registers job `notify.send`
//!   ([`SendNotificationJob`]) and migrates table `notifications`.
//! - [`Notifier::send`] renders now and delivers now, or, with
//!   [`NotifyOptions::queue`], dispatches one `notify.send` job per channel
//!   through the app's `dyn Dispatcher`. [`Notifier::send_now`] never
//!   queues. [`Notifier::send_in`] sends as part of a `Uow`: the database
//!   row joins its transaction and the other channels go out after the
//!   commit. `send` and `send_now` do the same with the current unit of
//!   work (`navree_db::Uow::current`, e.g. the request's in a handler).
//! - Channels: database (a [`notifications`] row), mail (navree-mail's
//!   `Mailer`; the app needs `MailModule` and `ViewModule`) and broadcast
//!   (event [`NOTIFICATION_CREATED`] with `{id, type, data, read_at,
//!   created_at}` on [`Notifiable::broadcast_channel`] through the app's
//!   `dyn Broadcaster`, e.g. navree-realtime). A channel whose service is
//!   missing fails with [`NotifyError::Unavailable`]; the other channels
//!   still deliver.
//! - Routes (authenticated, for the signed-in user as notifiable type
//!   [`USER_NOTIFIABLE_TYPE`]): `GET /api/notifications?unread=` (a page,
//!   newest first), `GET /api/notifications/unread-count` (`{count}`),
//!   `POST /api/notifications/{id}/read` and
//!   `POST /api/notifications/read-all` (204). The same read state is
//!   available in code: [`Notifier::notifications`],
//!   [`Notifier::unread_count`], [`Notifier::mark_as_read`] and
//!   [`Notifier::mark_all_as_read`].
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_db::DatabaseModule;
//! use navree_kernel::Navree;
//! use navree_mail::{MailModule, TemplateMail};
//! use navree_notify::{Channel, Notifiable, Notification, Notifier, NotifyModule};
//! use navree_view::ViewModule;
//! use serde::{Deserialize, Serialize};
//! use serde_json::{Value, json};
//!
//! struct User {
//!     id: uuid::Uuid,
//!     email: String,
//! }
//!
//! impl Notifiable for User {
//!     fn notifiable_type(&self) -> &'static str {
//!         navree_notify::USER_NOTIFIABLE_TYPE
//!     }
//!
//!     fn notifiable_id(&self) -> String {
//!         self.id.to_string()
//!     }
//!
//!     fn route_mail(&self) -> Option<String> {
//!         Some(self.email.clone())
//!     }
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! struct DealWon {
//!     deal_id: String,
//!     title: String,
//! }
//!
//! impl Notification for DealWon {
//!     const TYPE: &'static str = "deal_won";
//!
//!     fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
//!         vec![Channel::Database, Channel::Broadcast, Channel::Mail]
//!     }
//!
//!     fn to_mail(&self, _n: &dyn Notifiable) -> Option<TemplateMail> {
//!         Some(TemplateMail {
//!             subject: format!("Deal won: {}", self.title),
//!             template: "mail/deal-won".into(),
//!             context: json!({ "title": self.title }),
//!             ..TemplateMail::default()
//!         })
//!     }
//!
//!     fn to_database(&self, _n: &dyn Notifiable) -> Option<Value> {
//!         Some(json!({
//!             "title": "Deal won",
//!             "message": format!("{} was won", self.title),
//!             "url": format!("/deals/{}", self.deal_id),
//!             "deal_id": self.deal_id,
//!         }))
//!     }
//! }
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), navree_kernel::BoxError> {
//! // .env: APP_NOTIFY__QUEUE=true (with QueueModule in the app)
//! let app = Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(DatabaseModule::from_config())
//!     .module(ViewModule::from_config())
//!     .module(MailModule::from_config())
//!     .module(NotifyModule::from_config())
//!     .boot()
//!     .await?;
//!
//! let notifier = app.ctx().get::<Notifier>().await?;
//! let owner = User { id: uuid::Uuid::now_v7(), email: "ada@example.com".into() };
//! notifier
//!     .send(&owner, DealWon { deal_id: "42".into(), title: "Acme renewal".into() })
//!     .await?;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod channel;
mod controller;
mod error;
mod job;
mod migration;
mod module;
mod notifiable;
mod notification;
pub mod notifications;
mod notifier;
mod options;
mod schema;
mod stored;

pub use channel::Channel;
pub use error::NotifyError;
pub use job::{Delivery, SendNotificationJob};
pub use migration::NOTIFICATIONS_TABLE;
pub use module::NotifyModule;
pub use notifiable::Notifiable;
pub use notification::Notification;
pub use notifier::{NOTIFICATION_CREATED, Notifier};
pub use options::NotifyOptions;
pub use stored::DatabaseNotification;

/// The notifiable type of users: the `/api/notifications` endpoints serve
/// the signed-in user's notifications stored under this type.
pub const USER_NOTIFIABLE_TYPE: &str = "user";
