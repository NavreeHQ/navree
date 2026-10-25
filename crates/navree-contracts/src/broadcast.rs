//! [`Broadcaster`]: push events to real-time channels.

use serde_json::Value;
use uuid::Uuid;

use crate::{BoxError, BoxFuture};

/// Sends an event to real-time channels (implemented by navree-realtime).
pub trait Broadcaster: Send + Sync + 'static {
    /// Broadcasts `event` with `data` to every channel in `channels`.
    fn broadcast(
        &self,
        channels: Vec<String>,
        event: String,
        data: Value,
    ) -> BoxFuture<'_, Result<(), BoxError>>;

    /// Ends every real-time subscription of user `user_id`, because what
    /// they may see changed (their roles or permissions) or they signed
    /// out: their connections close with a reconnect code, so clients
    /// connect again and authorize each private channel with their current
    /// credentials, and channels they may no longer join stay closed.
    ///
    /// The default does nothing (broadcasters without connections).
    fn end_user_subscriptions(&self, user_id: Uuid) -> BoxFuture<'_, Result<(), BoxError>> {
        let _ = user_id;
        Box::pin(async { Ok(()) })
    }
}
