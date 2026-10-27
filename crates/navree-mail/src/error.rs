//! [`MailError`]: everything that can go wrong while building or sending mail.

use navree_kernel::BoxError;
use navree_view::ViewError;

/// An error raised while building, validating, queueing or sending a message.
///
/// Messages name addresses and transports but never contain message bodies
/// or credentials: a body may hold a password-reset link, and an SMTP URL a
/// password.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MailError {
    /// A mailable's template failed to render.
    #[error(transparent)]
    View(#[from] ViewError),
    /// An address does not parse (`ada@example.com` or `Ada <ada@example.com>`).
    #[error("invalid `{field}` address `{address}`: {reason}")]
    InvalidAddress {
        /// The field holding it: `from`, `to`, `cc`, `bcc` or `reply_to`.
        field: &'static str,
        /// The address as given.
        address: String,
        /// Why it does not parse.
        reason: String,
    },
    /// A header value contains a line break (a header-injection attempt).
    #[error("the `{field}` header must not contain line breaks")]
    InvalidHeader {
        /// The header (`subject`).
        field: &'static str,
    },
    /// The message has no `to`, `cc` or `bcc` recipient.
    #[error("the message has no recipients")]
    NoRecipients,
    /// The message has no sender and the mailer no default one.
    #[error("the message has no sender (set `mail.from`)")]
    MissingSender,
    /// The mail configuration is invalid (an SMTP URL, an empty failover
    /// list, ...). The message never repeats a URL, which may hold a password.
    #[error("invalid mail configuration: {0}")]
    Config(String),
    /// A transport failed to deliver the message.
    #[error("mail transport `{transport}` failed: {source}")]
    Transport {
        /// The transport that failed (`smtp`, `log`, ...).
        transport: &'static str,
        /// The underlying error.
        #[source]
        source: BoxError,
    },
    /// [`Mailer::queue`](crate::Mailer::queue) could not dispatch the job.
    #[error("queueing the message failed: {source}")]
    Queue {
        /// The dispatcher's error.
        #[source]
        source: BoxError,
    },
    /// Every transport of a [`FailoverTransport`](crate::FailoverTransport) failed.
    #[error("every mail transport failed: {}", list(.errors))]
    AllTransportsFailed {
        /// Each transport's error, in order.
        errors: Vec<MailError>,
    },
    /// An extra header is not allowed: an invalid name, a line break in the
    /// value, or a header the transports write themselves.
    #[error("invalid header `{name}`: {reason}")]
    Header {
        /// The header name as given.
        name: String,
        /// Why it is refused.
        reason: &'static str,
    },
    /// A mail provider's API refused the request (`ses`, `mailgun`,
    /// `postmark`, `resend`). The message is the provider's own, never the
    /// request or its credentials.
    #[error("mail provider `{transport}` answered {status}: {message}")]
    Provider {
        /// The transport (`ses`, ...).
        transport: &'static str,
        /// The HTTP status.
        status: u16,
        /// The provider's error message.
        message: String,
    },
    /// A feature needs something the app does not have (`hold` needs
    /// `mail.delivery.enabled`, the delivery log needs `DatabaseModule`, ...).
    #[error("{0}")]
    Missing(&'static str),
    /// A delivery, outbox message, suppression or tenant transport does not
    /// exist (or belongs to another scope).
    #[error("{0} not found")]
    NotFound(&'static str),
    /// The record is not in a state that allows the change (releasing a
    /// message that is no longer held, ...).
    #[error("{0}")]
    Conflict(String),
    /// A transport failed its `usable` check: it could not authenticate or
    /// reach its server.
    #[error("the mail transport is not usable: {0}")]
    Unusable(String),
    /// A DNS lookup failed (other than "no such record").
    #[error("DNS lookup of `{name}` failed: {reason}")]
    Dns {
        /// The name looked up.
        name: String,
        /// Why it failed.
        reason: String,
    },
    /// Reading or writing the delivery tables failed.
    #[error("the mail delivery store failed: {source}")]
    Store {
        /// The underlying error.
        #[source]
        source: BoxError,
    },
}

impl From<sea_orm::DbErr> for MailError {
    fn from(error: sea_orm::DbErr) -> Self {
        MailError::Store {
            source: Box::new(error),
        }
    }
}

impl From<navree_crypto::CryptoError> for MailError {
    fn from(error: navree_crypto::CryptoError) -> Self {
        MailError::Store {
            source: Box::new(error),
        }
    }
}

/// `1) first; 2) second`, or `no transport configured`.
fn list(errors: &[MailError]) -> String {
    if errors.is_empty() {
        return "no transport configured".to_owned();
    }
    errors
        .iter()
        .enumerate()
        .map(|(i, error)| format!("{}) {error}", i + 1))
        .collect::<Vec<_>>()
        .join("; ")
}
