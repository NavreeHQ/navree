//! [`Channel`]: how a notification reaches its recipient.

use std::fmt;
use std::str::FromStr;

use navree_config::__private::{lookup_in, selector_error, selector_name};
use navree_config::DriverName;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::registered::CHANNEL_SLOT;

/// A delivery channel, listed by [`Notification::via`](crate::Notification::via):
/// a built-in channel, or one a module adds with
/// [`m.notification_channel`](crate::ModuleBuilderNotifyChannelsExt::notification_channel),
/// named with [`Channel::named`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// an incoming webhook or as a proactive bot message (`"teams"`).
    MicrosoftTeams,
    /// A text message from [`Notification::to_sms`](crate::Notification::to_sms),
    /// sent through Twilio, Vonage or Amazon SNS.
    Sms,
    /// A WhatsApp message from
    /// [`Notification::to_whatsapp`](crate::Notification::to_whatsapp), sent
    /// through Twilio or the WhatsApp Cloud API (`"whatsapp"`).
    WhatsApp,
    /// A browser push notification from
    /// [`Notification::to_web_push`](crate::Notification::to_web_push), sent
    /// to each of the notifiable's push subscriptions.
    WebPush,
    /// A notification on a native app (iOS through Apple's push service,
    /// Android through Firebase Cloud Messaging) from
    /// [`Notification::to_push`](crate::Notification::to_push), sent to
    /// each of the notifiable's [push devices](crate::PushDevices).
    Push,
    /// A channel a module adds with
    /// [`m.notification_channel`](crate::ModuleBuilderNotifyChannelsExt::notification_channel),
    /// by its name: its message comes from
    /// [`Notification::to_channel`](crate::Notification::to_channel) and its
    /// address from [`Notifiable::route_channel`](crate::Notifiable::route_channel).
    /// Write it with [`Channel::named`], which never puts a built-in
    /// channel's name here.
    Registered(DriverName),
}

impl Channel {
    /// The channels of the first release, in declaration order: every
    /// channel but [`Channel::Push`]. [`Channel::EVERY`] lists them all.
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

    /// Every built-in channel, in declaration order.
    pub const EVERY: &'static [Channel] = &[
        Channel::Mail,
        Channel::Database,
        Channel::Broadcast,
        Channel::Slack,
        Channel::MicrosoftTeams,
        Channel::Sms,
        Channel::WhatsApp,
        Channel::WebPush,
        Channel::Push,
    ];

    /// `mail`, `database`, `broadcast`, `slack`, `teams`, `sms`, `whatsapp`,
    /// `web_push`, `push`, or the name of a channel a module adds.
    pub fn as_str(self) -> &'static str {
        self.name_text()
    }

    /// Whether deliveries over this channel are recorded in
    /// `notification_deliveries`: Slack, Teams, SMS, WhatsApp, web push,
    /// push and every channel a module adds. Mail keeps its own delivery
    /// log, and the database and broadcast channels deliver inside the app.
    pub fn is_recorded(self) -> bool {
        matches!(
            self,
            Channel::Slack
                | Channel::MicrosoftTeams
                | Channel::Sms
                | Channel::WhatsApp
                | Channel::WebPush
                | Channel::Push
                | Channel::Registered(_)
        )
    }

    /// The channel called `name`: a built-in channel's name (`mail`,
    /// `database`, `broadcast`, `slack`, `teams`, `sms`, `whatsapp`,
    /// `web_push`, `push`) gives that channel, any other the channel a
    /// module adds under it with
    /// [`m.notification_channel`](crate::ModuleBuilderNotifyChannelsExt::notification_channel).
    ///
    /// The name must be a lower-case letter followed by up to 63 lower-case
    /// letters, digits or `_`; another one panics. Declare a channel as a
    /// `const`, where a bad name fails the build instead.
    ///
    /// ```
    /// use navree_notify::Channel;
    ///
    /// const TELEGRAM: Channel = Channel::named("telegram");
    /// assert_eq!(TELEGRAM.as_str(), "telegram");
    /// assert_eq!(Channel::named("mail"), Channel::Mail);
    /// assert_eq!(Channel::named("teams"), Channel::MicrosoftTeams);
    /// ```
    pub const fn named(name: &'static str) -> Channel {
        match built_in(name) {
            Some(channel) => channel,
            None => Channel::Registered(DriverName::from_static(name)),
        }
    }

    /// The channel called `name`, read from outside the code (a request, a
    /// stored row, a queued payload): a built-in channel, else a name the
    /// process already knows as a channel a module adds; `None` otherwise,
    /// also for the name of a driver of another kind (a cache store, a mail
    /// transport). It never adds a name, so made-up names cannot pile up.
    pub fn from_name(name: &str) -> Option<Channel> {
        match built_in(name) {
            Some(channel) => Some(channel),
            None => lookup_in(CHANNEL_SLOT.key(), name).map(Channel::Registered),
        }
    }

    /// The channel, with a `Registered` built-in channel's name turned into
    /// that channel (`Registered("mail")` is `Mail`).
    pub(crate) fn resolved(self) -> Channel {
        match self {
            Channel::Registered(name) => built_in(name.as_str()).unwrap_or(self),
            other => other,
        }
    }
}

/// The built-in channel called `name`, compared byte by byte (as a
/// `const fn` can).
const fn built_in(name: &str) -> Option<Channel> {
    let mut i = 0;
    while i < Channel::EVERY.len() {
        let channel = Channel::EVERY[i];
        if same(name, channel.name_text()) {
            return Some(channel);
        }
        i += 1;
    }
    None
}

impl Channel {
    /// [`as_str`](Channel::as_str), as a `const fn`.
    const fn name_text(self) -> &'static str {
        match self {
            Channel::Mail => "mail",
            Channel::Database => "database",
            Channel::Broadcast => "broadcast",
            Channel::Slack => "slack",
            Channel::MicrosoftTeams => "teams",
            Channel::Sms => "sms",
            Channel::WhatsApp => "whatsapp",
            Channel::WebPush => "web_push",
            Channel::Push => "push",
            Channel::Registered(name) => name.as_str(),
        }
    }
}

/// Whether `a` and `b` are the same text.
const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Only the built-in channels: a channel a module adds is read with
/// [`Channel::from_name`].
impl FromStr for Channel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Channel::EVERY
            .iter()
            .copied()
            .find(|channel| channel.as_str() == s)
            .ok_or_else(|| format!("unknown notification channel `{s}`"))
    }
}

/// The channel's name, [`as_str`](Channel::as_str).
impl Serialize for Channel {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// A built-in channel's name (also written as a table whose only key is
/// the name), or the name of a channel a module adds that the process
/// already knows ([`Channel::from_name`]): reading one never adds a name.
/// Another name is the error ``unknown notification channel `{name}` ``; a
/// text that is not a name at all is the error that lists the built-in
/// channels, which never repeats it.
impl<'de> Deserialize<'de> for Channel {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = selector_name(deserializer, "channel", &CHANNEL_SLOT)?;
        match Channel::from_name(&name) {
            Some(channel) => Ok(channel),
            None if DriverName::is_valid(&name) => Err(de::Error::custom(format!(
                "unknown notification channel `{name}`"
            ))),
            None => Err(de::Error::custom(selector_error(
                "channel",
                &CHANNEL_SLOT,
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use navree_config::__private::register_in;

    use super::*;

    #[test]
    fn names_round_trip_through_strings_and_serde() {
        for &channel in Channel::EVERY {
            assert_eq!(channel.as_str().parse::<Channel>(), Ok(channel));
            let json = serde_json::to_value(channel).unwrap();
            assert_eq!(json, channel.as_str());
            assert_eq!(serde_json::from_value::<Channel>(json).unwrap(), channel);
        }
        assert!("pigeon".parse::<Channel>().is_err());
    }

    #[test]
    fn only_the_external_channels_are_recorded() {
        let recorded: Vec<&str> = Channel::EVERY
            .iter()
            .filter(|c| c.is_recorded())
            .map(|c| c.as_str())
            .collect();
        assert_eq!(
            recorded,
            ["slack", "teams", "sms", "whatsapp", "web_push", "push"]
        );
    }

    #[test]
    fn all_keeps_the_channels_of_the_first_release() {
        assert_eq!(Channel::EVERY[..8], Channel::ALL);
        assert_eq!(Channel::EVERY.last(), Some(&Channel::Push));
    }

    #[test]
    fn channels_round_trip_and_from_str_stays_strict() {
        let doot = register_in(CHANNEL_SLOT.key(), "doot").unwrap();
        for (channel, name) in [
            (Channel::MicrosoftTeams, "teams"),
            (Channel::WhatsApp, "whatsapp"),
            (Channel::WebPush, "web_push"),
        ] {
            assert_eq!(serde_json::to_value(channel).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<Channel>(serde_json::json!(name)).unwrap(),
                channel
            );
        }

        // A channel a module adds round-trips by its name.
        let registered = Channel::named("doot");
        assert_eq!(registered, Channel::Registered(doot));
        assert_eq!(serde_json::to_value(registered).unwrap(), "doot");
        assert_eq!(
            serde_json::from_value::<Channel>(serde_json::json!("doot")).unwrap(),
            registered
        );
        assert!(registered.is_recorded());
        assert_eq!(registered.to_string(), "doot");

        // `FromStr` reads the built-in channels only.
        assert!("pigeon".parse::<Channel>().is_err());
        assert!("doot".parse::<Channel>().is_err());

        // `from_name` only finds names the process knows, and adds none.
        assert_eq!(Channel::from_name("doot"), Some(registered));
        assert_eq!(Channel::from_name("kabutar_9a"), None);
        let error = serde_json::from_value::<Channel>(serde_json::json!("kabutar_9a"))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "unknown notification channel `kabutar_9a`");
        assert_eq!(DriverName::lookup("kabutar_9a"), None);
        assert_eq!(Channel::EVERY.len(), 9);

        // A text that is not a name is never said back.
        let error = serde_json::from_value::<Channel>(serde_json::json!("Kabutar Old!"))
            .unwrap_err()
            .to_string();
        assert_eq!(error, selector_error("channel", &CHANNEL_SLOT));
        assert!(!error.contains("Kabutar"), "{error}");

        // A driver of another kind is no channel: its name reads as one
        // nobody registered, with the same text.
        register_in("cache.store", "sanduk_cache").unwrap();
        DriverName::register("memory").unwrap();
        for name in ["sanduk_cache", "memory"] {
            assert_eq!(Channel::from_name(name), None, "{name}");
            let error = serde_json::from_value::<Channel>(serde_json::json!(name))
                .unwrap_err()
                .to_string();
            assert_eq!(error, format!("unknown notification channel `{name}`"));
        }

        // A built-in channel's name is that channel, however it is written.
        for &channel in Channel::EVERY {
            let name = channel.as_str();
            assert_eq!(Channel::from_name(name), Some(channel), "{name}");
            assert_eq!(serde_json::to_value(channel).unwrap(), name);
        }
        assert_eq!(Channel::named("mail"), Channel::Mail);
        assert_eq!(Channel::named("teams"), Channel::MicrosoftTeams);
        assert_eq!(Channel::named("whatsapp"), Channel::WhatsApp);
        assert_eq!(Channel::from_name("web_push"), Some(Channel::WebPush));
        assert_eq!(
            serde_json::from_value::<Channel>(
                serde_json::to_value(Channel::named("mail")).unwrap()
            )
            .unwrap(),
            Channel::Mail
        );
        assert_eq!(
            Channel::Registered(DriverName::from_static("sms")).resolved(),
            Channel::Sms
        );
    }

    #[test]
    fn a_channel_written_as_a_one_key_table_reads_as_before() {
        register_in(CHANNEL_SLOT.key(), "doot").unwrap();
        assert_eq!(
            serde_json::from_value::<Channel>(serde_json::json!({"teams": null})).unwrap(),
            Channel::MicrosoftTeams
        );
        assert_eq!(
            serde_json::from_value::<Channel>(serde_json::json!({"mail": {}})).unwrap(),
            Channel::Mail
        );

        // A table never names a channel a module adds, even a known one.
        let error = serde_json::from_value::<Channel>(serde_json::json!({"doot": null}))
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "the channel must be `mail`, `database`, `broadcast`, `slack`, `teams`, `sms`, `whatsapp`, `web_push`, `push` or the name of a notification channel registered with m.notification_channel (a lower-case letter, then lower-case letters, digits or _)"
        );
        assert!(!error.contains("doot"), "{error}");
    }
}
