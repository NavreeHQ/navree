//! [`EventsError`] and [`ListenerFailure`].

use std::fmt;

use navis_kernel::BoxError;

/// Errors from [`EventBus`](crate::EventBus) operations. They are returned
/// as a `BoxError`; downcast to inspect them:
///
/// ```
/// # use navis_events::EventsError;
/// # fn inspect(error: navis_kernel::BoxError) {
/// if let Some(EventsError::ListenersFailed { failures, .. }) = error.downcast_ref() {
///     for failure in failures {
///         eprintln!("{} failed: {}", failure.listener, failure.error);
///     }
/// }
/// # }
/// ```
#[derive(Debug, thiserror::Error)]
pub enum EventsError {
    /// One or more listeners failed; every other listener still ran.
    #[error("{} listener(s) failed for event `{event}`: {}", failures.len(), list(failures))]
    ListenersFailed {
        /// The event name.
        event: String,
        /// Every failure, in delivery order.
        failures: Vec<ListenerFailure>,
    },
    /// A published JSON payload does not deserialize into the event type
    /// registered under that name; nothing was delivered.
    #[error("invalid payload for event `{event}`: {source}")]
    InvalidPayload {
        /// The event name.
        event: String,
        /// The deserialization error.
        source: serde_json::Error,
    },
    /// The app has shut down; the bus no longer delivers events.
    #[error("the app has shut down: events can no longer be delivered")]
    ShutDown,
}

/// One listener that failed to handle (or to queue) an event.
#[derive(Debug)]
pub struct ListenerFailure {
    /// The listener type name.
    pub listener: &'static str,
    /// The module that registered the listener.
    pub module: &'static str,
    /// What went wrong.
    pub error: BoxError,
}

impl fmt::Display for ListenerFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` (module `{}`): {}",
            self.listener, self.module, self.error
        )
    }
}

fn list(failures: &[ListenerFailure]) -> String {
    failures
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
