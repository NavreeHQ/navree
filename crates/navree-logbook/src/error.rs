//! [`LogbookError`].

use navree_http::AppError;

/// What can go wrong in Logbook.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LogbookError {
    /// Logbook is off (`logbook.enabled`, or a release build).
    #[error("Logbook is off (logbook.enabled)")]
    Disabled,
    /// The store failed (the SQLite file could not be opened or written).
    #[error("the Logbook store failed: {0}")]
    Store(String),
    /// No entry has this id.
    #[error("no Logbook entry `{0}`")]
    NotFound(String),
    /// The entry is not a request, so it cannot be replayed.
    #[error("Logbook entry `{0}` is not a request")]
    NotARequest(String),
    /// Replay is off (`logbook.replay = false`).
    #[error("replaying requests is off (logbook.replay = false)")]
    ReplayOff,
    /// An unsafe method was replayed without confirmation.
    #[error("replaying a {method} changes data again: send it with `confirm: true`")]
    Unconfirmed {
        /// The method (`POST`).
        method: String,
    },
    /// The edited request is not a valid request.
    #[error("invalid replay: {0}")]
    InvalidReplay(String),
    /// The replay could not be sent or read.
    #[error("the replay failed: {0}")]
    Replay(String),
}

impl From<sqlx::Error> for LogbookError {
    fn from(error: sqlx::Error) -> Self {
        LogbookError::Store(error.to_string())
    }
}

impl LogbookError {
    /// The HTTP error for this error: 404 when Logbook is off or the entry
    /// does not exist, 400 for a bad replay, 409 for an unconfirmed unsafe
    /// replay, 403 when replay is off, 500 for store failures.
    pub fn into_app_error(self) -> AppError {
        let message = self.to_string();
        match self {
            LogbookError::Disabled | LogbookError::NotFound(_) => AppError::not_found(),
            LogbookError::NotARequest(_) | LogbookError::InvalidReplay(_) => {
                AppError::bad_request(message)
            }
            LogbookError::Unconfirmed { .. } => AppError::conflict(message),
            LogbookError::ReplayOff => AppError::forbidden().with_detail(message),
            LogbookError::Store(_) | LogbookError::Replay(_) => AppError::internal(self),
        }
    }
}
