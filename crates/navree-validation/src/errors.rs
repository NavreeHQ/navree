//! [`ValidationErrors`]: field errors, rendered as a 400 error.

use std::collections::BTreeMap;

use navree_http::AppError;
use navree_http::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// Field errors: `{"field.path": ["message", ...]}`, fields in sorted order.
///
/// Converts into a 400 [`AppError`] (code `validation_failed`), which is
/// what the extractors reject with. It renders in the JSON error shape:
/// `message` lists every message in field order and `errors` maps each
/// field to its messages.
///
/// ```
/// use navree_validation::ValidationErrors;
///
/// let mut errors = ValidationErrors::new();
/// errors.add("name", "name should not be empty");
/// errors.add("email", "email has already been taken");
/// let error = errors.into_app_error();
/// assert_eq!(error.status.as_u16(), 400);
/// assert_eq!(
///     error.body(),
///     serde_json::json!({
///         "statusCode": 400,
///         "message": ["email has already been taken", "name should not be empty"],
///         "error": "Bad Request",
///         "code": "validation_failed",
///         "errors": {
///             "email": ["email has already been taken"],
///             "name": ["name should not be empty"],
///         },
///     })
/// );
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ValidationErrors(BTreeMap<String, Vec<String>>);

impl ValidationErrors {
    /// No errors.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `message` to `field` (a dotted path); duplicates are dropped.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        let messages = self.0.entry(field.into()).or_default();
        let message = message.into();
        if !messages.contains(&message) {
            messages.push(message);
        }
    }

    /// Whether there are no errors.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of fields with errors.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The messages of `field`.
    pub fn get(&self, field: &str) -> Option<&[String]> {
        self.0.get(field).map(Vec::as_slice)
    }

    /// Every field with its messages, fields in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.0
            .iter()
            .map(|(field, messages)| (field.as_str(), messages.as_slice()))
    }

    /// Adds every error of `other`.
    pub fn merge(&mut self, other: ValidationErrors) {
        for (field, messages) in other.0 {
            for message in messages {
                self.add(field.clone(), message);
            }
        }
    }

    /// The map of field errors.
    pub fn into_map(self) -> BTreeMap<String, Vec<String>> {
        self.0
    }

    /// A one-line summary (for logs): the first message, plus `(and N more
    /// errors)`.
    pub fn summary(&self) -> Option<String> {
        let mut all = self.0.values().flatten();
        let first = all.next()?;
        let rest = all.count();
        Some(match rest {
            0 => first.clone(),
            1 => format!("{first} (and 1 more error)"),
            n => format!("{first} (and {n} more errors)"),
        })
    }

    /// The 400 error for these errors.
    pub fn into_app_error(self) -> AppError {
        AppError::validation(self.0)
    }
}

impl From<ValidationErrors> for AppError {
    fn from(errors: ValidationErrors) -> Self {
        errors.into_app_error()
    }
}

impl IntoResponse for ValidationErrors {
    fn into_response(self) -> Response {
        self.into_app_error().into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_the_other_messages() {
        let mut errors = ValidationErrors::new();
        assert_eq!(errors.summary(), None);
        errors.add("a", "first");
        assert_eq!(errors.summary().as_deref(), Some("first"));
        errors.add("a", "first");
        errors.add("a", "second");
        errors.add("b", "third");
        assert_eq!(
            errors.summary().as_deref(),
            Some("first (and 2 more errors)")
        );
        assert_eq!(
            errors.get("a"),
            Some(&["first".to_owned(), "second".to_owned()][..])
        );
    }
}
