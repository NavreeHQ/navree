//! [`Notification`]: one message, many channels.

use navree_mail::TemplateMail;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    Channel, Notifiable, SlackMessage, SmsMessage, TeamsMessage, WebPushMessage, WhatsAppMessage,
};

/// A notification: it picks its channels per
/// recipient with [`via`](Notification::via) and renders itself for each
/// one. Send it with [`Notifier::send`](crate::Notifier::send).
///
/// Every `to_*` method may return `None` to skip that channel for this
/// recipient. [`to_database`](Notification::to_database) defaults to the
/// notification serialized as JSON, and
/// [`to_broadcast`](Notification::to_broadcast) to the database data.
///
/// ```
/// use navree_mail::TemplateMail;
/// use navree_notify::{Channel, Notifiable, Notification};
/// use serde::{Deserialize, Serialize};
/// use serde_json::{Value, json};
///
/// #[derive(Serialize, Deserialize)]
/// struct DealAssigned {
///     deal_id: String,
///     title: String,
/// }
///
/// impl Notification for DealAssigned {
///     const TYPE: &'static str = "deal_assigned";
///
///     fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
///         vec![Channel::Database, Channel::Broadcast, Channel::Mail]
///     }
///
///     fn to_mail(&self, _n: &dyn Notifiable) -> Option<TemplateMail> {
///         // No recipient: the notifiable's `route_mail` is used.
///         Some(TemplateMail {
///             subject: format!("Deal assigned: {}", self.title),
///             template: "mail/deal-assigned".into(),
///             context: json!({ "title": self.title }),
///             ..TemplateMail::default()
///         })
///     }
///
///     fn to_database(&self, _n: &dyn Notifiable) -> Option<Value> {
///         Some(json!({
///             "title": "Deal assigned",
///             "message": format!("{} is now yours", self.title),
///             "url": format!("/deals/{}", self.deal_id),
///             "deal_id": self.deal_id,
///         }))
///     }
/// }
/// ```
pub trait Notification: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The stable type name stored with database notifications and sent
    /// with broadcasts, e.g. `deal_assigned`.
    const TYPE: &'static str;

    /// The channels to deliver over for `notifiable`, in delivery order
    /// (duplicates are ignored).
    fn via(&self, notifiable: &dyn Notifiable) -> Vec<Channel>;

    /// The mail for the [`Channel::Mail`] channel. A mail without
    /// recipients goes to [`Notifiable::route_mail`] (and is skipped when
    /// that is `None`); the sender defaults to `mail.from`. Default: `None`
    /// (no mail).
    fn to_mail(&self, notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        let _ = notifiable;
        None
    }

    /// The `data` stored by the [`Channel::Database`] channel. Default: the
    /// notification serialized as JSON.
    fn to_database(&self, notifiable: &dyn Notifiable) -> Option<Value> {
        let _ = notifiable;
        serde_json::to_value(self).ok()
    }

    /// The `data` sent by the [`Channel::Broadcast`] channel. Default:
    /// [`to_database`](Notification::to_database).
    fn to_broadcast(&self, notifiable: &dyn Notifiable) -> Option<Value> {
        self.to_database(notifiable)
    }

    /// The message of the [`Channel::Slack`] channel. It goes to its own
    /// [`to`](SlackMessage::to) route, else [`Notifiable::route_slack`].
    /// Default: `None`.
    fn to_slack(&self, notifiable: &dyn Notifiable) -> Option<SlackMessage> {
        let _ = notifiable;
        None
    }

    /// The message of the [`Channel::MicrosoftTeams`] channel. It goes to
    /// its own [`to`](TeamsMessage::to) route, else
    /// [`Notifiable::route_teams`]. Default: `None`.
    fn to_teams(&self, notifiable: &dyn Notifiable) -> Option<TeamsMessage> {
        let _ = notifiable;
        None
    }

    /// The text of the [`Channel::Sms`] channel. It goes to its own
    /// [`to`](SmsMessage::to) number, else [`Notifiable::route_sms`].
    /// Default: `None`.
    fn to_sms(&self, notifiable: &dyn Notifiable) -> Option<SmsMessage> {
        let _ = notifiable;
        None
    }

    /// The message of the [`Channel::WhatsApp`] channel. It goes to its own
    /// [`to`](WhatsAppMessage::to) number, else
    /// [`Notifiable::route_whatsapp`]. Default: `None`.
    fn to_whatsapp(&self, notifiable: &dyn Notifiable) -> Option<WhatsAppMessage> {
        let _ = notifiable;
        None
    }

    /// The notification of the [`Channel::WebPush`] channel, pushed to each
    /// of the notifiable's push subscriptions. Default: `None`.
    fn to_web_push(&self, notifiable: &dyn Notifiable) -> Option<WebPushMessage> {
        let _ = notifiable;
        None
    }
}
