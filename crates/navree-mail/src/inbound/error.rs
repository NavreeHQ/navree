//! [`InboundError`]: what can go wrong while receiving mail.

use http::StatusCode;
use navree_http::AppError;
use navree_kernel::BoxError;

/// The `code` of a message over `mail.inbound.max_size` (413).
pub const TOO_LARGE_CODE: &str = "mail_too_large";

/// The `code` of a payload or message that cannot be read (400).
pub const INVALID_MAIL_CODE: &str = "invalid_inbound_mail";

/// The `code` of a reply token that is forged, damaged or expired (400).
pub const INVALID_REPLY_TOKEN_CODE: &str = "invalid_reply_token";

/// An error raised while receiving, parsing, storing or routing a message,
/// or while signing and checking reply tokens.
///
/// Messages never quote message bodies or secrets.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InboundError {
    /// The message is larger than `mail.inbound.max_size`.
    #[error("the message is {size} bytes, over the {max}-byte limit (`mail.inbound.max_size`)")]
    TooLarge {
        /// Its size in bytes.
        size: usize,
        /// The limit.
        max: usize,
    },
    /// The bytes hold no message.
    #[error("the message cannot be parsed: {0}")]
    Unparseable(String),
    /// A provider's payload is not what the provider sends.
    #[error("the {provider} payload is malformed: {reason}")]
    Malformed {
        /// The provider (`ses`, `mailgun`, `postmark`, `raw`, ...).
        provider: &'static str,
        /// What is wrong, without quoting the payload.
        reason: String,
    },
    /// The reply token was not signed with this app's key, or is damaged.
    #[error("the reply token is invalid")]
    InvalidToken,
    /// The reply token is older than `mail.inbound.reply_token_ttl_days`.
    #[error("the reply token expired")]
    ExpiredToken,
    /// A reply token address would not fit in an address's local part (64
    /// characters).
    #[error("the reply address `{local}@...` is longer than 64 characters before the `@`")]
    AddressTooLong {
        /// The local part.
        local: String,
    },
    /// Reply tokens need the application key.
    #[error(
        "reply tokens need the application key (`app.key` / APP_KEY, or CryptoModule with a key)"
    )]
    MissingKey,
    /// A mailbox refused the message on purpose
    /// ([`MailboxContext::reject`](super::MailboxContext::reject)).
    #[error("the message was rejected: {0}")]
    Rejected(String),
    /// A mailbox pattern does not parse.
    #[error("invalid mailbox pattern `{pattern}`: {reason}")]
    InvalidPattern {
        /// The pattern as registered.
        pattern: String,
        /// Why.
        reason: String,
    },
    /// The inbound configuration cannot work (a disk that is not
    /// configured, a missing `StorageModule`).
    #[error("invalid inbound mail configuration: {0}")]
    Config(String),
    /// Storing or reading the raw message failed.
    #[error("the raw message could not be stored or read: {0}")]
    Storage(#[from] navree_storage::StorageError),
    /// A mailbox's handler failed; the provider is answered with a 500 so
    /// it delivers the message again.
    #[error("mailbox `{mailbox}` failed: {error}")]
    Handler {
        /// The mailbox pattern.
        mailbox: String,
        /// The handler's error.
        #[source]
        error: BoxError,
    },
}

impl InboundError {
    /// Whether the error ends the message's handling for good: a rejection
    /// or a bad reply token. The provider is answered with a success, so it
    /// does not deliver the message again.
    pub fn is_rejection(&self) -> bool {
        matches!(
            self,
            InboundError::Rejected(_) | InboundError::InvalidToken | InboundError::ExpiredToken
        )
    }

    /// The error response: 413 `mail_too_large`, 400 `invalid_inbound_mail`
    /// for payloads and messages that cannot be read, 400
    /// `invalid_reply_token` for bad reply tokens; `None` (a 500)
    /// otherwise.
    pub fn to_app_error(&self) -> Option<AppError> {
        match self {
            InboundError::TooLarge { .. } => Some(
                AppError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    TOO_LARGE_CODE,
                    "Payload Too Large",
                )
                .with_detail(self.to_string()),
            ),
            InboundError::Unparseable(_) | InboundError::Malformed { .. } => Some(
                AppError::new(StatusCode::BAD_REQUEST, INVALID_MAIL_CODE, "Bad Request")
                    .with_detail(self.to_string()),
            ),
            InboundError::InvalidToken | InboundError::ExpiredToken => Some(
                AppError::new(
                    StatusCode::BAD_REQUEST,
                    INVALID_REPLY_TOKEN_CODE,
                    "Bad Request",
                )
                .with_detail(self.to_string()),
            ),
            _ => None,
        }
    }
}

/// The error mapper registered by `MailModule` (feature `inbound`).
pub(crate) fn map_inbound_error(error: &InboundError) -> Option<AppError> {
    error.to_app_error()
}
