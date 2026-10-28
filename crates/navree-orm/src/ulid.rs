//! [`Ulid`]: time-ordered 128-bit identifiers in 26 characters.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::value::{Field, FieldError, FieldKind, Kind, Value};

/// Crockford's base32 alphabet (no I, L, O or U).
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The length of a ULID's text form.
const LEN: usize = 26;

/// A ULID: a 48-bit millisecond timestamp followed by 80 random bits,
/// written as 26 characters of Crockford's base32
/// (`01JB3QG8ZK0000000000000000`). ULIDs sort by creation time, as text
/// and as numbers, which keeps indexes on time-series tables compact.
///
/// As a model key (`key_type = "ulid"`, inferred for `Ulid` fields) a nil
/// ULID is replaced by a fresh one when the record is inserted. Stored as
/// text (`CHAR(26)`); serialized as its text.
///
/// ```
/// use navree_orm::Ulid;
///
/// let id = Ulid::new();
/// let text = id.to_string();
/// assert_eq!(text.len(), 26);
/// assert_eq!(text.parse::<Ulid>().unwrap(), id);
/// assert!(Ulid::nil().is_nil());
/// assert!(Ulid::from_parts(1, 0) < Ulid::from_parts(2, 0));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ulid(u128);

/// Why a text is not a ULID.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a ULID (26 characters of Crockford base32)")]
pub struct UlidError(String);

impl Ulid {
    /// A new ULID for the current time.
    pub fn new() -> Self {
        let now = Utc::now().timestamp_millis();
        // Before 1970 or after the year 10889: neither happens on a running clock.
        let millis = u64::try_from(now).unwrap_or(0);
        Self::from_parts(millis, random_80())
    }

    /// The nil ULID (all zero bits).
    pub const fn nil() -> Self {
        Ulid(0)
    }

    /// A ULID from its timestamp (milliseconds since the Unix epoch; the
    /// low 48 bits are used) and its random part (the low 80 bits).
    pub const fn from_parts(millis: u64, random: u128) -> Self {
        let millis = (millis as u128) & ((1 << 48) - 1);
        let random = random & ((1 << 80) - 1);
        Ulid((millis << 80) | random)
    }

    /// Whether this is the nil ULID.
    pub const fn is_nil(&self) -> bool {
        self.0 == 0
    }

    /// The timestamp part, in milliseconds since the Unix epoch.
    pub const fn timestamp_ms(&self) -> u64 {
        (self.0 >> 80) as u64
    }

    /// The timestamp part as a date-time.
    pub fn datetime(&self) -> DateTime<Utc> {
        let millis = i64::try_from(self.timestamp_ms()).unwrap_or(i64::MAX);
        DateTime::from_timestamp_millis(millis).unwrap_or_default()
    }

    /// The 128 bits.
    pub const fn to_u128(&self) -> u128 {
        self.0
    }

    /// The ULID with these 128 bits.
    pub const fn from_u128(bits: u128) -> Self {
        Ulid(bits)
    }

    /// The same 128 bits as a UUID (for columns of UUID type).
    pub fn to_uuid(&self) -> Uuid {
        Uuid::from_u128(self.0)
    }
}

/// 80 random bits, taken from a version-4 UUID (122 random bits from the
/// operating system's generator).
fn random_80() -> u128 {
    Uuid::new_v4().as_u128() & ((1 << 80) - 1)
}

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = [0u8; LEN];
        let mut bits = self.0;
        for slot in text.iter_mut().rev() {
            // The low five bits index the alphabet.
            *slot = ALPHABET[(bits & 0x1f) as usize];
            bits >>= 5;
        }
        // The alphabet is ASCII.
        f.write_str(std::str::from_utf8(&text).unwrap_or_default())
    }
}

impl FromStr for Ulid {
    type Err = UlidError;

    /// Parses the 26-character text, ignoring case and reading `I` and `L`
    /// as `1` and `O` as `0`, as Crockford's base32 asks.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || UlidError(s.to_owned());
        if s.len() != LEN {
            return Err(invalid());
        }
        let mut bits: u128 = 0;
        for (i, c) in s.bytes().enumerate() {
            let digit = decode_char(c).ok_or_else(invalid)?;
            // The first character carries only 3 bits (26 * 5 = 130 > 128).
            if i == 0 && digit > 7 {
                return Err(invalid());
            }
            bits = (bits << 5) | u128::from(digit);
        }
        Ok(Ulid(bits))
    }
}

fn decode_char(c: u8) -> Option<u8> {
    let c = c.to_ascii_uppercase();
    let c = match c {
        b'I' | b'L' => b'1',
        b'O' => b'0',
        other => other,
    };
    ALPHABET
        .iter()
        .position(|&a| a == c)
        .and_then(|i| u8::try_from(i).ok())
}

impl Serialize for Ulid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Ulid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Cow::<str>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl From<Ulid> for Value {
    fn from(ulid: Ulid) -> Self {
        Value::String(ulid.to_string())
    }
}

impl Field for Ulid {
    const KIND: FieldKind = FieldKind::Ulid;

    fn to_value(&self) -> Value {
        Value::from(*self)
    }

    fn null() -> Value {
        Value::Null(Kind::String)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        match value {
            Value::Uuid(uuid) => Ok(Ulid(uuid.as_u128())),
            other => String::from_value(other)?
                .parse()
                .map_err(|error: UlidError| FieldError::Invalid(error.to_string())),
        }
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Ulid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(Ulid::nil().to_string(), "00000000000000000000000000");
        assert_eq!(
            Ulid::from_u128(u128::MAX).to_string(),
            "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"
        );
        let id = Ulid::from_parts(1_469_918_176_385, 0);
        assert_eq!(&id.to_string()[..10], "01ARYZ6S41");
        assert_eq!(id.timestamp_ms(), 1_469_918_176_385);
    }

    #[test]
    fn parsing_is_case_insensitive_and_strict_about_length_and_range() {
        let id = Ulid::new();
        let text = id.to_string();
        assert_eq!(text.to_lowercase().parse::<Ulid>().unwrap(), id);
        assert!(
            "8ZZZZZZZZZZZZZZZZZZZZZZZZZ".parse::<Ulid>().is_err(),
            "overflows 128 bits"
        );
        assert!("0123".parse::<Ulid>().is_err());
        assert!("0000000000000000000000000U".parse::<Ulid>().is_err());
        assert_eq!(
            "0000000000000000000000000L".parse::<Ulid>().unwrap(),
            Ulid::from_u128(1)
        );
    }

    #[test]
    fn new_ulids_carry_the_time_and_differ() {
        let before = Utc::now().timestamp_millis();
        let a = Ulid::new();
        let b = Ulid::new();
        assert_ne!(a, b);
        let ms = i64::try_from(a.timestamp_ms()).unwrap();
        assert!(ms >= before && ms <= Utc::now().timestamp_millis());
        assert!(!a.is_nil());
    }

    #[test]
    fn serde_and_values() {
        let id = Ulid::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<Ulid>(&json).unwrap(), id);
        assert_eq!(id.to_value(), Value::String(id.to_string()));
        assert_eq!(Ulid::from_value(id.to_value()).unwrap(), id);
        assert_eq!(
            Ulid::from_value(Value::Uuid(id.to_uuid())).unwrap(),
            id
        );
    }
}
