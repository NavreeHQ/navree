//! [`Path`]: the matched route's parameters, deserialized.

use std::any::type_name;
use std::fmt;
use std::ops::{Deref, DerefMut};

use http::StatusCode;
use http::request::Parts;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor,
};
use serde::forward_to_deserialize_any;

use super::rejection::PathRejection;
use super::{FromRequestParts, OptionalFromRequestParts};
use crate::routing::PathParams;

/// The route's path parameters, deserialized into `T`, already
/// percent-decoded (see [`routing`](crate::routing) for what a parameter may
/// hold).
///
/// - one parameter: any `T` that reads from text (`Path<Uuid>`,
///   `Path<String>`, `Path<u64>`, a unit-variant enum);
/// - several: a tuple in route order (`Path<(Uuid, u32)>`) or a struct whose
///   fields are named like the parameters;
/// - all of them: `Path<HashMap<String, String>>` or
///   `Path<Vec<(String, String)>>`.
///
/// A value that does not parse is a 400 (`"Invalid URL: Cannot parse `id`
/// with value `x` to a `u32`"`); a `T` that does not fit the route (two
/// parameters into a `u32`) is a 500, since the route itself is wrong.
///
/// ```
/// use http::Method;
/// use navree_http::{Path, RouteDef};
/// use uuid::Uuid;
///
/// async fn show(Path((deal, note)): Path<(Uuid, u32)>) -> String {
///     format!("{deal} {note}")
/// }
///
/// let route = RouteDef::on(Method::GET, "/deals/{deal}/notes/{note}", show).public();
/// # let _ = route;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Path<T>(pub T);

impl<T> Deref for Path<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Path<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: DeserializeOwned + Send> FromRequestParts for Path<T> {
    type Rejection = PathRejection;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Self::from_params(params_of(parts))
    }
}

impl<T: DeserializeOwned + Send> Path<T> {
    /// Deserializes `T` from the parameters.
    pub fn from_params(params: &PathParams) -> Result<Self, PathRejection> {
        let pairs: Vec<(&str, &str)> = params.iter().collect();
        T::deserialize(PathDeserializer { params: &pairs })
            .map(Path)
            .map_err(|error| PathRejection(error.kind))
    }
}

/// `Option<Path<T>>`: `None` on a route without parameters.
impl<T: DeserializeOwned + Send> OptionalFromRequestParts for Path<T> {
    type Rejection = PathRejection;

    async fn from_request_parts(parts: &mut Parts) -> Result<Option<Self>, Self::Rejection> {
        match Self::from_params(params_of(parts)) {
            Ok(path) => Ok(Some(path)),
            Err(PathRejection(PathErrorKind::WrongNumberOfParameters { got: 0, .. })) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// The request's parameters; none outside a matched route.
fn params_of(parts: &Parts) -> &PathParams {
    static EMPTY: std::sync::OnceLock<PathParams> = std::sync::OnceLock::new();
    parts
        .extensions
        .get::<PathParams>()
        .unwrap_or_else(|| EMPTY.get_or_init(PathParams::empty))
}

/// What went wrong deserializing [`Path`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathErrorKind {
    /// The route has another number of parameters than `T` reads (500).
    WrongNumberOfParameters {
        /// The route's parameters.
        got: usize,
        /// What `T` reads.
        expected: usize,
    },
    /// The value of a named parameter does not parse (400).
    ParseErrorAtKey {
        /// The parameter.
        key: String,
        /// Its value.
        value: String,
        /// The type it should parse as.
        expected_type: &'static str,
    },
    /// The value at a position does not parse (400).
    ParseErrorAtIndex {
        /// The position.
        index: usize,
        /// The value.
        value: String,
        /// The type it should parse as.
        expected_type: &'static str,
    },
    /// The only value does not parse (400).
    ParseError {
        /// The value.
        value: String,
        /// The type it should parse as.
        expected_type: &'static str,
    },
    /// `T` cannot be read from parameters at all, such as a nested map (500).
    UnsupportedType {
        /// The type.
        name: &'static str,
    },
    /// The value of a parameter was refused by `T`'s own deserializer (400).
    DeserializeError {
        /// The parameter.
        key: String,
        /// Its value.
        value: String,
        /// Why.
        message: String,
    },
    /// Anything else `T` reported (400).
    Message(String),
}

impl PathErrorKind {
    /// 400 for a value the client sent, 500 for a `T` that does not fit the
    /// route.
    pub fn status(&self) -> StatusCode {
        match self {
            PathErrorKind::WrongNumberOfParameters { .. }
            | PathErrorKind::UnsupportedType { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl fmt::Display for PathErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathErrorKind::Message(message) => f.write_str(message),
            PathErrorKind::WrongNumberOfParameters { got, expected } => {
                write!(
                    f,
                    "Wrong number of path arguments for `Path`. Expected {expected} but got {got}"
                )?;
                if *expected == 1 {
                    f.write_str(". Note that multiple parameters must be extracted with a tuple `Path<(_, _)>` or a struct `Path<YourParams>`")?;
                }
                Ok(())
            }
            PathErrorKind::UnsupportedType { name } => write!(f, "Unsupported type `{name}`"),
            PathErrorKind::ParseErrorAtKey {
                key,
                value,
                expected_type,
            } => write!(
                f,
                "Cannot parse `{key}` with value `{value}` to a `{expected_type}`"
            ),
            PathErrorKind::ParseError {
                value,
                expected_type,
            } => write!(f, "Cannot parse `{value}` to a `{expected_type}`"),
            PathErrorKind::ParseErrorAtIndex {
                index,
                value,
                expected_type,
            } => write!(
                f,
                "Cannot parse value at index {index} with value `{value}` to a `{expected_type}`"
            ),
            PathErrorKind::DeserializeError {
                key,
                value,
                message,
            } => write!(f, "Cannot parse `{key}` with value `{value}`: {message}"),
        }
    }
}

/// The deserializer's error.
#[derive(Debug)]
struct PathError {
    kind: PathErrorKind,
}

impl PathError {
    fn new(kind: PathErrorKind) -> Self {
        Self { kind }
    }

    fn wrong_number(got: usize, expected: usize) -> Self {
        Self::new(PathErrorKind::WrongNumberOfParameters { got, expected })
    }

    fn unsupported<T>() -> Self {
        Self::new(PathErrorKind::UnsupportedType {
            name: type_name::<T>(),
        })
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl std::error::Error for PathError {}

impl de::Error for PathError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::new(PathErrorKind::Message(message.to_string()))
    }
}

/// Where a value sits: under a name (maps and structs) or at a position
/// (sequences and tuples; the name is kept for pair tuples).
#[derive(Clone, Copy, Debug)]
enum At<'de> {
    Key(&'de str),
    Index { index: usize, key: &'de str },
}

impl<'de> At<'de> {
    fn key(self) -> &'de str {
        match self {
            At::Key(key) | At::Index { key, .. } => key,
        }
    }
}

/// The whole parameter list.
struct PathDeserializer<'a, 'de> {
    params: &'a [(&'de str, &'de str)],
}

impl<'de> PathDeserializer<'_, 'de> {
    /// The only parameter, for a `T` that reads one value.
    fn single(&self) -> Result<(&'de str, &'de str), PathError> {
        match self.params {
            [single] => Ok(*single),
            params => Err(PathError::wrong_number(params.len(), 1)),
        }
    }
}

/// Parses the only value as `$ty`.
macro_rules! single_value {
    ($($method:ident => $visit:ident, $ty:ty, $name:literal;)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
                let (_, value) = self.single()?;
                let parsed = value.parse::<$ty>().map_err(|_| {
                    PathError::new(PathErrorKind::ParseError {
                        value: value.to_owned(),
                        expected_type: $name,
                    })
                })?;
                visitor.$visit(parsed)
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for PathDeserializer<'_, 'de> {
    type Error = PathError;

    single_value! {
        deserialize_bool => visit_bool, bool, "bool";
        deserialize_i8 => visit_i8, i8, "i8";
        deserialize_i16 => visit_i16, i16, "i16";
        deserialize_i32 => visit_i32, i32, "i32";
        deserialize_i64 => visit_i64, i64, "i64";
        deserialize_i128 => visit_i128, i128, "i128";
        deserialize_u8 => visit_u8, u8, "u8";
        deserialize_u16 => visit_u16, u16, "u16";
        deserialize_u32 => visit_u32, u32, "u32";
        deserialize_u64 => visit_u64, u64, "u64";
        deserialize_u128 => visit_u128, u128, "u128";
        deserialize_f32 => visit_f32, f32, "f32";
        deserialize_f64 => visit_f64, f64, "f64";
        deserialize_string => visit_string, String, "String";
        deserialize_byte_buf => visit_string, String, "String";
        deserialize_char => visit_char, char, "char";
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        let (key, value) = self.single()?;
        visitor
            .visit_borrowed_str(value)
            .map_err(|error| keyed(error, At::Key(key), value))
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_option<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_seq(Sequence {
            params: self.params,
            index: 0,
        })
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        if self.params.len() != len {
            return Err(PathError::wrong_number(self.params.len(), len));
        }
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_map(Map {
            params: self.params,
            value: None,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, PathError> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, PathError> {
        let (_, value) = self.single()?;
        visitor.visit_enum(Variant { name: value })
    }
}

/// A `Message` from `T` on the value at `at` becomes a `DeserializeError`
/// naming the parameter.
fn keyed(error: PathError, at: At<'_>, value: &str) -> PathError {
    match error.kind {
        PathErrorKind::Message(message) => PathError::new(PathErrorKind::DeserializeError {
            key: at.key().to_owned(),
            value: value.to_owned(),
            message,
        }),
        kind => PathError::new(kind),
    }
}

/// The parameters as a map.
struct Map<'a, 'de> {
    params: &'a [(&'de str, &'de str)],
    value: Option<(&'de str, &'de str)>,
}

impl<'de> MapAccess<'de> for Map<'_, 'de> {
    type Error = PathError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, PathError> {
        let Some((first, rest)) = self.params.split_first() else {
            return Ok(None);
        };
        self.params = rest;
        self.value = Some(*first);
        seed.deserialize(Key(first.0)).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, PathError> {
        let (key, value) = self
            .value
            .take()
            .ok_or_else(|| PathError::custom("value is missing"))?;
        seed.deserialize(Value {
            at: Some(At::Key(key)),
            value,
        })
    }
}

/// The parameters as a sequence.
struct Sequence<'a, 'de> {
    params: &'a [(&'de str, &'de str)],
    index: usize,
}

impl<'de> SeqAccess<'de> for Sequence<'_, 'de> {
    type Error = PathError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, PathError> {
        let Some(((key, value), rest)) = self.params.split_first() else {
            return Ok(None);
        };
        self.params = rest;
        let index = self.index;
        self.index += 1;
        seed.deserialize(Value {
            at: Some(At::Index { index, key }),
            value,
        })
        .map(Some)
    }
}

/// A parameter name.
struct Key<'de>(&'de str);

impl<'de> de::Deserializer<'de> for Key<'de> {
    type Error = PathError;

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_borrowed_str(self.0)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_borrowed_str(self.0)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_borrowed_str(self.0)
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(PathError::custom("Unexpected key type"))
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char bytes
        byte_buf option unit unit_struct seq tuple
        tuple_struct map newtype_struct struct enum ignored_any
    }
}

/// One parameter value.
struct Value<'de> {
    at: Option<At<'de>>,
    value: &'de str,
}

/// Parses the value as `$ty`; the error names where it sits.
macro_rules! parse_value {
    ($($method:ident => $visit:ident, $ty:ty, $name:literal;)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
                let parsed = self.value.parse::<$ty>().map_err(|_| self.parse_error($name))?;
                visitor.$visit(parsed)
            }
        )*
    };
}

impl Value<'_> {
    fn parse_error(&self, expected_type: &'static str) -> PathError {
        let value = self.value.to_owned();
        PathError::new(match self.at {
            Some(At::Key(key)) => PathErrorKind::ParseErrorAtKey {
                key: key.to_owned(),
                value,
                expected_type,
            },
            Some(At::Index { index, .. }) => PathErrorKind::ParseErrorAtIndex {
                index,
                value,
                expected_type,
            },
            None => PathErrorKind::ParseError {
                value,
                expected_type,
            },
        })
    }
}

impl<'de> de::Deserializer<'de> for Value<'de> {
    type Error = PathError;

    parse_value! {
        deserialize_bool => visit_bool, bool, "bool";
        deserialize_i8 => visit_i8, i8, "i8";
        deserialize_i16 => visit_i16, i16, "i16";
        deserialize_i32 => visit_i32, i32, "i32";
        deserialize_i64 => visit_i64, i64, "i64";
        deserialize_i128 => visit_i128, i128, "i128";
        deserialize_u8 => visit_u8, u8, "u8";
        deserialize_u16 => visit_u16, u16, "u16";
        deserialize_u32 => visit_u32, u32, "u32";
        deserialize_u64 => visit_u64, u64, "u64";
        deserialize_u128 => visit_u128, u128, "u128";
        deserialize_f32 => visit_f32, f32, "f32";
        deserialize_f64 => visit_f64, f64, "f64";
        deserialize_string => visit_string, String, "String";
        deserialize_byte_buf => visit_string, String, "String";
        deserialize_char => visit_char, char, "char";
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        let (at, value) = (self.at, self.value);
        visitor.visit_borrowed_str(value).map_err(|error| match at {
            Some(at) => keyed(error, at, value),
            None => error,
        })
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_borrowed_bytes(self.value.as_bytes())
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        visitor.visit_newtype_struct(self)
    }

    /// `(String, String)` pairs in `Path<Vec<(String, String)>>`: the name,
    /// then the value.
    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        match (len, self.at) {
            (2, Some(At::Index { key, .. })) => visitor.visit_seq(Pair {
                key: Some(key),
                value: Some(self.value),
            }),
            (2, Some(At::Key(_))) => Err(PathError::custom("array types are not supported")),
            _ => Err(PathError::unsupported::<V::Value>()),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_map<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(PathError::unsupported::<V::Value>())
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, PathError> {
        visitor.visit_enum(Variant { name: self.value })
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_unit()
    }
}

/// A `(name, value)` pair.
struct Pair<'de> {
    key: Option<&'de str>,
    value: Option<&'de str>,
}

impl<'de> SeqAccess<'de> for Pair<'de> {
    type Error = PathError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, PathError> {
        if let Some(key) = self.key.take() {
            return seed.deserialize(Key(key)).map(Some);
        }
        self.value
            .take()
            .map(|value| seed.deserialize(Value { at: None, value }))
            .transpose()
    }
}

/// A unit variant named by a value.
struct Variant<'de> {
    name: &'de str,
}

impl<'de> de::EnumAccess<'de> for Variant<'de> {
    type Error = PathError;
    type Variant = UnitOnly;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, UnitOnly), PathError> {
        Ok((seed.deserialize(Key(self.name))?, UnitOnly))
    }
}

struct UnitOnly;

impl<'de> de::VariantAccess<'de> for UnitOnly {
    type Error = PathError;

    fn unit_variant(self) -> Result<(), PathError> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        _seed: T,
    ) -> Result<T::Value, PathError> {
        Err(PathError::new(PathErrorKind::UnsupportedType {
            name: "newtype enum variant",
        }))
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, PathError> {
        Err(PathError::new(PathErrorKind::UnsupportedType {
            name: "tuple enum variant",
        }))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, PathError> {
        Err(PathError::new(PathErrorKind::UnsupportedType {
            name: "struct enum variant",
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use serde::Deserialize;

    use super::*;

    fn params(pairs: &[(&str, &str)]) -> PathParams {
        let names: Arc<[Box<str>]> = pairs.iter().map(|(name, _)| (*name).into()).collect();
        let values: Vec<&str> = pairs.iter().map(|(_, value)| *value).collect();
        PathParams::decode(names, &values, false).expect("valid")
    }

    fn path<T: DeserializeOwned + Send>(pairs: &[(&str, &str)]) -> Result<T, PathErrorKind> {
        Path::<T>::from_params(&params(pairs))
            .map(|Path(value)| value)
            .map_err(|rejection| rejection.0)
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Ids {
        user: u32,
        team: String,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum Tab {
        Notes,
        Files,
    }

    #[test]
    fn one_value_reads_into_any_text_type() {
        assert_eq!(path::<u32>(&[("id", "42")]), Ok(42));
        assert_eq!(
            path::<String>(&[("id", "caf%C3%A9")]),
            Ok("café".to_owned())
        );
        assert_eq!(path::<Tab>(&[("tab", "notes")]), Ok(Tab::Notes));
        assert_eq!(
            path::<uuid::Uuid>(&[("id", "67e55044-10b1-426f-9247-bb680e5fe0c8")])
                .map(|id| id.to_string()),
            Ok("67e55044-10b1-426f-9247-bb680e5fe0c8".to_owned())
        );
    }

    #[test]
    fn several_values_read_into_tuples_structs_and_maps() {
        let pairs = [("user", "7"), ("team", "core")];
        assert_eq!(path::<(u32, String)>(&pairs), Ok((7, "core".to_owned())));
        assert_eq!(
            path::<Ids>(&pairs),
            Ok(Ids {
                user: 7,
                team: "core".to_owned()
            })
        );
        let map = path::<HashMap<String, String>>(&pairs).unwrap();
        assert_eq!(map["team"], "core");
        assert_eq!(
            path::<Vec<(String, String)>>(&pairs).unwrap(),
            [
                ("user".to_owned(), "7".to_owned()),
                ("team".to_owned(), "core".to_owned())
            ]
        );
        assert_eq!(path::<()>(&[]), Ok(()));
    }

    #[test]
    fn rejections_read_as_before() {
        let render = |kind: PathErrorKind| (kind.status().as_u16(), kind.to_string());
        assert_eq!(
            render(path::<u32>(&[("id", "x")]).unwrap_err()),
            (400, "Cannot parse `x` to a `u32`".to_owned())
        );
        assert_eq!(
            render(path::<Ids>(&[("user", "x"), ("team", "a")]).unwrap_err()),
            (
                400,
                "Cannot parse `user` with value `x` to a `u32`".to_owned()
            )
        );
        assert_eq!(
            render(path::<(String, u8)>(&[("a", "x"), ("b", "300")]).unwrap_err()),
            (
                400,
                "Cannot parse value at index 1 with value `300` to a `u8`".to_owned()
            )
        );
        let uuid = path::<uuid::Uuid>(&[("id", "not-a-uuid")]).unwrap_err();
        assert_eq!(uuid.status(), StatusCode::BAD_REQUEST);
        assert!(
            uuid.to_string()
                .starts_with("Cannot parse `id` with value `not-a-uuid`: UUID parsing failed"),
            "{uuid}"
        );
        assert_eq!(
            render(path::<u32>(&[("a", "1"), ("b", "2")]).unwrap_err()),
            (500, "Wrong number of path arguments for `Path`. Expected 1 but got 2. Note that multiple parameters must be extracted with a tuple `Path<(_, _)>` or a struct `Path<YourParams>`".to_owned())
        );
        assert_eq!(
            render(path::<(u32, u32, u32)>(&[("a", "1"), ("b", "2")]).unwrap_err()),
            (
                500,
                "Wrong number of path arguments for `Path`. Expected 3 but got 2".to_owned()
            )
        );
        assert_eq!(
            path::<Tab>(&[("tab", "other")]).unwrap_err().to_string(),
            "unknown variant `other`, expected `notes` or `files`"
        );
        let nested = path::<HashMap<String, Vec<u8>>>(&[("a", "1")]).unwrap_err();
        assert_eq!(nested.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
