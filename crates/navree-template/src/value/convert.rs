//! Conversions between [`Value`] and Rust types.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::{Repr, Value, ValueMap, cannot_convert};
use crate::error::Error;

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value(Repr::Bool(value))
    }
}

macro_rules! from_int {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for Value {
                fn from(value: $ty) -> Value {
                    Value(Repr::Int(i128::from(value)))
                }
            }
        )*
    };
}

from_int!(i8, i16, i32, i64, i128, u8, u16, u32, u64);

impl From<usize> for Value {
    fn from(value: usize) -> Value {
        // usize is at most 64 bits on every supported target.
        Value(Repr::Int(i128::try_from(value).unwrap_or(i128::MAX)))
    }
}

impl From<isize> for Value {
    fn from(value: isize) -> Value {
        Value(Repr::Int(i128::try_from(value).unwrap_or(i128::MAX)))
    }
}

impl From<u128> for Value {
    fn from(value: u128) -> Value {
        match i128::try_from(value) {
            Ok(value) => Value(Repr::Int(value)),
            #[allow(clippy::cast_precision_loss)]
            Err(_) => Value(Repr::Float(value as f64)),
        }
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Value {
        Value(Repr::Float(f64::from(value)))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Value {
        Value(Repr::Float(value))
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value(Repr::Str(Arc::from(value), false))
    }
}

impl From<&String> for Value {
    fn from(value: &String) -> Value {
        Value::from(value.as_str())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Value {
        Value(Repr::Str(Arc::from(value), false))
    }
}

impl From<Cow<'_, str>> for Value {
    fn from(value: Cow<'_, str>) -> Value {
        Value::from(value.as_ref())
    }
}

impl From<Arc<str>> for Value {
    fn from(value: Arc<str>) -> Value {
        Value(Repr::Str(value, false))
    }
}

impl From<char> for Value {
    fn from(value: char) -> Value {
        let mut buffer = [0; 4];
        Value::from(&*value.encode_utf8(&mut buffer))
    }
}

impl From<()> for Value {
    fn from((): ()) -> Value {
        Value::NONE
    }
}

impl From<&Value> for Value {
    fn from(value: &Value) -> Value {
        value.clone()
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Value {
        value.map_or(Value::NONE, Into::into)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(values: Vec<T>) -> Value {
        Value::list(values.into_iter().map(Into::into).collect())
    }
}

impl<K: Into<Value>, V: Into<Value>> From<BTreeMap<K, V>> for Value {
    fn from(map: BTreeMap<K, V>) -> Value {
        Value::map(map.into_iter().map(|(k, v)| (k.into(), v.into())).collect())
    }
}

impl<K: Into<Value>, V: Into<Value>, S> From<HashMap<K, V, S>> for Value {
    fn from(map: HashMap<K, V, S>) -> Value {
        Value::map(map.into_iter().map(|(k, v)| (k.into(), v.into())).collect())
    }
}

impl<V: Into<Value>> FromIterator<V> for Value {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Value {
        Value::list(iter.into_iter().map(Into::into).collect())
    }
}

impl<K: Into<Value>, V: Into<Value>> FromIterator<(K, V)> for super::Kwargs {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> super::Kwargs {
        let map: ValueMap = iter
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        super::Kwargs::new(Arc::new(map))
    }
}

macro_rules! try_from_int {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<Value> for $ty {
                type Error = Error;

                fn try_from(value: Value) -> Result<$ty, Error> {
                    let integer = match value.0 {
                        Repr::Bool(value) => Some(i128::from(value)),
                        Repr::Int(value) => Some(value),
                        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                        Repr::Float(float) if float as i64 as f64 == float => Some(i128::from(float as i64)),
                        _ => None,
                    };
                    integer
                        .and_then(|integer| <$ty>::try_from(integer).ok())
                        .ok_or_else(|| cannot_convert(&value, stringify!($ty)))
                }
            }
        )*
    };
}

try_from_int!(
    i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, usize, isize
);

impl TryFrom<Value> for f64 {
    type Error = Error;

    fn try_from(value: Value) -> Result<f64, Error> {
        match value.0 {
            #[allow(clippy::cast_precision_loss)]
            Repr::Int(integer) => Ok(integer as f64),
            Repr::Float(float) => Ok(float),
            _ => Err(cannot_convert(&value, "f64")),
        }
    }
}

impl TryFrom<Value> for f32 {
    type Error = Error;

    fn try_from(value: Value) -> Result<f32, Error> {
        #[allow(clippy::cast_possible_truncation)]
        f64::try_from(value).map(|float| float as f32)
    }
}

impl TryFrom<Value> for bool {
    type Error = Error;

    fn try_from(value: Value) -> Result<bool, Error> {
        match value.0 {
            Repr::Bool(value) => Ok(value),
            _ => Err(cannot_convert(&value, "bool")),
        }
    }
}

impl TryFrom<Value> for String {
    type Error = Error;

    fn try_from(value: Value) -> Result<String, Error> {
        Ok(value.to_string())
    }
}
