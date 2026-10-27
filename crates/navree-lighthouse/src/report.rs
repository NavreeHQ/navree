//! [`Report`]: an error or message your code reports, and [`AsError`].

use std::collections::BTreeMap;
use std::error::Error;

use serde::Serialize;
use serde_json::Value;

use crate::model::Level;

/// Something that can be reported as an error: any `std::error::Error`
/// (its type name is kept for the fingerprint) or a `dyn Error`.
pub trait AsError {
    /// The error as a trait object.
    fn as_error(&self) -> &(dyn Error + 'static);

    /// The error's type name, when it is known statically
    /// (`billing::InvoiceError`).
    fn error_type(&self) -> Option<&'static str>;
}

impl<E: Error + 'static> AsError for E {
    fn as_error(&self) -> &(dyn Error + 'static) {
        self
    }

    fn error_type(&self) -> Option<&'static str> {
        Some(std::any::type_name::<E>())
    }
}

impl AsError for dyn Error + 'static {
    fn as_error(&self) -> &(dyn Error + 'static) {
        self
    }

    fn error_type(&self) -> Option<&'static str> {
        None
    }
}

impl AsError for dyn Error + Send + 'static {
    fn as_error(&self) -> &(dyn Error + 'static) {
        self
    }

    fn error_type(&self) -> Option<&'static str> {
        None
    }
}

impl AsError for dyn Error + Send + Sync + 'static {
    fn as_error(&self) -> &(dyn Error + 'static) {
        self
    }

    fn error_type(&self) -> Option<&'static str> {
        None
    }
}

/// A report for [`Lighthouse::capture`](crate::Lighthouse::capture): an
/// error (with its source chain) or a message, with a level, context
/// values, tags, an optional fingerprint, user and backtrace.
///
/// ```
/// use navree_lighthouse::{Level, Report};
///
/// let error = std::io::Error::other("card declined");
/// let report = Report::error(&error)
///     .level(Level::Warning)
///     .context("invoice_id", 1042)
///     .tag("provider", "acme-pay")
///     .fingerprint(["acme-pay", "card-declined"]);
/// assert_eq!(report.message_text(), "card declined");
/// assert_eq!(report.level_value(), Level::Warning);
/// ```
#[derive(Clone, Debug)]
pub struct Report {
    pub(crate) message: String,
    pub(crate) chain: Vec<String>,
    pub(crate) error_type: Option<&'static str>,
    pub(crate) level: Level,
    pub(crate) context: BTreeMap<String, Value>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) fingerprint: Option<Vec<String>>,
    pub(crate) user: Option<String>,
    pub(crate) backtrace: Option<String>,
    pub(crate) location: Option<String>,
}

impl Report {
    /// A report of `error`: its message and every source's.
    #[track_caller]
    pub fn error<E: AsError + ?Sized>(error: &E) -> Report {
        let chain = error_chain(error.as_error());
        Report {
            message: chain.first().cloned().unwrap_or_default(),
            chain,
            error_type: error.error_type(),
            ..Report::new(String::new())
        }
    }

    /// A report of a message, without an error value.
    #[track_caller]
    pub fn message(text: impl Into<String>) -> Report {
        let text = text.into();
        Report {
            chain: vec![text.clone()],
            ..Report::new(text)
        }
    }

    #[track_caller]
    fn new(message: String) -> Report {
        let caller = std::panic::Location::caller();
        Report {
            message,
            chain: Vec::new(),
            error_type: None,
            level: Level::Error,
            context: BTreeMap::new(),
            tags: BTreeMap::new(),
            fingerprint: None,
            user: None,
            backtrace: None,
            location: Some(format!(
                "{}:{}:{}",
                caller.file(),
                caller.line(),
                caller.column()
            )),
        }
    }

    /// The level: `Error` (the default), `Warning` or `Info`.
    #[must_use]
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Adds a value shown on the occurrence (any `Serialize` value; masked
    /// when its key contains a redacted fragment).
    #[must_use]
    pub fn context(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        self.context.insert(key.into(), value);
        self
    }

    /// Adds a tag: a short string issues can be filtered by.
    #[must_use]
    pub fn tag(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.tags.insert(key.into(), value.to_string());
        self
    }

    /// Replaces the fingerprint, so these reports group together however
    /// their messages differ.
    #[must_use]
    pub fn fingerprint<I, S>(mut self, parts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fingerprint = Some(parts.into_iter().map(Into::into).collect());
        self
    }

    /// Sets the user when there is no request (a job working for someone).
    #[must_use]
    pub fn user(mut self, id: impl ToString) -> Self {
        self.user = Some(id.to_string());
        self
    }

    /// Captures a backtrace where the report is built.
    #[must_use]
    pub fn backtrace(mut self) -> Self {
        self.backtrace = Some(std::backtrace::Backtrace::force_capture().to_string());
        self
    }

    /// The message (the error's, or the text).
    pub fn message_text(&self) -> &str {
        &self.message
    }

    /// The level.
    pub fn level_value(&self) -> Level {
        self.level
    }
}

/// The message of `error`, then of each source, skipping a source whose
/// text the previous message already ends with.
pub(crate) fn error_chain(error: &(dyn Error + 'static)) -> Vec<String> {
    let mut chain = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if chain.last().is_none_or(|last| !last.ends_with(&text)) {
            chain.push(text);
        }
        source = cause.source();
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Outer(std::io::Error);

    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("cannot charge invoice")
        }
    }

    impl Error for Outer {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn errors_keep_their_chain_and_type() {
        let error = Outer(std::io::Error::other("card declined"));
        let report = Report::error(&error);
        assert_eq!(report.chain, ["cannot charge invoice", "card declined"]);
        assert!(report.error_type.unwrap().ends_with("Outer"));
        assert!(report.location.as_deref().unwrap().contains("report.rs"));
        let dynamic: &(dyn Error + 'static) = &error;
        let report = Report::error(dynamic);
        assert_eq!(report.error_type, None);
        assert_eq!(report.message, "cannot charge invoice");
    }

    #[test]
    fn the_builder_sets_every_field() {
        let report = Report::message("Payment event without a customer")
            .level(Level::Warning)
            .context("invoice_id", 42)
            .tag("provider", "acme-pay")
            .fingerprint(["acme-pay", "card-declined"])
            .user("0199c1d2")
            .backtrace();
        assert_eq!(report.chain, ["Payment event without a customer"]);
        assert_eq!(report.level, Level::Warning);
        assert_eq!(report.context["invoice_id"], 42);
        assert_eq!(report.tags["provider"], "acme-pay");
        assert_eq!(
            report.fingerprint.as_deref(),
            Some(&["acme-pay".to_owned(), "card-declined".to_owned()][..])
        );
        assert_eq!(report.user.as_deref(), Some("0199c1d2"));
        assert!(report.backtrace.is_some());
    }
}
