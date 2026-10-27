//! [`MailgunTransport`]: Mailgun's `messages` API.

use std::fmt;

use navree_contracts::Secret;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use super::{address, basic, client_for, endpoint, join, refusal, unreachable, unusable};
use crate::{MailError, MailTransport, Message, Sent};

/// Where a Mailgun account lives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MailgunRegion {
    /// `https://api.mailgun.net` (default).
    #[default]
    Us,
    /// `https://api.eu.mailgun.net`.
    Eu,
}

/// The settings of a [`MailgunTransport`] (`{ type = "mailgun", ... }` or
/// `mailgun://API_KEY@DOMAIN?region=eu`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MailgunConfig {
    /// The sending domain (`mg.acme.test`).
    pub domain: String,
    /// The API key.
    pub api_key: Secret<String>,
    /// The account's region (default `us`).
    pub region: MailgunRegion,
    /// The API's base URL (default from the region).
    pub endpoint: Option<String>,
}

/// Sends through Mailgun's API (`POST /v3/<domain>/messages`, a form
/// authenticated with `api:<key>`).
///
/// Extra headers go as `h:<name>`, [`Message::metadata`] as `v:<key>`
/// variables (which Mailgun echoes in its events). The provider id is the
/// returned `id` without its angle brackets, as Mailgun's events carry it.
/// [`check`](MailTransport::check) reads the domain.
pub struct MailgunTransport {
    config: MailgunConfig,
    base: Url,
    client: HttpClient,
}

impl MailgunTransport {
    /// A transport for `config`; the domain and the API key are required.
    pub fn new(config: MailgunConfig) -> Result<Self, MailError> {
        if config.domain.trim().is_empty() || config.api_key.expose().is_empty() {
            return Err(MailError::Config(
                "the mailgun transport needs a `domain` and an `api_key`".to_owned(),
            ));
        }
        let default = match config.region {
            MailgunRegion::Us => "https://api.mailgun.net",
            MailgunRegion::Eu => "https://api.eu.mailgun.net",
        };
        let base = endpoint(config.endpoint.as_deref(), default, "mailgun")?;
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

    /// The sending domain.
    pub fn domain(&self) -> &str {
        &self.config.domain
    }

    /// The form fields of `message`.
    fn form(message: &Message) -> Result<Vec<(String, String)>, MailError> {
        let from = message.from.as_deref().ok_or(MailError::MissingSender)?;
        let mut form = vec![("from".to_owned(), address("from", from)?)];
        for (field, list) in [("to", &message.to), ("cc", &message.cc), ("bcc", &message.bcc)] {
            for recipient in list {
                form.push((field.to_owned(), address(field, recipient)?));
            }
        }
        form.push(("subject".to_owned(), message.subject.clone()));
        if let Some(text) = &message.text {
            form.push(("text".to_owned(), text.clone()));
        }
        if let Some(html) = &message.html {
            form.push(("html".to_owned(), html.clone()));
        }
        if let Some(reply_to) = &message.reply_to {
            form.push(("h:Reply-To".to_owned(), address("reply_to", reply_to)?));
        }
        for (name, value) in &message.headers {
            form.push((format!("h:{name}"), value.clone()));
        }
        for (key, value) in &message.metadata {
            form.push((format!("v:{key}"), value.clone()));
        }
        Ok(form)
    }
}

impl MailTransport for MailgunTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move { self.deliver(message).await.map(drop) })
    }

    fn deliver<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<Sent, MailError>> {
        Box::pin(async move {
            let url = join(&self.base, &format!("/v3/{}/messages", self.config.domain))?;
            let response = self
                .client
                .post(url.as_str())
                .header("authorization", basic("api", self.config.api_key.expose()))
                .form(&Self::form(message)?)
                .send()
                .await
                .map_err(|error| unreachable("mailgun", error))?;
            if !response.is_success() {
                return Err(refusal("mailgun", &response));
            }
            let id = response.json::<Value>().ok().and_then(|body| {
                body["id"]
                    .as_str()
                    .map(|id| id.trim_start_matches('<').trim_end_matches('>').to_owned())
            });
            let sent = Sent::new("mailgun");
            Ok(match id {
                Some(id) => sent.with_provider_id(id),
                None => sent,
            })
        })
    }

    fn name(&self) -> &'static str {
        "mailgun"
    }

    fn check(&self) -> BoxFuture<'_, Result<(), MailError>> {
        Box::pin(async move {
            let url = join(&self.base, &format!("/v3/domains/{}", self.config.domain))?;
            let response = self
                .client
                .get(url.as_str())
                .header("authorization", basic("api", self.config.api_key.expose()))
                .send()
                .await
                .map_err(|error| MailError::Unusable(format!("mailgun: {error}")))?;
            if response.is_success() {
                Ok(())
            } else {
                Err(unusable("mailgun", &response))
            }
        })
    }
}

impl fmt::Debug for MailgunTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailgunTransport")
            .field("domain", &self.config.domain)
            .field("endpoint", &self.base.as_str())
            .finish_non_exhaustive()
    }
}
