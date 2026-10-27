//! [`VonageSms`]: Vonage's Messages API.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_contracts::{BoxFuture, Secret};
use navree_http_client::HttpClient;
use serde_json::{Value, json};
use url::Url;

use super::{ChannelError, SentMessage, SmsDriver, client_for, endpoint, join, refused, unreachable};
use crate::{SmsMessage, SmsOptions, VonageOptions};

/// The provider name of the Vonage driver.
const PROVIDER: &str = "vonage";

/// Sends text messages through Vonage's Messages API (`POST /v1/messages`,
/// channel `sms`), from [`SmsMessage::from`](crate::SmsMessage::from) or
/// `notify.sms.from`, with `notify.sms.status_callback` as the message's
/// `webhook_url`. The message id is the `message_uuid`.
pub struct VonageSms {
    api_key: String,
    api_secret: Secret<String>,
    base: Url,
    client: HttpClient,
    from: Option<String>,
    status_callback: Option<String>,
}

impl VonageSms {
    /// A driver for the credentials in `vonage` and the defaults in `sms`.
    pub fn new(vonage: &VonageOptions, sms: &SmsOptions) -> Result<Self, String> {
        if vonage.api_key.is_empty() || vonage.api_secret.expose().is_empty() {
            return Err("the vonage driver needs notify.vonage.api_key and notify.vonage.api_secret".into());
        }
        let base = endpoint(
            vonage.endpoint.as_deref(),
            "https://api.nexmo.com",
            "notify.vonage.endpoint",
        )?;
        let client = client_for(vonage.endpoint.as_ref().map(|_| &base))?;
        Ok(Self {
            api_key: vonage.api_key.clone(),
            api_secret: vonage.api_secret.clone(),
            base,
            client,
            from: sms.from.clone(),
            status_callback: sms.status_callback.clone(),
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }
}

impl SmsDriver for VonageSms {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a SmsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            let from = message
                .from
                .as_deref()
                .or(self.from.as_deref())
                .ok_or_else(|| ChannelError::Config("set notify.sms.from or the message's sender".into()))?;
            let mut body = json!({
                "message_type": "text",
                "channel": "sms",
                "to": to.trim_start_matches('+'),
                "from": from.trim_start_matches('+'),
                "text": message.body,
            });
            if let Some(callback) = &self.status_callback {
                body["webhook_url"] = json!(callback);
            }
            let credentials =
                STANDARD.encode(format!("{}:{}", self.api_key, self.api_secret.expose()));
            let response = self
                .client
                .post(join(&self.base, "/v1/messages"))
                .header("authorization", format!("Basic {credentials}"))
                .json(&body)
                .send()
                .await
                .map_err(|error| unreachable(PROVIDER, error))?;
            if !response.is_success() {
                return Err(refused(PROVIDER, &response, |body| {
                    // RFC 7807: the code is the fragment of `type`.
                    let code = body["type"]
                        .as_str()
                        .and_then(|t| t.rsplit_once('#'))
                        .map(|(_, code)| code.to_owned());
                    let message = match (body["title"].as_str(), body["detail"].as_str()) {
                        (Some(title), Some(detail)) => Some(format!("{title}: {detail}")),
                        (Some(title), None) => Some(title.to_owned()),
                        (None, detail) => detail.map(str::to_owned),
                    };
                    (code, message)
                }));
            }
            let answer: Value = response.json().unwrap_or(Value::Null);
            let sent = SentMessage::new(PROVIDER);
            Ok(match answer["message_uuid"].as_str() {
                Some(id) => sent.id(id),
                None => sent,
            })
        })
    }
}

impl fmt::Debug for VonageSms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VonageSms")
            .field("api_key", &self.api_key)
            .finish_non_exhaustive()
    }
}
