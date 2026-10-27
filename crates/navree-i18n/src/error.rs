//! [`I18nError`] and its mapping to HTTP errors.

use std::collections::BTreeMap;

use navree_http::AppError;
use sea_orm::DbErr;

/// What can go wrong in the localization module.
#[derive(Debug, thiserror::Error)]
pub enum I18nError {
    /// A string that is not a BCP 47 language tag.
    #[error("`{0}` is not a valid BCP 47 language tag")]
    InvalidLocale(String),
    /// A valid tag that matches none of `i18n.locales`.
    #[error("locale `{locale}` is not supported (i18n.locales: {supported})")]
    UnsupportedLocale {
        /// The tag asked for.
        locale: String,
        /// The supported locales, comma-separated.
        supported: String,
    },
    /// A message that neither the locale nor its fallbacks define, with
    /// `i18n.missing = "error"`.
    #[error("message `{id}` is missing from locale `{locale}` and its fallbacks")]
    MissingMessage {
        /// The locale of the lookup.
        locale: String,
        /// The message id.
        id: String,
    },
    /// A message that could not be formatted completely (an unknown
    /// variable or function), with `i18n.missing = "error"`.
    #[error("message `{id}` in locale `{locale}`: {reason}")]
    Format {
        /// The locale of the lookup.
        locale: String,
        /// The message id.
        id: String,
        /// What went wrong.
        reason: String,
    },
    /// The Fluent files could not be loaded: every problem, each naming the
    /// file and line.
    #[error("{}", .0.join("\n"))]
    Files(Vec<String>),
    /// Saved locales need `DatabaseModule`.
    #[error("saved locales need DatabaseModule in the app")]
    NoDatabase,
    /// A database error while reading or saving a locale.
    #[error(transparent)]
    Database(#[from] DbErr),
}

/// `InvalidLocale` and `UnsupportedLocale` are 400s with `errors.locale`;
/// the others stay 500s.
pub(crate) fn map_i18n_error(error: &I18nError) -> Option<AppError> {
    match error {
        I18nError::InvalidLocale(_) | I18nError::UnsupportedLocale { .. } => {
            Some(AppError::validation(BTreeMap::from([(
                "locale".to_owned(),
                vec![error.to_string()],
            )])))
        }
        _ => None,
    }
}
