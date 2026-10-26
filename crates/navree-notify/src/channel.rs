//! [`Channel`]: how a notification reaches its recipient.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A delivery channel, listed by [`Notification::via`](crate::Notification::via).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
}

impl Channel {
    /// `mail`, `database` or `broadcast`.
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Mail => "mail",
            Channel::Database => "database",
            Channel::Broadcast => "broadcast",
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
