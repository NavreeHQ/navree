//! [`TeamsApi`]: incoming webhooks and proactive Bot Framework messages.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use navree_contracts::{BoxFuture, Secret};
use navree_http_client::HttpClient;
use navree_kernel::WeakAppContext;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use super::slack::Tokens;
use super::{ChannelError, SentMessage, TeamsDriver, client_for, endpoint, refused, text_of, unreachable};
use crate::{ConnectionTokens, TeamsMessage, TeamsOptions, TeamsRoute};

/// The provider name of the Teams driver.
const PROVIDER: &str = "teams";

/// The scope of Bot Framework tokens.
pub(crate) const BOT_FRAMEWORK_SCOPE: &str = "https://api.botframework.com/.default";

/// Sends Microsoft Teams messages: a [`TeamsRoute::Webhook`] is posted to
/// its URL with the message as an Adaptive Card; a [`TeamsRoute::Bot`] is
/// sent as a proactive message (`POST {serviceUrl}/v3/conversations/{id}/activities`)
/// with the connection's token (through the app's [`ConnectionTokens`]) or
/// a Bot Framework token for `notify.teams.app_id` (client credentials,
/// cached until a minute before it expires). The message id is the new
/// activity's id.
pub struct TeamsApi {
    app_id: Option<String>,
    app_password: Secret<String>,
    token_url: Url,
    client: HttpClient,
    tokens: Tokens,
    cached: Mutex<Option<(Secret<String>, Instant)>>,
}

impl TeamsApi {
    /// A driver for `options`.
    pub fn new(options: &TeamsOptions) -> Result<Self, String> {
        let default = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            options.tenant
        );
        let token_url = endpoint(options.token_url.as_deref(), &default, "notify.teams.token_url")?;
        let client = client_for(options.token_url.as_ref().map(|_| &token_url))?;
        Ok(Self {
            app_id: options.app_id.clone().filter(|id| !id.is_empty()),
            app_password: options.app_password.clone(),
            token_url,
            client,
            tokens: Tokens::default(),
            cached: Mutex::new(None),
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }

    /// This driver, taking connection tokens from `tokens`.
    #[must_use]
    pub fn with_tokens(mut self, tokens: Arc<dyn ConnectionTokens>) -> Self {
        self.tokens = Tokens::direct(tokens);
        self
    }

    /// This driver, taking connection tokens from the app's
    /// `dyn ConnectionTokens`.
    #[must_use]
    pub(crate) fn with_app(mut self, app: WeakAppContext) -> Self {
        self.tokens = Tokens::app(app);
        self
    }

    async fn webhook(&self, url: &str, message: &TeamsMessage) -> Result<SentMessage, ChannelError> {
        let response = self
            .client
            .post(url)
            .json(&message.webhook_payload())
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))?;
        if !response.is_success() {
            return Err(refused(PROVIDER, &response, |body| {
                let error = &body["error"];
                (
                    error["code"].as_str().map(str::to_owned),
                    error["message"].as_str().map(str::to_owned),
                )
            }));
        }
        Ok(SentMessage::new(PROVIDER))
    }

    async fn proactive(
        &self,
        service_url: &str,
        conversation_id: &str,
        connection: Option<Uuid>,
        message: &TeamsMessage,
    ) -> Result<SentMessage, ChannelError> {
        let token = match connection {
            Some(connection) => self.tokens.token(connection).await?,
            None => self.bot_token().await?,
        };
        let url = activities_url(service_url, conversation_id)?;
        let response = self
            .client
            .post(url.as_str())
            .bearer(token.expose())
            .json(&message.activity())
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))?;
        if !response.is_success() {
            return Err(refused(PROVIDER, &response, |body| {
                let error = &body["error"];
                (
                    error["code"].as_str().map(str::to_owned),
                    error["message"].as_str().map(str::to_owned),
                )
            }));
        }
        let sent = SentMessage::new(PROVIDER);
        let answer: Value = response.json().unwrap_or(Value::Null);
        Ok(match text_of(&answer["id"]) {
            Some(id) => sent.id(id),
            None => sent,
        })
    }

    /// A Bot Framework token for the configured bot, cached.
    async fn bot_token(&self) -> Result<Secret<String>, ChannelError> {
        if let Some((token, until)) = &*self.cached.lock().unwrap_or_else(PoisonError::into_inner)
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let Some(app_id) = &self.app_id else {
            return Err(ChannelError::Config(
                "a Teams conversation route needs notify.teams.app_id or a connection".into(),
            ));
        };
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", app_id.as_str()),
            ("client_secret", self.app_password.expose().as_str()),
            ("scope", BOT_FRAMEWORK_SCOPE),
        ];
        let response = self
            .client
            .post(self.token_url.as_str())
            .form(&form)
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))?;
        if !response.is_success() {
            return Err(refused(PROVIDER, &response, |body| {
                (
                    body["error"].as_str().map(str::to_owned),
                    body["error_description"]
                        .as_str()
                        .map(|d| d.lines().next().unwrap_or(d).to_owned()),
                )
            }));
        }
        let answer: Value = response.json().unwrap_or(Value::Null);
        let token = answer["access_token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| ChannelError::Refused {
                provider: PROVIDER.to_owned(),
                status: response.status().as_u16(),
                code: None,
                message: "the token response has no access_token".to_owned(),
            })?;
        let lifetime = answer["expires_in"].as_u64().unwrap_or(3600);
        let token = Secret::new(token.to_owned());
        let until = Instant::now() + Duration::from_secs(lifetime.saturating_sub(60));
        *self.cached.lock().unwrap_or_else(PoisonError::into_inner) = Some((token.clone(), until));
        Ok(token)
    }
}

/// `{service_url}/v3/conversations/{conversation_id}/activities`, the id
/// escaped as one path segment.
fn activities_url(service_url: &str, conversation_id: &str) -> Result<Url, ChannelError> {
    let mut url = Url::parse(service_url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .ok_or_else(|| ChannelError::InvalidRecipient("the Teams service URL is not an http(s) URL".into()))?;
    url.path_segments_mut()
        .map_err(|()| ChannelError::InvalidRecipient("the Teams service URL cannot hold a path".into()))?
        .pop_if_empty()
        .extend(["v3", "conversations", conversation_id, "activities"]);
    Ok(url)
}

impl TeamsDriver for TeamsApi {
    fn send<'a>(
        &'a self,
        route: &'a TeamsRoute,
        message: &'a TeamsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            match route {
                TeamsRoute::Webhook { url } => self.webhook(url, message).await,
                TeamsRoute::Bot {
                    service_url,
                    conversation_id,
                    connection,
                } => {
                    self.proactive(service_url, conversation_id, *connection, message)
                        .await
                }
            }
        })
    }
}

impl fmt::Debug for TeamsApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TeamsApi")
            .field("app_id", &self.app_id)
            .field("token_url", &self.token_url.as_str())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_ids_are_escaped_as_one_segment() {
        let url = activities_url("https://smba.trafficmanager.net/emea/", "a:1X/y z").unwrap();
        assert_eq!(
            url.as_str(),
            "https://smba.trafficmanager.net/emea/v3/conversations/a:1X%2Fy%20z/activities"
        );
        let url = activities_url("https://smba.trafficmanager.net/amer", "19:abc@thread.skype").unwrap();
        assert_eq!(
            url.as_str(),
            "https://smba.trafficmanager.net/amer/v3/conversations/19:abc@thread.skype/activities"
        );
        assert!(activities_url("ftp://x", "a").is_err());
    }
}
