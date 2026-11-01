//! [`Error`] and [`ErrorKind`].

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

/// An error raised while compiling or rendering a template.
///
/// It carries a [kind](Error::kind), an optional detail message, and where
/// it happened: the template's [name](Error::name), [line](Error::line) and
/// [column](Error::column). Messages describe what went wrong but never
/// contain the values a template was rendered with.
pub struct Error {
    repr: Box<Repr>,
}

struct Repr {
    kind: ErrorKind,
    detail: Option<Cow<'static, str>>,
    name: Option<String>,
    line: Option<usize>,
    column: Option<usize>,
    source: Option<Arc<dyn std::error::Error + Send + Sync>>,
}

/// What kind of error happened.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// An operation on values that do not support it.
    InvalidOperation,
    /// The template's syntax is invalid.
    SyntaxError,
    /// A template was not found.
    TemplateNotFound,
    /// Too many arguments were passed to a function, filter, test or macro.
    TooManyArguments,
    /// A required argument is missing.
    MissingArgument,
    /// A filter is unknown.
    UnknownFilter,
    /// A test is unknown.
    UnknownTest,
    /// A function is unknown.
    UnknownFunction,
    /// A method is unknown.
    UnknownMethod,
    /// A string literal has an invalid escape sequence.
    BadEscape,
    /// An undefined value was used where the undefined behavior forbids it.
    UndefinedError,
    /// A value could not be serialized into a template value.
    BadSerialization,
    /// An included template failed to render.
    BadInclude,
    /// Unpacking a value into several names failed.
    CannotUnpack,
    /// A block does not exist.
    UnknownBlock,
    /// A rendering limit was reached (see [`Limits`](crate::Limits)).
    LimitExceeded,
}

impl ErrorKind {
    fn description(self) -> &'static str {
        match self {
            ErrorKind::InvalidOperation => "invalid operation",
            ErrorKind::SyntaxError => "syntax error",
            ErrorKind::TemplateNotFound => "template not found",
            ErrorKind::TooManyArguments => "too many arguments",
            ErrorKind::MissingArgument => "missing argument",
            ErrorKind::UnknownFilter => "unknown filter",
            ErrorKind::UnknownTest => "unknown test",
            ErrorKind::UnknownFunction => "unknown function",
            ErrorKind::UnknownMethod => "unknown method",
            ErrorKind::BadEscape => "bad string escape",
            ErrorKind::UndefinedError => "undefined value",
            ErrorKind::BadSerialization => "could not serialize to value",
            ErrorKind::BadInclude => "could not render include",
            ErrorKind::CannotUnpack => "cannot unpack",
            ErrorKind::UnknownBlock => "unknown block",
            ErrorKind::LimitExceeded => "limit exceeded",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

impl Error {
    /// An error of `kind` with a detail message.
    pub fn new(kind: ErrorKind, detail: impl Into<Cow<'static, str>>) -> Error {
        Error {
            repr: Box::new(Repr {
                kind,
                detail: Some(detail.into()),
                name: None,
                line: None,
                column: None,
                source: None,
            }),
        }
    }

    /// Attaches the error that caused this one.
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Error {
        self.repr.source = Some(Arc::new(source));
        self
    }

    /// The kind of error.
    pub fn kind(&self) -> ErrorKind {
        self.repr.kind
    }

    /// The detail message, if any.
    pub fn detail(&self) -> Option<&str> {
        self.repr.detail.as_deref()
    }

    /// The name of the template the error happened in.
    pub fn name(&self) -> Option<&str> {
        self.repr.name.as_deref()
    }

    /// The line (1-based) the error happened on.
    pub fn line(&self) -> Option<usize> {
        self.repr.line
    }

    /// The column (1-based, in characters) the error happened at.
    pub fn column(&self) -> Option<usize> {
        self.repr.column
    }

    /// Whether the error already says where it happened.
    pub(crate) fn has_location(&self) -> bool {
        self.repr.line.is_some()
    }

    /// Records where the error happened, unless it already says so.
    pub(crate) fn locate(&mut self, name: &str, line: usize, column: usize) {
        if !self.has_location() {
            self.repr.name = Some(name.to_owned());
            self.repr.line = Some(line);
            self.repr.column = Some(column);
        }
    }

    pub(crate) fn undefined() -> Error {
        Error::from(ErrorKind::UndefinedError)
    }

    pub(crate) fn invalid(detail: impl Into<Cow<'static, str>>) -> Error {
        Error::new(ErrorKind::InvalidOperation, detail)
    }

    pub(crate) fn limit(detail: impl Into<Cow<'static, str>>) -> Error {
        Error::new(ErrorKind::LimitExceeded, detail)
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error {
            repr: Box::new(Repr {
                kind,
                detail: None,
                name: None,
                line: None,
                column: None,
                source: None,
            }),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail() {
            Some(detail) => write!(f, "{}: {detail}", self.kind())?,
            None => write!(f, "{}", self.kind())?,
        }
        if let Some(name) = self.name() {
            write!(f, " (in {name}:{})", self.line().unwrap_or(0))?;
        }
        Ok(())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Error");
        debug.field("kind", &self.kind());
        if let Some(detail) = self.detail() {
            debug.field("detail", &detail);
        }
        if let Some(name) = self.name() {
            debug.field("name", &name);
        }
        if let Some(line) = self.line() {
            debug.field("line", &line);
        }
        if let Some(column) = self.column() {
            debug.field("column", &column);
        }
        if let Some(source) = &self.repr.source {
            debug.field("source", source);
        }
        debug.finish()
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.repr
            .source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::new(ErrorKind::BadSerialization, msg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_template_and_line() {
        let mut error = Error::undefined();
        error.locate("mail/welcome.txt", 3, 7);
        assert_eq!(error.to_string(), "undefined value (in mail/welcome.txt:3)");
        // The first location wins.
        error.locate("layout.txt", 9, 1);
        assert_eq!(error.line(), Some(3));
        assert_eq!(error.column(), Some(7));
        assert_eq!(error.name(), Some("mail/welcome.txt"));

        let error = Error::new(ErrorKind::InvalidOperation, "no");
        assert_eq!(error.to_string(), "invalid operation: no");
    }
}
