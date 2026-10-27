//! [`Cast`]: converting a field between its Rust type and its stored
//! column value, for types that are not [`Field`]s themselves.
//!
//! A field opts in with `#[model(cast = ..)]`:
//!
//! | Attribute | Cast | Field type | Stored as |
//! |---|---|---|---|
//! | `cast = "json"` | [`AsJson`] | any `Serialize + DeserializeOwned` type (structs, `Vec`, maps) | JSON |
//! | `cast = "string_enum"` | [`StringEnum`] | a unit enum, by its serde name | text |
//! | `cast = "int_enum"` | [`IntEnum`] | `Copy + Into<i64> + TryFrom<i64>` (a `#[repr]` enum) | an integer |
//! | `cast = path::To::Cast` | yours | `T` where the cast is `Cast<T>` | `Cast::Stored` |
//!
//! A cast on an `Option<T>` field casts the inner value; NULL stays `None`.
//! Typed columns of cast fields encode condition values with the cast, so
//! `Deal::STAGE.eq(Stage::Won)` compares with the stored `'won'`.
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

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::row::Row;
use crate::value::{Field, FieldError, Value};

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

/// Converts fields of type `T` to and from their stored form.
pub trait Cast<T>: 'static {
    /// The stored form (a [`Field`]: text, an integer, JSON, ...).
    type Stored: Field;

    /// The stored form to the field (reading from the database).
    fn get(stored: Self::Stored) -> Result<T, CastError>;

    /// The field to its stored form (writing to the database, comparing in
    /// conditions).
    fn set(value: &T) -> Result<Self::Stored, CastError>;
}

/// Encodes a field through cast `C` (the encoder of a typed column).
pub fn encode<C: Cast<T>, T>(value: &T) -> Result<Value, String> {
    C::set(value)
        .map(|stored| stored.to_value())
        .map_err(|error| error.to_string())
}

/// Decodes a field through cast `C` (the decoder of a typed column).
pub fn decode<C: Cast<T>, T>(row: &Row, column: &str) -> Result<T, FieldError> {
    let stored = row.field::<C::Stored>(column)?;
    C::get(stored).map_err(|error| FieldError::Invalid(error.to_string()))
}

/// Encodes a nullable field through cast `C` (`None` is a typed NULL).
pub fn encode_option<C: Cast<T>, T>(value: &Option<T>) -> Result<Value, String> {
    match value {
        Some(value) => encode::<C, T>(value),
        None => Ok(<C::Stored as Field>::null()),
    }
}

/// Decodes a nullable field through cast `C` (NULL is `None`).
pub fn decode_option<C: Cast<T>, T>(row: &Row, column: &str) -> Result<Option<T>, FieldError> {
    match row.field::<Option<C::Stored>>(column)? {
        Some(stored) => C::get(stored)
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
