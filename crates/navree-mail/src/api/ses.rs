//! [`SesTransport`]: Amazon SES v2 `SendEmail`, signed with SigV4.

use std::fmt;

use chrono::Utc;
use navree_contracts::Secret;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::sigv4::{Credentials, Signer};
use super::{address, addresses, client_for, endpoint, join, refusal, unreachable, unusable};
use crate::{MailError, MailTransport, Message, Sent};

/// The settings of an [`SesTransport`] (`{ type = "ses", ... }` or
/// `ses://ACCESS_KEY_ID:SECRET@REGION?configuration_set=..`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SesConfig {
    /// The AWS region (`eu-west-1`).
    pub region: String,
    /// The access key id; empty reads `AWS_ACCESS_KEY_ID` (and
    /// `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`) when the transport is
    /// built.
    pub access_key_id: String,
    /// The secret access key.
    pub secret_access_key: Secret<String>,
    /// A session token, for temporary credentials.
    pub session_token: Option<Secret<String>>,
    /// The configuration set whose event destinations (an SNS topic)
    /// report deliveries, bounces and complaints.
    pub configuration_set: Option<String>,
    /// The API's base URL (default `https://email.<region>.amazonaws.com`).
    pub endpoint: Option<String>,
}

/// Sends through the Amazon SES v2 API (`POST /v2/email/outbound-emails`),
/// each request signed with AWS Signature Version 4.
///
/// The message goes as `Simple` content (subject, text, HTML, extra
/// headers); [`Message::metadata`] becomes message tags (characters other
/// than letters, digits, `_` and `-` are replaced by `_`), and the
/// configuration set is named when configured, so its event destinations
/// report what happens to the message. The provider id is SES's
/// `MessageId`. [`check`](MailTransport::check) calls `GetAccount`.
pub struct SesTransport {
    config: SesConfig,
    credentials: Credentials,
    base: Url,
    client: HttpClient,
}

impl SesTransport {
    /// A transport for `config`. Fails with [`MailError::Config`] when the
    /// region or the credentials are missing.
    pub fn new(config: SesConfig) -> Result<Self, MailError> {
        if config.region.trim().is_empty() {
            return Err(MailError::Config(
                "the ses transport needs a `region`".to_owned(),
            ));
        }
        let credentials = credentials(&config)?;
        let default = format!("https://email.{}.amazonaws.com", config.region);
        let base = endpoint(config.endpoint.as_deref(), &default, "ses")?;
        let client = client_for(config.endpoint.as_ref().map(|_| &base))?;
        Ok(Self {
            config,
            credentials,
            base,
            client,
        })
    }

    /// This transport, sending with `client` (a fake in tests).
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }

    /// The region.
    pub fn region(&self) -> &str {
        &self.config.region
    }

    /// The request body of `message`.
    fn body(&self, message: &Message) -> Result<Value, MailError> {
        let from = message.from.as_deref().ok_or(MailError::MissingSender)?;
        let mut body = json!({});
        if let Some(text) = &message.text {
            body["Text"] = json!({ "Data": text, "Charset": "UTF-8" });
        }
        if let Some(html) = &message.html {
            body["Html"] = json!({ "Data": html, "Charset": "UTF-8" });
        }
        let mut simple = json!({
            "Subject": { "Data": message.subject, "Charset": "UTF-8" },
            "Body": body,
        });
        if !message.headers.is_empty() {
            simple["Headers"] = message
                .headers
                .iter()
                .map(|(name, value)| json!({ "Name": name, "Value": value }))
                .collect();
        }
        let mut request = json!({
            "FromEmailAddress": address("from", from)?,
            "Destination": {
                "ToAddresses": addresses("to", &message.to)?,
                "CcAddresses": addresses("cc", &message.cc)?,
                "BccAddresses": addresses("bcc", &message.bcc)?,
            },
            "Content": { "Simple": simple },
        });
        if let Some(reply_to) = &message.reply_to {
            request["ReplyToAddresses"] = json!([address("reply_to", reply_to)?]);
        }
        if !message.metadata.is_empty() {
            request["EmailTags"] = message
                .metadata
                .iter()
                .map(|(name, value)| json!({ "Name": tag_text(name), "Value": tag_text(value) }))
                .collect();
        }
        if let Some(set) = self.config.configuration_set.as_deref().filter(|s| !s.is_empty()) {
            request["ConfigurationSetName"] = json!(set);
        }
        Ok(request)
    }

    /// The SigV4 headers for a request.
    fn signed(&self, method: &str, url: &Url, body: &[u8]) -> Vec<(String, String)> {
        Signer {
            credentials: &self.credentials,
            region: &self.config.region,
            service: "ses",
        }
        .sign(
            method,
            url,
            &[("content-type", "application/json")],
            body,
            Utc::now(),
        )
    }
}

/// SES accepts letters, digits, `_` and `-` in tag names and values.
fn tag_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The credentials from `config`, or from the environment.
fn credentials(config: &SesConfig) -> Result<Credentials, MailError> {
    let from_env = config.access_key_id.trim().is_empty();
    let (access_key_id, secret_access_key, session_token) = if from_env {
        (
            std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default(),
            std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
            std::env::var("AWS_SESSION_TOKEN").ok(),
        )
    } else {
        (
            config.access_key_id.clone(),
            config.secret_access_key.expose().clone(),
            config
                .session_token
                .as_ref()
                .map(|token| token.expose().clone()),
        )
    };
    if access_key_id.is_empty() || secret_access_key.is_empty() {
        return Err(MailError::Config(
            "the ses transport needs `access_key_id` and `secret_access_key` (or AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY)"
                .to_owned(),
        ));
    }
    Ok(Credentials {
        access_key_id,
        secret_access_key,
        session_token: session_token.filter(|token| !token.is_empty()),
    })
}

impl MailTransport for SesTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move { self.deliver(message).await.map(drop) })
    }

    fn deliver<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<Sent, MailError>> {
        Box::pin(async move {
            let url = join(&self.base, "/v2/email/outbound-emails")?;
            let body = serde_json::to_vec(&self.body(message)?)
                .map_err(|error| MailError::Config(error.to_string()))?;
            let mut request = self
                .client
                .post(url.as_str())
                .header("content-type", "application/json");
            for (name, value) in self.signed("POST", &url, &body) {
                request = request.header(name, value);
            }
            let response = request
                .body(body)
                .send()
                .await
                .map_err(|error| unreachable("ses", error))?;
            if !response.is_success() {
                return Err(refusal("ses", &response));
            }
            let id = response
                .json::<Value>()
                .ok()
                .and_then(|body| body["MessageId"].as_str().map(str::to_owned));
            let sent = Sent::new("ses");
            Ok(match id {
                Some(id) => sent.with_provider_id(id),
                None => sent,
            })
        })
    }

    fn name(&self) -> &'static str {
        "ses"
    }

    fn check(&self) -> BoxFuture<'_, Result<(), MailError>> {
        Box::pin(async move {
            let url = join(&self.base, "/v2/email/account")?;
            let mut request = self
                .client
                .get(url.as_str())
                .header("content-type", "application/json");
            for (name, value) in self.signed("GET", &url, b"") {
                request = request.header(name, value);
            }
            let response = request
                .send()
                .await
                .map_err(|error| MailError::Unusable(format!("ses: {error}")))?;
            if response.is_success() {
                Ok(())
            } else {
                Err(unusable("ses", &response))
            }
        })
    }
}

impl fmt::Debug for SesTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SesTransport")
            .field("region", &self.config.region)
            .field("configuration_set", &self.config.configuration_set)
            .field("endpoint", &self.base.as_str())
            .finish_non_exhaustive()
    }
}
