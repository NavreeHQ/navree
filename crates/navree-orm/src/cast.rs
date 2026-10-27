//! [`Cast`]: converting a field between its Rust type and its stored
//! column value, for types that are not [`Field`]s themselves.
//!
//! A field opts in with `#[model(cast = ..)]`:
//!
//! | Attribute | Cast | Field type | Stored as |
//! |---|---|---|---|
//! | `cast = "json"` | [`AsJson`] | any `Serialize + DeserializeOwned` type (structs, `Vec`, maps, sets of enums) | JSON |
//! | `cast = "string_enum"` | [`StringEnum`] | a unit enum, by its serde name | text |
//! | `cast = "int_enum"` | [`IntEnum`] | `Copy + Into<i64> + TryFrom<i64>` (a `#[repr]` enum) | an integer |
//! | `cast = "decimal(2)"` | [`DecimalScale`] | `Decimal`, rounded half away from zero to the scale | a decimal |
//! | `cast = "datetime_format(\"%d/%m/%Y %H:%M\")"` | [`DateTimeFormat`] | `DateTime<Utc>`, `DateTime<FixedOffset>`, `NaiveDateTime`, `NaiveDate`, `NaiveTime` | text in that format |
//! | `cast = "unix_timestamp"` / `"unix_timestamp_ms"` | [`UnixTimestamp`] / [`UnixTimestampMs`] | `DateTime<Utc>` | integer seconds / milliseconds |
//! | `cast = "encrypted"` | [`Encrypted`] | `String`, `Vec<u8>` | an encrypted payload (navree-crypto) |
//! | `cast = "encrypted_json"` | [`EncryptedJson`] | any `Serialize + DeserializeOwned` type | encrypted JSON |
//! | `cast = "hashed"` | [`Hashed`] | `String` | an Argon2id hash (navree-crypto) |
//! | `cast = "binary_uuid"` / `"binary_ulid"` | [`BinaryUuid`] / [`BinaryUlid`] | `Uuid` / [`Ulid`](crate::Ulid) | 16 bytes |
//! | `cast = "vector"` | [`Vector`] | `Vec<f32>` | a vector (pgvector, MySQL 9, MariaDB 11.7) |
//! | `cast` (no value) | `<T as` [`Castable`]`>::Cast` | a type that picks its own cast | the cast's |
//! | `cast = path::To::Cast` | yours | `T` where the cast is `Cast<T>` | `Cast::Stored` |
//! | `cast(path::To::Cast, 2, Eur)` | `path::To::Cast<2, Eur>` | a cast with arguments | the cast's |
//! | `inbound = path::To::Cast` | [`Inbound`] | a [`Field`] transformed on write only ([`InboundCast`]) | itself |
//! | `cast = path, columns(a: A, b: B)` | yours ([`MultiColumnCast`]) | one field over several columns | each column |
//!
//! A cast on an `Option<T>` field casts the inner value; NULL stays `None`.
//! Typed columns of cast fields encode condition values with the cast, so
//! `Deal::STAGE.eq(Stage::Won)` compares with the stored `'won'`.
//!
//! Casts are static: `get` and `set` take no `self`. Casts that need an app
//! service (the encrypter, the hasher) set [`Cast::CONTEXT`] and read it in
//! [`Cast::get_with`] / [`Cast::set_with`] from the [`CastContext`] the ORM
//! resolves from the executor's app (see [`CastContext`]); work that must
//! be asynchronous (hashing a password off the async threads) happens in
//! [`Cast::prepare`], before a record is written.
//!
//! ```
//! use navree_orm::cast::{Cast, CastError};
//!
//! /// Cents stored as a decimal string, e.g. for a legacy column.
//! struct CentsAsText;
//!
//! impl Cast<i64> for CentsAsText {
//!     type Stored = String;
//!
//!     fn get(stored: String) -> Result<i64, CastError> {
//!         stored.parse().map_err(|_| CastError::new(format!("`{stored}` is not a number of cents")))
//!     }
//!
//!     fn set(value: &i64) -> Result<String, CastError> {
//!         Ok(value.to_string())
//!     }
//! }
//!
//! assert_eq!(CentsAsText::get("1250".into()).unwrap(), 1250);
//! assert!(CentsAsText::get("12.5".into()).is_err());
//! ```

use std::fmt;

use navree_kernel::BoxFuture;
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use crate::attributes::CastContext;
pub use crate::attributes::casts::{
    BinaryUlid, BinaryUuid, Castable, DateTimeFormat, DateTimeFormatSpec, DecimalScale, Encrypted,
    FormattedDateTime,
    EncryptedJson, EncryptedValue, Hashed, Inbound, InboundCast, MultiColumnCast, StoredColumns,
    StoredVector, UnixTimestamp, UnixTimestampMs, Vector, is_password_hash,
};
use crate::row::Row;
use crate::value::{Field, FieldError, FieldKind, Value, same_value};

/// Why a cast failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastError(String);

impl CastError {
    /// An error with `message`.
    pub fn new(message: impl Into<String>) -> Self {
        CastError(message.into())
    }
}

impl fmt::Display for CastError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CastError {}

impl From<FieldError> for CastError {
    fn from(error: FieldError) -> Self {
        CastError::new(error.to_string())
    }
}

/// Converts fields of type `T` to and from their stored form.
///
/// `get` and `set` are all a cast must write. The other methods have
/// defaults and refine how the ORM treats the field:
///
/// | Method | Used for | Default |
/// |---|---|---|
/// | [`CONTEXT`](Cast::CONTEXT), [`get_with`](Cast::get_with), [`set_with`](Cast::set_with) | casts that need an app service | `get` / `set`, no context |
/// | [`eq_stored`](Cast::eq_stored), [`same`](Cast::same) | dirty tracking | stored values compared like other columns |
/// | [`from_json`](Cast::from_json) | mass assignment (`fill`) of untyped input | the input is the stored form |
/// | [`serialize`](Cast::serialize) | the value in serialized records | the field's own `Serialize` |
/// | [`increment`](Cast::increment), [`decrement`](Cast::decrement) | `Record::increment_cast` | unsupported |
/// | [`prepare`](Cast::prepare) | asynchronous work before a write (hashing) | nothing |
/// | [`is_stale`](Cast::is_stale) | rewriting stored values that are outdated (old encryption keys) | never |
pub trait Cast<T>: 'static {
    /// The stored form (a [`Field`]: text, an integer, JSON, ...).
    type Stored: Field;

    /// Whether the cast reads services from a [`CastContext`] (the
    /// encrypter, the hasher, the dialect, ...). When a model has such a
    /// cast, the ORM resolves the context from the executor's app before
    /// it reads or writes the model's records.
    const CONTEXT: bool = false;

    /// What the field's values are in serialized records (for OpenAPI
    /// schemas and date-time formatting); [`FieldKind::Any`] unless the
    /// cast says.
    const KIND: FieldKind = FieldKind::Any;

    /// The stored form to the field (reading from the database).
    fn get(stored: Self::Stored) -> Result<T, CastError>;

    /// The field to its stored form (writing to the database, comparing in
    /// conditions).
    fn set(value: &T) -> Result<Self::Stored, CastError>;

    /// [`get`](Cast::get) with the services of `context`; what the ORM
    /// calls. Casts that need a service implement this one, and `get` as
    /// `Self::get_with(stored, &CastContext::current())`.
    fn get_with(stored: Self::Stored, context: &CastContext) -> Result<T, CastError> {
        let _ = context;
        Self::get(stored)
    }

    /// [`set`](Cast::set) with the services of `context`; what the ORM
    /// calls.
    fn set_with(value: &T, context: &CastContext) -> Result<Self::Stored, CastError> {
        let _ = context;
        Self::set(value)
    }

    /// The NULL a nullable field's `None` is stored as. Defaults to the
    /// stored type's NULL; casts whose column type differs between
    /// databases give the NULL of `context`'s dialect (PostgreSQL checks a
    /// NULL's type against the column's).
    fn null_with(context: &CastContext) -> Value {
        let _ = context;
        <Self::Stored as Field>::null()
    }

    /// Whether two stored values hold the same thing (dirty tracking).
    /// Defaults to the column comparison: NULLs equal, floats within a
    /// small tolerance, decimals numerically, date-times by instant, JSON
    /// structurally.
    fn eq_stored(a: &Self::Stored, b: &Self::Stored) -> bool {
        same_value(&a.to_value(), &b.to_value())
    }

    /// Whether two field values would be stored as the same thing (dirty
    /// tracking compares a record's field with its original this way).
    /// Defaults to encoding both and comparing them with
    /// [`eq_stored`](Cast::eq_stored); a value that cannot be encoded
    /// counts as different. Casts whose stored form is not deterministic
    /// (encryption, hashing) compare the field values instead.
    fn same(a: &T, b: &T, context: &CastContext) -> bool {
        match (Self::set_with(a, context), Self::set_with(b, context)) {
            (Ok(a), Ok(b)) => Self::eq_stored(&a, &b),
            _ => false,
        }
    }

    /// The field from untyped input (a JSON value given to `fill`, a
    /// request body). Defaults to reading the input as the stored form, so
    /// a JSON cast takes JSON and a string enum takes its name; casts whose
    /// stored form is not what users send (encrypted, hashed, binary ids,
    /// timestamps) take the plain value instead.
    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<T, CastError> {
        let stored = Self::Stored::from_json(input)?;
        Self::get_with(stored, context)
    }

    /// The value in serialized records, when the cast decides it; `None`
    /// (the default) keeps what the field's own `Serialize` writes.
    fn serialize(value: &T) -> Option<Result<serde_json::Value, CastError>> {
        let _ = value;
        None
    }

    /// `value + amount`, for increments through the cast
    /// ([`Record::increment_cast`](crate::Record::increment_cast)); the
    /// database adds `amount`'s stored form. Unsupported by default.
    fn increment(value: &T, amount: &T) -> Result<T, CastError> {
        let _ = (value, amount);
        Err(CastError::new(
            "this cast does not support increments: implement Cast::increment and Cast::decrement",
        ))
    }

    /// `value - amount`, for decrements through the cast. Unsupported by
    /// default.
    fn decrement(value: &T, amount: &T) -> Result<T, CastError> {
        let _ = (value, amount);
        Err(CastError::new(
            "this cast does not support decrements: implement Cast::increment and Cast::decrement",
        ))
    }

    /// Asynchronous work on the field before the record is written (after
    /// the `saving`, `creating` and `updating` hooks): the hashed cast
    /// hashes a plain value here, off the async threads. Nothing by
    /// default.
    fn prepare<'a>(value: &'a mut T, context: &'a CastContext) -> BoxFuture<'a, Result<(), CastError>> {
        let _ = (value, context);
        Box::pin(std::future::ready(Ok(())))
    }

    /// Whether the stored value of `column` in `row` should be rewritten
    /// even though the field did not change: the encrypted casts report
    /// payloads encrypted with a previous key, so the next save re-encrypts
    /// them with the current one. Never by default.
    fn is_stale(row: &Row, column: &str, context: &CastContext) -> bool {
        let _ = (row, column, context);
        false
    }
}

/// Encodes a field through cast `C` (the encoder of a typed column), with
/// the current [`CastContext`].
pub fn encode<C: Cast<T>, T>(value: &T) -> Result<Value, String> {
    CastContext::with_current(|context| C::set_with(value, context))
        .map(|stored| stored.to_value())
        .map_err(|error| error.to_string())
}

/// Decodes a field through cast `C` (the decoder of a typed column), with
/// the current [`CastContext`].
pub fn decode<C: Cast<T>, T>(row: &Row, column: &str) -> Result<T, FieldError> {
    let stored = row.field::<C::Stored>(column)?;
    CastContext::with_current(|context| C::get_with(stored, context))
        .map_err(|error| FieldError::Invalid(error.to_string()))
}

/// Encodes a nullable field through cast `C` (`None` is a typed NULL).
pub fn encode_option<C: Cast<T>, T>(value: &Option<T>) -> Result<Value, String> {
    match value {
        Some(value) => encode::<C, T>(value),
        None => Ok(CastContext::with_current(C::null_with)),
    }
}

/// Decodes a nullable field through cast `C` (NULL is `None`).
pub fn decode_option<C: Cast<T>, T>(row: &Row, column: &str) -> Result<Option<T>, FieldError> {
    match row.field::<Option<C::Stored>>(column)? {
        Some(stored) => CastContext::with_current(|context| C::get_with(stored, context))
            .map(Some)
            .map_err(|error| FieldError::Invalid(error.to_string())),
        None => Ok(None),
    }
}

/// Any serde type stored as JSON (`#[model(cast = "json")]`): structs,
/// vectors, maps, embedded documents.
///
/// ```
/// use navree_orm::cast::{AsJson, Cast};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, PartialEq, Serialize, Deserialize)]
/// struct Address {
///     city: String,
/// }
///
/// let stored = <AsJson as Cast<Address>>::set(&Address { city: "Oslo".into() }).unwrap();
/// assert_eq!(stored, serde_json::json!({"city": "Oslo"}));
/// let address: Address = AsJson::get(stored).unwrap();
/// assert_eq!(address, Address { city: "Oslo".into() });
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct AsJson;

impl<T: Serialize + DeserializeOwned> Cast<T> for AsJson {
    type Stored = serde_json::Value;

    const KIND: FieldKind = FieldKind::Json;

    fn get(stored: serde_json::Value) -> Result<T, CastError> {
        serde_json::from_value(stored)
            .map_err(|error| CastError::new(format!("invalid JSON: {error}")))
    }

    fn set(value: &T) -> Result<serde_json::Value, CastError> {
        serde_json::to_value(value)
            .map_err(|error| CastError::new(format!("cannot encode as JSON: {error}")))
    }
}

/// A unit enum stored as its serde name (`#[model(cast = "string_enum")]`):
/// `#[serde(rename_all = "snake_case")] enum Stage { ClosedWon }` is
/// stored as `closed_won`.
///
/// ```
/// use navree_orm::cast::{Cast, StringEnum};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, PartialEq, Serialize, Deserialize)]
/// #[serde(rename_all = "snake_case")]
/// enum Stage {
///     Lead,
///     ClosedWon,
/// }
///
/// assert_eq!(<StringEnum as Cast<Stage>>::set(&Stage::ClosedWon).unwrap(), "closed_won");
/// let stage: Stage = StringEnum::get("lead".into()).unwrap();
/// assert_eq!(stage, Stage::Lead);
/// assert!(<StringEnum as Cast<Stage>>::get("won".into()).is_err());
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct StringEnum;

impl<T: Serialize + DeserializeOwned> Cast<T> for StringEnum {
    type Stored = String;

    const KIND: FieldKind = FieldKind::Text;

    fn get(stored: String) -> Result<T, CastError> {
        serde_json::from_value(serde_json::Value::String(stored.clone())).map_err(|_| {
            CastError::new(format!(
                "`{stored}` is not a variant of {}",
                short_type_name::<T>()
            ))
        })
    }

    fn set(value: &T) -> Result<String, CastError> {
        match serde_json::to_value(value) {
            Ok(serde_json::Value::String(name)) => Ok(name),
            Ok(other) => Err(CastError::new(format!(
                "{} does not serialize as a string (found {other}); use a unit enum",
                short_type_name::<T>()
            ))),
            Err(error) => Err(CastError::new(error.to_string())),
        }
    }
}

/// An enum stored as an integer (`#[model(cast = "int_enum")]`), through
/// `Into<i64>` and `TryFrom<i64>` (a `#[repr(i32)]` enum with those
/// conversions).
///
/// ```
/// use navree_orm::cast::{Cast, IntEnum};
///
/// #[derive(Clone, Copy, Debug, PartialEq)]
/// enum Priority {
///     Low = 1,
///     High = 2,
/// }
///
/// impl From<Priority> for i64 {
///     fn from(p: Priority) -> i64 {
///         p as i64
///     }
/// }
///
/// impl TryFrom<i64> for Priority {
///     type Error = String;
///     fn try_from(n: i64) -> Result<Self, String> {
///         match n {
///             1 => Ok(Priority::Low),
///             2 => Ok(Priority::High),
///             other => Err(format!("no priority {other}")),
///         }
///     }
/// }
///
/// assert_eq!(IntEnum::set(&Priority::High).unwrap(), 2);
/// assert_eq!(<IntEnum as Cast<Priority>>::get(1).unwrap(), Priority::Low);
/// assert!(<IntEnum as Cast<Priority>>::get(9).is_err());
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct IntEnum;

impl<T> Cast<T> for IntEnum
where
    T: Copy + Into<i64> + TryFrom<i64>,
    <T as TryFrom<i64>>::Error: fmt::Display,
{
    type Stored = i64;

    fn get(stored: i64) -> Result<T, CastError> {
        T::try_from(stored).map_err(|error| CastError::new(error.to_string()))
    }

    fn set(value: &T) -> Result<i64, CastError> {
        Ok((*value).into())
    }
}

/// `T`'s name without its module path, for messages.
fn short_type_name<T>() -> &'static str {
    let full = std::any::type_name::<T>();
    let base = full.split('<').next().unwrap_or(full);
    base.rsplit("::").next().unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
    struct Point {
        x: i32,
    }

    #[test]
    fn short_names() {
        assert_eq!(short_type_name::<Point>(), "Point");
        assert_eq!(short_type_name::<Vec<Point>>(), "Vec");
    }

    #[test]
    fn encode_and_decode_through_a_cast() {
        assert_eq!(
            encode::<AsJson, Point>(&Point { x: 3 }).unwrap(),
            Value::Json(Some(Box::new(serde_json::json!({"x": 3}))))
        );
        let row = Row::from_values(vec![(
            "point".into(),
            Value::from(serde_json::json!({"x": 5})),
        )]);
        assert_eq!(
            decode::<AsJson, Point>(&row, "point").unwrap(),
            Point { x: 5 }
        );
        assert!(matches!(
            decode::<AsJson, Point>(&row, "missing"),
            Err(FieldError::Missing)
        ));
    }

    #[test]
    fn string_enums_reject_structs() {
        let error = <StringEnum as Cast<Point>>::set(&Point { x: 1 }).unwrap_err();
        assert!(
            error.to_string().contains("does not serialize as a string"),
            "{error}"
        );
    }
}
