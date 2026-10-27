//! [`VapidWebPush`]: encrypted push messages with a VAPID signature.

use std::fmt;

use chrono::{TimeDelta, Utc};
use navree_contracts::BoxFuture;
use navree_crypto::webpush::{self, VapidKey, WebPushError};
use navree_http_client::{HttpClient, HttpClientOptions};
use url::Url;

use super::{ChannelError, PushTarget, SentMessage, WebPushDriver, unreachable};
use crate::{WebPushMessage, WebPushOptions};

/// The provider name of the web push driver.
const PROVIDER: &str = "web_push";

/// How long a VAPID signature is valid (push services accept up to 24 h).
const SIGNATURE_HOURS: i64 = 12;

/// Sends browser push notifications (RFC 8030): each message's
/// [`payload`](WebPushMessage::payload) is encrypted for its subscription
/// (`aes128gcm`, RFC 8291) and posted to the subscription's endpoint with a
/// VAPID `Authorization` (RFC 8292, ES256 through `navree_crypto`), `TTL`
/// (the message's, else `notify.web_push.ttl_secs`) and, when set,
/// `Urgency` and `Topic`. `404` and `410` mean the subscription is gone
/// ([`ChannelError::Gone`]). The message id is the push service's
/// `Location`.
///
/// Endpoints come from browsers, so they are sent through an HTTP client
/// with the default SSRF policy: internal addresses are refused.
pub struct VapidWebPush {
    vapid: VapidKey,
    subject: String,
    ttl_secs: u32,
    client: HttpClient,
}

impl VapidWebPush {
    /// A driver for `options` (both VAPID halves and the subject).
    pub fn new(options: &WebPushOptions) -> Result<Self, String> {
        let public = options
            .public_key
            .as_deref()
            .filter(|k| !k.is_empty())
            .ok_or("the web push driver needs notify.web_push.public_key")?;
        let vapid = VapidKey::from_base64url(public, options.private_key.expose())
            .map_err(|error| format!("notify.web_push: {error}"))?;
        let client = HttpClient::new(HttpClientOptions {
            retries: 0,
            ..HttpClientOptions::default()
        })
        .map_err(|error| format!("cannot build the HTTP client: {error}"))?;
        Ok(Self {
            vapid,
            subject: options.subject.clone(),
            ttl_secs: options.ttl_secs,
            client,
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }

    /// The application server key browsers subscribe with.
    pub fn application_server_key(&self) -> String {
        self.vapid.application_server_key()
    }
}

/// The origin of `endpoint`: the VAPID audience.
fn audience(endpoint: &str) -> Result<String, ChannelError> {
    let url = Url::parse(endpoint)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .ok_or_else(|| ChannelError::InvalidRecipient("the push endpoint is not an http(s) URL".into()))?;
    Ok(url.origin().ascii_serialization())
}

impl WebPushDriver for VapidWebPush {
    fn send<'a>(
        &'a self,
        target: &'a PushTarget,
        message: &'a WebPushMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            let invalid = |what: &str| ChannelError::InvalidRecipient(format!("the subscription's `{what}` is not base64url"));
            let p256dh = webpush::decode_key(&target.p256dh).ok_or_else(|| invalid("p256dh"))?;
            let auth = webpush::decode_key(&target.auth).ok_or_else(|| invalid("auth"))?;
            let payload = serde_json::to_vec(&message.payload())
                .map_err(|error| ChannelError::Config(format!("the push payload is not JSON: {error}")))?;
            let body = webpush::encrypt(&p256dh, &auth, &payload).map_err(|error| match error {
                WebPushError::InvalidSubscription(reason) => {
                    ChannelError::InvalidRecipient(reason.to_owned())
                }
                other => ChannelError::Config(other.to_string()),
            })?;
            let expires = (Utc::now() + TimeDelta::hours(SIGNATURE_HOURS)).timestamp();
            let authorization = self
                .vapid
                .authorization(&audience(&target.endpoint)?, &self.subject, expires)
                .map_err(|error| ChannelError::Config(error.to_string()))?;
            let mut request = self
                .client
                .post(&target.endpoint)
                .header("authorization", authorization)
                .header("content-encoding", "aes128gcm")
                .header("content-type", "application/octet-stream")
                .header("ttl", message.ttl_secs.unwrap_or(self.ttl_secs).to_string());
            if let Some(urgency) = message.urgency {
                request = request.header("urgency", urgency.as_str());
            }
            if let Some(topic) = &message.topic {
                request = request.header("topic", topic.as_str());
            }
            let response = request
                .body(body)
                .send()
                .await
                .map_err(|error| unreachable(PROVIDER, error))?;
            match response.status().as_u16() {
                200..=299 => {
                    let sent = SentMessage::new(PROVIDER);
                    Ok(match response.header("location") {
                        Some(location) => sent.id(location),
                        None => sent,
                    })
                }
                404 | 410 => Err(ChannelError::Gone),
                status => Err(ChannelError::Refused {
                    provider: PROVIDER.to_owned(),
                    status,
                    code: None,
                    message: {
                        let text = response.text();
                        let text = text.trim();
                        if text.is_empty() {
                            "the push service refused the message".to_owned()
                        } else {
                            text.chars().take(200).collect()
                        }
                    },
                }),
            }
        })
    }
}

impl fmt::Debug for VapidWebPush {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VapidWebPush")
            .field("vapid", &self.vapid)
            .field("subject", &self.subject)
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audience_is_the_endpoints_origin() {
        assert_eq!(
            audience("https://fcm.googleapis.com/fcm/send/abc:def").unwrap(),
            "https://fcm.googleapis.com"
        );
        assert_eq!(
            audience("http://127.0.0.1:8080/push/1").unwrap(),
            "http://127.0.0.1:8080"
        );
        assert!(audience("mailto:x").is_err());
    }
}
