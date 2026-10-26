//! [`Locale`]: a BCP 47 language tag.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::Country;
use crate::failure::serde_error;

/// A locale: a well-formed BCP 47 language tag, as Unicode locale
/// identifiers define it (`en`, `en-GB`, `ar-EG`, `zh-Hant-TW`,
/// `ar-EG-u-nu-arab`), in canonical case.
///
/// It deserializes from the tag in any case (`"EN-gb"` is `en-GB`) and
/// serializes as the canonical tag. Subtags are separated by hyphens:
/// `en_GB` is not a tag. Inside the extractors a malformed tag is a field
/// error with the message key `locale` (`locale must be a valid locale`).
/// The check is about form, not about which languages exist.
///
/// ```
/// use navree_validation::Locale;
///
/// let locale: Locale = "zh-hant-tw".parse().unwrap();
/// assert_eq!(locale.as_str(), "zh-Hant-TW");
/// assert_eq!(locale.language(), "zh");
/// assert_eq!(locale.script(), Some("Hant"));
/// assert_eq!(locale.region(), Some("TW"));
/// assert_eq!(locale.country().map(|c| c.as_str()), Some("TW"));
/// assert!(Locale::parse("en_GB").is_err());
/// ```
#[derive(Clone)]
pub struct Locale {
    tag: String,
    parsed: icu_locale_core::Locale,
}

/// A string that is not a well-formed BCP 47 language tag.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a BCP 47 language tag")]
pub struct InvalidLocale(String);

impl Locale {
    /// The locale written as `tag`, in any case.
    pub fn parse(tag: &str) -> Result<Locale, InvalidLocale> {
        let parsed = icu_locale_core::Locale::try_from_str(tag)
            .map_err(|_| InvalidLocale(tag.to_owned()))?;
        Ok(Locale {
            tag: parsed.to_string(),
            parsed,
        })
    }

    /// The canonical tag (`"en-GB"`).
    pub fn as_str(&self) -> &str {
        &self.tag
    }

    /// The language subtag (`"en"`; `"und"` when undetermined).
    pub fn language(&self) -> &str {
        self.parsed.id.language.as_str()
    }

    /// The script subtag (`"Hant"`), if any.
    pub fn script(&self) -> Option<&str> {
        self.parsed.id.script.as_ref().map(|script| script.as_str())
    }

    /// The region subtag (`"GB"`, or a UN M.49 area such as `"419"`), if
    /// any.
    pub fn region(&self) -> Option<&str> {
        self.parsed.id.region.as_ref().map(|region| region.as_str())
    }

    /// The region as a [`Country`], when it is an ISO 3166-1 code.
    pub fn country(&self) -> Option<Country> {
        self.region().and_then(|region| Country::parse(region).ok())
    }
}

impl PartialEq for Locale {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag
    }
}

impl Eq for Locale {}

impl PartialOrd for Locale {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Locale {
    fn cmp(&self, other: &Self) -> Ordering {
        self.tag.cmp(&other.tag)
    }
}

impl Hash for Locale {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tag.hash(state);
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.tag)
    }
}

impl fmt::Debug for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Locale({})", self.tag)
    }
}

impl FromStr for Locale {
    type Err = InvalidLocale;

    fn from_str(tag: &str) -> Result<Self, Self::Err> {
        Locale::parse(tag)
    }
}

impl AsRef<str> for Locale {
    fn as_ref(&self) -> &str {
        &self.tag
    }
}

impl From<Locale> for String {
    fn from(locale: Locale) -> Self {
        locale.tag
    }
}

impl Serialize for Locale {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.tag)
    }
}

impl<'de> Deserialize<'de> for Locale {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LocaleVisitor;

        impl Visitor<'_> for LocaleVisitor {
            type Value = Locale;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }

            fn visit_str<E: de::Error>(self, tag: &str) -> Result<Locale, E> {
                Locale::parse(tag).map_err(|invalid| serde_error("locale", invalid))
            }
        }

        deserializer.deserialize_str(LocaleVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_canonicalized() {
        for (input, tag) in [
            ("en", "en"),
            ("EN-gb", "en-GB"),
            ("ar-eg", "ar-EG"),
            ("hi-deva-in", "hi-Deva-IN"),
            ("es-419", "es-419"),
            ("ar-EG-U-NU-ARAB", "ar-EG-u-nu-arab"),
            ("und", "und"),
        ] {
            assert_eq!(Locale::parse(input).unwrap().as_str(), tag, "{input}");
        }
        let latin_america = Locale::parse("es-419").unwrap();
        assert_eq!(latin_america.region(), Some("419"));
        assert_eq!(latin_america.country(), None);
        assert_eq!(latin_america.script(), None);
    }

    #[test]
    fn malformed_tags_are_rejected() {
        for input in ["", "e", "en_US", "en-", "-en", "123", "en--GB", "en GB", "ëñ"] {
            let error = Locale::parse(input).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("`{input}` is not a BCP 47 language tag")
            );
        }
    }

    #[test]
    fn equality_and_serde_follow_the_canonical_tag() {
        let a = Locale::parse("en-gb").unwrap();
        let b: Locale = serde_json::from_str(r#""EN-GB""#).unwrap();
        assert_eq!(a, b);
        assert_eq!(serde_json::to_string(&b).unwrap(), r#""en-GB""#);
        assert_eq!(format!("{a} {a:?}"), "en-GB Locale(en-GB)");
        assert!(serde_json::from_str::<Locale>(r#""en_GB""#).is_err());
    }
}
