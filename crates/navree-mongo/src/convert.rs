//! Column values ([`Value`]) and BSON, both ways, and documents as rows.
//!
//! | Value | BSON | Read back as |
//! |---|---|---|
//! | `bool` | boolean | `Bool` |
//! | `i8`, `i16`, `i32`, `u8`, `u16` | 32-bit integer | `Int` |
//! | `i64`, `u32`, `u64` up to `i64::MAX` | 64-bit integer | `BigInt` |
//! | `f32`, `f64` | double | `Double` |
//! | [`Decimal`] | Decimal128 | `Decimal` (text beyond 28 digits) |
//! | `String`, `char` | string | `String` |
//! | [`ObjectId`](crate::ObjectId) | ObjectId | `Enum` of type `ObjectId` |
//! | `Uuid` | binary, subtype 4 | `Uuid` |
//! | `Vec<u8>` | binary, generic subtype | `Bytes` |
//! | `DateTime<Utc>`, `DateTime<FixedOffset>`, `NaiveDateTime` (UTC) | date (milliseconds) | `ChronoDateTimeUtc` |
//! | `NaiveDate` | date at midnight UTC | `ChronoDateTimeUtc` (reads as a `NaiveDate`) |
//! | `NaiveTime` | string `HH:MM:SS.ffffff` | `String` (reads as a `NaiveTime`) |
//! | JSON (`serde_json::Value`, JSON casts) | embedded document, array or scalar | `Json` (relaxed extended JSON) |
//! | NULL of any type | null | NULL |
//!
//! JSON is read as [extended JSON], so a JSON cast whose value holds
//! `{"$oid": ".."}` or `{"$date": ".."}` stores a native ObjectId or date
//! (see [`extended`](crate::extended)), and embedded documents come back as
//! relaxed extended JSON: plain JSON for strings, numbers, booleans,
//! arrays and objects, `{"$oid": ..}` and `{"$date": ..}` for the others.
//!
//! [extended JSON]: https://www.mongodb.com/docs/manual/reference/mongodb-extended-json/

use std::str::FromStr;

use bson::spec::BinarySubtype;
use bson::{Binary, Bson, Decimal128, Document};
use chrono::{NaiveTime, TimeZone, Utc};
use navree_orm::{Decimal, OrmError, Row, Value, is_null};

use crate::error::invalid;
use crate::object_id::{OBJECT_ID_TYPE, ObjectId};

/// The text a time of day is stored as: fixed width, so text order is
/// time order.
const TIME_FORMAT: &str = "%H:%M:%S%.6f";

/// `value` as BSON (see the [module docs](self) for the mapping).
///
/// ```
/// use bson::Bson;
/// use navree_mongo::{ObjectId, to_bson};
/// use navree_orm::{Decimal, Kind, Value};
///
/// assert_eq!(to_bson(&Value::from(42_i32)).unwrap(), Bson::Int32(42));
/// assert_eq!(to_bson(&Value::from(42_i64)).unwrap(), Bson::Int64(42));
/// let id = ObjectId::new();
/// assert_eq!(to_bson(&id.to_value()).unwrap(), Bson::ObjectId(id.into()));
/// assert_eq!(to_bson(&Value::from(Decimal::new(1250, 2))).unwrap().to_string(), "12.50");
/// assert_eq!(to_bson(&Value::Null(Kind::String)).unwrap(), Bson::Null);
/// ```
pub fn to_bson(value: &Value) -> Result<Bson, OrmError> {
    if is_null(value) {
        return Ok(Bson::Null);
    }
    Ok(match value {
        Value::Bool(b) => Bson::Boolean(*b),
        Value::I8(v) => Bson::Int32(i32::from(*v)),
        Value::I16(v) => Bson::Int32(i32::from(*v)),
        Value::I32(v) => Bson::Int32(*v),
        Value::I64(v) => Bson::Int64(*v),
        Value::U8(v) => Bson::Int32(i32::from(*v)),
        Value::U16(v) => Bson::Int32(i32::from(*v)),
        Value::U32(v) => Bson::Int64(i64::from(*v)),
        Value::U64(v) => Bson::Int64(i64::try_from(*v).map_err(|_| {
            invalid(format!(
                "{v} does not fit MongoDB's 64-bit integers (store it as a Decimal)"
            ))
        })?),
        Value::F32(v) => Bson::Double(f64::from(*v)),
        Value::F64(v) => Bson::Double(*v),
        Value::String(s) => Bson::String(s.clone()),
        Value::Enum { type_name, value } if type_name == OBJECT_ID_TYPE => {
            let id = ObjectId::parse(value).map_err(|error| invalid(error.to_string()))?;
            Bson::ObjectId(id.into())
        }
        Value::Enum { value, .. } => Bson::String(value.clone()),
        Value::Bytes(bytes) => Bson::Binary(Binary {
            subtype: BinarySubtype::Generic,
            bytes: bytes.clone(),
        }),
        Value::Json(json) => json_to_bson(json)?,
        Value::Date(date) => Bson::DateTime(bson::DateTime::from_chrono(
            Utc.from_utc_datetime(&date.and_time(NaiveTime::MIN)),
        )),
        Value::Time(time) => Bson::String(time.format(TIME_FORMAT).to_string()),
        Value::DateTime(at) => {
            Bson::DateTime(bson::DateTime::from_chrono(Utc.from_utc_datetime(at)))
        }
        Value::Timestamp(at) => Bson::DateTime(bson::DateTime::from_chrono(*at)),
        Value::TimestampTz(at) => {
            Bson::DateTime(bson::DateTime::from_chrono(*at))
        }
        Value::Uuid(uuid) => Bson::Binary(Binary {
            subtype: BinarySubtype::Uuid,
            bytes: uuid.as_bytes().to_vec(),
        }),
        Value::Decimal(decimal) => Bson::Decimal128(decimal128(decimal)?),
        // Vectors and what later versions add: their JSON form.
        other => json_to_bson(&navree_orm::to_json(other))?,
    })
}

/// `decimal` as a Decimal128.
fn decimal128(decimal: &Decimal) -> Result<Decimal128, OrmError> {
    Decimal128::from_str(&decimal.to_string())
        .map_err(|error| invalid(format!("{decimal} is not a Decimal128: {error}")))
}

/// JSON as BSON, reading extended JSON (`{"$oid": ..}`, `{"$date": ..}`,
/// `{"$numberDecimal": ..}`) as the native types.
pub(crate) fn json_to_bson(json: &serde_json::Value) -> Result<Bson, OrmError> {
    Bson::try_from(json.clone()).map_err(|error| {
        invalid(format!(
            "JSON that is not valid extended JSON cannot be stored: {error}"
        ))
    })
}

/// BSON as a [`Value`] (see the [module docs](self) for the mapping).
///
/// ```
/// use bson::{Bson, doc};
/// use navree_mongo::from_bson;
/// use navree_orm::Value;
///
/// assert_eq!(from_bson(Bson::Int32(7)), Value::I32(7));
/// assert_eq!(
///     from_bson(Bson::Document(doc! { "city": "Oslo", "floor": 3 })),
///     Value::from(serde_json::json!({ "city": "Oslo", "floor": 3 }))
/// );
/// assert!(navree_orm::is_null(&from_bson(Bson::Null)));
/// ```
pub fn from_bson(bson: Bson) -> Value {
    match bson {
        Bson::Double(v) => Value::F64(v),
        Bson::String(s) | Bson::Symbol(s) => Value::String(s),
        Bson::Boolean(b) => Value::Bool(b),
        Bson::Null | Bson::Undefined => Value::Null(navree_orm::Kind::String),
        Bson::Int32(v) => Value::I32(v),
        Bson::Int64(v) => Value::I64(v),
        Bson::ObjectId(id) => ObjectId::from(id).to_value(),
        Bson::DateTime(at) => Value::Timestamp(at.to_chrono()),
        Bson::Binary(binary) => match binary.subtype {
            BinarySubtype::Uuid if binary.bytes.len() == 16 => {
                match uuid::Uuid::from_slice(&binary.bytes) {
                    Ok(uuid) => Value::Uuid(uuid),
                    Err(_) => Value::Bytes(binary.bytes),
                }
            }
            _ => Value::Bytes(binary.bytes),
        },
        Bson::Decimal128(decimal) => {
            let text = decimal.to_string();
            match Decimal::from_str(&text).or_else(|_| Decimal::from_scientific(&text)) {
                Ok(decimal) => Value::Decimal(decimal),
                // NaN, infinities, more digits than a Decimal holds.
                Err(_) => Value::String(text),
            }
        }
        other @ (Bson::Document(_)
        | Bson::Array(_)
        | Bson::RegularExpression(_)
        | Bson::JavaScriptCode(_)
        | Bson::JavaScriptCodeWithScope(_)
        | Bson::Timestamp(_)
        | Bson::MaxKey
        | Bson::MinKey
        | Bson::DbPointer(_)) => Value::Json(other.into_relaxed_extjson()),
    }
}

/// A document as a row: the `columns` asked for, in order (a missing field
/// is NULL, as an absent column would be), or every field in document
/// order.
///
/// ```
/// use bson::doc;
/// use navree_mongo::document_row;
///
/// let columns = ["title".to_owned(), "stage".to_owned()];
/// let row = document_row(doc! { "title": "Renewal", "amount": 5 }, Some(&columns));
/// assert_eq!(row.columns(), ["title", "stage"]);
/// assert_eq!(row.get::<String>("title").unwrap(), "Renewal");
/// assert_eq!(row.get::<Option<String>>("stage").unwrap(), None);
/// ```
pub fn document_row(mut document: Document, columns: Option<&[String]>) -> Row {
    match columns {
        Some(columns) => Row::from_values(
            columns
                .iter()
                .map(|column| {
                    let value = document
                        .remove(column.as_str())
                        .map_or(Value::Null(navree_orm::Kind::String), from_bson);
                    (column.clone(), value)
                })
                .collect(),
        ),
        None => Row::from_values(
            document
                .into_iter()
                .map(|(field, value)| (field, from_bson(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, NaiveDate, NaiveDateTime};
    use navree_orm::Field;

    use super::*;

    fn round_trip<T: Field + PartialEq + std::fmt::Debug>(value: T) -> T {
        let bson = to_bson(&value.to_value()).unwrap();
        T::from_value(from_bson(bson)).unwrap()
    }

    #[test]
    fn fields_round_trip() {
        assert!(round_trip(true));
        assert_eq!(round_trip(-7_i8), -7);
        assert_eq!(round_trip(1_234_i16), 1_234);
        assert_eq!(round_trip(i32::MIN), i32::MIN);
        assert_eq!(round_trip(i64::MAX), i64::MAX);
        assert_eq!(round_trip(u32::MAX), u32::MAX);
        assert_eq!(round_trip(2.5_f32), 2.5);
        assert_eq!(round_trip(0.1_f64), 0.1);
        assert_eq!(
            round_trip(Decimal::new(1_234_567_890_123, 4)),
            Decimal::new(1_234_567_890_123, 4)
        );
        assert_eq!(round_trip("naïve ✓".to_owned()), "naïve ✓");
        assert_eq!(round_trip(vec![0_u8, 1, 255]), vec![0, 1, 255]);
        let uuid = uuid::Uuid::now_v7();
        assert_eq!(round_trip(uuid), uuid);
        let id = ObjectId::new();
        assert_eq!(round_trip(id), id);
        let day = NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();
        assert_eq!(round_trip(day), day);
        let time = NaiveTime::from_hms_micro_opt(8, 30, 15, 250_000).unwrap();
        assert_eq!(round_trip(time), time);
        let json = serde_json::json!({ "tags": ["a", "b"], "n": 3, "nested": { "ok": true } });
        assert_eq!(round_trip(json.clone()), json);
        assert_eq!(round_trip(Some(3_i64)), Some(3));
        assert_eq!(round_trip(None::<String>), None);
    }

    #[test]
    fn date_times_keep_milliseconds() {
        let at = DateTime::parse_from_rfc3339("2026-10-27T09:30:00.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        let back: DateTime<Utc> = round_trip(at);
        assert_eq!(back.to_rfc3339(), "2026-10-27T09:30:00.123+00:00");
        let naive: NaiveDateTime = round_trip(at.naive_utc());
        assert_eq!(naive, back.naive_utc());
        let offset = DateTime::parse_from_rfc3339("2026-10-27T11:30:00.500+02:00").unwrap();
        let back: DateTime<chrono::FixedOffset> = round_trip(offset);
        assert_eq!(back, offset, "the same instant, read in UTC");
        assert_eq!(
            to_bson(&Value::from(NaiveDate::from_ymd_opt(2026, 10, 27).unwrap())).unwrap(),
            Bson::DateTime(bson::DateTime::from_millis(1_793_059_200_000))
        );
    }

    #[test]
    fn values_map_to_the_native_types() {
        assert_eq!(to_bson(&Value::from(3_u8)).unwrap(), Bson::Int32(3));
        assert_eq!(to_bson(&Value::from(3_u64)).unwrap(), Bson::Int64(3));
        let error = to_bson(&Value::from(u64::MAX)).unwrap_err();
        assert!(error.to_string().contains("does not fit"), "{error}");
        let uuid = uuid::Uuid::now_v7();
        assert_eq!(
            to_bson(&Value::from(uuid)).unwrap(),
            Bson::Binary(Binary {
                subtype: BinarySubtype::Uuid,
                bytes: uuid.as_bytes().to_vec()
            })
        );
        assert_eq!(
            to_bson(&Value::from(NaiveTime::from_hms_opt(8, 5, 0).unwrap())).unwrap(),
            Bson::String("08:05:00.000000".into())
        );
    }

    #[test]
    fn extended_json_keeps_native_types_in_embedded_documents() {
        let id = ObjectId::new();
        let json = serde_json::json!({
            "owner": { "$oid": id.to_hex() },
            "at": { "$date": "2026-10-27T09:30:00Z" },
            "amount": { "$numberDecimal": "12.50" },
            "plain": "text",
        });
        let Bson::Document(document) = to_bson(&Value::from(json.clone())).unwrap() else {
            panic!("a document");
        };
        assert_eq!(document.get_object_id("owner").unwrap(), id.into());
        assert!(document.get_datetime("at").is_ok());
        assert!(matches!(document.get("amount"), Some(Bson::Decimal128(_))));
        // Read back as relaxed extended JSON.
        let back = from_bson(Bson::Document(document));
        assert_eq!(
            navree_orm::to_json(&back)["owner"],
            serde_json::json!({ "$oid": id.to_hex() })
        );
        assert_eq!(navree_orm::to_json(&back)["plain"], "text");
    }

    #[test]
    fn decimals_beyond_rust_decimal_read_as_text() {
        let huge: Decimal128 = "1.234567890123456789012345678901234E+40".parse().unwrap();
        assert!(matches!(
            from_bson(Bson::Decimal128(huge)),
            Value::String(_)
        ));
        let nan: Decimal128 = "NaN".parse().unwrap();
        assert_eq!(
            from_bson(Bson::Decimal128(nan)),
            Value::String("NaN".into())
        );
    }

    #[test]
    fn rows_fill_missing_fields_with_null() {
        let document = bson::doc! { "_id": 1, "name": "Acme", "extra": true };
        let columns = vec!["_id".to_owned(), "name".to_owned(), "country".to_owned()];
        let row = document_row(document.clone(), Some(&columns));
        assert_eq!(row.columns(), ["_id", "name", "country"]);
        assert!(is_null(&row.value("country").unwrap()));
        let row = document_row(document, None);
        assert_eq!(row.columns(), ["_id", "name", "extra"]);
    }
}
