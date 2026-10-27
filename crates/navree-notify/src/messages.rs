//! What a notification renders for the Slack, Teams, SMS, WhatsApp and web
//! push channels, and where those messages go.

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
/// let message = SlackMessage::new("Deal won: Acme renewal").blocks(json!([
///     { "type": "section", "text": { "type": "mrkdwn", "text": "*Acme renewal* was won" } }
/// ]));
/// assert_eq!(message.text, "Deal won: Acme renewal");
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
/// let sms = SmsMessage::new("Your code is 481 516").from("Acme");
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
///     .body("Acme renewal was won")
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

#[cfg(test)]
mod tests {
    use super::*;

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
            .body("Acme")
            .url("/deals/1")
            .ttl_secs(60)
            .urgency(Urgency::High)
            .topic("deals");
        assert_eq!(
            push.payload(),
            json!({ "title": "Deal won", "body": "Acme", "url": "/deals/1" })
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
