//! [`EventSink`]: publish named events without depending on navree-events.

use serde_json::Value;

use crate::{BoxError, BoxFuture};

/// Publishes an event by name with a JSON payload (implemented by navree-events).
pub trait EventSink: Send + Sync + 'static {
    /// Publishes `payload` as event `name`.
    fn publish(&self, name: &str, payload: Value) -> BoxFuture<'_, Result<(), BoxError>>;
}
