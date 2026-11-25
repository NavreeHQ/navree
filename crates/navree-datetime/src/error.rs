//! [`Error`] and [`ErrorKind`].

use std::borrow::Cow;
use std::fmt;

/// What a fallible function of this crate returns.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What kind of failure an [`Error`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A value the type cannot hold: a year outside the range, an instant or
    /// a span past its ends, an offset past its limit.
    OutOfRange,
    /// A value that does not exist: month 13, 30 February, hour 24.
    Invalid,
    /// Text that could not be read as the value asked for.
    Parse,
    /// A local time that happens twice in its zone (the clocks went back),
    /// where exactly one instant was asked for.
    Ambiguous,
    /// A local time that does not exist in its zone (the clocks went
    /// forward over it).
    Gap,
}

/// What went wrong making or reading a date or a time.
///
/// [`kind`](Error::kind) says which sort of failure it is and
/// [`message`](Error::message) names the value that was refused.
///
/// ```
/// use navree_datetime::{Date, ErrorKind};
///
/// let error = Date::new(2026, 2, 30).unwrap_err();
/// assert_eq!(error.kind(), ErrorKind::Invalid);
/// assert_eq!(error.to_string(), "2026-02 has no day 30");
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: Cow<'static, str>,
}

impl Error {
    /// An error of `kind` with `message`.
    pub fn new(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> Error {
        Error {
            kind,
            message: message.into(),
        }
    }

    /// A value the type cannot hold.
    pub fn out_of_range(message: impl Into<Cow<'static, str>>) -> Error {
        Error::new(ErrorKind::OutOfRange, message)
    }

    /// A value that does not exist.
    pub fn invalid(message: impl Into<Cow<'static, str>>) -> Error {
        Error::new(ErrorKind::Invalid, message)
    }

    /// Text that could not be read.
    pub fn parse(message: impl Into<Cow<'static, str>>) -> Error {
        Error::new(ErrorKind::Parse, message)
    }

    /// What kind of failure it is.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .finish()
    }
}

impl std::error::Error for Error {}
