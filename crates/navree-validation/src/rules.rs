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
//! The string rules accept `str`, `String`, `Cow<str>`, [`Folded`](crate::Folded)
//! strings and `Option`s of them; `None` passes (combine with `required` to
//! demand a value).
//!
//! For international input: [`unicode_name`] (names in any script),
//! [`country`], [`locale`] and, with the `phone` feature, `phone`. They
//! check strings and leave them as sent; the typed fields
//! ([`Country`](crate::Country), [`Locale`](crate::Locale), `Phone`) also
//! normalize them.

use std::borrow::Cow;

use chrono::NaiveDate;
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

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

/// Punctuation a name may contain besides letters, marks and numbers: the
/// space, hyphens, apostrophes, the period, the middle dot (Catalan
/// `l·l`), and the zero-width non-joiner and joiner that Persian and Indic
/// names are written with.
const NAME_PUNCTUATION: &[char] = &[
    ' ', '-', '\u{2010}', '\'', '\u{2019}', '.', '\u{00b7}', '\u{200c}', '\u{200d}',
];

/// The most combining marks a name may stack in a row ("Zalgo" text stacks
/// dozens).
const MAX_MARKS: usize = 4;

/// A personal name in any script (message key `unicode_name`): letters,
/// combining marks and numbers of every script (`Amritpal Singh`,
/// `अमृतपाल सिंह`, `محمد عبد الله`, `李小龙`), spaces, hyphens, apostrophes
/// (`'` and `’`), periods, the middle dot, and the zero-width (non-)joiner.
/// At least one letter, and no more than four combining marks in a row.
/// Symbols, emoji, markup characters and control characters fail.
pub fn unicode_name<T: RuleInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.rule_str() {
        Some(text) if !is_unicode_name(text) => Err(rule_error("unicode_name", &[])),
        _ => Ok(()),
    }
}

fn is_unicode_name(text: &str) -> bool {
    let mut letter = false;
    let mut marks = 0;
    for c in text.chars() {
        match c.general_category_group() {
            GeneralCategoryGroup::Mark => {
                marks += 1;
                if marks > MAX_MARKS {
                    return false;
                }
                continue;
            }
            GeneralCategoryGroup::Letter => letter = true,
            GeneralCategoryGroup::Number => {}
            _ if NAME_PUNCTUATION.contains(&c) => {}
            _ => return false,
        }
        marks = 0;
    }
    letter
}

/// An ISO 3166-1 alpha-2 country code, in any case (message key
/// `country`). The [`Country`](crate::Country) type also normalizes it.
pub fn country<T: RuleInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.rule_str() {
        Some(text) if crate::Country::parse(text).is_err() => Err(rule_error("country", &[])),
        _ => Ok(()),
    }
}

/// A well-formed BCP 47 language tag (message key `locale`). The
/// [`Locale`](crate::Locale) type also normalizes it.
pub fn locale<T: RuleInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.rule_str() {
        Some(text) if crate::Locale::parse(text).is_err() => Err(rule_error("locale", &[])),
        _ => Ok(()),
    }
}

/// A valid phone number (message key `phone`), with or without a country
/// code: one without is read in the request's default region (see
/// [`Phone`](crate::Phone), which also turns it into E.164). Feature
/// `phone`.
#[cfg(feature = "phone")]
pub fn phone<T: RuleInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.rule_str().map(crate::Phone::parse_in_context) {
        Some(Ok(Some(_))) | None => Ok(()),
        // Invalid, or the extractor has yet to resolve the region (it then
        // validates again).
        Some(Err(_) | Ok(None)) => Err(rule_error("phone", &[])),
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

    #[test]
    fn names_in_any_script() {
        for name in [
            "Ada Lovelace",
            "अमृतपाल सिंह",
            "مُحَمَّد",
            "فاطمه\u{200c}زهرا",
            "Gal·la O’Neil-Smith Jr.",
            "Nguye\u{0302}\u{0303}n",
            "José María",
            "Ⅻ Louis",
        ] {
            assert!(is_unicode_name(name), "{name}");
        }
        for name in [
            "",
            " ",
            "-'.",
            "123",
            "Ada!",
            "Ada😀",
            "<b>",
            "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}",
            "Ada\u{0}",
            "Ada\u{202e}",
            "Ada\u{a0}Lovelace",
        ] {
            assert!(!is_unicode_name(name), "{name:?}");
        }
        // Four marks in a row are fine; the count restarts after a letter.
        assert!(is_unicode_name(
            "a\u{0300}\u{0301}\u{0302}\u{0303}b\u{0300}"
        ));
        assert!(unicode_name(&None::<String>, &()).is_ok());
        assert!(unicode_name("Ada 2", &()).is_ok());
    }

    #[test]
    fn country_and_locale_strings() {
        assert!(country("gb", &()).is_ok());
        assert!(country("UK", &()).is_err());
        assert!(country(&None::<&str>, &()).is_ok());
        assert!(locale("en-gb", &()).is_ok());
        assert!(locale("en_GB", &()).is_err());
        assert_eq!(
            country("GBR", &()).unwrap_err().message(),
            "value must be a valid ISO 3166-1 alpha-2 country code"
        );
        assert_eq!(
            locale("x", &()).unwrap_err().message(),
            "value must be a valid locale"
        );
    }

    #[cfg(feature = "phone")]
    #[test]
    fn phone_strings() {
        assert!(phone("+44 7400 123456", &()).is_ok());
        assert!(phone(&None::<String>, &()).is_ok());
        assert_eq!(
            phone("07400 123456", &()).unwrap_err().message(),
            "value must be a valid phone number"
        );
        let gb = crate::InputContext::new().with_default_region(crate::Country::parse("GB").ok());
        assert!(gb.scope(|| phone("07400 123456", &())).is_ok());
    }
}
