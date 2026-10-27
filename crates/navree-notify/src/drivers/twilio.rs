//! [`TwilioSms`] and [`TwilioWhatsApp`]: Twilio's Messages API.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_contracts::{BoxFuture, Secret};
use navree_http_client::HttpClient;
use serde_json::{Map, Value, json};
use url::Url;

use super::{
    ChannelError, SentMessage, SmsDriver, WhatsAppDriver, client_for, endpoint, join, refused,
    text_of, unreachable,
};
use crate::{SmsMessage, SmsOptions, TwilioOptions, WhatsAppMessage, WhatsAppOptions};

/// The provider name of the Twilio drivers.
const PROVIDER: &str = "twilio";

/// Twilio's error code for a recipient who replied STOP.
pub(crate) const UNSUBSCRIBED: &str = "21610";

/// The Messages API client both drivers share.
struct Twilio {
    account_sid: String,
    username: String,
    auth_token: Secret<String>,
    base: Url,
    client: HttpClient,
}

impl Twilio {
    fn new(options: &TwilioOptions) -> Result<Self, String> {
        if options.account_sid.is_empty() || options.auth_token.expose().is_empty() {
            return Err(
                "the twilio driver needs notify.twilio.account_sid and notify.twilio.auth_token"
                    .into(),
            );
        }
        let base = endpoint(
            options.endpoint.as_deref(),
            "https://api.twilio.com",
            "notify.twilio.endpoint",
        )?;
        let client = client_for(options.endpoint.as_ref().map(|_| &base))?;
        Ok(Self {
            account_sid: options.account_sid.clone(),
            username: options
                .api_key
                .clone()
                .filter(|key| !key.is_empty())
                .unwrap_or_else(|| options.account_sid.clone()),
            auth_token: options.auth_token.clone(),
            base,
            client,
        })
    }

    /// `POST /2010-04-01/Accounts/{sid}/Messages.json` with `form`.
    async fn create(&self, form: &[(&str, String)]) -> Result<SentMessage, ChannelError> {
        let url = join(
            &self.base,
            &format!("/2010-04-01/Accounts/{}/Messages.json", self.account_sid),
        );
        let credentials =
            STANDARD.encode(format!("{}:{}", self.username, self.auth_token.expose()));
        let response = self
            .client
            .post(url)
            .header("authorization", format!("Basic {credentials}"))
            .form(form)
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))?;
        if !response.is_success() {
            let error = refused(PROVIDER, &response, |body| {
                (
                    text_of(&body["code"]),
                    body["message"].as_str().map(str::to_owned),
                )
            });
            if let ChannelError::Refused {
                code: Some(code), ..
            } = &error
                && code == UNSUBSCRIBED
            {
                return Err(ChannelError::OptedOut {
                    provider: PROVIDER.to_owned(),
                });
            }
            return Err(error);
        }
        let answer: Value = response.json().unwrap_or(Value::Null);
        let sent = SentMessage::new(PROVIDER);
        Ok(match answer["sid"].as_str() {
            Some(sid) => sent.id(sid),
            None => sent,
        })
    }
}

/// The sender parameter: a messaging service (`MG...`) or `From`.
fn sender(from: &str) -> (&'static str, String) {
    if from.starts_with("MG") && !from.starts_with('+') {
        ("MessagingServiceSid", from.to_owned())
    } else {
        ("From", from.to_owned())
    }
}

/// Sends text messages through Twilio's Messages API, from
/// [`SmsMessage::from`](crate::SmsMessage::from) or `notify.sms.from` (a
/// `MG...` sender is a messaging service), with `notify.sms.status_callback`
/// as the `StatusCallback`. The message id is the `SM...` SID; Twilio's
/// error `21610` (the recipient replied STOP) is
/// [`ChannelError::OptedOut`].
pub struct TwilioSms {
    twilio: Twilio,
    from: Option<String>,
    status_callback: Option<String>,
}

impl TwilioSms {
    /// A driver for the credentials in `twilio` and the defaults in `sms`.
    pub fn new(twilio: &TwilioOptions, sms: &SmsOptions) -> Result<Self, String> {
        Ok(Self {
            twilio: Twilio::new(twilio)?,
            from: sms.from.clone(),
            status_callback: sms.status_callback.clone(),
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.twilio.client = client;
        self
    }
}

impl SmsDriver for TwilioSms {
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
                .ok_or_else(|| {
                    ChannelError::Config("set notify.sms.from or the message's sender".into())
                })?;
            let (name, value) = sender(from);
            let mut form = vec![
                ("To", to.to_owned()),
                (name, value),
                ("Body", message.body.clone()),
            ];
            if let Some(callback) = &self.status_callback {
                form.push(("StatusCallback", callback.clone()));
            }
            self.twilio.create(&form).await
        })
    }
}

impl fmt::Debug for TwilioSms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioSms")
            .field("account_sid", &self.twilio.account_sid)
            .finish_non_exhaustive()
    }
}

/// Sends WhatsApp messages through Twilio's Messages API (`whatsapp:`
/// addresses), from [`WhatsAppMessage::from`](crate::WhatsAppMessage::from)
/// or `notify.whatsapp.from`. A template is a Content template: its `name`
/// is the Content SID (`HX...`) and its parameters the `ContentVariables`
/// `{"1": .., "2": ..}`.
pub struct TwilioWhatsApp {
    twilio: Twilio,
    from: Option<String>,
    status_callback: Option<String>,
}

impl TwilioWhatsApp {
    /// A driver for the credentials in `twilio` and the defaults in
    /// `whatsapp`.
    pub fn new(twilio: &TwilioOptions, whatsapp: &WhatsAppOptions) -> Result<Self, String> {
        Ok(Self {
            twilio: Twilio::new(twilio)?,
            from: whatsapp.from.clone(),
            status_callback: whatsapp.status_callback.clone(),
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.twilio.client = client;
        self
    }
}

/// `whatsapp:+1...`, unless it already is.
fn whatsapp_address(number: &str) -> String {
    if number.starts_with("whatsapp:") {
        number.to_owned()
    } else {
        format!("whatsapp:{number}")
    }
}

impl WhatsAppDriver for TwilioWhatsApp {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a WhatsAppMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            let from = message
                .from
                .as_deref()
                .or(self.from.as_deref())
                .ok_or_else(|| {
                    ChannelError::Config("set notify.whatsapp.from or the message's sender".into())
                })?;
            let mut form = vec![("To", whatsapp_address(to))];
            let (name, value) = sender(from);
            form.push(if name == "From" {
                (name, whatsapp_address(&value))
            } else {
                (name, value)
            });
            match (&message.template, &message.body) {
                (Some(template), _) => {
                    form.push(("ContentSid", template.name.clone()));
                    if !template.parameters.is_empty() {
                        let variables: Map<String, Value> = template
                            .parameters
                            .iter()
                            .enumerate()
                            .map(|(i, value)| ((i + 1).to_string(), json!(value)))
                            .collect();
                        form.push(("ContentVariables", Value::Object(variables).to_string()));
                    }
                }
                (None, Some(body)) => form.push(("Body", body.clone())),
                (None, None) => {
                    return Err(ChannelError::Config(
                        "a WhatsApp message needs a body or a template".into(),
                    ));
                }
            }
            if let Some(callback) = &self.status_callback {
                form.push(("StatusCallback", callback.clone()));
            }
            self.twilio.create(&form).await
        })
    }
}

impl fmt::Debug for TwilioWhatsApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioWhatsApp")
            .field("account_sid", &self.twilio.account_sid)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messaging_services_are_told_apart_from_numbers() {
        assert_eq!(sender("MG123").0, "MessagingServiceSid");
        assert_eq!(sender("+15005550006").0, "From");
        assert_eq!(sender("Acme").0, "From");
    }

    #[test]
    fn whatsapp_addresses_get_their_prefix_once() {
        assert_eq!(whatsapp_address("+1555"), "whatsapp:+1555");
        assert_eq!(whatsapp_address("whatsapp:+1555"), "whatsapp:+1555");
    }
}
