//! [`IntegrationTokens`]: connections as the tokens of navree-notify's bot
//! routes (crate feature `notify`).

use std::fmt;
use std::sync::Arc;

use navree_contracts::{BoxFuture, Secret};
use navree_kernel::BoxError;
use navree_notify::ConnectionTokens;
use uuid::Uuid;

use crate::Integrations;

/// navree-notify's `dyn ConnectionTokens`, registered by
/// `IntegrationsModule`: a Slack or Microsoft Teams route that names a
/// `connection` is sent with that connection's current access token
/// ([`Integrations::token`]: cached, refreshed or minted under a lock).
///
/// So a workspace's Slack install posts with its own bot token
/// (`SlackRoute::channel(..).connection(id)`), and a customer tenant's
/// Teams bot sends proactive messages with an app-only token of a
/// `microsoft` connection whose scope set asks for
/// `https://api.botframework.com/.default`
/// (`TeamsRoute::conversation(..).connection(id)`).
pub struct IntegrationTokens {
    integrations: Arc<Integrations>,
}

impl IntegrationTokens {
    /// Tokens of `integrations`' connections.
    pub fn new(integrations: Arc<Integrations>) -> Self {
        Self { integrations }
    }
}

impl ConnectionTokens for IntegrationTokens {
    fn token(&self, connection: Uuid) -> BoxFuture<'_, Result<Secret<String>, BoxError>> {
        Box::pin(async move { Ok(self.integrations.token(connection).await?) })
    }
}

impl fmt::Debug for IntegrationTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntegrationTokens").finish_non_exhaustive()
    }
}
