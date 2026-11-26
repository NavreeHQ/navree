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
//! use navree_datetime::Timestamp;
//! use navree_mongo::ObjectId;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Assignment {
//!     #[serde(with = "navree_mongo::extended::object_id")]
//!     user: ObjectId,
//!     #[serde(with = "navree_mongo::extended::date_time")]
//!     at: Timestamp,
//! }
//!
//! let user = ObjectId::new();
//! let at = Timestamp::parse_rfc3339("2026-10-27T09:30:00.250Z").unwrap();
//! let json = serde_json::to_value(Assignment { user, at }).unwrap();
//! assert_eq!(json["user"], serde_json::json!({ "$oid": user.to_hex() }));
//! assert_eq!(json["at"], serde_json::json!({ "$date": "2026-10-27T09:30:00.250Z" }));
//! ```
//!
//! The same struct then serializes that way in API responses too; keep a
//! separate response type when clients should see plain strings.
//!
//! [extended JSON]: https://www.mongodb.com/docs/manual/reference/mongodb-extended-json/

use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serializer};

pub use navree_datetime::bson::extended::{date_time, option_date_time};

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

#[cfg(test)]
mod tests {
    use navree_datetime::Timestamp;
    use serde::Serialize;

    use super::*;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Stamp {
        #[serde(with = "super::date_time")]
        at: Timestamp,
        #[serde(with = "super::option_date_time", default)]
        until: Option<Timestamp>,
        #[serde(with = "super::option_object_id", default)]
        by: Option<ObjectId>,
    }

    #[test]
    fn dates_read_every_extended_form() {
        let at = Timestamp::parse_rfc3339("2026-10-27T09:30:00.250Z").unwrap();
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
            at: Timestamp::parse_rfc3339("2026-10-27T09:30:00.250Z").unwrap(),
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
