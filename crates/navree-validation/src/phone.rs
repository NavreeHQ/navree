//! [`Phone`]: a phone number in E.164 form (feature `phone`).

use std::fmt;
use std::hash::{Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::str::FromStr;

use phonenumber::{Mode, PhoneNumber};
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::failure::serde_error;
use crate::{Country, context, fold_digits};

/// Longer input is not a phone number (the limit of the reference library).
const MAX_INPUT: usize = 250;

/// A valid phone number, kept in E.164 form (`+447400123456`).
///
/// It deserializes from what people type: spaces, dashes, brackets,
/// `tel:` URIs, an extension (`ext. 12`), and digits of any script
/// (`+٩٧١ ٥٠ ١٢٣ ٤٥٦٧`, folded by [`fold_digits`]). A number written with
/// a country code (`+44 ...`) stands alone; one written without (`07400
/// 123456`) is read in the request's default region: the app's
/// [`RegionSource`](crate::RegionSource), else
/// `validation.phone.default_region`. Without either it is invalid.
///
/// Inside the extractors an invalid number is a field error with the
/// message key `phone` (`phone must be a valid phone number`). "Valid"
/// means the number matches its region's numbering plan; nobody checks
/// that it is in service.
///
/// It serializes as the E.164 string, which is the form to store. The
/// extension is not part of E.164: read it from
/// [`extension`](Phone::extension) before storing, if you keep it.
///
/// ```
/// use navree_validation::{Country, Phone};
///
/// let phone = Phone::parse("+44 7400 123456", None).unwrap();
/// assert_eq!(phone.as_str(), "+447400123456");
/// assert_eq!(phone.country_code(), 44);
/// assert_eq!(phone.region(), Country::parse("GB").ok());
/// assert_eq!(phone.international(), "+44 7400 123456");
/// assert_eq!(phone.national(), "07400 123456");
///
/// // Without a country code, the number needs a region.
/// let uae = Country::parse("AE").ok();
/// assert_eq!(Phone::parse("٠٥٠ ١٢٣ ٤٥٦٧", uae).unwrap().as_str(), "+971501234567");
/// assert!(Phone::parse("050 123 4567", None).is_err());
/// ```
#[derive(Clone)]
pub struct Phone {
    e164: String,
    number: PhoneNumber,
}

/// A string that is not a valid phone number.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a valid phone number")]
pub struct InvalidPhone(String);

impl Phone {
    /// Parses `text`, reading a number without a country code in
    /// `default_region`.
    pub fn parse(text: &str, default_region: Option<Country>) -> Result<Phone, InvalidPhone> {
        let invalid = || InvalidPhone(text.to_owned());
        if text.len() > MAX_INPUT {
            return Err(invalid());
        }
        let folded = fold_digits(text);
        let region = default_region.and_then(|country| country.as_str().parse().ok());
        // The parser has panicked on odd input before; a panic here is just
        // an invalid number.
        let parsed = catch_unwind(AssertUnwindSafe(|| {
            phonenumber::parse(region, folded.as_ref())
                .ok()
                .filter(phonenumber::is_valid)
        }));
        let number = parsed.ok().flatten().ok_or_else(invalid)?;
        Ok(Phone {
            e164: number.format().mode(Mode::E164).to_string(),
            number,
        })
    }

    /// The E.164 form (`"+447400123456"`).
    pub fn as_str(&self) -> &str {
        &self.e164
    }

    /// The E.164 form, owned.
    pub fn into_string(self) -> String {
        self.e164
    }

    /// The country calling code (`44`).
    pub fn country_code(&self) -> u16 {
        self.number.code().value()
    }

    /// The national significant number (`7400123456`).
    pub fn national_number(&self) -> u64 {
        self.number.national().value()
    }

    /// The region the number belongs to, when it is an ISO 3166-1 country
    /// (`None` for non-geographic numbers and areas such as Kosovo).
    pub fn region(&self) -> Option<Country> {
        let id = self.number.country().id()?;
        Country::parse(id.as_ref()).ok()
    }

    /// The extension (`"12"` from `ext. 12`), which E.164 leaves out.
    pub fn extension(&self) -> Option<&str> {
        self.number.extension().map(|extension| extension.as_ref())
    }

    /// The international format (`"+44 7400 123456"`), with the extension
    /// if any (`" x12"`).
    pub fn international(&self) -> String {
        self.number.format().mode(Mode::International).to_string()
    }

    /// The national format (`"07400 123456"`), with the extension if any.
    pub fn national(&self) -> String {
        self.number.format().mode(Mode::National).to_string()
    }

    /// Whether `text` must be read in a region (no country code).
    pub(crate) fn needs_region(text: &str) -> bool {
        let text = text.trim_start();
        let text = text.strip_prefix("tel:").unwrap_or(text);
        !text.starts_with(['+', '\u{ff0b}'])
    }

    /// Parses `text` with the region in effect: `Ok(None)` while an
    /// extractor has not resolved it yet.
    pub(crate) fn parse_in_context(text: &str) -> Result<Option<Phone>, InvalidPhone> {
        let region = if Phone::needs_region(&fold_digits(text)) {
            match context::current() {
                Some(context) => context.default_region(),
                None => return Ok(None),
            }
        } else {
            None
        };
        Phone::parse(text, region).map(Some)
    }
}

impl PartialEq for Phone {
    fn eq(&self, other: &Self) -> bool {
        self.e164 == other.e164 && self.extension() == other.extension()
    }
}

impl Eq for Phone {}

impl Hash for Phone {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.e164.hash(state);
        self.extension().hash(state);
    }
}

impl fmt::Display for Phone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.e164)
    }
}

impl fmt::Debug for Phone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.extension() {
            Some(extension) => write!(f, "Phone({} ext. {extension})", self.e164),
            None => write!(f, "Phone({})", self.e164),
        }
    }
}

impl FromStr for Phone {
    type Err = InvalidPhone;

    /// An international number (`+44 ...`); see [`Phone::parse`] for
    /// numbers without a country code.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Phone::parse(text, None)
    }
}

impl AsRef<str> for Phone {
    fn as_ref(&self) -> &str {
        &self.e164
    }
}

impl From<Phone> for String {
    fn from(phone: Phone) -> Self {
        phone.e164
    }
}

impl Serialize for Phone {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.e164)
    }
}

impl<'de> Deserialize<'de> for Phone {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PhoneVisitor;

        impl Visitor<'_> for PhoneVisitor {
            type Value = Phone;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<Phone, E> {
                match Phone::parse_in_context(text) {
                    Ok(Some(phone)) => Ok(phone),
                    // The extractor resolves the region and runs again.
                    Ok(None) => Err(serde_error(
                        "phone",
                        "the default region is not resolved yet",
                    )),
                    Err(invalid) => Err(serde_error("phone", invalid)),
                }
            }
        }

        deserializer.deserialize_str(PhoneVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InputContext;

    fn region(code: &str) -> Option<Country> {
        Country::parse(code).ok()
    }

    #[test]
    fn international_numbers_in_any_digits() {
        for (input, e164) in [
            ("+44 7400 123456", "+447400123456"),
            ("+44 (0) 7400 123456", "+447400123456"),
            ("+1 201-555-0123", "+12015550123"),
            ("+٩٧١ ٥٠ ١٢٣ ٤٥٦٧", "+971501234567"),
            ("+۹۸ ۹۱۲ ۳۴۵ ۶۷۸۹", "+989123456789"),
            ("+९१ ८१२३४ ५६७८९", "+918123456789"),
            ("tel:+44-7400-123456", "+447400123456"),
            ("＋４４ ７４００ １２３４５６", "+447400123456"),
        ] {
            let phone = Phone::parse(input, None).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(phone.as_str(), e164, "{input}");
            assert_eq!(phone, input.parse::<Phone>().unwrap());
        }
    }

    #[test]
    fn national_numbers_read_in_the_region() {
        assert_eq!(
            Phone::parse("07400 123456", region("GB")).unwrap().as_str(),
            "+447400123456"
        );
        assert_eq!(
            Phone::parse("٠١٠٠ ١٢٣ ٤٥٦٧", region("EG"))
                .unwrap()
                .as_str(),
            "+201001234567"
        );
        // The international prefix dialled from the region.
        assert_eq!(
            Phone::parse("00 971 50 123 4567", region("GB"))
                .unwrap()
                .as_str(),
            "+971501234567"
        );
        assert!(Phone::parse("07400 123456", None).is_err());
        // A region without numbering metadata reads nothing.
        assert!(Phone::parse("12345", region("AQ")).is_err());
    }

    #[test]
    fn invalid_input_is_an_error_not_a_panic() {
        let long = "1".repeat(300);
        for input in [
            "",
            "+",
            "++44",
            "hello",
            "+44 12",
            "+999 1234 5678",
            "+0",
            "tel:",
            "ext. 12",
            "+44 7400 123456 789 012",
            "+1 (((((",
            "+४४",
            &long,
            "\u{0}",
            "+44\u{202e}7400123456",
        ] {
            let error = Phone::parse(input, region("GB")).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("`{input}` is not a valid phone number")
            );
        }
    }

    #[test]
    fn details_and_formats() {
        let phone = Phone::parse("+44 7400 123456 ext. 12", None).unwrap();
        assert_eq!(phone.as_str(), "+447400123456");
        assert_eq!(phone.extension(), Some("12"));
        assert_eq!(phone.country_code(), 44);
        assert_eq!(phone.national_number(), 7_400_123_456);
        assert_eq!(phone.region(), region("GB"));
        // The formats for people show the extension; E.164 can't.
        assert_eq!(phone.international(), "+44 7400 123456 x12");
        assert_eq!(phone.national(), "07400 123456 x12");
        assert_eq!(
            format!("{phone} {phone:?}"),
            "+447400123456 Phone(+447400123456 ext. 12)"
        );
        assert_ne!(phone, Phone::parse("+447400123456", None).unwrap());
        assert_eq!(String::from(phone.clone()), "+447400123456");
        assert_eq!(phone.into_string(), "+447400123456");
    }

    #[test]
    fn serde_uses_the_scope_region() {
        let phone: Phone = serde_json::from_str(r#""+44 7400 123456""#).unwrap();
        assert_eq!(serde_json::to_value(&phone).unwrap(), "+447400123456");

        let error = serde_json::from_str::<Phone>(r#""07400 123456""#).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("`07400 123456` is not a valid phone number"),
            "{error}"
        );
        let gb = InputContext::new().with_default_region(region("GB"));
        let phone: Phone = gb
            .scope(|| serde_json::from_str(r#""07400 123456""#))
            .unwrap();
        assert_eq!(phone.as_str(), "+447400123456");
    }

    #[test]
    fn which_numbers_need_a_region() {
        assert!(Phone::needs_region("07400 123456"));
        assert!(Phone::needs_region("00 44 7400 123456"));
        assert!(!Phone::needs_region(" +44 7400 123456"));
        assert!(!Phone::needs_region("tel:+44-7400-123456"));
        assert!(!Phone::needs_region("＋44"));
    }
}
