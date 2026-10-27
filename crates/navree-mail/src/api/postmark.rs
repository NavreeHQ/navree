//! [`PostmarkTransport`]: Postmark's `email` API.

use std::fmt;

use navree_contracts::Secret;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::{address, addresses, client_for, endpoint, join, refusal, unreachable, unusable};
use crate::{MailError, MailTransport, Message, Sent};

/// The settings of a [`PostmarkTransport`] (`{ type = "postmark", ... }`
/// or `postmark://SERVER_TOKEN@api.postmarkapp.com?stream=outbound`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PostmarkConfig {
    /// The server API token.
    pub server_token: Secret<String>,
    /// The message stream (default: the server's `outbound` stream).
    pub message_stream: Option<String>,
    /// The API's base URL (default `https://api.postmarkapp.com`).
    pub endpoint: Option<String>,
}

/// Sends through Postmark's API (`POST /email`, authenticated with the
/// `X-Postmark-Server-Token` header).
///
/// Extra headers go as `Headers`, [`Message::metadata`] as `Metadata`
/// (which Postmark echoes in its webhooks). The provider id is Postmark's
/// `MessageID`. [`check`](MailTransport::check) reads the server.
pub struct PostmarkTransport {
    config: PostmarkConfig,
    base: Url,
    client: HttpClient,
}

impl PostmarkTransport {
    /// A transport for `config`; the server token is required.
    pub fn new(config: PostmarkConfig) -> Result<Self, MailError> {
        if config.server_token.expose().is_empty() {
            return Err(MailError::Config(
                "the postmark transport needs a `server_token`".to_owned(),
            ));
        }
        let base = endpoint(
            config.endpoint.as_deref(),
            "https://api.postmarkapp.com",
            "postmark",
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
    fn body(&self, message: &Message) -> Result<Value, MailError> {
        let from = message.from.as_deref().ok_or(MailError::MissingSender)?;
        let mut body = json!({
            "From": address("from", from)?,
            "To": addresses("to", &message.to)?.join(", "),
            "Subject": message.subject,
        });
        if !message.cc.is_empty() {
            body["Cc"] = json!(addresses("cc", &message.cc)?.join(", "));
        }
        if !message.bcc.is_empty() {
            body["Bcc"] = json!(addresses("bcc", &message.bcc)?.join(", "));
        }
        if let Some(text) = &message.text {
            body["TextBody"] = json!(text);
        }
        if let Some(html) = &message.html {
            body["HtmlBody"] = json!(html);
        }
        if let Some(reply_to) = &message.reply_to {
            body["ReplyTo"] = json!(address("reply_to", reply_to)?);
        }
        if !message.headers.is_empty() {
            body["Headers"] = message
                .headers
                .iter()
                .map(|(name, value)| json!({ "Name": name, "Value": value }))
                .collect();
        }
        if !message.metadata.is_empty() {
            body["Metadata"] = json!(message.metadata);
        }
        if let Some(stream) = self.config.message_stream.as_deref().filter(|s| !s.is_empty()) {
            body["MessageStream"] = json!(stream);
        }
        Ok(body)
    }
}

impl MailTransport for PostmarkTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move { self.deliver(message).await.map(drop) })
    }

    fn deliver<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<Sent, MailError>> {
        Box::pin(async move {
            let url = join(&self.base, "/email")?;
            let response = self
                .client
                .post(url.as_str())
                .header("accept", "application/json")
                .header("x-postmark-server-token", self.config.server_token.expose().as_str())
                .json(&self.body(message)?)
                .send()
                .await
                .map_err(|error| unreachable("postmark", error))?;
            let body = response.json::<Value>().ok();
            let error_code = body
                .as_ref()
                .and_then(|b| b["ErrorCode"].as_i64())
                .unwrap_or(0);
            if !response.is_success() || error_code != 0 {
                return Err(refusal("postmark", &response));
            }
            let id = body.and_then(|b| b["MessageID"].as_str().map(str::to_owned));
            let sent = Sent::new("postmark");
            Ok(match id {
                Some(id) => sent.with_provider_id(id),
                None => sent,
            })
        })
    }

    fn name(&self) -> &'static str {
        "postmark"
    }

    fn check(&self) -> BoxFuture<'_, Result<(), MailError>> {
        Box::pin(async move {
            let url = join(&self.base, "/server")?;
            let response = self
                .client
                .get(url.as_str())
                .header("accept", "application/json")
                .header("x-postmark-server-token", self.config.server_token.expose().as_str())
                .send()
                .await
                .map_err(|error| MailError::Unusable(format!("postmark: {error}")))?;
            if response.is_success() {
                Ok(())
            } else {
                Err(unusable("postmark", &response))
            }
        })
    }
}

impl fmt::Debug for PostmarkTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostmarkTransport")
            .field("message_stream", &self.config.message_stream)
            .field("endpoint", &self.base.as_str())
            .finish_non_exhaustive()
    }
}
