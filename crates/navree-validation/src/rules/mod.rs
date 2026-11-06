//! The rules: the functions behind `#[validate(...)]`'s built-in rules
//! (`email`, `url`, `ip`, `length`, `range`, ...) and Navree's rules for
//! `custom(...)` (`uuid`, `date_ymd`, `one_of`, `confirmed`, ...), each
//! with a table-driven message (`email`, `uuid`, `date_format`, `in`,
//! `confirmed`).
//!
//! ```
//! use navree_validation::Validate;
//! use navree_validation::rules::{self, confirmed};
//!
//! #[derive(Validate)]
//! struct NewDeal {
//!     #[validate(custom(rules::uuid))]
//!     company_id: String,
//!     #[validate(custom(rules::date_ymd))]
//!     expected_close_date: Option<String>,
//!     #[validate(custom(rules::one_of(&["lead", "qualified", "won", "lost"])))]
//!     stage: String,
//!     #[validate(length(min = 8), custom(confirmed(&self.password_confirmation)))]
//!     password: String,
//!     #[validate(skip)]
//!     password_confirmation: String,
//! }
//! ```
//!
//! A rule is a function of the value, `fn(&T) -> Result<(), Violation>`,
//! so you can also call one in your own `custom` rule. The text rules
//! accept `str`, `String`, `Cow<str>`, [`Folded`](crate::Folded) strings
//! and `Option`s of them; `None` passes (combine with `required` to demand
//! a value).
//!
//! For international input: [`unicode_name`] (names in any script),
//! [`country`], [`locale`] and, with the `phone` feature, `phone`. They
//! check strings and leave them as sent; the typed fields
//! ([`Country`](crate::Country), [`Locale`](crate::Locale), `Phone`) also
//! normalize them.

use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::rc::Rc;
use std::sync::Arc;

use chrono::NaiveDate;
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

use crate::{Violation, rule_error};

mod email;
mod measure;
mod url;

pub use email::is_email;
pub use measure::{
    Each, Length, Measure, Pattern, Range, Required, Unit, length, pattern, range, required,
    same_as,
};
pub use url::is_url;

/// A value the text rules can check: `None` means "no value" and passes.
#[diagnostic::on_unimplemented(
    message = "this rule checks text, and `{Self}` is not text",
    label = "the rule needs a `String`, `&str`, `Cow<str>` or `Folded` text (or an `Option` of one)"
)]
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

impl<T: RuleInput + ?Sized> RuleInput for Box<T> {
    fn rule_str(&self) -> Option<&str> {
        (**self).rule_str()
    }
}

impl<T: RuleInput + ?Sized> RuleInput for Arc<T> {
    fn rule_str(&self) -> Option<&str> {
        (**self).rule_str()
    }
}

impl<T: RuleInput + ?Sized> RuleInput for Rc<T> {
    fn rule_str(&self) -> Option<&str> {
        (**self).rule_str()
    }
}

impl<T: RuleInput> RuleInput for Option<T> {
    fn rule_str(&self) -> Option<&str> {
        self.as_ref().and_then(RuleInput::rule_str)
    }
}

/// Fails with `key` when the text is there and `valid` refuses it.
fn text_rule<T: RuleInput + ?Sized>(
    value: &T,
    key: &'static str,
    valid: impl FnOnce(&str) -> bool,
) -> Result<(), Violation> {
    match value.rule_str() {
        Some(text) if !valid(text) => Err(Violation::rule(key)),
        _ => Ok(()),
    }
}

/// An email address (message key `email`); see [`is_email`] for what
/// passes.
pub fn email<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "email", is_email)
}

/// An absolute URL (message key `url`); see [`is_url`] for what passes.
pub fn url<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "url", is_url)
}

/// An IPv4 or IPv6 address (message key `ip`).
pub fn ip<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "ip", |text| text.parse::<IpAddr>().is_ok())
}

/// An IPv4 address in dotted decimal, `192.0.2.1` (message key `ipv4`).
pub fn ipv4<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "ipv4", |text| text.parse::<Ipv4Addr>().is_ok())
}

/// An IPv6 address, `2001:db8::1` (message key `ipv6`).
pub fn ipv6<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "ipv6", |text| text.parse::<Ipv6Addr>().is_ok())
}

/// ASCII characters only (message key `ascii`).
pub fn ascii<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "ascii", str::is_ascii)
}

/// Letters and digits only, of any script (message key `alpha_num`).
pub fn alphanumeric<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "alpha_num", |text| {
        text.chars().all(char::is_alphanumeric)
    })
}

/// A payment card number (message key `credit_card`): 12 to 19 digits,
/// spaces and hyphens between them allowed, with a valid Luhn check digit.
pub fn credit_card<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "credit_card", is_card_number)
}

fn is_card_number(text: &str) -> bool {
    if !text
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b' ' || byte == b'-')
    {
        return false;
    }
    let digits: Vec<u32> = text
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|byte| u32::from(byte - b'0'))
        .collect();
    if !(12..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &digit)| match (i % 2, digit * 2) {
            (0, _) => digit,
            (_, doubled) if doubled > 9 => doubled - 9,
            (_, doubled) => doubled,
        })
        .sum();
    sum.is_multiple_of(10)
}

/// The text contains `needle` (message key `contains`, with `:value`).
pub fn contains<T: RuleInput + ?Sized>(value: &T, needle: &str) -> Result<(), Violation> {
    match value.rule_str() {
        Some(text) if !text.contains(needle) => {
            Err(Violation::rule("contains").with("value", needle))
        }
        _ => Ok(()),
    }
}

/// The text starts with `prefix` (message key `starts_with`, with
/// `:values`).
pub fn starts_with<T: RuleInput + ?Sized>(value: &T, prefix: &str) -> Result<(), Violation> {
    match value.rule_str() {
        Some(text) if !text.starts_with(prefix) => {
            Err(Violation::rule("starts_with").with("values", prefix))
        }
        _ => Ok(()),
    }
}

/// The text ends with `suffix` (message key `ends_with`, with `:values`).
pub fn ends_with<T: RuleInput + ?Sized>(value: &T, suffix: &str) -> Result<(), Violation> {
    match value.rule_str() {
        Some(text) if !text.ends_with(suffix) => {
            Err(Violation::rule("ends_with").with("values", suffix))
        }
        _ => Ok(()),
    }
}

/// A UUID in the hyphenated form (`018f…-…`) (message key `uuid`).
pub fn uuid<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "uuid", is_uuid)
}

fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| bytes[i] == b'-')
        && uuid::Uuid::try_parse(text).is_ok()
}

/// A calendar date written exactly `YYYY-MM-DD` (message key
/// `date_format`, with `:format` = `YYYY-MM-DD`).
pub fn date_ymd<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
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
pub fn one_of<'a, T: RuleInput + ?Sized>(
    allowed: &'a [&'a str],
) -> impl FnOnce(&T) -> Result<(), Violation> + 'a {
    move |value| match value.rule_str() {
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
pub fn unicode_name<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "unicode_name", is_unicode_name)
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
pub fn country<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "country", |text| crate::Country::parse(text).is_ok())
}

/// A well-formed BCP 47 language tag (message key `locale`). The
/// [`Locale`](crate::Locale) type also normalizes it.
pub fn locale<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    text_rule(value, "locale", |text| crate::Locale::parse(text).is_ok())
}

/// A valid phone number (message key `phone`), with or without a country
/// code: one without is read in the request's default region (see
/// [`Phone`](crate::Phone), which also turns it into E.164). Feature
/// `phone`.
#[cfg(feature = "phone")]
pub fn phone<T: RuleInput + ?Sized>(value: &T) -> Result<(), Violation> {
    match value.rule_str().map(crate::Phone::parse_in_context) {
        Some(Ok(Some(_))) | None => Ok(()),
        // Invalid, or the extractor has yet to resolve the region (it then
        // validates again).
        Some(Err(_) | Ok(None)) => Err(Violation::rule("phone")),
    }
}

/// The field equals its confirmation (message key `confirmed`, as in
/// `password` / `password_confirmation`). The error is on the field itself.
pub fn confirmed<'a, V, W>(confirmation: &'a W) -> impl FnOnce(&V) -> Result<(), Violation> + 'a
where
    V: PartialEq<W> + ?Sized,
    W: ?Sized,
{
    move |value| {
        if value == confirmation {
            Ok(())
        } else {
            Err(Violation::rule("confirmed"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_and_options_are_rule_inputs() {
        assert!(uuid("not-a-uuid").is_err());
        assert!(uuid(&None::<String>).is_ok());
        assert!(uuid(&Some(uuid::Uuid::now_v7().to_string())).is_ok());
        assert!(date_ymd(&Cow::Borrowed("2024-13-01")).is_err());
        assert!(date_ymd("2024-12-31").is_ok());
        assert!(one_of(&["a", "b"])("b").is_ok());
        assert!(one_of(&["a", "b"])("c").is_err());
        assert!(confirmed(&"x".to_owned())(&"x".to_owned()).is_ok());
        assert!(email(&Box::<str>::from("anu@example.com")).is_ok());
    }

    #[test]
    fn built_in_text_rules() {
        assert!(email("anu@example.com").is_ok());
        assert_eq!(email("anu@").unwrap_err().key(), Some("email"));
        assert!(url("https://example.com").is_ok());
        assert_eq!(
            url("example.com").unwrap_err().message(),
            "value must be a URL address"
        );
        assert!(ip("2001:db8::1").is_ok() && ip("192.0.2.1").is_ok());
        assert!(ip("192.0.2").is_err());
        assert!(ipv4("192.0.2.1").is_ok() && ipv4("::1").is_err());
        assert!(ipv6("::1").is_ok() && ipv6("192.0.2.1").is_err());
        assert!(ascii("abc!").is_ok() && ascii("abç").is_err());
        assert!(alphanumeric("abc123ç").is_ok() && alphanumeric("a b").is_err());
        assert_eq!(
            alphanumeric("a-b").unwrap_err().message(),
            "value must contain only letters and numbers"
        );
        assert_eq!(
            contains("abc", "x").unwrap_err().message(),
            "value must contain a x string"
        );
        assert_eq!(
            starts_with("abc", "x").unwrap_err().message(),
            "value must start with x"
        );
        assert_eq!(
            ends_with("abc", "x").unwrap_err().message(),
            "value must end with x"
        );
        assert!(contains("abc", "b").is_ok());
        assert!(starts_with("abc", "a").is_ok() && ends_with("abc", "c").is_ok());
    }

    #[test]
    fn card_numbers_need_a_luhn_check_digit() {
        for number in [
            "4111 1111 1111 1111",
            "5500-0000-0000-0004",
            "378282246310005",
        ] {
            assert!(credit_card(number).is_ok(), "{number}");
        }
        for number in [
            "4111 1111 1111 1112",
            "4111",
            "41111111111111111111",
            "4111a111111111111",
            "",
        ] {
            assert!(credit_card(number).is_err(), "{number}");
        }
    }

    #[test]
    fn names_in_any_script() {
        for name in [
            "Anu Lakhotia",
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
            "Anu!",
            "Anu😀",
            "<b>",
            "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}",
            "Anu\u{0}",
            "Anu\u{202e}",
            "Anu\u{a0}Lakhotia",
        ] {
            assert!(!is_unicode_name(name), "{name:?}");
        }
        // Four marks in a row are fine; the count restarts after a letter.
        assert!(is_unicode_name(
            "a\u{0300}\u{0301}\u{0302}\u{0303}b\u{0300}"
        ));
        assert!(unicode_name(&None::<String>).is_ok());
        assert!(unicode_name("Anu 2").is_ok());
    }

    #[test]
    fn country_and_locale_strings() {
        assert!(country("gb").is_ok());
        assert!(country("UK").is_err());
        assert!(country(&None::<&str>).is_ok());
        assert!(locale("en-gb").is_ok());
        assert!(locale("en_GB").is_err());
        assert_eq!(
            country("GBR").unwrap_err().message(),
            "value must be a valid ISO 3166-1 alpha-2 country code"
        );
        assert_eq!(
            locale("x").unwrap_err().message(),
            "value must be a valid locale"
        );
    }

    #[cfg(feature = "phone")]
    #[test]
    fn phone_strings() {
        assert!(phone("+44 7400 123456").is_ok());
        assert!(phone(&None::<String>).is_ok());
        assert_eq!(
            phone("07400 123456").unwrap_err().message(),
            "value must be a valid phone number"
        );
        let gb = crate::InputContext::new().with_default_region(crate::Country::parse("GB").ok());
        assert!(gb.scope(|| phone("07400 123456")).is_ok());
    }
}
