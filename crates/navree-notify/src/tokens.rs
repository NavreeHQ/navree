//! [`ConnectionTokens`]: bearer tokens of the app's third-party
//! connections, for bot routes that name one.

use navree_contracts::{BoxFuture, Secret};
use navree_kernel::BoxError;
use uuid::Uuid;

/// Gives the access token of a connection to a third-party API, so a
/// [`SlackRoute`](crate::SlackRoute) or [`TeamsRoute`](crate::TeamsRoute)
/// that names a `connection` is sent with that connection's token (a
/// workspace's Slack install, a customer tenant's Teams bot).
///
/// `IntegrationsModule` registers one (`Arc<dyn ConnectionTokens>`) when
/// navree-integrations is built with its `notify` feature; the token comes
/// from `Integrations::token`, refreshed when it is due. Register your own
/// for connections kept elsewhere.
///
/// ```
/// use navree_contracts::{BoxFuture, Secret};
/// use navree_kernel::BoxError;
/// use navree_notify::ConnectionTokens;
/// use uuid::Uuid;
///
/// struct OneToken(String);
///
/// impl ConnectionTokens for OneToken {
///     fn token(&self, _connection: Uuid) -> BoxFuture<'_, Result<Secret<String>, BoxError>> {
///         let token = Secret::new(self.0.clone());
///         Box::pin(async move { Ok(token) })
///     }
/// }
/// ```
pub trait ConnectionTokens: Send + Sync + 'static {
    /// The current access token of `connection`.
    fn token(&self, connection: Uuid) -> BoxFuture<'_, Result<Secret<String>, BoxError>>;
}
