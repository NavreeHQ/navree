//! Helpers both resources share: input normalisation (serde
//! `deserialize_with` functions), the `not_blank` rule, the search text of
//! a query, the documented page shape and CSV cells.

use navree::db::PageMeta;
use navree::validation::Violation;
use navree::validation::rules::RuleInput;
use serde::de::value::StringDeserializer;
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;

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

/// An optional query parameter: a blank value (`?owner_id=`) is the same as
/// leaving it out; anything else must parse as `T` (a UUID, an enum
/// string), with serde's error otherwise.
pub(crate) fn blank_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    match Option::<String>::deserialize(deserializer)?.and_then(non_blank) {
        None => Ok(None),
        Some(value) => T::deserialize(StringDeserializer::<D::Error>::new(value)).map(Some),
    }
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
/// (`+1 555-0100`), which cannot form a formula.
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
        assert_eq!(search_text(Some("  Acme ")), Some("Acme"));
        assert_eq!(search_text(Some("   ")), None);
        assert_eq!(search_text(None), None);
    }

    #[test]
    fn patches_change_only_what_was_sent() {
        let mut name = "Acme".to_owned();
        set_some(&mut name, None);
        assert_eq!(name, "Acme");
        set_some(&mut name, Some("Globex".to_owned()));
        assert_eq!(name, "Globex");
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
        assert_eq!(csv_safe("+1 555-0100"), "+1 555-0100");
        assert_eq!(csv_safe("+44 (20) 7946.0000"), "+44 (20) 7946.0000");
        assert_eq!(csv_safe("Ada"), "Ada");
        assert_eq!(csv_safe("a=b"), "a=b");
        assert_eq!(csv_safe(""), "");
    }

    #[test]
    fn blank_query_values_are_left_out() {
        #[derive(Debug, Deserialize)]
        struct Q {
            #[serde(default, deserialize_with = "blank_as_none")]
            id: Option<uuid::Uuid>,
        }
        let parse = |json: serde_json::Value| serde_json::from_value::<Q>(json).map(|q| q.id);
        assert_eq!(parse(serde_json::json!({ "id": "  " })).ok(), Some(None));
        assert_eq!(parse(serde_json::json!({})).ok(), Some(None));
        let id = uuid::Uuid::now_v7();
        assert_eq!(parse(serde_json::json!({ "id": id })).ok(), Some(Some(id)));
        let error = parse(serde_json::json!({ "id": "nope" })).expect_err("not a UUID");
        assert!(error.to_string().contains("UUID parsing failed"), "{error}");
    }
}
