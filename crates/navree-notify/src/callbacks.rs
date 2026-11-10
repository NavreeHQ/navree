//! [`ChannelCallback`]: what providers report about sent messages (status
//! callbacks) and what recipients send back (inbound messages, STOP).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Channel, DeliveryStatus};

/// The replies that opt a number out (matched on the whole message,
/// ignoring case and trailing punctuation).
pub const OPT_OUT_KEYWORDS: [&str; 8] = [
    "STOP",
    "STOPALL",
    "UNSUBSCRIBE",
    "CANCEL",
    "END",
    "QUIT",
    "OPTOUT",
    "REVOKE",
];

/// The replies that opt a number back in.
pub const OPT_IN_KEYWORDS: [&str; 4] = ["START", "YES", "UNSTOP", "OPTIN"];

/// One thing a provider reported in a callback. Parse a provider's request
/// body with [`from_twilio`](ChannelCallback::from_twilio),
/// [`from_vonage`](ChannelCallback::from_vonage) or
/// [`from_meta`](ChannelCallback::from_meta), after verifying its signature
/// (navree-webhooks' `VerifiedWebhook<Twilio>`, `<Vonage>`, `<Meta>`), and
/// apply it with [`DeliveryLog::ingest`](crate::DeliveryLog::ingest).
///
/// ```
/// use navree_notify::{ChannelCallback, DeliveryStatus};
///
/// let callbacks = ChannelCallback::from_twilio(
///     b"MessageSid=SM123&MessageStatus=delivered&To=%2B15005550006",
/// );
/// let ChannelCallback::Status(update) = &callbacks[0] else { unreachable!() };
/// assert_eq!(update.message_id, "SM123");
/// assert_eq!(update.status, DeliveryStatus::Delivered);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChannelCallback {
    /// A sent message's new status.
    Status(StatusUpdate),
    /// A message a recipient sent to the app's number.
    Inbound(InboundMessage),
}

/// A sent message's new status, by the provider's message id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusUpdate {
    /// The provider: `twilio`, `vonage` or `meta`.
    pub provider: String,
    /// The provider's message id (what [`SentMessage::message_id`](crate::SentMessage) held).
    pub message_id: String,
    /// The new status.
    pub status: DeliveryStatus,
    /// The provider's error code (Twilio's `30003`, the Cloud API's
    /// `131026`).
    pub error_code: Option<String>,
    /// The provider's error message.
    pub error: Option<String>,
}

impl StatusUpdate {
    /// A status update from `provider` about message `message_id`.
    pub fn new(
        provider: impl Into<String>,
        message_id: impl Into<String>,
        status: DeliveryStatus,
    ) -> Self {
        Self {
            provider: provider.into(),
            message_id: message_id.into(),
            status,
            error_code: None,
            error: None,
        }
    }

    /// With the provider's error code and message.
    #[must_use]
    pub fn error(mut self, code: Option<String>, message: Option<String>) -> Self {
        self.error_code = code;
        self.error = message;
        self
    }

    /// Whether the error means the recipient opted out (Twilio's `21610`,
    /// the Cloud API's `131050`).
    pub fn is_opt_out(&self) -> bool {
        matches!(
            (self.provider.as_str(), self.error_code.as_deref()),
            ("twilio", Some("21610")) | ("meta", Some("131050"))
        )
    }
}

/// A message a recipient sent to the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundMessage {
    /// The provider: `twilio`, `vonage` or `meta`.
    pub provider: String,
    /// [`Channel::Sms`] or [`Channel::WhatsApp`].
    pub channel: Channel,
    /// The sender's number (`+15005550006`).
    pub from: String,
    /// The app's number it was sent to.
    pub to: Option<String>,
    /// The text.
    pub body: String,
    /// The provider's own reading of an opt-out keyword (Twilio's
    /// `OptOutType`: `STOP`, `START` or `HELP`).
    pub opt_out_type: Option<String>,
}

/// What an inbound message asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keyword {
    /// Stop sending (STOP, UNSUBSCRIBE, ...).
    OptOut,
    /// Send again (START, YES, UNSTOP).
    OptIn,
}

impl InboundMessage {
    /// The opt-out keyword the message is, if any: the provider's reading
    /// when it gives one, else the whole text matched against
    /// [`OPT_OUT_KEYWORDS`] and [`OPT_IN_KEYWORDS`].
    pub fn keyword(&self) -> Option<Keyword> {
        match self
            .opt_out_type
            .as_deref()
            .map(str::to_ascii_uppercase)
            .as_deref()
        {
            Some("STOP") => return Some(Keyword::OptOut),
            Some("START") => return Some(Keyword::OptIn),
            Some(_) => return None,
            None => {}
        }
        keyword(&self.body)
    }
}

/// The keyword `text` is, if any.
pub fn keyword(text: &str) -> Option<Keyword> {
    let word = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim()
        .to_ascii_uppercase();
    if OPT_OUT_KEYWORDS.contains(&word.as_str()) {
        Some(Keyword::OptOut)
    } else if OPT_IN_KEYWORDS.contains(&word.as_str()) {
        Some(Keyword::OptIn)
    } else {
        None
    }
}

/// `+` and the digits, for a number a provider wrote without the plus.
fn plus(number: &str) -> String {
    let number = number.trim();
    if number.starts_with('+') {
        number.to_owned()
    } else {
        format!("+{number}")
    }
}

impl ChannelCallback {
    /// Reads a Twilio request (form-encoded): a status callback
    /// (`MessageSid`, `MessageStatus`, `ErrorCode`) or an incoming message
    /// (`From`, `Body`, `OptOutType`) for SMS or WhatsApp (`whatsapp:`
    /// addresses).
    pub fn from_twilio(body: &[u8]) -> Vec<ChannelCallback> {
        let params: HashMap<String, String> = form_urlencoded::parse(body).into_owned().collect();
        let get = |name: &str| {
            params
                .get(name)
                .map(String::as_str)
                .filter(|v| !v.is_empty())
        };
        let status = get("MessageStatus").or_else(|| get("SmsStatus"));
        if let (Some(sid), Some(status)) = (get("MessageSid").or_else(|| get("SmsSid")), status)
            && let Some(status) = twilio_status(status)
        {
            let code = get("ErrorCode").map(str::to_owned);
            let message = get("ErrorMessage")
                .map(str::to_owned)
                .or_else(|| code.as_ref().map(|code| format!("Twilio error {code}")));
            return vec![ChannelCallback::Status(
                StatusUpdate::new("twilio", sid, status).error(code, message),
            )];
        }
        let (Some(from), Some(text)) = (get("From"), params.get("Body")) else {
            return Vec::new();
        };
        let (channel, from) = match from.strip_prefix("whatsapp:") {
            Some(number) => (Channel::WhatsApp, number),
            None => (Channel::Sms, from),
        };
        vec![ChannelCallback::Inbound(InboundMessage {
            provider: "twilio".to_owned(),
            channel,
            from: plus(from),
            to: get("To").map(|to| to.trim_start_matches("whatsapp:").to_owned()),
            body: text.clone(),
            opt_out_type: get("OptOutType").map(str::to_owned),
        })]
    }

    /// Reads a Vonage Messages API webhook (JSON): a message status
    /// (`message_uuid`, `status`, `error`) or an inbound message (`from`,
    /// `text`).
    pub fn from_vonage(body: &[u8]) -> Vec<ChannelCallback> {
        let Ok(event) = serde_json::from_slice::<Value>(body) else {
            return Vec::new();
        };
        let channel = match event["channel"].as_str() {
            Some("whatsapp") => Channel::WhatsApp,
            _ => Channel::Sms,
        };
        if let (Some(id), Some(status)) = (event["message_uuid"].as_str(), event["status"].as_str())
        {
            let Some(status) = vonage_status(status) else {
                return Vec::new();
            };
            let error = &event["error"];
            let code = error["type"]
                .as_str()
                .and_then(|t| t.rsplit_once('#'))
                .map(|(_, code)| code.to_owned());
            let message = error["title"]
                .as_str()
                .or(error["detail"].as_str())
                .map(str::to_owned);
            return vec![ChannelCallback::Status(
                StatusUpdate::new("vonage", id, status).error(code, message),
            )];
        }
        match (event["from"].as_str(), event["text"].as_str()) {
            (Some(from), Some(text)) => vec![ChannelCallback::Inbound(InboundMessage {
                provider: "vonage".to_owned(),
                channel,
                from: plus(from),
                to: event["to"].as_str().map(plus),
                body: text.to_owned(),
                opt_out_type: None,
            })],
            _ => Vec::new(),
        }
    }

    /// Reads a WhatsApp Cloud API webhook (JSON): every status
    /// (`statuses`) and every text or button reply (`messages`) of every
    /// entry and change.
    pub fn from_meta(body: &[u8]) -> Vec<ChannelCallback> {
        let Ok(event) = serde_json::from_slice::<Value>(body) else {
            return Vec::new();
        };
        let mut callbacks = Vec::new();
        let values = event["entry"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|entry| entry["changes"].as_array().into_iter().flatten())
            .map(|change| &change["value"]);
        for value in values {
            let to = value["metadata"]["display_phone_number"].as_str().map(plus);
            for status in value["statuses"].as_array().into_iter().flatten() {
                let (Some(id), Some(state)) = (status["id"].as_str(), status["status"].as_str())
                else {
                    continue;
                };
                let Some(state) = meta_status(state) else {
                    continue;
                };
                let error = &status["errors"][0];
                let code = crate::drivers::text_of(&error["code"]);
                let message = error["title"]
                    .as_str()
                    .or(error["message"].as_str())
                    .map(str::to_owned);
                callbacks.push(ChannelCallback::Status(
                    StatusUpdate::new("meta", id, state).error(code, message),
                ));
            }
            for message in value["messages"].as_array().into_iter().flatten() {
                let Some(from) = message["from"].as_str() else {
                    continue;
                };
                let text = message["text"]["body"]
                    .as_str()
                    .or(message["button"]["text"].as_str())
                    .or(message["interactive"]["button_reply"]["title"].as_str());
                let Some(text) = text else {
                    continue;
                };
                callbacks.push(ChannelCallback::Inbound(InboundMessage {
                    provider: "meta".to_owned(),
                    channel: Channel::WhatsApp,
                    from: plus(from),
                    to: to.clone(),
                    body: text.to_owned(),
                    opt_out_type: None,
                }));
            }
        }
        callbacks
    }
}

/// Twilio's `MessageStatus`, or `None` for inbound states.
fn twilio_status(status: &str) -> Option<DeliveryStatus> {
    Some(match status {
        "queued" | "accepted" | "scheduled" | "sending" | "sent" => DeliveryStatus::Sent,
        "delivered" => DeliveryStatus::Delivered,
        "read" => DeliveryStatus::Read,
        "undelivered" | "failed" | "canceled" => DeliveryStatus::Failed,
        _ => return None,
    })
}

/// Vonage's message `status`.
fn vonage_status(status: &str) -> Option<DeliveryStatus> {
    Some(match status {
        "submitted" => DeliveryStatus::Sent,
        "delivered" => DeliveryStatus::Delivered,
        "read" => DeliveryStatus::Read,
        "rejected" | "undeliverable" => DeliveryStatus::Failed,
        _ => return None,
    })
}

/// The Cloud API's `status`.
fn meta_status(status: &str) -> Option<DeliveryStatus> {
    Some(match status {
        "sent" => DeliveryStatus::Sent,
        "delivered" => DeliveryStatus::Delivered,
        "read" => DeliveryStatus::Read,
        "failed" => DeliveryStatus::Failed,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn status(callbacks: &[ChannelCallback]) -> &StatusUpdate {
        match &callbacks[0] {
            ChannelCallback::Status(update) => update,
            other => panic!("not a status: {other:?}"),
        }
    }

    fn inbound(callbacks: &[ChannelCallback]) -> &InboundMessage {
        match &callbacks[0] {
            ChannelCallback::Inbound(message) => message,
            other => panic!("not inbound: {other:?}"),
        }
    }

    #[test]
    fn twilio_status_callbacks_map_their_states_and_errors() {
        let callbacks = ChannelCallback::from_twilio(
            b"SmsSid=SM1&SmsStatus=undelivered&MessageStatus=undelivered&To=%2B15005550006&MessageSid=SM1&ErrorCode=30003&AccountSid=AC1",
        );
        let update = status(&callbacks);
        assert_eq!(update.provider, "twilio");
        assert_eq!(update.message_id, "SM1");
        assert_eq!(update.status, DeliveryStatus::Failed);
        assert_eq!(update.error_code.as_deref(), Some("30003"));
        assert_eq!(update.error.as_deref(), Some("Twilio error 30003"));
        for (state, expected) in [
            ("queued", DeliveryStatus::Sent),
            ("sent", DeliveryStatus::Sent),
            ("delivered", DeliveryStatus::Delivered),
            ("read", DeliveryStatus::Read),
            ("failed", DeliveryStatus::Failed),
        ] {
            let body = format!("MessageSid=SM2&MessageStatus={state}");
            assert_eq!(
                status(&ChannelCallback::from_twilio(body.as_bytes())).status,
                expected
            );
        }
    }

    #[test]
    fn twilio_incoming_messages_are_inbound_with_their_channel() {
        let callbacks = ChannelCallback::from_twilio(
            b"MessageSid=SM9&SmsStatus=received&From=%2B15005550006&To=%2B15005550001&Body=Stop&OptOutType=STOP",
        );
        let message = inbound(&callbacks);
        assert_eq!(message.channel, Channel::Sms);
        assert_eq!(message.from, "+15005550006");
        assert_eq!(message.body, "Stop");
        assert_eq!(message.keyword(), Some(Keyword::OptOut));

        let callbacks = ChannelCallback::from_twilio(
            b"From=whatsapp%3A%2B447400123456&To=whatsapp%3A%2B15005550001&Body=hello",
        );
        let message = inbound(&callbacks);
        assert_eq!(message.channel, Channel::WhatsApp);
        assert_eq!(message.from, "+447400123456");
        assert_eq!(message.to.as_deref(), Some("+15005550001"));
        assert_eq!(message.keyword(), None);
        assert!(ChannelCallback::from_twilio(b"nothing=here").is_empty());
    }

    #[test]
    fn keywords_match_the_whole_message() {
        assert_eq!(keyword(" stop "), Some(Keyword::OptOut));
        assert_eq!(keyword("Unsubscribe."), Some(Keyword::OptOut));
        assert_eq!(keyword("START"), Some(Keyword::OptIn));
        assert_eq!(keyword("unstop!"), Some(Keyword::OptIn));
        assert_eq!(keyword("please stop texting"), None);
        assert_eq!(keyword(""), None);
        let help = InboundMessage {
            provider: "twilio".into(),
            channel: Channel::Sms,
            from: "+1".into(),
            to: None,
            body: "STOP".into(),
            opt_out_type: Some("HELP".into()),
        };
        assert_eq!(help.keyword(), None, "the provider's reading wins");
    }

    #[test]
    fn vonage_statuses_and_inbound_messages() {
        let body = json!({
            "message_uuid": "aaaaaaaa-bbbb-cccc-dddd-0123456789ab",
            "to": "447700900000",
            "from": "Agra",
            "timestamp": "2026-10-28T10:00:00Z",
            "status": "rejected",
            "channel": "sms",
            "error": { "type": "https://developer.nexmo.com/api-errors/messages-olympus#1320", "title": "Message failed" }
        });
        let callbacks = ChannelCallback::from_vonage(body.to_string().as_bytes());
        let update = status(&callbacks);
        assert_eq!(update.provider, "vonage");
        assert_eq!(update.status, DeliveryStatus::Failed);
        assert_eq!(update.error_code.as_deref(), Some("1320"));
        assert_eq!(update.error.as_deref(), Some("Message failed"));

        let body = json!({
            "channel": "sms",
            "message_uuid": "x",
            "to": "447700900001",
            "from": "447700900000",
            "text": "STOP",
            "message_type": "text",
        });
        let callbacks = ChannelCallback::from_vonage(body.to_string().as_bytes());
        let message = inbound(&callbacks);
        assert_eq!(message.from, "+447700900000");
        assert_eq!(message.keyword(), Some(Keyword::OptOut));
        assert!(ChannelCallback::from_vonage(b"not json").is_empty());
    }

    #[test]
    fn meta_webhooks_carry_statuses_and_replies() {
        let body = json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "WABA",
                "changes": [{
                    "field": "messages",
                    "value": {
                        "messaging_product": "whatsapp",
                        "metadata": { "display_phone_number": "15550001111", "phone_number_id": "1055" },
                        "statuses": [
                            { "id": "wamid.A", "status": "read", "timestamp": "1700000000", "recipient_id": "447400123456" },
                            { "id": "wamid.B", "status": "failed", "timestamp": "1700000000", "recipient_id": "447400123456",
                              "errors": [{ "code": 131026, "title": "Message undeliverable" }] }
                        ],
                        "messages": [
                            { "from": "447400123456", "id": "wamid.C", "timestamp": "1700000000", "type": "text", "text": { "body": "STOP" } },
                            { "from": "447400123457", "id": "wamid.D", "timestamp": "1700000000", "type": "button", "button": { "text": "Yes", "payload": "yes" } },
                            { "from": "447400123458", "id": "wamid.E", "timestamp": "1700000000", "type": "image", "image": { "id": "1" } }
                        ]
                    }
                }]
            }]
        });
        let callbacks = ChannelCallback::from_meta(body.to_string().as_bytes());
        assert_eq!(callbacks.len(), 4);
        assert_eq!(status(&callbacks).status, DeliveryStatus::Read);
        let ChannelCallback::Status(failed) = &callbacks[1] else {
            panic!()
        };
        assert_eq!(failed.error_code.as_deref(), Some("131026"));
        let ChannelCallback::Inbound(stop) = &callbacks[2] else {
            panic!()
        };
        assert_eq!(stop.from, "+447400123456");
        assert_eq!(stop.to.as_deref(), Some("+15550001111"));
        assert_eq!(stop.keyword(), Some(Keyword::OptOut));
        let ChannelCallback::Inbound(yes) = &callbacks[3] else {
            panic!()
        };
        assert_eq!(yes.keyword(), Some(Keyword::OptIn));
    }

    #[test]
    fn opt_out_errors_are_recognized_per_provider() {
        let twilio = StatusUpdate::new("twilio", "SM", DeliveryStatus::Failed)
            .error(Some("21610".into()), None);
        assert!(twilio.is_opt_out());
        let meta = StatusUpdate::new("meta", "w", DeliveryStatus::Failed)
            .error(Some("131050".into()), None);
        assert!(meta.is_opt_out());
        let other = StatusUpdate::new("twilio", "SM", DeliveryStatus::Failed)
            .error(Some("30003".into()), None);
        assert!(!other.is_opt_out());
    }
}
