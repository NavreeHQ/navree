//! `application/x-www-form-urlencoded`: the parser and the serde
//! deserializer behind [`Query`](crate::Query) and [`Form`](crate::Form).
//!
//! **Parsing** follows the URL standard's urlencoded parser: the input is
//! split on `&`, empty pieces are skipped, each piece splits at its first
//! `=` into a name and a value (a piece without `=` has the value `""`);
//! then in both, `+` becomes a space, each `%` followed by two hex digits
//! becomes its byte (any other `%` stays), and the bytes are read as UTF-8
//! with invalid sequences replaced by U+FFFD.
//!
//! **Deserializing**: the pairs make a map (a struct's fields by name, a
//! `HashMap`, or `Vec<(K, V)>` in order). A value deserializes from its
//! text: numbers and booleans are parsed (`true`/`false`), `Option<T>` is
//! `Some` whenever the key is present (an empty value is `Some("")` and
//! fails to parse as a number), an enum is a unit variant named by the
//! value. A key that appears twice is an error for a struct field (as serde
//! reports a duplicate field) and keeps the last value in a `HashMap`.
//! Nested values (a sequence or map inside a field) are not supported.

use std::borrow::Cow;

use serde::de::value::MapDeserializer;
use serde::de::{self, Error as _, IntoDeserializer};
use serde::forward_to_deserialize_any;

use crate::routing::percent_decode;

/// The error of deserializing urlencoded pairs.
pub type Error = serde::de::value::Error;

/// Parses urlencoded pairs (see the [module documentation](self)).
///
/// ```
/// use navree_http::extract::urlencoded::parse;
///
/// let pairs: Vec<_> = parse(b"q=caf%C3%A9+au+lait&&page=2&flag").collect();
/// assert_eq!(pairs, [("q".into(), "café au lait".into()), ("page".into(), "2".into()), ("flag".into(), "".into())]);
/// ```
pub fn parse(input: &[u8]) -> Parse<'_> {
    Parse { input }
}

/// The pairs of [`parse`].
#[derive(Clone, Copy, Debug)]
pub struct Parse<'a> {
    input: &'a [u8],
}

impl<'a> Iterator for Parse<'a> {
    type Item = (Cow<'a, str>, Cow<'a, str>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.input.is_empty() {
                return None;
            }
            let (piece, rest) = match self.input.iter().position(|byte| *byte == b'&') {
                Some(at) => (&self.input[..at], &self.input[at + 1..]),
                None => (self.input, &[][..]),
            };
            self.input = rest;
            if piece.is_empty() {
                continue;
            }
            let (name, value) = match piece.iter().position(|byte| *byte == b'=') {
                Some(at) => (&piece[..at], &piece[at + 1..]),
                None => (piece, &[][..]),
            };
            return Some((decode(name), decode(value)));
        }
    }
}

/// `+` to space, percent-decoding, then UTF-8 (lossy).
fn decode(input: &[u8]) -> Cow<'_, str> {
    let spaced: Cow<'_, [u8]> = if input.contains(&b'+') {
        Cow::Owned(
            input
                .iter()
                .map(|byte| if *byte == b'+' { b' ' } else { *byte })
                .collect(),
        )
    } else {
        Cow::Borrowed(input)
    };
    let decoded = match spaced {
        Cow::Borrowed(bytes) => percent_decode(bytes),
        Cow::Owned(bytes) => Cow::Owned(percent_decode(&bytes).into_owned()),
    };
    match decoded {
        Cow::Borrowed(bytes) => String::from_utf8_lossy(bytes),
        Cow::Owned(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Cow::Owned(text),
            Err(error) => Cow::Owned(String::from_utf8_lossy(error.as_bytes()).into_owned()),
        },
    }
}

/// Deserializes `T` from urlencoded `input`.
///
/// ```
/// use navree_http::extract::urlencoded::from_bytes;
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Debug, PartialEq)]
/// struct Filter {
///     q: String,
///     page: Option<u32>,
/// }
///
/// let filter: Filter = from_bytes(b"q=deals&page=2").unwrap();
/// assert_eq!(filter, Filter { q: "deals".into(), page: Some(2) });
/// let error = from_bytes::<Filter>(b"q=deals&page=two").unwrap_err();
/// assert_eq!(error.to_string(), "invalid digit found in string");
/// ```
pub fn from_bytes<'de, T: de::Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
    T::deserialize(Deserializer::new(input))
}

/// The serde deserializer over urlencoded pairs. Structs, maps and
/// sequences of pairs at the top level.
pub struct Deserializer<'de> {
    inner: MapDeserializer<'de, Pairs<'de>, Error>,
}

impl<'de> Deserializer<'de> {
    /// A deserializer over the pairs of `input`.
    pub fn new(input: &'de [u8]) -> Self {
        Self {
            inner: MapDeserializer::new(Pairs(parse(input))),
        }
    }
}

impl<'de> de::Deserializer<'de> for Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_map(self.inner)
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_seq(self.inner)
    }

    fn deserialize_unit<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.inner.end()?;
        visitor.visit_unit()
    }

    forward_to_deserialize_any! {
        bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string option bytes
        byte_buf unit_struct newtype_struct tuple_struct struct identifier tuple
        enum ignored_any
    }
}

/// The parsed pairs, as deserializer parts.
struct Pairs<'de>(Parse<'de>);

impl<'de> Iterator for Pairs<'de> {
    type Item = (Part<'de>, Part<'de>);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(name, value)| (Part(name), Part(value)))
    }
}

/// One name or value.
struct Part<'de>(Cow<'de, str>);

impl<'de> IntoDeserializer<'de> for Part<'de> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

/// Parses the text as `$ty`, then hands the value to the visitor.
macro_rules! parsed {
    ($($ty:ident => $method:ident,)*) => {
        $(
            fn $method<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                match self.0.parse::<$ty>() {
                    Ok(value) => value.into_deserializer().$method(visitor),
                    Err(error) => Err(Error::custom(error)),
                }
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for Part<'de> {
    type Error = Error;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Cow::Borrowed(text) => visitor.visit_borrowed_str(text),
            Cow::Owned(text) => visitor.visit_string(text),
        }
    }

    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }

    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_enum(UnitVariant(self.0))
    }

    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    forward_to_deserialize_any! {
        char str string unit bytes byte_buf unit_struct tuple_struct struct
        identifier tuple ignored_any seq map
    }

    parsed! {
        bool => deserialize_bool,
        u8 => deserialize_u8,
        u16 => deserialize_u16,
        u32 => deserialize_u32,
        u64 => deserialize_u64,
        i8 => deserialize_i8,
        i16 => deserialize_i16,
        i32 => deserialize_i32,
        i64 => deserialize_i64,
        f32 => deserialize_f32,
        f64 => deserialize_f64,
    }
}

/// An enum named by a value: unit variants only.
struct UnitVariant<'de>(Cow<'de, str>);

impl<'de> de::EnumAccess<'de> for UnitVariant<'de> {
    type Error = Error;
    type Variant = UnitOnly;

    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, UnitOnly), Error> {
        let variant = seed.deserialize(Part(self.0))?;
        Ok((variant, UnitOnly))
    }
}

struct UnitOnly;

impl<'de> de::VariantAccess<'de> for UnitOnly {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(
        self,
        _seed: T,
    ) -> Result<T::Value, Error> {
        Err(Error::custom("expected unit variant"))
    }

    fn tuple_variant<V: de::Visitor<'de>>(
        self,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Error> {
        Err(Error::custom("expected unit variant"))
    }

    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Error> {
        Err(Error::custom("expected unit variant"))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use serde::Deserialize;

    use super::*;

    fn pairs(input: &str) -> Vec<(String, String)> {
        parse(input.as_bytes())
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect()
    }

    #[test]
    fn pairs_split_on_ampersands_and_the_first_equals_sign() {
        assert_eq!(pairs(""), []);
        assert_eq!(pairs("&&&"), []);
        assert_eq!(
            pairs("a=1&b=2"),
            [("a".into(), "1".into()), ("b".into(), "2".into())]
        );
        assert_eq!(pairs("a"), [("a".into(), String::new())]);
        assert_eq!(pairs("=x"), [(String::new(), "x".into())]);
        assert_eq!(pairs("a=b=c"), [("a".into(), "b=c".into())]);
        assert_eq!(
            pairs("a=1&&a=2"),
            [("a".into(), "1".into()), ("a".into(), "2".into())]
        );
    }

    #[test]
    fn plus_is_a_space_and_escapes_decode_once() {
        assert_eq!(pairs("q=a+b"), [("q".into(), "a b".into())]);
        assert_eq!(pairs("q=a%2Bb"), [("q".into(), "a+b".into())]);
        assert_eq!(pairs("q=%2541"), [("q".into(), "%41".into())]);
        assert_eq!(pairs("q=100%"), [("q".into(), "100%".into())]);
        assert_eq!(pairs("q=%zz"), [("q".into(), "%zz".into())]);
        assert_eq!(pairs("caf%C3%A9=1"), [("café".into(), "1".into())]);
        assert_eq!(pairs("q=%26%3D"), [("q".into(), "&=".into())]);
        assert_eq!(
            pairs("q=%FF"),
            [("q".into(), "\u{FFFD}".into())],
            "lossy UTF-8"
        );
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "lowercase")]
    enum Sort {
        Asc,
        Desc,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Filter {
        q: String,
        page: Option<u32>,
        active: Option<bool>,
        sort: Option<Sort>,
        #[serde(default)]
        tags: Option<String>,
    }

    #[test]
    fn structs_read_their_fields_by_name() {
        let filter: Filter = from_bytes(b"q=deals+won&page=2&active=true&sort=desc").unwrap();
        assert_eq!(
            filter,
            Filter {
                q: "deals won".into(),
                page: Some(2),
                active: Some(true),
                sort: Some(Sort::Desc),
                tags: None,
            }
        );
        let error = from_bytes::<Filter>(b"page=1").unwrap_err();
        assert_eq!(error.to_string(), "missing field `q`");
        let error = from_bytes::<Filter>(b"q=x&page=").unwrap_err();
        assert_eq!(error.to_string(), "cannot parse integer from empty string");
        let error = from_bytes::<Filter>(b"q=x&active=yes").unwrap_err();
        assert_eq!(
            error.to_string(),
            "provided string was not `true` or `false`"
        );
        let error = from_bytes::<Filter>(b"q=x&sort=up").unwrap_err();
        assert_eq!(
            error.to_string(),
            "unknown variant `up`, expected `asc` or `desc`"
        );
        let error = from_bytes::<Filter>(b"q=x&q=y").unwrap_err();
        assert_eq!(error.to_string(), "duplicate field `q`");
    }

    #[test]
    fn maps_and_pair_lists_take_every_pair() {
        let map: HashMap<String, String> = from_bytes(b"a=1&b=2&a=3").unwrap();
        assert_eq!(map["a"], "3", "the last value wins");
        let ordered: BTreeMap<String, u8> = from_bytes(b"b=2&a=1").unwrap();
        assert_eq!(
            ordered.into_iter().collect::<Vec<_>>(),
            [("a".into(), 1), ("b".into(), 2)]
        );
        let list: Vec<(String, String)> = from_bytes(b"a=1&a=2").unwrap();
        assert_eq!(list, [("a".into(), "1".into()), ("a".into(), "2".into())]);
        assert!(from_bytes::<()>(b"").is_ok());
        assert!(from_bytes::<()>(b"a=1").is_err());
    }

    #[test]
    fn nested_values_are_refused() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Nested {
            ids: Vec<u32>,
        }
        assert!(from_bytes::<Nested>(b"ids=1&ids=2").is_err());
    }
}
