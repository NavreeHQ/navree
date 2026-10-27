//! Serde helpers for the fields of embedded documents (JSON casts) that
//! should keep MongoDB's native types.
//!
//! A JSON cast (`#[model(cast = "json")]`) stores a serde type as an
//! embedded document, and its JSON is read as [extended JSON]: a field that
//! serializes as `{"$oid": ".."}` is stored as a native ObjectId, one that
//! serializes as `{"$date": ..}` as a native date, so indexes, `$lookup`s
//! and date operators work on them. These modules make a field serialize
//! that way (and read either form back):
//!
//! ```
//! use chrono::{DateTime, Utc};
//! use navree_mongo::ObjectId;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Assignment {
//!     #[serde(with = "navree_mongo::extended::object_id")]
//!     user: ObjectId,
//!     #[serde(with = "navree_mongo::extended::date_time")]
//!     at: DateTime<Utc>,
//! }
//!
//! let user = ObjectId::new();
//! let at = DateTime::parse_from_rfc3339("2026-10-27T09:30:00.250Z").unwrap().with_timezone(&Utc);
//! let json = serde_json::to_value(Assignment { user, at }).unwrap();
//! assert_eq!(json["user"], serde_json::json!({ "$oid": user.to_hex() }));
//! assert_eq!(json["at"], serde_json::json!({ "$date": "2026-10-27T09:30:00.250Z" }));
//! ```
//!
//! The same struct then serializes that way in API responses too; keep a
//! separate response type when clients should see plain strings.
//!
//! [extended JSON]: https://www.mongodb.com/docs/manual/reference/mongodb-extended-json/

use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serializer};

use crate::object_id::ObjectId;

/// An [`ObjectId`] as `{"$oid": ".."}`: a native ObjectId once stored.
pub mod object_id {
    use super::*;

    /// Writes `{"$oid": "<24 hex digits>"}`.
    pub fn serialize<S: Serializer>(id: &ObjectId, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("$oid", &id.to_hex())?;
        map.end()
    }

    /// Reads `{"$oid": ".."}` or the hexadecimal digits.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ObjectId, D::Error> {
        ObjectId::deserialize(deserializer)
    }
}

/// An `Option<ObjectId>` as `{"$oid": ".."}` or `null`.
pub mod option_object_id {
    use super::*;

    /// Writes `{"$oid": ".."}` or `null`.
    pub fn serialize<S: Serializer>(
        id: &Option<ObjectId>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match id {
            Some(id) => super::object_id::serialize(id, serializer),
            None => serializer.serialize_none(),
        }
    }

    /// Reads `{"$oid": ".."}`, the hexadecimal digits or `null`.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<ObjectId>, D::Error> {
        Option::<ObjectId>::deserialize(deserializer)
    }
}

/// A `DateTime<Utc>` as `{"$date": "<RFC 3339>"}`: a native date (to the
/// millisecond) once stored.
pub mod date_time {
    use super::*;

    /// Writes `{"$date": "2026-10-27T09:30:00.250Z"}`.
    pub fn serialize<S: Serializer>(at: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("$date", &at.to_rfc3339_opts(SecondsFormat::Millis, true))?;
        map.end()
    }

    /// Reads `{"$date": "<RFC 3339>"}`, `{"$date": {"$numberLong": "<ms>"}}`,
    /// `{"$date": <ms>}` or an RFC 3339 string.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error> {
        deserializer.deserialize_any(DateVisitor)
    }
}

/// An `Option<DateTime<Utc>>` as `{"$date": ..}` or `null`.
pub mod option_date_time {
    use super::*;

    /// Writes `{"$date": ..}` or `null`.
    pub fn serialize<S: Serializer>(
        at: &Option<DateTime<Utc>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match at {
            Some(at) => super::date_time::serialize(at, serializer),
            None => serializer.serialize_none(),
        }
    }

    /// Reads what [`date_time`](super::date_time) reads, or `null`.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<DateTime<Utc>>, D::Error> {
        struct OptionalDate;

        impl<'de> Visitor<'de> for OptionalDate {
            type Value = Option<DateTime<Utc>>;

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

/// Reads the forms [`date_time::deserialize`] documents.
struct DateVisitor;

impl DateVisitor {
    fn millis<E: de::Error>(millis: i64) -> Result<DateTime<Utc>, E> {
        DateTime::from_timestamp_millis(millis)
            .ok_or_else(|| E::custom(format!("{millis} milliseconds is not a date")))
    }
}

impl<'de> Visitor<'de> for DateVisitor {
    type Value = DateTime<Utc>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a date: {\"$date\": \"<RFC 3339>\"} or an RFC 3339 string")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
        DateTime::parse_from_rfc3339(text)
            .map(|at| at.with_timezone(&Utc))
            .map_err(|_| E::custom(format!("`{text}` is not an RFC 3339 date-time")))
    }

    fn visit_i64<E: de::Error>(self, millis: i64) -> Result<Self::Value, E> {
        DateVisitor::millis(millis)
    }

    fn visit_u64<E: de::Error>(self, millis: u64) -> Result<Self::Value, E> {
        let millis = i64::try_from(millis).map_err(|_| E::custom("the date is out of range"))?;
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

/// The value under `$date`: RFC 3339 text, milliseconds, or
/// `{"$numberLong": "<ms>"}`.
struct DateValue(DateTime<Utc>);

impl<'de> Deserialize<'de> for DateValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(DateVisitor).map(DateValue)
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Stamp {
        #[serde(with = "super::date_time")]
        at: DateTime<Utc>,
        #[serde(with = "super::option_date_time", default)]
        until: Option<DateTime<Utc>>,
        #[serde(with = "super::option_object_id", default)]
        by: Option<ObjectId>,
    }

    #[test]
    fn dates_read_every_extended_form() {
        let at = DateTime::parse_from_rfc3339("2026-10-27T09:30:00.250Z")
            .unwrap()
            .with_timezone(&Utc);
        for json in [
            serde_json::json!({ "at": { "$date": "2026-10-27T09:30:00.250Z" } }),
            serde_json::json!({ "at": { "$date": { "$numberLong": "1793093400250" } } }),
            serde_json::json!({ "at": { "$date": 1_793_093_400_250_i64 } }),
            serde_json::json!({ "at": "2026-10-27T09:30:00.250Z" }),
        ] {
            let stamp: Stamp = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(stamp.at, at, "{json}");
            assert_eq!(stamp.until, None);
        }
        assert!(serde_json::from_value::<Stamp>(serde_json::json!({ "at": "soon" })).is_err());
    }

    #[test]
    fn values_round_trip_through_bson() {
        let stamp = Stamp {
            at: DateTime::parse_from_rfc3339("2026-10-27T09:30:00.250Z")
                .unwrap()
                .with_timezone(&Utc),
            until: None,
            by: Some(ObjectId::new()),
        };
        let json = serde_json::to_value(&stamp).unwrap();
        let bson = crate::convert::json_to_bson(&json).unwrap();
        let bson::Bson::Document(document) = &bson else {
            panic!("a document");
        };
        assert!(document.get_datetime("at").is_ok());
        assert!(document.get_object_id("by").is_ok());
        let back: Stamp = serde_json::from_value(bson.into_relaxed_extjson()).unwrap();
        assert_eq!(back, stamp);
    }
}
