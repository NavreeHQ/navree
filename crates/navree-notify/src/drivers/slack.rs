//! [`SlackApi`]: incoming webhooks and `chat.postMessage`.

use std::fmt;
use std::sync::Arc;

use navree_contracts::{BoxFuture, Secret};
use navree_http_client::HttpClient;
use navree_kernel::WeakAppContext;
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

use super::{
    ChannelError, SentMessage, SlackDriver, client_for, endpoint, join, refused, unreachable,
};
use crate::{ConnectionTokens, SlackMessage, SlackOptions, SlackRoute};

/// The provider name of the Slack driver.
const PROVIDER: &str = "slack";

/// Sends Slack messages: a [`SlackRoute::Webhook`] is posted to its URL,
/// a [`SlackRoute::Bot`] with `chat.postMessage`, authenticated with the
/// connection's token (through the app's
/// [`ConnectionTokens`]) or with `notify.slack.bot_token`. The message id
/// is the posted message's `ts` (bot routes).
pub struct SlackApi {
    base: Url,
    bot_token: Secret<String>,
    client: HttpClient,
    tokens: Tokens,
}

impl SlackApi {
    /// A driver for `options`.
    pub fn new(options: &SlackOptions) -> Result<Self, String> {
        let base = endpoint(
            options.endpoint.as_deref(),
            "https://slack.com/api",
            "notify.slack.endpoint",
        )?;
        let client = client_for(options.endpoint.as_ref().map(|_| &base))?;
        Ok(Self {
            base,
            bot_token: options.bot_token.clone(),
            client,
            tokens: Tokens::default(),
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

    async fn webhook(
        &self,
        url: &str,
        message: &SlackMessage,
    ) -> Result<SentMessage, ChannelError> {
        let response = self
            .client
            .post(url)
            .secret_url()
            .json(&message.payload())
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))?;
        if !response.is_success() {
            let code = response.text().trim().to_owned();
            return Err(ChannelError::Refused {
                provider: PROVIDER.to_owned(),
                status: response.status().as_u16(),
                code: (!code.is_empty()).then(|| code.clone()),
                message: if code.is_empty() {
                    "the webhook refused the message".to_owned()
                } else {
                    code
                },
            });
        }
        Ok(SentMessage::new(PROVIDER))
    }

    async fn post_message(
        &self,
        channel: &str,
        connection: Option<Uuid>,
        message: &SlackMessage,
    ) -> Result<SentMessage, ChannelError> {
        let token = match connection {
            Some(connection) => self.tokens.token(connection).await?,
            None if self.bot_token.expose().is_empty() => {
                return Err(ChannelError::Config(
                    "a Slack channel route needs notify.slack.bot_token or a connection".into(),
                ));
            }
            None => self.bot_token.clone(),
        };
        let mut body = message.payload();
        body["channel"] = json!(channel);
        let response = self
            .client
            .post(join(&self.base, "/chat.postMessage"))
            .bearer(token.expose())
            .json(&body)
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))?;
        if !response.is_success() {
            return Err(refused(PROVIDER, &response, |body| {
                (body["error"].as_str().map(str::to_owned), None)
            }));
        }
        let answer: Value = response.json().unwrap_or(Value::Null);
        if answer["ok"] != true {
            let code = answer["error"]
                .as_str()
                .unwrap_or("unknown_error")
                .to_owned();
            return Err(ChannelError::Refused {
                provider: PROVIDER.to_owned(),
                status: response.status().as_u16(),
                code: Some(code.clone()),
                message: code,
            });
        }
        let sent = SentMessage::new(PROVIDER);
        Ok(match answer["ts"].as_str() {
            Some(ts) => sent.id(ts),
            None => sent,
        })
    }
}

impl SlackDriver for SlackApi {
    fn send<'a>(
        &'a self,
        route: &'a SlackRoute,
        message: &'a SlackMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            match route {
                SlackRoute::Webhook { url } => self.webhook(url, message).await,
                SlackRoute::Bot {
                    channel,
                    connection,
                } => self.post_message(channel, *connection, message).await,
            }
        })
    }
}

impl fmt::Debug for SlackApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlackApi")
            .field("base", &self.base.as_str())
            .finish_non_exhaustive()
    }
}

/// Where bot routes with a `connection` get their token: a given
/// [`ConnectionTokens`], or the app's.
#[derive(Clone, Default)]
pub(crate) struct Tokens {
    direct: Option<Arc<dyn ConnectionTokens>>,
    app: Option<WeakAppContext>,
}

impl Tokens {
    pub(crate) fn direct(tokens: Arc<dyn ConnectionTokens>) -> Self {
        Self {
            direct: Some(tokens),
            app: None,
        }
    }

    pub(crate) fn app(app: WeakAppContext) -> Self {
        Self {
            direct: None,
            app: Some(app),
        }
    }

    /// The token of `connection`.
    pub(crate) async fn token(&self, connection: Uuid) -> Result<Secret<String>, ChannelError> {
        let tokens = match (&self.direct, &self.app) {
            (Some(tokens), _) => Some(tokens.clone()),
            (None, Some(app)) => match app.upgrade() {
                Some(ctx) => ctx.try_get::<dyn ConnectionTokens>().await,
                None => None,
            },
            (None, None) => None,
        };
        let tokens = tokens.ok_or_else(|| {
            ChannelError::Config(format!(
                "the route names connection {connection}, but the app has no `dyn ConnectionTokens` (add IntegrationsModule with navree-integrations' `notify` feature)"
            ))
        })?;
        tokens.token(connection).await.map_err(|error| {
            ChannelError::Config(format!("no token for connection {connection}: {error}"))
        })
    }
}
