//! [`NotifyError`].

use std::collections::BTreeMap;

use navree_db::DbError;
use navree_kernel::BoxError;
use navree_mail::MailError;
use navree_orm::OrmError;

use crate::{Channel, ChannelError};

/// An error raised while sending a notification or reading stored ones.
///
/// Messages never contain notification data or mail bodies.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NotifyError {
    /// A database query failed (the database channel or a read-state query).
    #[error("notification database error: {0}")]
    Database(#[from] DbError),
    /// An ORM operation on a channel table failed (the database channel,
    /// the delivery log, preferences, held deliveries, opt-outs or push
    /// subscriptions).
    #[error("notification database error: {0}")]
    Orm(#[from] OrmError),
    /// The mail channel failed to send.
    #[error("the mail channel failed: {0}")]
    Mail(#[from] MailError),
    /// The broadcast channel failed.
    #[error("the broadcast channel failed: {0}")]
    Broadcast(#[source] BoxError),
    /// A Slack, Teams, SMS, WhatsApp or web push driver did not send the
    /// message.
    #[error("the {channel} channel failed: {source}")]
    Channel {
        /// The channel.
        channel: Channel,
        /// What the driver reported.
        #[source]
        source: ChannelError,
    },
    /// A recipient address is not one the channel can use (a number that is
    /// not E.164, an invalid push subscription).
    #[error("invalid recipient: {0}")]
    InvalidRecipient(String),
    /// Dispatching job `notify.send` failed.
    #[error("queueing the notification failed: {0}")]
    Queue(#[source] BoxError),
    /// A channel's service is missing from the app (`DatabaseModule` for
    /// the database channel and the channel tables, `MailModule` for mail,
    /// a `dyn Broadcaster` such as `RealtimeModule` for broadcast, a driver
    /// for SMS, WhatsApp and web push), or the app has shut down
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
    /// A change to notification preferences was refused: the messages by
    /// field (`categories.security.channels.mail`), as a 422 response
    /// shows them.
    #[error("invalid notification preferences: {}", fields(.0))]
    InvalidPreferences(BTreeMap<String, Vec<String>>),
}

/// `field: message; field: message`.
fn fields(errors: &BTreeMap<String, Vec<String>>) -> String {
    errors
        .iter()
        .map(|(field, messages)| format!("{field}: {}", messages.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
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
