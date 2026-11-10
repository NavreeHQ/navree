//! [`Channel`]: how a notification reaches its recipient.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A delivery channel, listed by [`Notification::via`](crate::Notification::via).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Channel {
    /// An email rendered from [`Notification::to_mail`](crate::Notification::to_mail),
    /// sent with navree-mail's `Mailer`.
    Mail,
    /// A row in the `notifications` table, with read state, built from
    /// [`Notification::to_database`](crate::Notification::to_database).
    Database,
    /// A `notification.created` event on the notifiable's real-time channel,
    /// through the app's `dyn Broadcaster`.
    Broadcast,
    /// A Slack message from [`Notification::to_slack`](crate::Notification::to_slack),
    /// posted to an incoming webhook or with a bot token.
    Slack,
    /// A Microsoft Teams message from
    /// [`Notification::to_teams`](crate::Notification::to_teams), posted to
    /// an incoming webhook or as a proactive bot message.
    #[serde(rename = "teams")]
    MicrosoftTeams,
    /// A text message from [`Notification::to_sms`](crate::Notification::to_sms),
    /// sent through Twilio, Vonage or Amazon SNS.
    Sms,
    /// A WhatsApp message from
    /// [`Notification::to_whatsapp`](crate::Notification::to_whatsapp), sent
    /// through Twilio or the WhatsApp Cloud API.
    #[serde(rename = "whatsapp")]
    WhatsApp,
    /// A browser push notification from
    /// [`Notification::to_web_push`](crate::Notification::to_web_push), sent
    /// to each of the notifiable's push subscriptions.
    WebPush,
}

impl Channel {
    /// Every channel, in declaration order.
    pub const ALL: [Channel; 8] = [
        Channel::Mail,
        Channel::Database,
        Channel::Broadcast,
        Channel::Slack,
        Channel::MicrosoftTeams,
        Channel::Sms,
        Channel::WhatsApp,
        Channel::WebPush,
    ];

    /// `mail`, `database`, `broadcast`, `slack`, `teams`, `sms`, `whatsapp`
    /// or `web_push`.
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Mail => "mail",
            Channel::Database => "database",
            Channel::Broadcast => "broadcast",
            Channel::Slack => "slack",
            Channel::MicrosoftTeams => "teams",
            Channel::Sms => "sms",
            Channel::WhatsApp => "whatsapp",
            Channel::WebPush => "web_push",
        }
    }

    /// Whether deliveries over this channel are recorded in
    /// `notification_deliveries`: Slack, Teams, SMS, WhatsApp and web push.
    /// Mail keeps its own delivery log, and the database and broadcast
    /// channels deliver inside the app.
    pub fn is_recorded(self) -> bool {
        matches!(
            self,
            Channel::Slack
                | Channel::MicrosoftTeams
                | Channel::Sms
                | Channel::WhatsApp
                | Channel::WebPush
        )
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Channel::ALL
            .into_iter()
            .find(|channel| channel.as_str() == s)
            .ok_or_else(|| format!("unknown notification channel `{s}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_through_strings_and_serde() {
        for channel in Channel::ALL {
            assert_eq!(channel.as_str().parse::<Channel>(), Ok(channel));
            let json = serde_json::to_value(channel).unwrap();
            assert_eq!(json, channel.as_str());
            assert_eq!(serde_json::from_value::<Channel>(json).unwrap(), channel);
        }
        assert!("pigeon".parse::<Channel>().is_err());
    }

    #[test]
    fn only_the_external_channels_are_recorded() {
        let recorded: Vec<&str> = Channel::ALL
            .into_iter()
            .filter(|c| c.is_recorded())
            .map(Channel::as_str)
            .collect();
        assert_eq!(recorded, ["slack", "teams", "sms", "whatsapp", "web_push"]);
    }
}
