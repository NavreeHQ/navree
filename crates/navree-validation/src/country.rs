//! [`Country`]: an ISO 3166-1 alpha-2 country code.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::failure::serde_error;

/// Every officially assigned ISO 3166-1 alpha-2 code, sorted.
#[rustfmt::skip]
const CODES: &[&str] = &[
    "AD", "AE", "AF", "AG", "AI", "AL", "AM", "AO", "AQ", "AR", "AS", "AT", "AU", "AW", "AX", "AZ",
    "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BL", "BM", "BN", "BO", "BQ", "BR", "BS",
    "BT", "BV", "BW", "BY", "BZ",
    "CA", "CC", "CD", "CF", "CG", "CH", "CI", "CK", "CL", "CM", "CN", "CO", "CR", "CU", "CV", "CW",
    "CX", "CY", "CZ",
    "DE", "DJ", "DK", "DM", "DO", "DZ",
    "EC", "EE", "EG", "EH", "ER", "ES", "ET",
    "FI", "FJ", "FK", "FM", "FO", "FR",
    "GA", "GB", "GD", "GE", "GF", "GG", "GH", "GI", "GL", "GM", "GN", "GP", "GQ", "GR", "GS", "GT",
    "GU", "GW", "GY",
    "HK", "HM", "HN", "HR", "HT", "HU",
    "ID", "IE", "IL", "IM", "IN", "IO", "IQ", "IR", "IS", "IT",
    "JE", "JM", "JO", "JP",
    "KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ",
    "LA", "LB", "LC", "LI", "LK", "LR", "LS", "LT", "LU", "LV", "LY",
    "MA", "MC", "MD", "ME", "MF", "MG", "MH", "MK", "ML", "MM", "MN", "MO", "MP", "MQ", "MR", "MS",
    "MT", "MU", "MV", "MW", "MX", "MY", "MZ",
    "NA", "NC", "NE", "NF", "NG", "NI", "NL", "NO", "NP", "NR", "NU", "NZ",
    "OM",
    "PA", "PE", "PF", "PG", "PH", "PK", "PL", "PM", "PN", "PR", "PS", "PT", "PW", "PY",
    "QA",
    "RE", "RO", "RS", "RU", "RW",
    "SA", "SB", "SC", "SD", "SE", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN", "SO", "SR", "SS",
    "ST", "SV", "SX", "SY", "SZ",
    "TC", "TD", "TF", "TG", "TH", "TJ", "TK", "TL", "TM", "TN", "TO", "TR", "TT", "TV", "TW", "TZ",
    "UA", "UG", "UM", "US", "UY", "UZ",
    "VA", "VC", "VE", "VG", "VI", "VN", "VU",
    "WF", "WS",
    "YE", "YT",
    "ZA", "ZM", "ZW",
];

/// A country: one of the 249 officially assigned ISO 3166-1 alpha-2 codes
/// (`GB`, `AE`, `IN`), in upper case.
///
/// It deserializes from the code in any case (`"gb"` is `GB`) and
/// serializes as the upper-case code. Inside the extractors an unknown code
/// is a field error with the message key `country` (`country must be a
/// valid ISO 3166-1 alpha-2 country code`). Codes that are only reserved
/// (`UK`, `EU`) or user-assigned (`XK`) are not countries here.
///
/// ```
/// use navree_validation::Country;
///
/// let country: Country = "ae".parse().unwrap();
/// assert_eq!(country.as_str(), "AE");
/// assert_eq!(serde_json::to_string(&country).unwrap(), r#""AE""#);
/// assert!(Country::parse("UK").is_err());
/// assert_eq!(Country::all().count(), 249);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Country(&'static str);

/// A string that is not an ISO 3166-1 alpha-2 country code.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not an ISO 3166-1 alpha-2 country code")]
pub struct InvalidCountry(String);

impl Country {
    /// The country with `code`, in any case.
    pub fn parse(code: &str) -> Result<Country, InvalidCountry> {
        let invalid = || InvalidCountry(code.to_owned());
        let &[a, b] = code.as_bytes() else {
            return Err(invalid());
        };
        let upper = [a.to_ascii_uppercase(), b.to_ascii_uppercase()];
        CODES
            .binary_search_by(|candidate| candidate.as_bytes().cmp(&upper[..]))
            .map(|index| Country(CODES[index]))
            .map_err(|_| invalid())
    }

    /// The upper-case code (`"GB"`).
    pub fn as_str(&self) -> &'static str {
        self.0
    }

    /// Every country, in code order.
    pub fn all() -> impl Iterator<Item = Country> {
        CODES.iter().map(|code| Country(code))
    }
}

impl fmt::Display for Country {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl fmt::Debug for Country {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Country({})", self.0)
    }
}

impl FromStr for Country {
    type Err = InvalidCountry;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Country::parse(code)
    }
}

impl AsRef<str> for Country {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl PartialEq<str> for Country {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Country {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl From<Country> for String {
    fn from(country: Country) -> Self {
        country.0.to_owned()
    }
}

impl Serialize for Country {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for Country {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CountryVisitor;

        impl Visitor<'_> for CountryVisitor {
            type Value = Country;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }

            fn visit_str<E: de::Error>(self, code: &str) -> Result<Country, E> {
                Country::parse(code).map_err(|invalid| serde_error("country", invalid))
            }
        }

        deserializer.deserialize_str(CountryVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_and_complete() {
        assert_eq!(CODES.len(), 249);
        assert!(CODES.windows(2).all(|w| w[0] < w[1]), "sorted, no duplicates");
        assert!(
            CODES
                .iter()
                .all(|code| code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()))
        );
    }

    #[test]
    fn codes_parse_in_any_case() {
        for (input, code) in [("gb", "GB"), ("Ae", "AE"), ("IN", "IN"), ("zw", "ZW")] {
            assert_eq!(Country::parse(input).unwrap().as_str(), code);
        }
        for input in ["", "G", "GBR", "UK", "EU", "XK", "ZZ", "1A", "ÅL", "g b"] {
            let error = Country::parse(input).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("`{input}` is not an ISO 3166-1 alpha-2 country code")
            );
        }
        let gb = Country::parse("GB").unwrap();
        assert_eq!(gb, "GB");
        assert_eq!(format!("{gb} {gb:?}"), "GB Country(GB)");
        assert_eq!(String::from(gb), "GB");
    }

    #[test]
    fn serde_round_trip() {
        let country: Country = serde_json::from_str(r#""eg""#).unwrap();
        assert_eq!(serde_json::to_value(country).unwrap(), "EG");
        let error = serde_json::from_str::<Country>(r#""XX""#).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("`XX` is not an ISO 3166-1 alpha-2 country code"),
            "{error}"
        );
        assert!(serde_json::from_str::<Country>("44").is_err());
    }
}
