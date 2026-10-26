//! [`NotifyError`].

use navree_kernel::BoxError;
use navree_mail::MailError;
use sea_orm::DbErr;

use crate::Channel;

/// An error raised while sending a notification or reading stored ones.
///
/// Messages never contain notification data or mail bodies.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NotifyError {
    /// A database query failed (the database channel or a read-state query).
    #[error("notification database error: {0}")]
    Database(#[from] DbErr),
    /// The mail channel failed to send.
    #[error("the mail channel failed: {0}")]
    Mail(#[from] MailError),
    /// The broadcast channel failed.
    #[error("the broadcast channel failed: {0}")]
    Broadcast(#[source] BoxError),
    /// Dispatching job `notify.send` failed.
    #[error("queueing the notification failed: {0}")]
    Queue(#[source] BoxError),
    /// A channel's service is missing from the app (`DatabaseModule` for
    /// the database channel, `MailModule` for mail, a `dyn Broadcaster` such
    /// as `RealtimeModule` for broadcast), or the app has shut down
    /// (`channel` is `None`).
    #[error("{} is not available: {reason}", describe(*.channel))]
    Unavailable {
        /// The channel, or `None` when nothing can be sent.
        channel: Option<Channel>,
        /// What is missing.
        reason: String,
    },
    /// Several channels failed; each error in channel order.
    #[error("{} notification channels failed: {}", .0.len(), list(.0))]
    Several(Vec<NotifyError>),
}

impl NotifyError {
    /// One error from a list of failures: `None` when the list is empty,
    /// the error itself when there is one, [`NotifyError::Several`] otherwise.
    pub(crate) fn collect(mut errors: Vec<NotifyError>) -> Option<NotifyError> {
        match errors.len() {
            0 => None,
            1 => errors.pop(),
            _ => Some(NotifyError::Several(errors)),
        }
    }
}

/// `the mail channel`, or `the notifier`.
fn describe(channel: Option<Channel>) -> String {
    match channel {
        Some(channel) => format!("the {channel} channel"),
        None => "the notifier".to_owned(),
    }
}

/// `1) first; 2) second`.
fn list(errors: &[NotifyError]) -> String {
    errors
        .iter()
        .enumerate()
        .map(|(i, error)| format!("{}) {error}", i + 1))
        .collect::<Vec<_>>()
        .join("; ")
}
