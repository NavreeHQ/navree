//! `serde` for [`PhoneNumber`] (its E.164 string) and [`Region`] (its code).

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::{PhoneNumber, Region};

impl Serialize for PhoneNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// From a number written with its country calling code: E.164, or any
/// international form people write.
impl<'de> Deserialize<'de> for PhoneNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NumberVisitor;

        impl Visitor<'_> for NumberVisitor {
            type Value = PhoneNumber;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a phone number with its country calling code")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<PhoneNumber, E> {
                PhoneNumber::parse(text, None)
                    .map_err(|error| E::custom(format_args!("`{text}`: {error}")))
            }
        }

        deserializer.deserialize_str(NumberVisitor)
    }
}

impl Serialize for Region {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Region {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RegionVisitor;

        impl Visitor<'_> for RegionVisitor {
            type Value = Region;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a region code such as `GB`")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<Region, E> {
                Region::parse(text)
                    .ok_or_else(|| E::custom(format_args!("`{text}` is not a region code")))
            }
        }

        deserializer.deserialize_str(RegionVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_regions_round_trip_as_strings() {
        let number = PhoneNumber::parse("+44 7400 123456", None).unwrap();
        let json = serde_json::to_string(&number).unwrap();
        assert_eq!(json, r#""+447400123456""#);
        assert_eq!(serde_json::from_str::<PhoneNumber>(&json).unwrap(), number);
        assert_eq!(
            serde_json::from_str::<PhoneNumber>(r#""+44 (0) 7400 123-456""#).unwrap(),
            number
        );
        let error = serde_json::from_str::<PhoneNumber>(r#""07400 123456""#).unwrap_err();
        assert!(error.to_string().contains("no region"), "{error}");

        let gb = Region::parse("GB").unwrap();
        assert_eq!(serde_json::to_string(&gb).unwrap(), r#""GB""#);
        assert_eq!(serde_json::from_str::<Region>(r#""gb""#).unwrap(), gb);
        assert!(serde_json::from_str::<Region>(r#""UK""#).is_err());
    }
}
