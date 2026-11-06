//! Helpers both resources share: input normalisation (serde
//! `deserialize_with` functions), the `not_blank` rule, the search text of
//! a query, the documented page shape and CSV cells.

use navree::db::PageMeta;
use navree::openapi::ToSchema;
use navree::validation::Violation;
use navree::validation::rules::RuleInput;
use serde::{Deserialize, Deserializer, Serialize};

/// The longest text the CRM stores in a short text column.
pub(crate) const MAX_TEXT: usize = 255;

/// A required string, trimmed (so `"  "` fails `not_blank`).
pub(crate) fn trimmed<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    String::deserialize(deserializer).map(|value| value.trim().to_owned())
}

/// A field that may be left out but not `null` (`name` in a PATCH): a
/// trimmed string when present. `null` is an error ("should not be
/// empty"); use with `#[serde(default)]`.
pub(crate) fn present_trimmed<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    trimmed(deserializer).map(Some)
}

/// A field that may be left out but not `null` (`status`, `owner_id` in a
/// PATCH); use with `#[serde(default)]`.
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// An optional text field: trimmed, and blank means `None`.
pub(crate) fn optional_text<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer).map(|value| value.and_then(non_blank))
}

/// A nullable field of a PATCH body: left out is `None` (unchanged),
/// `null` is `Some(None)` (cleared); use with `#[serde(default)]`.
pub(crate) fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// A nullable text field of a PATCH body: trimmed, and blank clears it
/// like `null`; use with `#[serde(default)]`.
pub(crate) fn nullable_text<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    optional_text(deserializer).map(Some)
}

/// `value` trimmed, or `None` when that leaves nothing.
fn non_blank(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The `required` rule for strings that are present but blank: "<field>
/// should not be empty", like a missing field. `None` passes.
pub(crate) fn not_blank<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    match value.rule_str() {
        Some(text) if text.trim().is_empty() => {
            Err(navree::validation::rule_error("required", &[]))
        }
        _ => Ok(()),
    }
}

/// The trimmed search text, or `None` when there is nothing to search for.
pub(crate) fn search_text(search: Option<&str>) -> Option<&str> {
    search.map(str::trim).filter(|search| !search.is_empty())
}

/// Sets `field` when the request sent a value for it (PATCH semantics: a
/// field left out stays unchanged).
pub(crate) fn set_some<T>(field: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *field = value;
    }
}

/// The pagination metadata of a page, as documented (`meta`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PageInfo {
    /// The page number (1-based).
    pub current_page: u64,
    /// The page size (default 15, at most 100).
    pub per_page: u64,
    /// The number of matching records.
    pub total: u64,
    /// The last page number (at least 1).
    pub last_page: u64,
}

impl From<PageMeta> for PageInfo {
    fn from(meta: PageMeta) -> Self {
        PageInfo {
            current_page: meta.current_page,
            per_page: meta.per_page,
            total: meta.total,
            last_page: meta.last_page,
        }
    }
}

/// A CSV cell that spreadsheets show as text: a value starting with `=`,
/// `+`, `-`, `@`, a tab or a carriage return is prefixed with `'` (CSV
/// formula injection), unless it only has phone-number characters
/// (`+91 98765 43210`), which cannot form a formula.
pub(crate) fn csv_safe(value: &str) -> std::borrow::Cow<'_, str> {
    let risky = value
        .chars()
        .next()
        .is_some_and(|first| matches!(first, '=' | '+' | '-' | '@' | '\t' | '\r'));
    let phone_like = value
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '(' | ')' | '.'));
    if risky && !phone_like {
        format!("'{value}").into()
    } else {
        value.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_text_is_the_trimmed_term() {
        assert_eq!(search_text(Some("  Agra ")), Some("Agra"));
        assert_eq!(search_text(Some("   ")), None);
        assert_eq!(search_text(None), None);
    }

    #[test]
    fn patches_change_only_what_was_sent() {
        let mut name = "Agra".to_owned();
        set_some(&mut name, None);
        assert_eq!(name, "Agra");
        set_some(&mut name, Some("Gulati".to_owned()));
        assert_eq!(name, "Gulati");
    }

    #[test]
    fn csv_cells_cannot_start_formulas() {
        assert_eq!(csv_safe("=1+2"), "'=1+2");
        assert_eq!(csv_safe("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(csv_safe("+cmd|' /C calc'!A0"), "'+cmd|' /C calc'!A0");
        assert_eq!(
            csv_safe("-2+3+cmd|' /C calc'!A0"),
            "'-2+3+cmd|' /C calc'!A0"
        );
        assert_eq!(csv_safe("\tx"), "'\tx");
        // Phone numbers and plain text are left alone.
        assert_eq!(csv_safe("+91 98765 43210"), "+91 98765 43210");
        assert_eq!(csv_safe("+44 (20) 7946.0000"), "+44 (20) 7946.0000");
        assert_eq!(csv_safe("Anu"), "Anu");
        assert_eq!(csv_safe("a=b"), "a=b");
        assert_eq!(csv_safe(""), "");
    }
}
