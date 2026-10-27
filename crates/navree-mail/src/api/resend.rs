//! [`ResendTransport`]: Resend's `emails` API.

use std::fmt;

use navree_contracts::Secret;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::{address, addresses, client_for, endpoint, join, refusal, unreachable, unusable};
use crate::{MailError, MailTransport, Message, Sent};

/// The settings of a [`ResendTransport`] (`{ type = "resend", ... }` or
/// `resend://API_KEY@api.resend.com`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ResendConfig {
    /// The API key (`re_...`).
    pub api_key: Secret<String>,
    /// The API's base URL (default `https://api.resend.com`).
    pub endpoint: Option<String>,
}

/// Sends through Resend's API (`POST /emails`, a bearer token).
///
/// Extra headers go as `headers`, [`Message::metadata`] as `tags`. The
/// provider id is Resend's `id`. [`check`](MailTransport::check) lists the
/// account's domains.
pub struct ResendTransport {
    config: ResendConfig,
    base: Url,
    client: HttpClient,
}

impl ResendTransport {
    /// A transport for `config`; the API key is required.
    pub fn new(config: ResendConfig) -> Result<Self, MailError> {
        if config.api_key.expose().is_empty() {
            return Err(MailError::Config(
                "the resend transport needs an `api_key`".to_owned(),
            ));
        }
        let base = endpoint(
            config.endpoint.as_deref(),
            "https://api.resend.com",
            "resend",
        )?;
        let client = client_for(config.endpoint.as_ref().map(|_| &base))?;
        Ok(Self {
            config,
            base,
            client,
        })
    }

    /// This transport, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }

    /// The request body of `message`.
    fn body(message: &Message) -> Result<Value, MailError> {
        let from = message.from.as_deref().ok_or(MailError::MissingSender)?;
        let mut body = json!({
            "from": address("from", from)?,
            "to": addresses("to", &message.to)?,
            "subject": message.subject,
        });
        if !message.cc.is_empty() {
            body["cc"] = json!(addresses("cc", &message.cc)?);
        }
        if !message.bcc.is_empty() {
            body["bcc"] = json!(addresses("bcc", &message.bcc)?);
        }
        if let Some(text) = &message.text {
            body["text"] = json!(text);
        }
        if let Some(html) = &message.html {
            body["html"] = json!(html);
        }
        if let Some(reply_to) = &message.reply_to {
            body["reply_to"] = json!(address("reply_to", reply_to)?);
        }
        if !message.headers.is_empty() {
            body["headers"] = json!(message.headers);
        }
        if !message.metadata.is_empty() {
            body["tags"] = message
                .metadata
                .iter()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect();
        }
        Ok(body)
    }
}

impl MailTransport for ResendTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move { self.deliver(message).await.map(drop) })
    }

    fn deliver<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<Sent, MailError>> {
        Box::pin(async move {
            let url = join(&self.base, "/emails")?;
            let response = self
                .client
                .post(url.as_str())
                .bearer(self.config.api_key.expose())
                .json(&Self::body(message)?)
                .send()
                .await
                .map_err(|error| unreachable("resend", error))?;
            if !response.is_success() {
                return Err(refusal("resend", &response));
            }
            let id = response
                .json::<Value>()
                .ok()
                .and_then(|body| body["id"].as_str().map(str::to_owned));
            let sent = Sent::new("resend");
            Ok(match id {
                Some(id) => sent.with_provider_id(id),
                None => sent,
            })
        })
    }

    fn name(&self) -> &'static str {
        "resend"
    }

    fn check(&self) -> BoxFuture<'_, Result<(), MailError>> {
        Box::pin(async move {
            let url = join(&self.base, "/domains")?;
            let response = self
                .client
                .get(url.as_str())
                .bearer(self.config.api_key.expose())
                .send()
                .await
                .map_err(|error| MailError::Unusable(format!("resend: {error}")))?;
            if response.is_success() {
                Ok(())
            } else {
                Err(unusable("resend", &response))
            }
        })
    }
}

impl fmt::Debug for ResendTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResendTransport")
            .field("endpoint", &self.base.as_str())
            .finish_non_exhaustive()
    }
}
