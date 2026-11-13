//! # navree-notify
//!
//! `NotifyModule`: notifications delivered over mail, database, broadcast,
//! Slack, Microsoft Teams, SMS, WhatsApp, web push and native app push
//! channels, with read state, a delivery log and opt-outs.
//!
//! - A [`Notification`] lists its [`Channel`]s per recipient
//!   ([`via`](Notification::via)) and renders itself for each one:
//!   [`to_mail`](Notification::to_mail) (a navree-mail
//!   [`TemplateMail`](navree_mail::TemplateMail)),
//!   [`to_database`](Notification::to_database) (the stored `data`),
//!   [`to_broadcast`](Notification::to_broadcast),
//!   [`to_slack`](Notification::to_slack), [`to_teams`](Notification::to_teams),
//!   [`to_sms`](Notification::to_sms), [`to_whatsapp`](Notification::to_whatsapp),
//!   [`to_web_push`](Notification::to_web_push) and
//!   [`to_push`](Notification::to_push). A [`Notifiable`]
//!   (usually a user) names the owner of stored notifications and routes
//!   each channel (`route_mail`, `route_slack`, `route_sms`...).
//! - [`NotifyModule`] is global: it provides and exports [`Notifier`]
//!   ([`NotifyOptions`], section `notify`), [`DeliveryLog`], [`OptOuts`],
//!   [`PushSubscriptions`] and [`PushDevices`], registers job `notify.send`
//!   ([`SendNotificationJob`]) and the commands `notify:vapid` and
//!   `notify:release`, and migrates `notifications`,
//!   `notification_deliveries`, `notification_opt_outs`,
//!   `push_subscriptions`, `push_devices`, `notification_preferences` and
//!   `notification_deferrals` (migration set `notify`; PostgreSQL, MySQL
//!   and SQLite).
//! - The Slack, Teams, SMS, WhatsApp, web push and push channels send
//!   through a driver per channel ([`SlackDriver`], [`TeamsDriver`],
//!   [`SmsDriver`], [`WhatsAppDriver`], [`WebPushDriver`], [`PushDriver`]):
//!   the app's own (a fake such as [`SmsFake`]), else the one
//!   `notify.<channel>` configures ([`SlackApi`], [`TeamsApi`],
//!   [`TwilioSms`], [`VonageSms`], [`SnsSms`], [`TwilioWhatsApp`],
//!   [`MetaWhatsApp`], [`VapidWebPush`], and for native apps [`ApnsPush`]
//!   and [`FcmPush`] from section `notify.push`). Numbers are
//!   read as E.164; opted-out numbers are skipped; each attempt is a
//!   [`NotificationDelivery`] row, moved forward by provider callbacks
//!   ([`ChannelCallback`], [`DeliveryLog::ingest`]).
//! - [`Notifier::send`] renders now and delivers now, or, with
//!   [`NotifyOptions::queue`], dispatches one `notify.send` job per channel
//!   through the app's `dyn Dispatcher`. [`Notifier::send_now`] never
//!   queues. [`Notifier::send_in`] sends as part of a `Uow`: the database
//!   row joins its transaction and the other channels go out after the
//!   commit. `send` and `send_now` do the same with the current unit of
//!   work (`navree_db::Uow::current`, e.g. the request's in a handler).
//! - Channels: database (a [`notifications`] row, a navree-orm model apps
//!   query and prune with the ORM), mail (navree-mail's
//!   `Mailer`; the app needs `MailModule` and `ViewModule`) and broadcast
//!   (event [`NOTIFICATION_CREATED`] with `{id, type, data, read_at,
//!   created_at}` on [`Notifiable::broadcast_channel`] through the app's
//!   `dyn Broadcaster`, e.g. navree-realtime). A channel whose service is
//!   missing fails with [`NotifyError::Unavailable`]; the other channels
//!   still deliver.
//! - Preferences: a notification has a category
//!   ([`Notification::CATEGORY`]), a digest ([`Notification::DIGEST`]), an
//!   urgency and a send window, set by hand or with `#[notification]`;
//!   people and workspaces switch channels per category, choose digests
//!   and quiet hours ([`Preferences`], [`NotificationPreferences`], table
//!   `notification_preferences`). The notifier skips channels turned off,
//!   holds digest mail and deliveries outside quiet hours or the send
//!   window on the recipient's clock ([`NotificationDeferral`], table
//!   `notification_deferrals`) and `notify:release` (every minute on the
//!   scheduler, or [`Notifier::release_due`]) sends them, digests as one
//!   mail. A workspace's mail is restyled through [`TemplateOverrides`].
//! - Routes (authenticated, for the signed-in user as notifiable type
//!   [`USER_NOTIFIABLE_TYPE`]): `GET /api/notifications` (a page, newest
//!   first, in the list-query grammar: `filter[unread]=true`,
//!   `filter[type]`, `filter[created_at][gte]`; see [`NotificationList`]),
//!   `GET /api/notifications/unread-count` (`{count}`),
//!   `POST /api/notifications/{id}/read` and
//!   `POST /api/notifications/read-all` (204), the push subscriptions:
//!   `GET /api/notifications/push-key`, `POST` and `DELETE
//!   /api/notifications/push-subscriptions`, the native app devices: `POST`
//!   and `DELETE /api/notifications/push-devices` (a signed-in session
//!   owns each device; ending the session ends it), and the preferences: `GET` and
//!   `PATCH /api/notifications/preferences` and
//!   `/api/notifications/preferences/workspace`. The same read state is
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
//! let owner = User { id: uuid::Uuid::now_v7(), email: "anu@example.com".into() };
//! notifier
//!     .send(&owner, DealWon { deal_id: "42".into(), title: "Agra renewal".into() })
//!     .await?;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// `#[notification]` names this crate `::navree_notify`, also from inside.
extern crate self as navree_notify;

mod callbacks;
mod catalog;
mod channel;
mod channels;
mod commands;
mod controller;
mod deferred;
mod deliveries;
mod devices;
mod digest;
mod drivers;
mod error;
mod fake;
mod job;
mod messages;
mod migration;
mod module;
mod notifiable;
mod notification;
pub mod notifications;
mod notifier;
mod opt_outs;
mod options;
mod overrides;
mod preferences;
mod push;
mod recipient;
mod schema;
mod store;
mod stored;
mod timing;
mod tokens;
#[cfg(feature = "ui")]
pub mod ui;

pub use callbacks::{
    ChannelCallback, InboundMessage, Keyword, OPT_IN_KEYWORDS, OPT_OUT_KEYWORDS, StatusUpdate,
    keyword,
};
pub use catalog::{
    ModuleBuilderNotifyExt, NotificationCatalog, NotificationCategory, NotificationKind,
};
pub use channel::Channel;
pub use deferred::{DeferralReason, NotificationDeferral, Released};
pub use deliveries::{DeliveryLog, DeliveryStatus, Ingested, NotificationDelivery};
pub use devices::{
    DeviceTarget, LiveSessions, NewPushDevice, PushDevice, PushDevices, PushEnvironment,
    PushPlatform,
};
pub use digest::{DIGEST_TYPE, DigestItem};
pub use drivers::apns::ApnsPush;
pub use drivers::fcm::FcmPush;
pub use drivers::log::{LogSms, LogWhatsApp};
pub use drivers::meta::MetaWhatsApp;
pub use drivers::slack::SlackApi;
pub use drivers::sns::SnsSms;
pub use drivers::teams::TeamsApi;
pub use drivers::twilio::{TwilioSms, TwilioWhatsApp};
pub use drivers::vonage::VonageSms;
pub use drivers::web_push::VapidWebPush;
pub use drivers::{
    ChannelError, PushDriver, PushTarget, SentMessage, SlackDriver, SmsDriver, TeamsDriver,
    WebPushDriver, WhatsAppDriver,
};
pub use error::NotifyError;
pub use fake::{
    FAKE_PROVIDER, PushFake, Sent, SlackFake, SmsFake, TeamsFake, WebPushFake, WhatsAppFake,
};
pub use job::{Delivery, SendNotificationJob};
pub use messages::{
    PushMessage, SlackMessage, SlackRoute, SmsMessage, TeamsMessage, TeamsRoute, Urgency,
    WebPushMessage, WhatsAppMessage, WhatsAppTemplate,
};
pub use migration::{
    DEFERRALS_TABLE, DELIVERIES_TABLE, NOTIFICATIONS_TABLE, OPT_OUTS_TABLE, PREFERENCES_TABLE,
    PUSH_DEVICES_TABLE, PUSH_SUBSCRIPTIONS_TABLE,
};
pub use module::NotifyModule;
/// `#[notification(category = "..", digest = "daily", urgent)]` on an
/// `impl Notification for ..` block: sets [`Notification::CATEGORY`],
/// [`Notification::DIGEST`] and [`Notification::URGENT`] (see
/// `navree_macros::notification`).
pub use navree_macros::notification;
pub use notifiable::Notifiable;
pub use notification::Notification;
pub use notifier::{INBOX_TOPIC, NOTIFICATION_CREATED, Notifier};
pub use opt_outs::{NotificationOptOut, OptOutSource, OptOuts};
pub use options::{
    ApnsOptions, DeferredOptions, DigestOptions, FcmOptions, MetaOptions, NotifyOptions,
    PreferenceOptions, PushOptions, SlackOptions, SmsDriverKind, SmsOptions, SnsOptions,
    TeamsOptions, TwilioOptions, VonageOptions, WebPushOptions, WhatsAppDriverKind,
    WhatsAppOptions,
};
pub use overrides::{MailOverride, NotificationOverrides, TemplateOverrides};
pub use preferences::{
    CategoryPreferences, MAX_CATEGORIES, MAX_CATEGORY_LEN, NotificationPreference,
    NotificationPreferences, PreferenceOwner, Preferences, ResolvedPreferences,
    WORKSPACE_OWNER_TYPE, check_category,
};
pub use push::{NewPushSubscription, PushKeys, PushSubscription, PushSubscriptions};
pub use recipient::RecipientScope;
pub use stored::{DatabaseNotification, NotificationList};
pub use timing::{Digest, QuietHours};
pub use tokens::ConnectionTokens;

/// The notifiable type of users: the `/api/notifications` endpoints serve
/// the signed-in user's notifications stored under this type.
pub const USER_NOTIFIABLE_TYPE: &str = "user";
