//! Conversions to and from [`::bson::DateTime`], and serde helpers for
//! MongoDB Extended JSON dates (`{"$date": "..."}`).

use std::fmt;

use ::serde::de::{self, MapAccess, Visitor};
use ::serde::ser::SerializeMap;
use ::serde::{Deserialize, Deserializer, Serializer};

use crate::{Error, Precision, Timestamp, Zoned};

#[cfg_attr(docsrs, doc(cfg(feature = "bson")))]
impl From<Timestamp> for ::bson::DateTime {
    fn from(at: Timestamp) -> Self {
        ::bson::DateTime::from_millis(at.as_millis())
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "bson")))]
impl From<Zoned> for ::bson::DateTime {
    fn from(at: Zoned) -> Self {
        ::bson::DateTime::from_millis(at.timestamp().as_millis())
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "bson")))]
impl TryFrom<::bson::DateTime> for Timestamp {
    type Error = Error;

    fn try_from(dt: ::bson::DateTime) -> Result<Self, Self::Error> {
        Timestamp::from_millis(dt.timestamp_millis())
    }
}

/// Serde helpers for embedded document fields stored as MongoDB Extended JSON
/// dates (`{"$date": "..."}`).
pub mod extended {
    use super::*;

    /// A [`Timestamp`] as `{"$date": "<RFC 3339>"}`: stored as a native BSON
    /// date (to the millisecond) in MongoDB.
    pub mod date_time {
        use super::*;

        /// Writes `{"$date": "2026-10-27T09:30:00.250Z"}`.
        pub fn serialize<S: Serializer>(at: &Timestamp, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("$date", &at.to_rfc3339_with(Precision::Millis, true))?;
            map.end()
        }

        /// Reads `{"$date": "<RFC 3339>"}`, `{"$date": {"$numberLong": "<ms>"}}`,
        /// `{"$date": <ms>}` or an RFC 3339 string.
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Timestamp, D::Error> {
            deserializer.deserialize_any(DateVisitor)
        }
    }

    /// An `Option<Timestamp>` as `{"$date": ..}` or `null`.
    pub mod option_date_time {
        use super::*;

        /// Writes `{"$date": ..}` or `null`.
        pub fn serialize<S: Serializer>(
            at: &Option<Timestamp>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match at {
                Some(at) => super::date_time::serialize(at, serializer),
                None => serializer.serialize_none(),
            }
        }

        /// Reads what [`date_time`] reads, or `null`.
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Timestamp>, D::Error> {
            struct OptionalDate;

            impl<'de> Visitor<'de> for OptionalDate {
                type Value = Option<Timestamp>;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a date or null")
                }

                fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                    Ok(None)
                }

                fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                    Ok(None)
                }

                fn visit_some<D: Deserializer<'de>>(
                    self,
                    deserializer: D,
                ) -> Result<Self::Value, D::Error> {
                    super::date_time::deserialize(deserializer).map(Some)
                }
            }

            deserializer.deserialize_option(OptionalDate)
        }
    }

    struct DateVisitor;

    impl DateVisitor {
        fn millis<E: de::Error>(millis: i64) -> Result<Timestamp, E> {
            Timestamp::from_millis(millis)
                .map_err(|_| E::custom(format!("{millis} milliseconds is not a date")))
        }
    }

    impl<'de> Visitor<'de> for DateVisitor {
        type Value = Timestamp;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a date: {\"$date\": \"<RFC 3339>\"} or an RFC 3339 string")
        }

        fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
            text.parse::<Timestamp>()
                .map_err(|_| E::custom(format!("`{text}` is not an RFC 3339 date-time")))
        }

        fn visit_i64<E: de::Error>(self, millis: i64) -> Result<Self::Value, E> {
            DateVisitor::millis(millis)
        }

        fn visit_u64<E: de::Error>(self, millis: u64) -> Result<Self::Value, E> {
            let millis =
                i64::try_from(millis).map_err(|_| E::custom("the date is out of range"))?;
            DateVisitor::millis(millis)
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut at = None;
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "$date" => at = Some(map.next_value::<DateValue>()?.0),
                    // A native date read through BSON's deserializer.
                    "$numberLong" => {
                        let text: String = map.next_value()?;
                        let millis = text.parse::<i64>().map_err(de::Error::custom)?;
                        at = Some(DateVisitor::millis(millis)?);
                    }
                    other => return Err(de::Error::unknown_field(other, &["$date"])),
                }
            }
            at.ok_or_else(|| de::Error::missing_field("$date"))
        }
    }

    struct DateValue(Timestamp);

    impl<'de> Deserialize<'de> for DateValue {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_any(DateVisitor).map(DateValue)
        }
    }
}
