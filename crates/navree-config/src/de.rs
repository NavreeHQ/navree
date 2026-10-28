//! Reads typed sections out of the tree: a serde [`Deserializer`] for
//! [`Value`] that converts between scalar types on demand.
//!
//! Environment variables are strings, so `"10"` has to fill a `u32` and
//! `"true"` a `bool`. The rules, which apply to every source:
//!
//! - integers: an integer, an integral float, a string in Rust's integer
//!   syntax (`"+80"`, not `" 80"` or `"8.0"`), a boolean or one of
//!   `yes`/`on`/`no`/`off` (as 1 or 0);
//! - booleans: a boolean, a number (zero is `false`) or `true`/`false`,
//!   `yes`/`no`, `on`/`off`, `1`/`0` in any case;
//! - floats: a number, a boolean or a string in Rust's float syntax;
//! - strings: any scalar, formatted;
//! - lists and tables never come from strings.
//!
//! Errors carry the dotted key below the section (`servers[1].port`) and the
//! source of the value; they never carry a string value.

use std::collections::btree_map;
use std::fmt;

use serde::de::value::StrDeserializer;
use serde::de::{self, DeserializeSeed, Visitor};

use crate::redact::REDACTED;
use crate::value::{Kind, Origin, Table, Value};

/// What a value was when a conversion failed. Strings are not kept: the
/// message says `string [redacted]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Unexpected {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str,
    Unit,
    Seq,
    Map,
}

impl Unexpected {
    fn describe(self, hide_numbers: bool) -> String {
        let number = |kind: &str, text: String| {
            if hide_numbers {
                format!("{kind} {REDACTED}")
            } else {
                format!("{kind} `{text}`")
            }
        };
        match self {
            Unexpected::Bool(b) => number("boolean", b.to_string()),
            Unexpected::Int(i) => number("64-bit integer", i.to_string()),
            Unexpected::UInt(u) => number("64-bit unsigned integer", u.to_string()),
            Unexpected::Float(f) => number("floating point", f.to_string()),
            Unexpected::Str => format!("string {REDACTED}"),
            Unexpected::Unit => "unit value".to_owned(),
            Unexpected::Seq => "sequence".to_owned(),
            Unexpected::Map => "map".to_owned(),
        }
    }
}

#[derive(Debug)]
enum Failure {
    /// A value of the wrong type for the field.
    Type {
        unexpected: Unexpected,
        expected: &'static str,
    },
    /// A required field is absent; the path below the section.
    Missing(String),
    /// Anything a `Deserialize` implementation reports.
    Message(String),
}

/// A value does not fit the type that asked for it.
pub(crate) struct DeError {
    failure: Failure,
    /// The dotted key below the section (`servers[1].port`), when known.
    key: Option<String>,
    /// The source of the offending value, for type errors.
    origin: Option<Origin>,
}

impl DeError {
    fn ty(origin: Option<Origin>, unexpected: Unexpected, expected: &'static str) -> Self {
        DeError {
            failure: Failure::Type {
                unexpected,
                expected,
            },
            key: None,
            origin,
        }
    }

    fn message(message: impl Into<String>) -> Self {
        DeError {
            failure: Failure::Message(message.into()),
            key: None,
            origin: None,
        }
    }

    /// The dotted key below the section, when the error is about one.
    pub(crate) fn key(&self) -> Option<&str> {
        match &self.failure {
            Failure::Missing(path) => Some(path),
            _ => self.key.as_deref(),
        }
    }

    /// Puts `segment` (`servers`, `[1]`) in front of the key.
    fn prepend(mut self, segment: &str) -> Self {
        let join = |rest: &str| {
            if rest.is_empty() || rest.starts_with('[') {
                format!("{segment}{rest}")
            } else {
                format!("{segment}.{rest}")
            }
        };
        match &mut self.failure {
            Failure::Missing(path) => *path = join(path),
            _ => self.key = Some(join(self.key.as_deref().unwrap_or(""))),
        }
        self
    }

    /// The message. With `hide_numbers`, numbers and booleans in type errors
    /// are masked too (the key looks secret).
    pub(crate) fn render(&self, hide_numbers: bool) -> String {
        let mut out = match &self.failure {
            Failure::Type {
                unexpected,
                expected,
            } => format!(
                "invalid type: {}, expected {expected}",
                unexpected.describe(hide_numbers)
            ),
            Failure::Missing(path) => {
                return format!("missing configuration field {path:?}");
            }
            Failure::Message(message) => message.clone(),
        };
        if let Some(key) = &self.key {
            out.push_str(&format!(" for key `{key}`"));
        }
        if let Some(origin) = &self.origin {
            out.push_str(&format!(" in {origin}"));
        }
        out
    }
}

impl fmt::Display for DeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(false))
    }
}

/// Prints the message with numbers masked: a stray `{:?}` must not leak.
impl fmt::Debug for DeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(true))
    }
}

impl std::error::Error for DeError {}

impl de::Error for DeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        DeError::message(message.to_string())
    }

    fn missing_field(field: &'static str) -> Self {
        DeError {
            failure: Failure::Missing(field.to_owned()),
            key: None,
            origin: None,
        }
    }
}

type Result<T> = std::result::Result<T, DeError>;

impl Value {
    fn type_error(&self, unexpected: Unexpected, expected: &'static str) -> DeError {
        DeError::ty(self.origin.clone(), unexpected, expected)
    }

    /// What this value is, for an error about it.
    fn unexpected(&self) -> Unexpected {
        match &self.kind {
            Kind::Null => Unexpected::Unit,
            Kind::Bool(b) => Unexpected::Bool(*b),
            Kind::Int(i) => Unexpected::Int(*i),
            Kind::UInt(u) => Unexpected::UInt(*u),
            Kind::Float(f) => Unexpected::Float(*f),
            Kind::String(_) => Unexpected::Str,
            Kind::Array(_) => Unexpected::Seq,
            Kind::Table(_) => Unexpected::Map,
        }
    }

    /// A boolean: `true`/`false`, `yes`/`no`, `on`/`off`, `1`/`0` in any case,
    /// or a number (zero is `false`).
    pub(crate) fn to_bool(&self) -> Result<bool> {
        match &self.kind {
            Kind::Bool(b) => Ok(*b),
            Kind::Int(i) => Ok(*i != 0),
            Kind::UInt(u) => Ok(*u != 0),
            Kind::Float(f) => Ok(*f != 0.0),
            Kind::String(s) => match s.to_lowercase().as_str() {
                "1" | "true" | "on" | "yes" => Ok(true),
                "0" | "false" | "off" | "no" => Ok(false),
                _ => Err(self.type_error(Unexpected::Str, "a boolean")),
            },
            _ => Err(self.type_error(self.unexpected(), "a boolean")),
        }
    }

    /// A signed integer, wide enough for every Rust integer type.
    fn to_i128(&self) -> Result<i128> {
        match &self.kind {
            Kind::Int(i) => Ok(i128::from(*i)),
            Kind::UInt(u) => Ok(i128::from(*u)),
            Kind::Bool(b) => Ok(i128::from(*b)),
            Kind::Float(f) => {
                integral(*f).ok_or_else(|| self.type_error(self.unexpected(), "an integer"))
            }
            Kind::String(s) => match s.to_lowercase().as_str() {
                "true" | "on" | "yes" => Ok(1),
                "false" | "off" | "no" => Ok(0),
                _ => s
                    .parse::<i128>()
                    .map_err(|_| self.type_error(Unexpected::Str, "an integer")),
            },
            _ => Err(self.type_error(self.unexpected(), "an integer")),
        }
    }

    /// An unsigned integer up to `u128::MAX`.
    fn to_u128(&self) -> Result<u128> {
        if let Kind::String(s) = &self.kind
            && !matches!(
                s.to_lowercase().as_str(),
                "true" | "on" | "yes" | "false" | "off" | "no"
            )
        {
            return s
                .parse::<u128>()
                .map_err(|_| self.type_error(Unexpected::Str, "an integer"));
        }
        u128::try_from(self.to_i128()?)
            .map_err(|_| self.type_error(self.unexpected(), "an unsigned 128 bit or less integer"))
    }

    fn to_i64(&self) -> Result<i64> {
        i64::try_from(self.to_i128()?)
            .map_err(|_| self.type_error(self.unexpected(), "a signed 64 bit or less integer"))
    }

    fn to_u64(&self) -> Result<u64> {
        u64::try_from(self.to_i128()?)
            .map_err(|_| self.type_error(self.unexpected(), "an unsigned 64 bit or less integer"))
    }

    fn to_f64(&self) -> Result<f64> {
        match &self.kind {
            Kind::Float(f) => Ok(*f),
            Kind::Int(i) => Ok(*i as f64),
            Kind::UInt(u) => Ok(*u as f64),
            Kind::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Kind::String(s) => match s.to_lowercase().as_str() {
                "true" | "on" | "yes" => Ok(1.0),
                "false" | "off" | "no" => Ok(0.0),
                _ => s
                    .parse::<f64>()
                    .map_err(|_| self.type_error(Unexpected::Str, "a floating point")),
            },
            _ => Err(self.type_error(self.unexpected(), "a floating point")),
        }
    }

    fn into_string(self) -> Result<String> {
        match self.kind {
            Kind::String(s) => Ok(s),
            Kind::Bool(b) => Ok(b.to_string()),
            Kind::Int(i) => Ok(i.to_string()),
            Kind::UInt(u) => Ok(u.to_string()),
            Kind::Float(f) => Ok(f.to_string()),
            Kind::Null | Kind::Array(_) | Kind::Table(_) => {
                Err(self.type_error(self.unexpected(), "a string"))
            }
        }
    }
}

/// `f` as an integer when it is one exactly (`2.0`, not `2.5`, `NaN` or `1e30`).
fn integral(f: f64) -> Option<i128> {
    // Every f64 below 2^127 in magnitude converts exactly.
    (f.is_finite() && f.fract() == 0.0 && f.abs() < 2f64.powi(127)).then_some(f as i128)
}

/// `value` as a `$ty`, through `$to` (`to_i64` or `to_u64`).
macro_rules! narrow {
    ($value:expr, $to:ident, $ty:ty, $expected:literal) => {{
        let wide = $value.$to()?;
        <$ty>::try_from(wide).map_err(|_| $value.type_error($value.unexpected(), $expected))?
    }};
}

impl<'de> de::Deserializer<'de> for Value {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self.kind {
            Kind::Null => visitor.visit_unit(),
            Kind::Bool(b) => visitor.visit_bool(b),
            Kind::Int(i) => visitor.visit_i64(i),
            Kind::UInt(u) => visitor.visit_u64(u),
            Kind::Float(f) => visitor.visit_f64(f),
            Kind::String(s) => visitor.visit_string(s),
            Kind::Array(items) => visitor.visit_seq(Seq::new(items)),
            Kind::Table(map) => visitor.visit_map(Map::new(map)),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_bool(self.to_bool()?)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i8(narrow!(self, to_i64, i8, "a signed 8 bit integer"))
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i16(narrow!(self, to_i64, i16, "a signed 16 bit integer"))
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i32(narrow!(self, to_i64, i32, "a signed 32 bit integer"))
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i64(self.to_i64()?)
    }

    fn deserialize_i128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i128(self.to_i128()?)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u8(narrow!(self, to_u64, u8, "an unsigned 8 bit integer"))
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u16(narrow!(self, to_u64, u16, "an unsigned 16 bit integer"))
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u32(narrow!(self, to_u64, u32, "an unsigned 32 bit integer"))
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u64(self.to_u64()?)
    }

    fn deserialize_u128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u128(self.to_u128()?)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_f32(self.to_f64()? as f32)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_f64(self.to_f64()?)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_string(self.into_string()?)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_string(self.into_string()?)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        // An explicit null is `None`, everything else is `Some`.
        match self.kind {
            Kind::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_enum(Enum {
            value: self,
            name,
            variants,
        })
    }

    serde::forward_to_deserialize_any! {
        char seq bytes byte_buf map struct unit identifier ignored_any
        unit_struct tuple_struct tuple
    }
}

struct Seq {
    items: std::iter::Enumerate<std::vec::IntoIter<Value>>,
}

impl Seq {
    fn new(items: Vec<Value>) -> Self {
        Seq {
            items: items.into_iter().enumerate(),
        }
    }
}

impl<'de> de::SeqAccess<'de> for Seq {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        match self.items.next() {
            Some((index, value)) => seed
                .deserialize(value)
                .map(Some)
                .map_err(|e| e.prepend(&format!("[{index}]"))),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }
}

struct Map {
    entries: btree_map::IntoIter<String, Value>,
    /// The entry whose key was handed out and whose value is next.
    pending: Option<(String, Value)>,
}

impl Map {
    fn new(table: Table) -> Self {
        Map {
            entries: table.into_iter(),
            pending: None,
        }
    }
}

impl<'de> de::MapAccess<'de> for Map {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        // Keys are strings that convert like any other: `HashMap<u16, _>` works.
        let handed_out = seed.deserialize(Value::key(key.clone()))?;
        self.pending = Some((key, value));
        Ok(Some(handed_out))
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
        let Some((key, value)) = self.pending.take() else {
            return Err(DeError::message("a map value was requested before its key"));
        };
        seed.deserialize(value).map_err(|e| e.prepend(&key))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

/// An enum is a string naming a unit variant, or a table with exactly one
/// key naming the variant and holding its content.
struct Enum {
    value: Value,
    name: &'static str,
    variants: &'static [&'static str],
}

fn structural_error(name: &str) -> DeError {
    DeError::message(format!(
        "value of enum {name} should be represented by either string or table with exactly one key"
    ))
}

impl<'de> de::EnumAccess<'de> for Enum {
    type Error = DeError;
    type Variant = Content;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Content)> {
        let (name, variants) = (self.name, self.variants);
        let (variant, content) = match self.value.kind {
            Kind::String(text) => match variants.iter().find(|v| **v == text) {
                Some(variant) => (*variant, None),
                // The text is a value, which may be a secret: do not echo it.
                None => {
                    return Err(DeError::message(format!(
                        "enum {name} has no variant matching this value, expected one of {}",
                        quoted(variants)
                    )));
                }
            },
            Kind::Table(mut table) if table.len() == 1 => {
                let Some((key, content)) = table.pop_first() else {
                    return Err(structural_error(name));
                };
                match variants.iter().find(|v| **v == key) {
                    Some(variant) => (*variant, Some(content)),
                    // A key is a name, not a value.
                    None => {
                        return Err(DeError::message(format!(
                            "enum {name} does not have variant constructor {key}"
                        )));
                    }
                }
            }
            _ => return Err(structural_error(name)),
        };
        let variant = seed.deserialize(StrDeserializer::<DeError>::new(variant))?;
        Ok((variant, Content(content)))
    }
}

/// The content of an enum variant: `None` when the enum was a bare string.
struct Content(Option<Value>);

impl Content {
    fn require(self, expected: &'static str) -> Result<Value> {
        self.0
            .ok_or_else(|| de::Error::invalid_type(de::Unexpected::UnitVariant, &expected))
    }
}

impl<'de> de::VariantAccess<'de> for Content {
    type Error = DeError;

    fn unit_variant(self) -> Result<()> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value> {
        seed.deserialize(self.require("newtype variant")?)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        de::Deserializer::deserialize_seq(self.require("tuple variant")?, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        de::Deserializer::deserialize_map(self.require("struct variant")?, visitor)
    }
}

/// `` `a`, `b` ``
fn quoted(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Deserializes `value` as `T`, giving errors the key path below it.
pub(crate) fn deserialize<T: de::DeserializeOwned>(value: &Value) -> Result<T> {
    T::deserialize(value.clone())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde::Deserialize;
    use serde_json::json;

    use super::*;
    use crate::value::from_json;

    fn tree(json: serde_json::Value) -> Value {
        from_json(&json, &Origin::from("test"), 1, 64).expect("shallow")
    }

    fn read<T: de::DeserializeOwned>(json: serde_json::Value) -> std::result::Result<T, String> {
        deserialize(&tree(json)).map_err(|e| e.render(false))
    }

    #[test]
    fn integral_floats_fill_integers_and_other_floats_do_not() {
        #[derive(Debug, Deserialize)]
        struct N {
            n: i32,
        }
        assert_eq!(read::<N>(json!({ "n": 2.0 })).unwrap().n, 2);
        assert_eq!(read::<N>(json!({ "n": -3.0 })).unwrap().n, -3);
        for bad in [json!(2.5), json!(1e30), json!(f64::NAN.to_string())] {
            assert!(read::<N>(json!({ "n": bad })).is_err());
        }
        assert_eq!(
            read::<N>(json!({ "n": 2.5 })).unwrap_err(),
            "invalid type: floating point `2.5`, expected an integer for key `n` in test"
        );
    }

    #[test]
    fn wide_integers_are_supported() {
        #[derive(Debug, Deserialize)]
        struct W {
            a: i128,
            b: u128,
        }
        let w =
            read::<W>(json!({ "a": "-170141183460469231731687303715884105728", "b": u64::MAX }))
                .unwrap();
        assert_eq!((w.a, w.b), (i128::MIN, u128::from(u64::MAX)));
        assert!(read::<W>(json!({ "a": 1, "b": -1 })).is_err());
    }

    #[test]
    fn range_errors_name_the_type_the_value_and_the_source() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct P {
            port: u16,
            small: i8,
        }
        assert_eq!(
            read::<P>(json!({ "port": 70000, "small": 0 })).unwrap_err(),
            "invalid type: 64-bit integer `70000`, expected an unsigned 16 bit integer for key `port` in test"
        );
        assert_eq!(
            read::<P>(json!({ "port": 1, "small": 200 })).unwrap_err(),
            "invalid type: 64-bit integer `200`, expected a signed 8 bit integer for key `small` in test"
        );
        assert_eq!(
            read::<P>(json!({ "port": -5, "small": 0 })).unwrap_err(),
            "invalid type: 64-bit integer `-5`, expected an unsigned 64 bit or less integer for key `port` in test"
        );
    }

    #[test]
    fn keys_inside_lists_have_a_dot_before_the_field() {
        #[derive(Debug, Deserialize)]
        struct Server {
            #[allow(dead_code)]
            port: u16,
        }
        #[derive(Debug, Deserialize)]
        struct Root {
            #[allow(dead_code)]
            servers: Vec<Server>,
        }
        assert_eq!(
            read::<Root>(json!({ "servers": [{ "port": 1 }, { "port": "x" }] })).unwrap_err(),
            "invalid type: string [redacted], expected an integer for key `servers[1].port` in test"
        );
        assert_eq!(
            read::<Root>(json!({ "servers": [{ "port": 1 }, {}] })).unwrap_err(),
            "missing configuration field \"servers[1].port\""
        );
        let grid: std::result::Result<Vec<Vec<u8>>, String> =
            deserialize::<Vec<Vec<u8>>>(&tree(json!([[1], [2, "x"]]))).map_err(|e| e.render(false));
        assert_eq!(
            grid.unwrap_err(),
            "invalid type: string [redacted], expected an integer for key `[1][1]` in test"
        );
    }

    #[test]
    fn a_string_for_a_newtype_variant_is_an_error_not_a_panic() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        enum Shape {
            Circle(u32),
            Pair(u32, u32),
            Rect { w: u32 },
            Dot,
        }
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Holder {
            shape: Shape,
        }
        for variant in ["Circle", "Pair", "Rect"] {
            let message = read::<Holder>(json!({ "shape": variant })).unwrap_err();
            assert!(
                message.contains("invalid type: unit variant, expected"),
                "{variant}: {message}"
            );
            assert!(message.ends_with("for key `shape`"), "{variant}: {message}");
        }
        assert!(read::<Holder>(json!({ "shape": "Dot" })).is_ok());
        assert!(read::<Holder>(json!({ "shape": { "Circle": 3 } })).is_ok());
        assert!(read::<Holder>(json!({ "shape": { "Pair": [1, 2] } })).is_ok());
        assert!(read::<Holder>(json!({ "shape": { "Rect": { "w": 1 } } })).is_ok());
    }

    #[test]
    fn an_unknown_variant_value_is_not_echoed_but_an_unknown_key_is() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        enum Level {
            Debug,
            Info,
        }
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Log {
            level: Level,
        }
        let message = read::<Log>(json!({ "level": "hunter2" })).unwrap_err();
        assert!(!message.contains("hunter2"), "{message}");
        assert_eq!(
            message,
            "enum Level has no variant matching this value, expected one of `Debug`, `Info` for key `level`"
        );
        assert_eq!(
            read::<Log>(json!({ "level": { "Trace": 1 } })).unwrap_err(),
            "enum Level does not have variant constructor Trace for key `level`"
        );
    }

    #[test]
    fn numbers_are_masked_when_the_key_looks_secret() {
        let error = deserialize::<HashMap<String, u8>>(&tree(json!({ "pin": 300 }))).unwrap_err();
        assert_eq!(
            error.render(true),
            "invalid type: 64-bit integer [redacted], expected an unsigned 8 bit integer for key `pin` in test"
        );
        assert!(error.render(false).contains("`300`"));
    }

    #[test]
    fn debug_of_an_error_does_not_print_numbers() {
        let error = deserialize::<HashMap<String, u8>>(&tree(json!({ "pin": 300 }))).unwrap_err();
        assert!(!format!("{error:?}").contains("300"));
    }

    #[test]
    fn numeric_keys_convert_like_values() {
        let map = read::<HashMap<u16, String>>(json!({ "80": "http", "443": "https" })).unwrap();
        assert_eq!(map[&443], "https");
    }

    #[test]
    fn booleans_and_floats_follow_the_documented_spellings() {
        #[derive(Debug, Deserialize)]
        struct B {
            b: bool,
            f: f64,
        }
        let ok = read::<B>(json!({ "b": "Yes", "f": "1e2" })).unwrap();
        assert!(ok.b);
        assert_eq!(ok.f, 100.0);
        assert!(read::<B>(json!({ "b": "maybe", "f": 1 })).is_err());
        assert!(read::<B>(json!({ "b": true, "f": "fast" })).is_err());
    }
}
