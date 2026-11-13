//! What a notification renders for the Slack, Teams, SMS, WhatsApp, web
//! push and push channels, and where those messages go.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// Where a Slack message goes: an incoming webhook, or a channel posted to
/// with a bot token (`chat.postMessage`).
///
/// ```
/// use navree_notify::SlackRoute;
///
/// let webhook = SlackRoute::webhook("https://hooks.slack.com/services/T000/B000/XXXX");
/// let channel = SlackRoute::channel("C0123456789"); // notify.slack.bot_token
/// let installed = SlackRoute::channel("C0123456789").connection(uuid::Uuid::nil());
/// # let _ = (webhook, channel, installed);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SlackRoute {
    /// An incoming webhook URL (`https://hooks.slack.com/services/...`).
    Webhook {
        /// The webhook URL. It is a secret: it is never recorded.
        url: String,
    },
    /// A channel id (`C0123456789`), or a user id for a direct message,
    /// posted with `chat.postMessage`.
    Bot {
        /// The channel or user id.
        channel: String,
        /// The integrations connection whose bot token posts it (an app's
        /// Slack install, through the app's `dyn ConnectionTokens`); `None`
        /// uses `notify.slack.bot_token`.
        connection: Option<Uuid>,
    },
}

impl SlackRoute {
    /// An incoming webhook.
    pub fn webhook(url: impl Into<String>) -> Self {
        SlackRoute::Webhook { url: url.into() }
    }

    /// A channel or user posted to with `notify.slack.bot_token`.
    pub fn channel(channel: impl Into<String>) -> Self {
        SlackRoute::Bot {
            channel: channel.into(),
            connection: None,
        }
    }

    /// The same channel, posted with the bot token of integrations
    /// connection `id`. A webhook route is returned unchanged.
    #[must_use]
    pub fn connection(self, id: Uuid) -> Self {
        match self {
            SlackRoute::Bot { channel, .. } => SlackRoute::Bot {
                channel,
                connection: Some(id),
            },
            webhook @ SlackRoute::Webhook { .. } => webhook,
        }
    }
}

/// A Slack message: `text` (shown in notifications and as the fallback),
/// optional Block Kit `blocks`, and a thread to reply in.
///
/// ```
/// use navree_notify::SlackMessage;
/// use serde_json::json;
///
/// let message = SlackMessage::new("Deal won: Agra renewal").blocks(json!([
///     { "type": "section", "text": { "type": "mrkdwn", "text": "*Agra renewal* was won" } }
/// ]));
/// assert_eq!(message.text, "Deal won: Agra renewal");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlackMessage {
    /// Where it goes instead of [`Notifiable::route_slack`](crate::Notifiable::route_slack).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<SlackRoute>,
    /// The text (`mrkdwn`), and the fallback of `blocks`.
    pub text: String,
    /// Block Kit blocks (a JSON array).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Value>,
    /// The `ts` of the message to reply to in its thread (bot routes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    /// Whether Slack unfurls the links in `text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfurl_links: Option<bool>,
}

impl SlackMessage {
    /// A message with `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// With Block Kit `blocks`.
    #[must_use]
    pub fn blocks(mut self, blocks: Value) -> Self {
        self.blocks = Some(blocks);
        self
    }

    /// As a reply in the thread of message `ts`.
    #[must_use]
    pub fn thread(mut self, ts: impl Into<String>) -> Self {
        self.thread_ts = Some(ts.into());
        self
    }

    /// To `route`, instead of the notifiable's.
    #[must_use]
    pub fn to(mut self, route: SlackRoute) -> Self {
        self.to = Some(route);
        self
    }

    /// The JSON body Slack receives (`chat.postMessage` adds `channel`).
    pub(crate) fn payload(&self) -> Value {
        let mut body = json!({ "text": self.text });
        if let Some(blocks) = &self.blocks {
            body["blocks"] = blocks.clone();
        }
        if let Some(ts) = &self.thread_ts {
            body["thread_ts"] = json!(ts);
        }
        if let Some(unfurl) = self.unfurl_links {
            body["unfurl_links"] = json!(unfurl);
        }
        body
    }
}

/// Where a Microsoft Teams message goes: an incoming webhook (a Teams
/// workflow's "post to a channel when a webhook request is received"), or
/// a conversation the app's bot is part of (a proactive message through
/// the Bot Framework).
///
/// ```
/// use navree_notify::TeamsRoute;
///
/// let webhook = TeamsRoute::webhook("https://prod-00.westus.logic.azure.com/workflows/abc/triggers/manual/paths/invoke?sig=x");
/// // Saved when the bot was installed for the user (`conversationUpdate`).
/// let bot = TeamsRoute::conversation("https://smba.trafficmanager.net/emea/", "a:1Xyz");
/// # let _ = (webhook, bot);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TeamsRoute {
    /// An incoming webhook URL.
    Webhook {
        /// The webhook URL. It is a secret: it is never recorded.
        url: String,
    },
    /// A conversation with the bot: a personal chat, a group chat or a
    /// channel.
    Bot {
        /// The conversation's `serviceUrl` (`https://smba.trafficmanager.net/...`).
        service_url: String,
        /// The conversation id.
        conversation_id: String,
        /// The integrations connection whose token the bot sends with
        /// (through the app's `dyn ConnectionTokens`); `None` uses the bot
        /// credentials in `notify.teams`.
        connection: Option<Uuid>,
    },
}

impl TeamsRoute {
    /// An incoming webhook.
    pub fn webhook(url: impl Into<String>) -> Self {
        TeamsRoute::Webhook { url: url.into() }
    }

    /// A conversation with the bot at `service_url`.
    pub fn conversation(
        service_url: impl Into<String>,
        conversation_id: impl Into<String>,
    ) -> Self {
        TeamsRoute::Bot {
            service_url: service_url.into(),
            conversation_id: conversation_id.into(),
            connection: None,
        }
    }

    /// The same conversation, sent with the token of integrations
    /// connection `id`. A webhook route is returned unchanged.
    #[must_use]
    pub fn connection(self, id: Uuid) -> Self {
        match self {
            TeamsRoute::Bot {
                service_url,
                conversation_id,
                ..
            } => TeamsRoute::Bot {
                service_url,
                conversation_id,
                connection: Some(id),
            },
            webhook @ TeamsRoute::Webhook { .. } => webhook,
        }
    }
}

/// A Microsoft Teams message: `text`, and optionally an Adaptive Card.
/// A webhook receives the card (a card holding `text` when there is none);
/// a bot conversation receives `text` with the card attached.
///
/// ```
/// use navree_notify::TeamsMessage;
/// use serde_json::json;
///
/// let message = TeamsMessage::new("Your training is due today").card(json!({
///     "type": "AdaptiveCard",
///     "version": "1.4",
///     "body": [{ "type": "TextBlock", "text": "Your training is due today", "wrap": true }],
///     "actions": [{ "type": "Action.OpenUrl", "title": "Start", "url": "https://app.acme.test/training" }]
/// }));
/// assert!(message.card.is_some());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TeamsMessage {
    /// Where it goes instead of [`Notifiable::route_teams`](crate::Notifiable::route_teams).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<TeamsRoute>,
    /// The text (Teams renders a subset of Markdown).
    pub text: String,
    /// An Adaptive Card (the card object, `"type": "AdaptiveCard"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<Value>,
    /// The summary shown in the activity feed (bot routes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// The content type of an Adaptive Card attachment.
pub(crate) const ADAPTIVE_CARD: &str = "application/vnd.microsoft.card.adaptive";

impl TeamsMessage {
    /// A message with `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// With an Adaptive Card.
    #[must_use]
    pub fn card(mut self, card: Value) -> Self {
        self.card = Some(card);
        self
    }

    /// With the activity feed summary.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// To `route`, instead of the notifiable's.
    #[must_use]
    pub fn to(mut self, route: TeamsRoute) -> Self {
        self.to = Some(route);
        self
    }

    /// The card a webhook receives: the message's, or one holding `text`.
    pub(crate) fn card_or_text(&self) -> Value {
        self.card.clone().unwrap_or_else(|| {
            json!({
                "type": "AdaptiveCard",
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "version": "1.4",
                "body": [{ "type": "TextBlock", "text": self.text, "wrap": true }],
            })
        })
    }

    /// The webhook body: a message with the card as its one attachment.
    pub(crate) fn webhook_payload(&self) -> Value {
        json!({
            "type": "message",
            "attachments": [{
                "contentType": ADAPTIVE_CARD,
                "contentUrl": null,
                "content": self.card_or_text(),
            }],
        })
    }

    /// The Bot Framework activity.
    pub(crate) fn activity(&self) -> Value {
        let mut activity = json!({ "type": "message", "text": self.text });
        if let Some(summary) = &self.summary {
            activity["summary"] = json!(summary);
        }
        if let Some(card) = &self.card {
            activity["attachments"] = json!([{ "contentType": ADAPTIVE_CARD, "content": card }]);
        }
        activity
    }
}

/// A text message. `to` and `from` default to the notifiable's
/// [`route_sms`](crate::Notifiable::route_sms) and `notify.sms.from`.
///
/// ```
/// use navree_notify::SmsMessage;
///
/// let sms = SmsMessage::new("Your code is 481 516").from("Agra");
/// assert_eq!(sms.body, "Your code is 481 516");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmsMessage {
    /// The recipient instead of the notifiable's (any format; it is read
    /// as E.164 in `notify.sms.default_region`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// The sender (a number in E.164, an alphanumeric sender id, or a
    /// Twilio messaging service `MG...`) instead of `notify.sms.from`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The text.
    pub body: String,
}

impl SmsMessage {
    /// A message with `body`.
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            ..Self::default()
        }
    }

    /// From `sender`.
    #[must_use]
    pub fn from(mut self, sender: impl Into<String>) -> Self {
        self.from = Some(sender.into());
        self
    }

    /// To `number`, instead of the notifiable's.
    #[must_use]
    pub fn to(mut self, number: impl Into<String>) -> Self {
        self.to = Some(number.into());
        self
    }
}

/// A WhatsApp message: free text (inside the 24-hour window after the
/// recipient last wrote), or an approved template (any time).
///
/// ```
/// use navree_notify::{WhatsAppMessage, WhatsAppTemplate};
///
/// let text = WhatsAppMessage::text("Your order has shipped");
/// let template = WhatsAppMessage::template(
///     WhatsAppTemplate::new("order_shipped", "en_US").parameter("A-1042"),
/// );
/// # let _ = (text, template);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WhatsAppMessage {
    /// The recipient instead of the notifiable's
    /// [`route_whatsapp`](crate::Notifiable::route_whatsapp).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// The sender instead of `notify.whatsapp.from` (Twilio).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// An approved template; it wins over `body`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<WhatsAppTemplate>,
}

impl WhatsAppMessage {
    /// A free-text message.
    pub fn text(body: impl Into<String>) -> Self {
        Self {
            body: Some(body.into()),
            ..Self::default()
        }
    }

    /// A template message.
    pub fn template(template: WhatsAppTemplate) -> Self {
        Self {
            template: Some(template),
            ..Self::default()
        }
    }

    /// To `number`, instead of the notifiable's.
    #[must_use]
    pub fn to(mut self, number: impl Into<String>) -> Self {
        self.to = Some(number.into());
        self
    }

    /// From `sender` (Twilio).
    #[must_use]
    pub fn from(mut self, sender: impl Into<String>) -> Self {
        self.from = Some(sender.into());
        self
    }
}

/// An approved WhatsApp template and the values of its body variables, in
/// order (`{{1}}`, `{{2}}`, ...).
///
/// `name` is the template's name for the WhatsApp Cloud API
/// (`order_shipped`) and its Content SID for Twilio (`HX...`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WhatsAppTemplate {
    /// The template name, or Twilio Content SID.
    pub name: String,
    /// The language code (`en_US`); Twilio's Content SID implies it.
    pub language: String,
    /// The body variables, in order.
    pub parameters: Vec<String>,
}

impl WhatsAppTemplate {
    /// Template `name` in `language`.
    pub fn new(name: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            language: language.into(),
            parameters: Vec::new(),
        }
    }

    /// With the next body variable.
    #[must_use]
    pub fn parameter(mut self, value: impl Into<String>) -> Self {
        self.parameters.push(value.into());
        self
    }
}

/// How soon a push service delivers a message to a sleeping device
/// (`Urgency` header, RFC 8030).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Urgency {
    /// Only when the device is on power and Wi-Fi.
    VeryLow,
    /// On power or Wi-Fi.
    Low,
    /// When the device is not low on battery.
    #[default]
    Normal,
    /// At once.
    High,
}

impl Urgency {
    /// `very-low`, `low`, `normal` or `high`.
    pub fn as_str(self) -> &'static str {
        match self {
            Urgency::VeryLow => "very-low",
            Urgency::Low => "low",
            Urgency::Normal => "normal",
            Urgency::High => "high",
        }
    }
}

/// A browser push notification. The service worker receives
/// [`payload`](WebPushMessage::payload), a JSON object with `title` and
/// the other fields that are set, and shows it with
/// `registration.showNotification(title, options)`.
///
/// ```
/// use navree_notify::{Urgency, WebPushMessage};
///
/// let push = WebPushMessage::new("Deal won")
///     .body("Agra renewal was won")
///     .url("/deals/42")
///     .urgency(Urgency::High);
/// assert_eq!(push.payload()["title"], "Deal won");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebPushMessage {
    /// The title.
    pub title: String,
    /// The body text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// An icon URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The URL to open when the notification is clicked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A tag: a new notification with the same tag replaces the old one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Anything else for the service worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// How long the push service keeps it for an offline device, in
    /// seconds, instead of `notify.web_push.ttl_secs`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u32>,
    /// The urgency (default: `normal`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency: Option<Urgency>,
    /// A topic: an undelivered message with the same topic is replaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

impl WebPushMessage {
    /// A notification titled `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// With body text.
    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// With an icon.
    #[must_use]
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Opening `url` when clicked.
    #[must_use]
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// With a tag.
    #[must_use]
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// With data for the service worker.
    #[must_use]
    pub fn data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Kept `secs` seconds for an offline device.
    #[must_use]
    pub fn ttl_secs(mut self, secs: u32) -> Self {
        self.ttl_secs = Some(secs);
        self
    }

    /// With an urgency.
    #[must_use]
    pub fn urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = Some(urgency);
        self
    }

    /// With a topic (at most 32 characters of the URL-safe base64
    /// alphabet).
    #[must_use]
    pub fn topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }

    /// What the service worker receives: `title` and each display field
    /// that is set (`body`, `icon`, `url`, `tag`, `data`).
    pub fn payload(&self) -> Value {
        let mut payload = Map::new();
        payload.insert("title".to_owned(), json!(self.title));
        let fields = [
            ("body", self.body.as_ref().map(|v| json!(v))),
            ("icon", self.icon.as_ref().map(|v| json!(v))),
            ("url", self.url.as_ref().map(|v| json!(v))),
            ("tag", self.tag.as_ref().map(|v| json!(v))),
            ("data", self.data.clone()),
        ];
        for (name, value) in fields {
            if let Some(value) = value {
                payload.insert(name.to_owned(), value);
            }
        }
        Value::Object(payload)
    }
}

/// The longest link a push notification carries, in bytes.
const MAX_HREF: usize = 2048;

/// The longest collapse key (Apple's `apns-collapse-id` limit), in bytes.
const MAX_COLLAPSE: usize = 64;

/// The data keys a [`PushMessage`] cannot use: what the message itself
/// writes there, and what Apple and Google keep for themselves.
const RESERVED_DATA_KEYS: [&str; 7] = [
    "aps",
    "href",
    "thread",
    "from",
    "notification",
    "message_type",
    "collapse_key",
];

/// A notification on a native app, sent over [`Channel::Push`](crate::Channel::Push)
/// to each of the recipient's [push devices](crate::PushDevices): Apple's
/// service for iOS, Firebase Cloud Messaging for Android.
///
/// It is an alert (a title, a body, a badge or a sound, shown by the
/// system), or [silent](PushMessage::silent) (nothing shown: the app is
/// woken to fetch what changed). `href` is the path inside the app the
/// client opens when the notification is tapped: it must be a path on the
/// app's own origin (`/deals/42?tab=notes`), never a URL, and a message
/// whose `href` is not one is refused before it is sent.
///
/// The text goes through Apple's or Google's servers and shows on a locked
/// screen: keep secrets (codes, balances, private messages) out of it, or
/// send a generic title with [`mutable_content`](PushMessage::mutable_content)
/// and let the app fetch the real text.
///
/// ```
/// use navree_notify::{PushMessage, Urgency};
///
/// let push = PushMessage::new("Deal won")
///     .body("Agra renewal was won by Gurpreet Kaur")
///     .href("/deals/42")
///     .badge(3)
///     .sound("default")
///     .thread("deals")
///     .collapse("deal-42")
///     .urgency(Urgency::High);
/// assert!(push.check().is_ok());
///
/// let refresh = PushMessage::silent().data("refresh", "inbox");
/// assert!(refresh.is_silent());
///
/// assert!(PushMessage::new("x").href("https://evil.example/").check().is_err());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PushMessage {
    /// The title (none for a silent push).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The body text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The path inside the app opened when it is tapped (`/deals/42`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    /// The number on the app's icon (Apple; Android shows a dot and counts
    /// it where the launcher does).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<u32>,
    /// A sound: `default`, or the name of a sound file in the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    /// The thread it is grouped in (Apple's `thread-id`; the `thread` data
    /// key on Android, where the app groups its notifications).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// A collapse key: a newer notification with the same key replaces
    /// this one, shown or still waiting (at most 64 bytes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapse: Option<String>,
    /// How long the service keeps it for an offline device, in seconds,
    /// instead of `notify.push.ttl_secs` (0: deliver now or never).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u32>,
    /// How soon it is delivered to a sleeping device. `high` is for
    /// what the person must see now; unset, an alert goes at once and a
    /// silent push when the device has power to spare.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency: Option<Urgency>,
    /// Lets the app's notification service extension change the content
    /// before it shows (Apple's `mutable-content`): send a generic text
    /// and have the app fetch the private one.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mutable_content: bool,
    /// Data for the app, as text (Firebase carries only text). The keys
    /// `aps`, `href`, `thread`, `from`, `notification`, `message_type`,
    /// `collapse_key` and those starting with `google` or `gcm` are
    /// reserved.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, String>,
}

impl PushMessage {
    /// An alert titled `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::default()
        }
    }

    /// A silent push: nothing is shown, the app is woken in the background
    /// with the [`data`](PushMessage::data) (Apple's `content-available`,
    /// a data message on Android). The system may delay or drop it.
    pub fn silent() -> Self {
        Self::default()
    }

    /// With body text.
    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Opening `path` (a path inside the app, `/deals/42`) when tapped.
    #[must_use]
    pub fn href(mut self, path: impl Into<String>) -> Self {
        self.href = Some(path.into());
        self
    }

    /// With the icon's badge set to `count`.
    #[must_use]
    pub fn badge(mut self, count: u32) -> Self {
        self.badge = Some(count);
        self
    }

    /// With a sound (`default`, or a sound file of the app).
    #[must_use]
    pub fn sound(mut self, sound: impl Into<String>) -> Self {
        self.sound = Some(sound.into());
        self
    }

    /// Grouped in thread `thread`.
    #[must_use]
    pub fn thread(mut self, thread: impl Into<String>) -> Self {
        self.thread = Some(thread.into());
        self
    }

    /// Replacing an earlier notification with the same `key`.
    #[must_use]
    pub fn collapse(mut self, key: impl Into<String>) -> Self {
        self.collapse = Some(key.into());
        self
    }

    /// Kept `secs` seconds for an offline device.
    #[must_use]
    pub fn ttl_secs(mut self, secs: u32) -> Self {
        self.ttl_secs = Some(secs);
        self
    }

    /// With an urgency.
    #[must_use]
    pub fn urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = Some(urgency);
        self
    }

    /// With `mutable-content`, for the app's notification service
    /// extension.
    #[must_use]
    pub fn mutable_content(mut self) -> Self {
        self.mutable_content = true;
        self
    }

    /// With the data `key` = `value` for the app.
    #[must_use]
    pub fn data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }

    /// Whether nothing is shown: no title, body, badge or sound.
    pub fn is_silent(&self) -> bool {
        self.title.is_none() && self.body.is_none() && self.badge.is_none() && self.sound.is_none()
    }

    /// Checks the message before it is sent: `href` is a path inside the
    /// app, `collapse` fits Apple's 64 bytes, `sound` names a file, and no
    /// data key is reserved. The error names the field.
    pub fn check(&self) -> Result<(), (&'static str, String)> {
        if let Some(href) = &self.href {
            check_href(href).map_err(|message| ("href", message))?;
        }
        if let Some(collapse) = &self.collapse
            && (collapse.is_empty() || collapse.len() > MAX_COLLAPSE)
        {
            return Err((
                "collapse",
                format!("collapse must be 1 to {MAX_COLLAPSE} bytes"),
            ));
        }
        if let Some(sound) = &self.sound
            && (sound.is_empty()
                || sound.len() > 255
                || sound.contains(['/', '\\'])
                || sound.chars().any(char::is_control))
        {
            return Err((
                "sound",
                "sound must be `default` or a sound file's name".to_owned(),
            ));
        }
        for key in self.data.keys() {
            let lower = key.to_ascii_lowercase();
            if key.is_empty()
                || RESERVED_DATA_KEYS.contains(&lower.as_str())
                || lower.starts_with("google")
                || lower.starts_with("gcm")
            {
                return Err(("data", format!("the data key `{key}` is reserved")));
            }
        }
        Ok(())
    }
}

/// Checks that `href` is a path on the app's own origin: it starts with
/// one `/`, names no scheme or host (`//host`, `/\host`, and their
/// percent-encoded forms), has no `..` segment and no control characters,
/// and is at most 2048 bytes. A query and a fragment are allowed.
pub(crate) fn check_href(href: &str) -> Result<(), String> {
    let refuse = || Err("href must be a path inside the app, such as `/deals/42`".to_owned());
    if href.is_empty() || href.len() > MAX_HREF || !href.starts_with('/') {
        return refuse();
    }
    if href
        .chars()
        .any(|c| c.is_control() || c == '\\' || c.is_whitespace())
    {
        return refuse();
    }
    // The path, decoded once: what a client would follow.
    let path = href.split(['?', '#']).next().unwrap_or_default();
    let decoded: String = percent_decode(path);
    if decoded.starts_with("//")
        || decoded.contains('\\')
        || decoded.chars().any(char::is_control)
        || decoded
            .split('/')
            .any(|segment| segment == ".." || segment == ".")
    {
        return refuse();
    }
    Ok(())
}

/// `text` with each `%XX` decoded once (invalid escapes are kept).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_href_must_be_a_path_inside_the_app() {
        for good in [
            "/",
            "/deals/42",
            "/deals/42?tab=notes&from=push",
            "/inbox#latest",
            "/search?q=%2F%2Fnot-a-host",
            "/files/a%20b",
        ] {
            assert!(check_href(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "deals/42",
            "https://evil.example/",
            "javascript:alert(1)",
            "//evil.example",
            "/\\evil.example",
            "/%2Fevil.example",
            "%2F%2Fevil.example",
            "/%2F%2Fevil.example",
            "/%5Cevil.example",
            "/deals/../admin",
            "/deals/%2E%2E/admin",
            "/deals/./42",
            "/deals\n/42",
            "/deals/%0A",
            "/deals 42",
        ] {
            assert!(check_href(bad).is_err(), "{bad:?}");
        }
        assert!(check_href(&format!("/{}", "a".repeat(MAX_HREF))).is_err());
    }

    #[test]
    fn push_messages_check_their_fields() {
        assert!(
            PushMessage::new("Deal won")
                .href("/deals/1")
                .check()
                .is_ok()
        );
        assert_eq!(
            PushMessage::new("x").href("//x").check().unwrap_err().0,
            "href"
        );
        assert_eq!(
            PushMessage::new("x")
                .collapse("c".repeat(65))
                .check()
                .unwrap_err()
                .0,
            "collapse"
        );
        assert_eq!(
            PushMessage::new("x").sound("../x").check().unwrap_err().0,
            "sound"
        );
        for key in ["aps", "href", "google.c.a", "GCM.x", "from", "collapse_key"] {
            assert_eq!(
                PushMessage::silent().data(key, "v").check().unwrap_err().0,
                "data",
                "{key}"
            );
        }
        assert!(PushMessage::silent().is_silent());
        assert!(!PushMessage::silent().badge(1).is_silent());
        assert!(
            PushMessage::silent()
                .data("refresh", "inbox")
                .check()
                .is_ok()
        );
    }

    #[test]
    fn slack_payloads_carry_only_what_is_set() {
        assert_eq!(SlackMessage::new("hi").payload(), json!({ "text": "hi" }));
        let full = SlackMessage::new("hi")
            .blocks(json!([{ "type": "divider" }]))
            .thread("1700000000.000100");
        assert_eq!(
            full.payload(),
            json!({
                "text": "hi",
                "blocks": [{ "type": "divider" }],
                "thread_ts": "1700000000.000100",
            })
        );
    }

    #[test]
    fn a_teams_webhook_gets_a_text_card_when_the_message_has_none() {
        let payload = TeamsMessage::new("Due today").webhook_payload();
        assert_eq!(payload["type"], "message");
        let attachment = &payload["attachments"][0];
        assert_eq!(attachment["contentType"], ADAPTIVE_CARD);
        assert_eq!(attachment["content"]["type"], "AdaptiveCard");
        assert_eq!(attachment["content"]["body"][0]["text"], "Due today");
    }

    #[test]
    fn a_teams_activity_attaches_the_card() {
        let card = json!({ "type": "AdaptiveCard", "version": "1.4", "body": [] });
        let activity = TeamsMessage::new("Due today")
            .summary("Training")
            .card(card.clone())
            .activity();
        assert_eq!(
            activity,
            json!({
                "type": "message",
                "text": "Due today",
                "summary": "Training",
                "attachments": [{ "contentType": ADAPTIVE_CARD, "content": card }],
            })
        );
        assert!(
            TeamsMessage::new("x")
                .activity()
                .get("attachments")
                .is_none()
        );
    }

    #[test]
    fn routes_take_a_connection_only_for_bots() {
        let id = Uuid::now_v7();
        assert_eq!(
            SlackRoute::channel("C1").connection(id),
            SlackRoute::Bot {
                channel: "C1".to_owned(),
                connection: Some(id)
            }
        );
        assert_eq!(
            SlackRoute::webhook("https://x").connection(id),
            SlackRoute::webhook("https://x")
        );
        assert!(matches!(
            TeamsRoute::conversation("https://smba", "a:1").connection(id),
            TeamsRoute::Bot { connection: Some(c), .. } if c == id
        ));
    }

    #[test]
    fn routes_serialize_with_a_type_tag() {
        assert_eq!(
            serde_json::to_value(SlackRoute::channel("C1")).unwrap(),
            json!({ "type": "bot", "channel": "C1", "connection": null })
        );
        assert_eq!(
            serde_json::to_value(TeamsRoute::webhook("https://x")).unwrap(),
            json!({ "type": "webhook", "url": "https://x" })
        );
    }

    #[test]
    fn push_payloads_hold_the_display_fields() {
        let push = WebPushMessage::new("Deal won")
            .body("Agra")
            .url("/deals/1")
            .ttl_secs(60)
            .urgency(Urgency::High)
            .topic("deals");
        assert_eq!(
            push.payload(),
            json!({ "title": "Deal won", "body": "Agra", "url": "/deals/1" })
        );
        assert_eq!(Urgency::VeryLow.as_str(), "very-low");
        assert_eq!(serde_json::to_value(Urgency::VeryLow).unwrap(), "very-low");
    }

    #[test]
    fn whatsapp_templates_collect_their_parameters() {
        let message =
            WhatsAppMessage::template(WhatsAppTemplate::new("order", "en_US").parameter("A-1"))
                .to("+15005550006");
        let template = message.template.unwrap();
        assert_eq!(template.parameters, ["A-1"]);
        assert_eq!(message.to.as_deref(), Some("+15005550006"));
    }
}
