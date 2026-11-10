//! The drivers behind the Slack, Teams, SMS, WhatsApp and web push
//! channels: one trait per channel, the HTTP drivers, and their helpers.

pub(crate) mod log;
pub(crate) mod meta;
pub(crate) mod slack;
pub(crate) mod sns;
pub(crate) mod teams;
pub(crate) mod twilio;
pub(crate) mod vonage;
pub(crate) mod web_push;

use navree_contracts::BoxFuture;
use navree_http_client::{HttpClient, HttpClientError, HttpClientOptions, Response};
use navree_kernel::BoxError;
use serde_json::Value;
use url::Url;

use crate::{
    SlackMessage, SlackRoute, SmsMessage, TeamsMessage, TeamsRoute, WebPushMessage, WhatsAppMessage,
};

/// What a provider answered when it accepted a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentMessage {
    /// The driver that sent it: `slack`, `teams`, `twilio`, `vonage`,
    /// `sns`, `meta`, `web_push`, `log` or `fake`.
    pub provider: String,
    /// The provider's id of the message (Twilio's `SM...`, Vonage's
    /// `message_uuid`, a WhatsApp `wamid`, Slack's `ts`, a Teams activity
    /// id), which status callbacks name.
    pub message_id: Option<String>,
}

impl SentMessage {
    /// Sent by `provider`, with no message id.
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            message_id: None,
        }
    }

    /// With the provider's message id.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.message_id = Some(id.into());
        self
    }
}

/// Why a channel driver did not send a message. Messages never contain
/// message bodies, tokens or webhook URLs.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ChannelError {
    /// The provider refused the message: its HTTP status, its error code
    /// (Twilio's `21211`, Slack's `channel_not_found`) and its message.
    #[error("{provider} refused the message (status {status}{}): {message}", code_suffix(.code.as_deref()))]
    Refused {
        /// The driver.
        provider: String,
        /// The HTTP status.
        status: u16,
        /// The provider's error code.
        code: Option<String>,
        /// The provider's message.
        message: String,
    },
    /// The provider could not be reached.
    #[error("{provider} could not be reached: {source}")]
    Unreachable {
        /// The driver.
        provider: String,
        /// The transport error.
        #[source]
        source: BoxError,
    },
    /// The recipient has opted out at the provider (Twilio's `21610`, the
    /// WhatsApp Cloud API's `131050`): the address is recorded as opted
    /// out.
    #[error("the recipient has opted out of messages from {provider}")]
    OptedOut {
        /// The driver.
        provider: String,
    },
    /// The recipient is not an address the channel can send to (a number
    /// that is not E.164, a malformed push subscription).
    #[error("invalid recipient: {0}")]
    InvalidRecipient(String),
    /// The push subscription has expired or was revoked (`404` or `410`
    /// from the push service): it is deleted.
    #[error("the push subscription is gone")]
    Gone,
    /// The driver is not configured for this message (a bot route without a
    /// token, a missing sender).
    #[error("{0}")]
    Config(String),
}

/// ` code 21211`, or nothing.
fn code_suffix(code: Option<&str>) -> String {
    code.map(|code| format!(", code {code}"))
        .unwrap_or_default()
}

/// Sends Slack messages. The built-in driver posts to incoming webhooks
/// and calls `chat.postMessage`; [`SlackFake`](crate::SlackFake) records
/// them. Install your own with `override_value::<dyn SlackDriver>(..)`.
pub trait SlackDriver: Send + Sync + 'static {
    /// Sends `message` to `route`.
    fn send<'a>(
        &'a self,
        route: &'a SlackRoute,
        message: &'a SlackMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>>;
}

/// Sends Microsoft Teams messages. The built-in driver posts to incoming
/// webhooks and sends proactive messages through the Bot Framework;
/// [`TeamsFake`](crate::TeamsFake) records them.
pub trait TeamsDriver: Send + Sync + 'static {
    /// Sends `message` to `route`.
    fn send<'a>(
        &'a self,
        route: &'a TeamsRoute,
        message: &'a TeamsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>>;
}

/// Sends text messages: Twilio, Vonage, Amazon SNS or the log (section
/// `notify.sms`); [`SmsFake`](crate::SmsFake) records them.
pub trait SmsDriver: Send + Sync + 'static {
    /// Sends `message` to `to`, a number in E.164 (`+15005550006`).
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a SmsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>>;
}

/// Sends WhatsApp messages: Twilio or the WhatsApp Cloud API (section
/// `notify.whatsapp`); [`WhatsAppFake`](crate::WhatsAppFake) records them.
pub trait WhatsAppDriver: Send + Sync + 'static {
    /// Sends `message` to `to`, a number in E.164.
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a WhatsAppMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>>;
}

/// One browser push subscription, as the browser gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushTarget {
    /// The push service URL.
    pub endpoint: String,
    /// The subscription's P-256 public key (base64url).
    pub p256dh: String,
    /// The subscription's auth secret (base64url).
    pub auth: String,
}

/// Sends browser push notifications. The built-in driver encrypts each
/// message for its subscription and signs a VAPID header (section
/// `notify.web_push`); [`WebPushFake`](crate::WebPushFake) records them.
pub trait WebPushDriver: Send + Sync + 'static {
    /// Sends `message` to one subscription. A subscription the push
    /// service no longer knows is [`ChannelError::Gone`].
    fn send<'a>(
        &'a self,
        target: &'a PushTarget,
        message: &'a WebPushMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>>;
}

/// An HTTP client for a provider: the default SSRF policy, with the host
/// of an explicitly configured `endpoint` allowed (a local mock server),
/// and no retries (a message is never sent twice).
pub(crate) fn client_for(endpoint: Option<&Url>) -> Result<HttpClient, String> {
    let mut options = HttpClientOptions {
        retries: 0,
        ..HttpClientOptions::default()
    };
    if let Some(host) = endpoint.and_then(Url::host_str) {
        options.ssrf.allow_hosts.push(host.to_owned());
    }
    HttpClient::new(options).map_err(|error| format!("cannot build the HTTP client: {error}"))
}

/// Parses an `endpoint` setting, or `default`.
pub(crate) fn endpoint(configured: Option<&str>, default: &str, key: &str) -> Result<Url, String> {
    let raw = configured
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .unwrap_or(default);
    let url = Url::parse(raw.trim_end_matches('/'))
        .map_err(|error| format!("`{key}` is not a URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("`{key}` must be an http(s) URL"));
    }
    Ok(url)
}

/// `base` joined with `path`.
pub(crate) fn join(base: &Url, path: &str) -> String {
    format!("{}{path}", base.as_str().trim_end_matches('/'))
}

/// [`ChannelError::Unreachable`] for a transport error.
pub(crate) fn unreachable(provider: &str, error: HttpClientError) -> ChannelError {
    ChannelError::Unreachable {
        provider: provider.to_owned(),
        source: Box::new(error),
    }
}

/// [`ChannelError::Refused`] from a failed response: its status, and the
/// code and message `extract` finds in its JSON body (else the body text).
pub(crate) fn refused(
    provider: &str,
    response: &Response,
    extract: impl Fn(&Value) -> (Option<String>, Option<String>),
) -> ChannelError {
    let status = response.status().as_u16();
    let (code, message) = match response.json::<Value>() {
        Ok(body) => extract(&body),
        Err(_) => (None, None),
    };
    let message = message.unwrap_or_else(|| {
        let text = response.text();
        let text = text.trim();
        if text.is_empty() {
            response
                .status()
                .canonical_reason()
                .unwrap_or("error")
                .to_owned()
        } else {
            text.chars().take(200).collect()
        }
    });
    ChannelError::Refused {
        provider: provider.to_owned(),
        status,
        code,
        message,
    }
}

/// A JSON member as a string (numbers are written out).
pub(crate) fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_name_the_code_when_there_is_one() {
        let error = ChannelError::Refused {
            provider: "twilio".to_owned(),
            status: 400,
            code: Some("21211".to_owned()),
            message: "Invalid 'To' Phone Number".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "twilio refused the message (status 400, code 21211): Invalid 'To' Phone Number"
        );
        let error = ChannelError::Refused {
            provider: "slack".to_owned(),
            status: 404,
            code: None,
            message: "no_service".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "slack refused the message (status 404): no_service"
        );
    }

    #[test]
    fn endpoints_must_be_http_urls() {
        assert_eq!(
            endpoint(None, "https://api.twilio.com", "notify.twilio.endpoint")
                .unwrap()
                .as_str(),
            "https://api.twilio.com/"
        );
        assert!(endpoint(Some("ftp://x"), "https://a", "k").is_err());
        assert!(endpoint(Some("not a url"), "https://a", "k").is_err());
        let base = endpoint(Some("http://127.0.0.1:9000/"), "https://a", "k").unwrap();
        assert_eq!(
            join(&base, "/v1/messages"),
            "http://127.0.0.1:9000/v1/messages"
        );
    }

    #[test]
    fn json_members_read_as_text() {
        assert_eq!(text_of(&serde_json::json!(21211)), Some("21211".to_owned()));
        assert_eq!(text_of(&serde_json::json!("x")), Some("x".to_owned()));
        assert_eq!(text_of(&serde_json::json!("")), None);
        assert_eq!(text_of(&serde_json::json!(null)), None);
    }
}
