//! [`MetaWhatsApp`]: the WhatsApp Cloud API.

use std::fmt;

use navree_contracts::{BoxFuture, Secret};
use navree_http_client::HttpClient;
use serde_json::{Value, json};
use url::Url;

use super::{
    ChannelError, SentMessage, WhatsAppDriver, client_for, endpoint, join, refused, text_of,
    unreachable,
};
use crate::{MetaOptions, WhatsAppMessage};

/// The provider name of the WhatsApp Cloud API driver.
const PROVIDER: &str = "meta";

/// The Cloud API's error code for a user who stopped marketing messages.
const STOPPED: &str = "131050";

/// Sends WhatsApp messages through the WhatsApp Cloud API
/// (`POST /{version}/{phone_number_id}/messages`, with
/// `notify.meta.access_token`): free text as a `text` message, a
/// [`WhatsAppTemplate`](crate::WhatsAppTemplate) as a `template` message
/// whose parameters fill the body. The message id is the `wamid`.
pub struct MetaWhatsApp {
    phone_number_id: String,
    access_token: Secret<String>,
    api_version: String,
    base: Url,
    client: HttpClient,
}

impl MetaWhatsApp {
    /// A driver for `options`.
    pub fn new(options: &MetaOptions) -> Result<Self, String> {
        if options.phone_number_id.is_empty() || options.access_token.expose().is_empty() {
            return Err(
                "the meta driver needs notify.meta.phone_number_id and notify.meta.access_token"
                    .into(),
            );
        }
        let base = endpoint(
            options.endpoint.as_deref(),
            "https://graph.facebook.com",
            "notify.meta.endpoint",
        )?;
        let client = client_for(options.endpoint.as_ref().map(|_| &base))?;
        Ok(Self {
            phone_number_id: options.phone_number_id.clone(),
            access_token: options.access_token.clone(),
            api_version: options.api_version.clone(),
            base,
            client,
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }
}

/// The request body for `message` to `to` (digits only).
fn body(to: &str, message: &WhatsAppMessage) -> Result<Value, ChannelError> {
    let to = to.trim_start_matches('+');
    match (&message.template, &message.body) {
        (Some(template), _) => {
            let mut content = json!({
                "name": template.name,
                "language": { "code": if template.language.is_empty() { "en_US" } else { &template.language } },
            });
            if !template.parameters.is_empty() {
                let parameters: Vec<Value> = template
                    .parameters
                    .iter()
                    .map(|text| json!({ "type": "text", "text": text }))
                    .collect();
                content["components"] = json!([{ "type": "body", "parameters": parameters }]);
            }
            Ok(json!({
                "messaging_product": "whatsapp",
                "recipient_type": "individual",
                "to": to,
                "type": "template",
                "template": content,
            }))
        }
        (None, Some(text)) => Ok(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "text",
            "text": { "preview_url": false, "body": text },
        })),
        (None, None) => Err(ChannelError::Config(
            "a WhatsApp message needs a body or a template".into(),
        )),
    }
}

impl WhatsAppDriver for MetaWhatsApp {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a WhatsAppMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            let payload = body(to, message)?;
            let url = join(
                &self.base,
                &format!("/{}/{}/messages", self.api_version, self.phone_number_id),
            );
            let response = self
                .client
                .post(url)
                .bearer(self.access_token.expose())
                .json(&payload)
                .send()
                .await
                .map_err(|error| unreachable(PROVIDER, error))?;
            if !response.is_success() {
                let error = refused(PROVIDER, &response, |body| {
                    let error = &body["error"];
                    (
                        text_of(&error["code"]),
                        error["message"].as_str().map(str::to_owned),
                    )
                });
                if let ChannelError::Refused {
                    code: Some(code), ..
                } = &error
                    && code == STOPPED
                {
                    return Err(ChannelError::OptedOut {
                        provider: PROVIDER.to_owned(),
                    });
                }
                return Err(error);
            }
            let answer: Value = response.json().unwrap_or(Value::Null);
            let sent = SentMessage::new(PROVIDER);
            Ok(match answer["messages"][0]["id"].as_str() {
                Some(id) => sent.id(id),
                None => sent,
            })
        })
    }
}

impl fmt::Debug for MetaWhatsApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetaWhatsApp")
            .field("phone_number_id", &self.phone_number_id)
            .field("api_version", &self.api_version)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WhatsAppTemplate;

    #[test]
    fn templates_fill_the_body_parameters() {
        let message = WhatsAppMessage::template(
            WhatsAppTemplate::new("order_shipped", "en_GB")
                .parameter("Anu")
                .parameter("A-1042"),
        );
        assert_eq!(
            body("+447400123456", &message).unwrap(),
            json!({
                "messaging_product": "whatsapp",
                "recipient_type": "individual",
                "to": "447400123456",
                "type": "template",
                "template": {
                    "name": "order_shipped",
                    "language": { "code": "en_GB" },
                    "components": [{ "type": "body", "parameters": [
                        { "type": "text", "text": "Anu" },
                        { "type": "text", "text": "A-1042" },
                    ] }],
                },
            })
        );
    }

    #[test]
    fn text_needs_a_body() {
        assert_eq!(
            body("+1", &WhatsAppMessage::text("hi")).unwrap()["text"]["body"],
            "hi"
        );
        assert!(body("+1", &WhatsAppMessage::default()).is_err());
    }
}
