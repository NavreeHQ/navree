//! [`ValidationErrors`]: field errors, rendered as a 422 problem.

use std::collections::BTreeMap;

use axum::response::{IntoResponse, Response};
use navree_http::AppError;
use serde::{Deserialize, Serialize};

/// Field errors: `{"field.path": ["message", ...]}`, fields in sorted order.
///
/// Converts into a 422 [`AppError`] (`type` `validation_failed`, the errors
/// in `errors` and, like Laravel, the first message plus a count of the rest
/// as `detail`), which is what the extractors reject with.
///
/// ```
/// use navree_validation::ValidationErrors;
///
/// let mut errors = ValidationErrors::new();
/// errors.add("email", "The email has already been taken.");
/// errors.add("name", "The name field is required.");
/// let problem = errors.into_app_error();
/// assert_eq!(problem.status.as_u16(), 422);
/// assert_eq!(
///     problem.detail.as_deref(),
///     Some("The email has already been taken. (and 1 more error)")
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

    /// Laravel's summary: the first message, plus `(and N more errors)`.
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

    /// The 422 problem for these errors.
    pub fn into_app_error(self) -> AppError {
        let summary = self.summary();
        let error = AppError::validation(self.0);
        match summary {
            Some(detail) => error.with_detail(detail),
            None => error,
        }
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
