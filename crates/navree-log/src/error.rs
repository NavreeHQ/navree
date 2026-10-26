//! [`LogError`].

/// An error from building the subscriber or changing log levels. Messages
/// name the offending setting; they never contain logged values.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    /// A level name that is not `trace`, `debug`, `info`, `warn`, `error` or `off`.
    #[error("invalid log level `{level}`: expected trace, debug, info, warn, error or off")]
    InvalidLevel {
        /// The rejected level.
        level: String,
    },
    /// A target that is not letters, digits, `_`, `-`, `.` and `::`.
    #[error("invalid log target `{target}`: use letters, digits, `_`, `-`, `.` and `::`")]
    InvalidTarget {
        /// The rejected target.
        target: String,
    },
    /// A filter directive list that `EnvFilter` rejects.
    #[error("invalid log filter `{directives}`: {message}")]
    InvalidFilter {
        /// The rejected directives.
        directives: String,
        /// Why they were rejected.
        message: String,
    },
    /// Invalid `file` settings.
    #[error("invalid log file settings: {message}")]
    InvalidFile {
        /// What is wrong.
        message: String,
    },
    /// The log file (or its directory) cannot be opened.
    #[error("cannot open log file `{}`: {message}", .path.display())]
    File {
        /// The configured file path.
        path: std::path::PathBuf,
        /// Why it cannot be opened.
        message: String,
    },
    /// Queued lines were not written to the log file within the timeout.
    #[error("log file flush timed out after {timeout:?}")]
    FlushTimedOut {
        /// How long the flush waited.
        timeout: std::time::Duration,
    },
    /// The log file writer thread has stopped; file lines are being lost.
    #[error("the log file writer thread has stopped")]
    FileWriterStopped,
    /// Another global tracing subscriber was installed before Navree's, so
    /// levels cannot be changed through [`LogControl`](crate::LogControl).
    #[error(
        "the process-wide tracing subscriber was not installed by navree-log, so log levels cannot be changed"
    )]
    NotInstalled,
    /// The subscriber this control belongs to has been dropped.
    #[error("the log subscriber this control belongs to has been dropped")]
    SubscriberDropped,
}
