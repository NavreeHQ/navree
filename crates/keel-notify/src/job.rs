//! [`SendNotificationJob`]: one rendered notification on one channel.

use chrono::{DateTime, Utc};
use keel_contracts::{BoxError, Job, JobContext};
use keel_mail::TemplateMail;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{Channel, Notifier};

/// Job `notify.send`: delivers one notification over one channel.
///
/// [`Notifier::send`] renders the notification for the notifiable when it is
/// sent, then (with [`NotifyOptions::queue`](crate::NotifyOptions::queue)
/// on) dispatches one job per channel, so each channel is retried on its
/// own. [`NotifyModule`](crate::NotifyModule) registers the job with the
/// queue. The payload holds the rendered output, not the notification, so
/// no notification type needs registering.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SendNotificationJob {
    /// The notification id, shared by its channels (the database row id).
    pub id: Uuid,
    /// [`Notification::TYPE`](crate::Notification::TYPE).
    #[serde(rename = "type")]
    pub notification_type: String,
    /// [`Notifiable::notifiable_type`](crate::Notifiable::notifiable_type).
    pub notifiable_type: String,
    /// [`Notifiable::notifiable_id`](crate::Notifiable::notifiable_id).
    pub notifiable_id: String,
    /// When the notification was sent.
    pub created_at: DateTime<Utc>,
    /// What to deliver, and where.
    pub delivery: Delivery,
}

/// A notification rendered for one channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "via", rename_all = "snake_case")]
pub enum Delivery {
    /// A mail with its recipients filled in.
    Mail {
        /// The mail, rendered by the mailer when delivered.
        mail: TemplateMail,
    },
    /// A `notifications` row.
    Database {
        /// The row's `data`.
        data: Value,
    },
    /// A `notification.created` event on a real-time channel.
    Broadcast {
        /// The real-time channel, e.g. `private-users.{id}`.
        channel: String,
        /// The notification's `data` in the event.
        data: Value,
    },
}

impl Delivery {
    /// The channel this delivery goes over.
    pub fn channel(&self) -> Channel {
        match self {
            Delivery::Mail { .. } => Channel::Mail,
            Delivery::Database { .. } => Channel::Database,
            Delivery::Broadcast { .. } => Channel::Broadcast,
        }
    }
}

impl Job for SendNotificationJob {
    const NAME: &'static str = "notify.send";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let notifier = ctx.app.get::<Notifier>().await?;
        notifier.deliver(&self).await?;
        Ok(())
    }
}
