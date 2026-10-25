//! Rules garde lacks, as functions for `#[garde(custom(...))]`, with
//! table-driven messages (`uuid`, `date_format`, `in`, `confirmed`).
//!
//! ```
//! use garde::Validate;
//! use navree_validation::rules::{self, confirmed};
//!
//! #[derive(Validate)]
//! struct NewDeal {
//!     #[garde(custom(rules::uuid))]
//!     company_id: String,
//!     #[garde(custom(rules::date_ymd))]
//!     expected_close_date: Option<String>,
//!     #[garde(custom(rules::one_of(&["lead", "qualified", "won", "lost"])))]
//!     stage: String,
//!     #[garde(length(min = 8), custom(confirmed(&self.password_confirmation)))]
//!     password: String,
//!     #[garde(skip)]
//!     password_confirmation: String,
//! }
//! ```
//!
//! The string rules accept `str`, `String`, `Cow<str>` and `Option`s of
//! them; `None` passes (combine with `required` to demand a value).

use std::borrow::Cow;

use chrono::NaiveDate;

use crate::rule_error;

/// A value the string rules can check: `None` means "no value" and passes.
pub trait RuleInput {
    /// The string to check, if any.
    fn rule_str(&self) -> Option<&str>;
}

impl RuleInput for str {
    fn rule_str(&self) -> Option<&str> {
        Some(self)
    }
}

impl RuleInput for String {
    fn rule_str(&self) -> Option<&str> {
        Some(self)
    }
}

impl RuleInput for Cow<'_, str> {
    fn rule_str(&self) -> Option<&str> {
        Some(self)
    }
}

impl<T: RuleInput + ?Sized> RuleInput for &T {
    fn rule_str(&self) -> Option<&str> {
        (**self).rule_str()
    }
}

impl<T: RuleInput> RuleInput for Option<T> {
    fn rule_str(&self) -> Option<&str> {
        self.as_ref().and_then(RuleInput::rule_str)
    }
}

/// A UUID in the hyphenated form (`018f…-…`) (message key `uuid`).
pub fn uuid<T: RuleInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.rule_str() {
        Some(text) if !is_uuid(text) => Err(rule_error("uuid", &[])),
        _ => Ok(()),
    }
}

fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| bytes[i] == b'-')
        && uuid::Uuid::try_parse(text).is_ok()
}

/// A calendar date written exactly `YYYY-MM-DD` (message key
/// `date_format`, with `:format` = `YYYY-MM-DD`).
pub fn date_ymd<T: RuleInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.rule_str() {
        Some(text) if !is_date_ymd(text) => {
            Err(rule_error("date_format", &[("format", &"YYYY-MM-DD")]))
        }
        _ => Ok(()),
    }
}

fn is_date_ymd(text: &str) -> bool {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
    shaped && NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

/// One of `allowed` (message key `in`, with `:values` = the allowed
/// values).
pub fn one_of<'a, T: RuleInput + ?Sized, C: ?Sized>(
    allowed: &'a [&'a str],
) -> impl FnOnce(&T, &C) -> garde::Result + 'a {
    move |value, _ctx| match value.rule_str() {
        Some(text) if !allowed.contains(&text) => {
            Err(rule_error("in", &[("values", &allowed.join(", "))]))
        }
        _ => Ok(()),
    }
}

/// The field equals its confirmation (message key `confirmed`, as in
/// `password` / `password_confirmation`). The error is on the field itself.
pub fn confirmed<'a, V, W, C>(confirmation: &'a W) -> impl FnOnce(&V, &C) -> garde::Result + 'a
where
    V: PartialEq<W> + ?Sized,
    W: ?Sized,
    C: ?Sized,
{
    move |value, _ctx| {
        if value == confirmation {
            Ok(())
        } else {
            Err(rule_error("confirmed", &[]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_and_options_are_rule_inputs() {
        assert!(uuid("not-a-uuid", &()).is_err());
        assert!(uuid(&None::<String>, &()).is_ok());
        assert!(uuid(&Some(uuid::Uuid::now_v7().to_string()), &()).is_ok());
        assert!(date_ymd(&Cow::Borrowed("2024-13-01"), &()).is_err());
        assert!(date_ymd("2024-12-31", &()).is_ok());
        assert!(one_of(&["a", "b"])("b", &()).is_ok());
        assert!(one_of(&["a", "b"])("c", &()).is_err());
        assert!(confirmed(&"x".to_owned())(&"x".to_owned(), &()).is_ok());
    }
}
