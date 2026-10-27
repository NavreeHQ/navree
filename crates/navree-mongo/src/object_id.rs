//! [`ObjectId`]: MongoDB's 12-byte ids as model keys and fields.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use navree_db::sea_orm::QueryResult;
use navree_db::sea_orm::sea_query::{Enum, OptionEnum};
use navree_orm::{Field, FieldError, KeyKind, PrimaryKey, Value, ValueFor};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The type name an ObjectId carries inside a [`Value`] (`Value::Enum`), so
/// conditions, rows and pagination cursors keep it apart from text.
pub const OBJECT_ID_TYPE: &str = "ObjectId";

/// A MongoDB ObjectId: 12 bytes, a timestamp in seconds, a random value
/// per process and a counter, so ids made later sort later.
///
/// As a model key (`id: ObjectId`, stored as `_id`) it is generated when it
/// is all zeros, the [`Default`]: a new record gets one when it is saved,
/// without asking the server. As a model field it is stored as a native
/// ObjectId. With serde it is its 24 hexadecimal digits (API responses),
/// and it reads those, extended JSON's `{"$oid": "..."}` and 12 raw bytes;
/// inside an embedded document (a JSON cast), mark the field
/// `#[serde(with = "navree_mongo::extended::object_id")]` to store it as a
/// native ObjectId too.
///
/// ```
/// use navree_mongo::ObjectId;
///
/// let id = ObjectId::new();
/// assert!(!id.is_nil());
/// assert_eq!(id.to_string().len(), 24);
/// assert_eq!(id.to_string().parse::<ObjectId>().unwrap(), id);
/// assert!(ObjectId::default().is_nil());
/// assert_eq!(serde_json::to_value(id).unwrap(), serde_json::json!(id.to_hex()));
/// assert!(ObjectId::parse("not hex").is_err());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ObjectId([u8; 12]);

impl ObjectId {
    /// A new id, made now.
    pub fn new() -> ObjectId {
        ObjectId(bson::oid::ObjectId::new().bytes())
    }

    /// The all-zero id: an unset key.
    pub const fn nil() -> ObjectId {
        ObjectId([0; 12])
    }

    /// Whether the id is all zeros (an unset key).
    pub fn is_nil(&self) -> bool {
        self.0 == [0; 12]
    }

    /// The id with these bytes.
    pub const fn from_bytes(bytes: [u8; 12]) -> ObjectId {
        ObjectId(bytes)
    }

    /// The id's bytes.
    pub const fn bytes(&self) -> [u8; 12] {
        self.0
    }

    /// Parses 24 hexadecimal digits.
    pub fn parse(text: &str) -> Result<ObjectId, ObjectIdError> {
        let text = text.trim();
        bson::oid::ObjectId::parse_str(text)
            .map(ObjectId::from)
            .map_err(|_| ObjectIdError(text.to_owned()))
    }

    /// The 24 hexadecimal digits.
    pub fn to_hex(&self) -> String {
        bson::oid::ObjectId::from(*self).to_hex()
    }

    /// When the id was made (to the second).
    pub fn timestamp(&self) -> DateTime<Utc> {
        bson::oid::ObjectId::from(*self).timestamp().to_chrono()
    }

    /// The id as a [`Value`]: an enum value of type
    /// [`OBJECT_ID_TYPE`] holding the hexadecimal digits.
    pub fn to_value(&self) -> Value {
        Value::Enum(OptionEnum::Some(Box::new(Enum {
            type_name: OBJECT_ID_TYPE.into(),
            value: Cow::Owned(self.to_hex()),
        })))
    }
}

/// Text that is not an ObjectId.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not an ObjectId (24 hexadecimal digits)")]
pub struct ObjectIdError(String);

impl From<bson::oid::ObjectId> for ObjectId {
    fn from(id: bson::oid::ObjectId) -> Self {
        ObjectId(id.bytes())
    }
}

impl From<ObjectId> for bson::oid::ObjectId {
    fn from(id: ObjectId) -> Self {
        bson::oid::ObjectId::from_bytes(id.0)
    }
}

impl From<ObjectId> for bson::Bson {
    fn from(id: ObjectId) -> Self {
        bson::Bson::ObjectId(id.into())
    }
}

impl From<ObjectId> for Value {
    fn from(id: ObjectId) -> Self {
        id.to_value()
    }
}

/// An ObjectId for an untyped column (`col("owner_id").eq(id)`).
impl ValueFor<Value> for ObjectId {
    fn into_value_for(self) -> Value {
        self.to_value()
    }
}

impl FromStr for ObjectId {
    type Err = ObjectIdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        ObjectId::parse(text)
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({})", self.to_hex())
    }
}

/// The 24 hexadecimal digits.
impl Serialize for ObjectId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

/// Hexadecimal digits, extended JSON (`{"$oid": "..."}`, which is also how
/// BSON's deserializer presents a native ObjectId) or 12 raw bytes.
impl<'de> Deserialize<'de> for ObjectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ObjectIdVisitor)
    }
}

struct ObjectIdVisitor;

impl<'de> Visitor<'de> for ObjectIdVisitor {
    type Value = ObjectId;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an ObjectId: 24 hexadecimal digits or {\"$oid\": \"...\"}")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<ObjectId, E> {
        ObjectId::parse(text).map_err(E::custom)
    }

    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<ObjectId, E> {
        <[u8; 12]>::try_from(bytes)
            .map(ObjectId)
            .map_err(|_| E::invalid_length(bytes.len(), &"12 bytes"))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ObjectId, A::Error> {
        let mut id = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == "$oid" {
                let text: String = map.next_value()?;
                id = Some(ObjectId::parse(&text).map_err(de::Error::custom)?);
            } else {
                return Err(de::Error::unknown_field(&key, &["$oid"]));
            }
        }
        id.ok_or_else(|| de::Error::missing_field("$oid"))
    }
}

/// The ObjectId in `value`: an ObjectId value, its hexadecimal text, its 12
/// bytes, or JSON holding either form.
fn object_id(value: Value) -> Result<ObjectId, FieldError> {
    if navree_orm::is_null(&value) {
        return Err(FieldError::Null);
    }
    match value {
        Value::Enum(OptionEnum::Some(id)) => {
            ObjectId::parse(&id.value).map_err(|error| FieldError::Invalid(error.to_string()))
        }
        Value::String(Some(text)) => {
            ObjectId::parse(&text).map_err(|error| FieldError::Invalid(error.to_string()))
        }
        Value::Bytes(Some(bytes)) => <[u8; 12]>::try_from(bytes.as_slice())
            .map(ObjectId)
            .map_err(|_| {
                FieldError::Invalid(format!("{} bytes are not an ObjectId (12)", bytes.len()))
            }),
        Value::Json(Some(json)) => serde_json::from_value::<ObjectId>(*json)
            .map_err(|error| FieldError::Invalid(error.to_string())),
        _ => Err(FieldError::Invalid(
            "expected an ObjectId, found a value of another type".to_owned(),
        )),
    }
}

/// An ObjectId column: an ObjectId value in documents; hexadecimal text
/// (`CHAR(24)`) or 12 bytes (`BINARY(12)`) in SQL databases.
impl Field for ObjectId {
    fn to_value(&self) -> Value {
        ObjectId::to_value(self)
    }

    fn null() -> Value {
        Value::Enum(OptionEnum::None(OBJECT_ID_TYPE.into()))
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        match <Option<String> as Field>::from_sql(row, column) {
            Ok(Some(text)) => object_id(Value::String(Some(text))),
            Ok(None) => Err(FieldError::Null),
            Err(FieldError::Invalid(first)) => match <Vec<u8> as Field>::from_sql(row, column) {
                Ok(bytes) => object_id(Value::Bytes(Some(bytes))),
                Err(_) => Err(FieldError::Invalid(first)),
            },
            Err(other) => Err(other),
        }
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        object_id(value)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("ObjectId")
    }
}

/// An ObjectId key, generated (without the server) when it is all zeros.
impl PrimaryKey for ObjectId {
    fn is_empty(&self) -> bool {
        self.is_nil()
    }

    fn generate(kind: KeyKind) -> Option<Self> {
        (kind == KeyKind::ObjectId).then(ObjectId::new)
    }

    fn empty() -> Self {
        ObjectId::nil()
    }

    fn to_key_string(&self) -> String {
        self.to_hex()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_carry_the_type() {
        let id = ObjectId::parse("65f1c0ffee0000000000002a").unwrap();
        let value = Field::to_value(&id);
        assert_eq!(
            navree_orm::to_json(&value),
            serde_json::json!("65f1c0ffee0000000000002a")
        );
        assert_eq!(<ObjectId as Field>::from_value(value).unwrap(), id);
        assert!(navree_orm::is_null(&<ObjectId as Field>::null()));
        assert!(navree_orm::same_value(&id.to_value(), &id.to_value()));
        assert!(!navree_orm::same_value(
            &id.to_value(),
            &ObjectId::new().to_value()
        ));
    }

    #[test]
    fn values_read_leniently() {
        let id = ObjectId::new();
        for value in [
            Value::String(Some(id.to_hex())),
            Value::Bytes(Some(id.bytes().to_vec())),
            Value::Json(Some(Box::new(serde_json::json!({ "$oid": id.to_hex() })))),
            Value::Json(Some(Box::new(serde_json::json!(id.to_hex())))),
        ] {
            assert_eq!(<ObjectId as Field>::from_value(value).unwrap(), id);
        }
        assert_eq!(
            <Option<ObjectId> as Field>::from_value(<ObjectId as Field>::null()).unwrap(),
            None
        );
        let error = <ObjectId as Field>::from_value(Value::from("nope")).unwrap_err();
        assert_eq!(
            error.to_string(),
            "`nope` is not an ObjectId (24 hexadecimal digits)"
        );
        assert_eq!(
            <ObjectId as Field>::from_value(Value::from(7_i64))
                .unwrap_err()
                .to_string(),
            "expected an ObjectId, found a value of another type"
        );
    }

    #[test]
    fn keys_are_generated_when_nil() {
        assert!(PrimaryKey::is_empty(&ObjectId::default()));
        let made = <ObjectId as PrimaryKey>::generate(KeyKind::ObjectId).unwrap();
        assert!(!made.is_nil());
        assert!(<ObjectId as PrimaryKey>::generate(KeyKind::UuidV7).is_none());
        let later = ObjectId::new();
        assert!(later > made, "ids made later sort later");
        assert_eq!(made.to_key_string(), made.to_hex());
        let age = Utc::now() - made.timestamp();
        assert!(age.num_seconds().abs() < 60, "{age}");
    }

    #[test]
    fn serde_reads_hex_and_extended_json() {
        let id = ObjectId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", id.to_hex()));
        assert_eq!(serde_json::from_str::<ObjectId>(&json).unwrap(), id);
        let extended = format!(r#"{{"$oid":"{}"}}"#, id.to_hex());
        assert_eq!(serde_json::from_str::<ObjectId>(&extended).unwrap(), id);
        assert!(serde_json::from_str::<ObjectId>(r#"{"oid":"x"}"#).is_err());

        // A native ObjectId read through BSON's deserializer.
        #[derive(Deserialize, PartialEq, Debug)]
        struct Holder {
            id: ObjectId,
        }
        let document = bson::doc! { "id": bson::oid::ObjectId::from(id) };
        assert_eq!(
            bson::deserialize_from_document::<Holder>(document).unwrap(),
            Holder { id }
        );
    }
}
