//! [`Notifiable`]: who receives notifications.

use uuid::Uuid;

use crate::{SlackRoute, TeamsRoute};

/// Something that receives notifications, usually a
/// user: it names the stored notifications' owner and routes the mail,
/// broadcast, Slack, Teams, SMS and WhatsApp channels. The web push channel
/// sends to the notifiable's saved push subscriptions.
///
/// ```
/// use navree_notify::Notifiable;
///
/// struct User {
///     id: uuid::Uuid,
///     email: String,
/// }
///
/// impl Notifiable for User {
///     fn notifiable_type(&self) -> &'static str {
///         navree_notify::USER_NOTIFIABLE_TYPE
///     }
///
///     fn notifiable_id(&self) -> String {
///         self.id.to_string()
///     }
///
///     fn route_mail(&self) -> Option<String> {
///         Some(self.email.clone())
///     }
/// }
/// ```
pub trait Notifiable: Send + Sync {
    /// The owner type stored with database notifications (`user`). The
    /// `/api/notifications` endpoints serve the notifications of type
    /// [`USER_NOTIFIABLE_TYPE`](crate::USER_NOTIFIABLE_TYPE) for the
    /// signed-in user.
    fn notifiable_type(&self) -> &'static str;

    /// The owner id stored with database notifications (for users, the
    /// UUID as a string).
    fn notifiable_id(&self) -> String;

    /// The address the mail channel sends to when the notification's mail
    /// names no recipient; `None` skips the mail channel.
    fn route_mail(&self) -> Option<String>;

    /// The real-time channel the broadcast channel publishes on; `None`
    /// skips the broadcast channel. Default: `private-users.{id}`.
    fn broadcast_channel(&self) -> Option<String> {
        Some(format!("private-users.{}", self.notifiable_id()))
    }

    /// Where the Slack channel posts when the message names no route; `None`
    /// (the default) skips it.
    fn route_slack(&self) -> Option<SlackRoute> {
        None
    }

    /// Where the Microsoft Teams channel posts when the message names no
    /// route; `None` (the default) skips it.
    fn route_teams(&self) -> Option<TeamsRoute> {
        None
    }

    /// The phone number the SMS channel texts (any format: it is read as
    /// E.164 in `notify.sms.default_region`); `None` (the default) skips it.
    fn route_sms(&self) -> Option<String> {
        None
    }

    /// The WhatsApp number the WhatsApp channel sends to; `None` (the
    /// default) skips it.
    fn route_whatsapp(&self) -> Option<String> {
        None
    }

    /// The workspace the notification is sent in: its
    /// [preferences](crate::Preferences) are the defaults under the
    /// notifiable's own, its zone stands in when the notifiable has none,
    /// its mail goes through the workspace's mail transport, with its
    /// [template overrides](crate::TemplateOverrides). Default: `None`, and
    /// then the workspace of the current unit of work (a workspace
    /// request's).
    fn workspace_id(&self) -> Option<Uuid> {
        None
    }

    /// The IANA time zone (`Asia/Kolkata`) quiet hours, send windows and
    /// digests are counted in. Default: `None`, and then the zone
    /// `TimeModule` gives the user (for notifiable type
    /// [`USER_NOTIFIABLE_TYPE`](crate::USER_NOTIFIABLE_TYPE)) or the
    /// workspace, else `time.default_zone`, else UTC.
    fn time_zone(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct User {
        id: u32,
    }

    impl Notifiable for User {
        fn notifiable_type(&self) -> &'static str {
            "user"
        }

        fn notifiable_id(&self) -> String {
            self.id.to_string()
        }

        fn route_mail(&self) -> Option<String> {
            None
        }
    }

    #[test]
    fn broadcast_channel_defaults_to_the_private_user_channel() {
        assert_eq!(
            User { id: 7 }.broadcast_channel().as_deref(),
            Some("private-users.7")
        );
    }
}
