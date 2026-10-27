//! [`SendNotificationJob`]: one rendered notification on one channel.

use chrono::{DateTime, Utc};
use navree_contracts::{BoxError, Job, JobContext};
use navree_mail::TemplateMail;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    Channel, Digest, DigestItem, Notifier, SlackMessage, SlackRoute, SmsMessage, TeamsMessage,
    TeamsRoute, WebPushMessage, WhatsAppMessage,
};

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
    /// [`Notification::CATEGORY`](crate::Notification::CATEGORY) (`None` in
    /// jobs queued before categories existed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The workspace the notification was sent in
    /// ([`Notifiable::workspace_id`](crate::Notifiable::workspace_id)), whose
    /// mail transport and template overrides its mail uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Uuid>,
    /// What to deliver, and where.
    pub delivery: Delivery,
}

/// A notification rendered for one channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "via", rename_all = "snake_case")]
#[non_exhaustive]
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
    /// A Slack message.
    Slack {
        /// Where it goes.
        route: SlackRoute,
        /// The message.
        message: SlackMessage,
    },
    /// A Microsoft Teams message.
    #[serde(rename = "teams")]
    MicrosoftTeams {
        /// Where it goes.
        route: TeamsRoute,
        /// The message.
        message: TeamsMessage,
    },
    /// A text message.
    Sms {
        /// The number, as the notification or notifiable gave it (read as
        /// E.164 when delivered).
        to: String,
        /// The message.
        message: SmsMessage,
    },
    /// A WhatsApp message.
    #[serde(rename = "whatsapp")]
    WhatsApp {
        /// The number, as given (read as E.164 when delivered).
        to: String,
        /// The message.
        message: WhatsAppMessage,
    },
    /// A browser push notification, to every push subscription the
    /// notifiable has when it is delivered.
    WebPush {
        /// The notification.
        message: WebPushMessage,
    },
    /// One digest mail batching several notifications' mails (over the
    /// mail channel).
    Digest {
        /// How often the recipient gets one.
        digest: Digest,
        /// The recipients.
        to: Vec<String>,
        /// The mails it batches, oldest first.
        items: Vec<DigestItem>,
    },
}

impl Delivery {
    /// The channel this delivery goes over.
    pub fn channel(&self) -> Channel {
        match self {
            Delivery::Mail { .. } | Delivery::Digest { .. } => Channel::Mail,
            Delivery::Database { .. } => Channel::Database,
            Delivery::Broadcast { .. } => Channel::Broadcast,
            Delivery::Slack { .. } => Channel::Slack,
            Delivery::MicrosoftTeams { .. } => Channel::MicrosoftTeams,
            Delivery::Sms { .. } => Channel::Sms,
            Delivery::WhatsApp { .. } => Channel::WhatsApp,
            Delivery::WebPush { .. } => Channel::WebPush,
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
