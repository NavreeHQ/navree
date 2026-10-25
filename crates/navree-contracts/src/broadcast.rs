//! [`Broadcaster`]: push events to real-time channels.

use serde_json::Value;

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
}
