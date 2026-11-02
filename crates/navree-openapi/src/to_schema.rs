//! [`ToSchema`] and its implementations for the types of the standard library,
//! chrono, uuid and serde_json.

use std::borrow::{Cow, ToOwned};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{Schema, Type};

/// A type described by a JSON Schema: `#[derive(ToSchema)]`, or by hand.
///
/// The derive reads the type's serde attributes, so the schema matches the JSON
/// serde reads and writes, and `#[schema(..)]` for what serde doesn't say.
/// Written by hand, [`schema`](ToSchema::schema) is the only method a type
/// needs:
///
/// ```
/// use navree_openapi::{Schema, ToSchema};
///
/// struct Zone(String);
///
/// impl ToSchema for Zone {
///     fn schema() -> Schema {
///         Schema::string().description("An IANA time zone name.")
///     }
/// }
///
/// assert_eq!(Zone::name(), "Zone");
/// ```
pub trait ToSchema {
    /// The name of the type's component schema: the last segment of its Rust
    /// path unless `#[schema(as = Name)]` says otherwise, followed by the names
    /// of its generic arguments (`Page<Note>` is `Page_Note`).
    fn name() -> Cow<'static, str> {
        let name = std::any::type_name::<Self>();
        let name = name.split('<').next().unwrap_or(name);
        Cow::Borrowed(name.rsplit("::").next().unwrap_or(name))
    }

    /// The type's schema.
    fn schema() -> Schema;

    /// What a field or an element of this type says: a `$ref` to the component,
    /// or for a type with no component of its own (`String`, `Vec<T>`) its
    /// schema.
    fn reference() -> Schema {
        Schema::reference_to(&Self::name())
    }

    /// Adds the component schemas [`schema`](ToSchema::schema) refers to.
    fn schemas(out: &mut Vec<(String, Schema)>) {
        let _ = out;
    }

    /// Adds the component schemas [`reference`](ToSchema::reference) refers to:
    /// this type's and what it refers to. A type already in `out` isn't walked
    /// again, so a type that contains itself ends. A different schema of the
    /// same name goes in beside the first, for the document to refuse.
    fn components(out: &mut Vec<(String, Schema)>) {
        let name = Self::name();
        let schema = Schema {
            rust_type: crate::schema::RustType(Some(std::any::type_name::<Self>())),
            ..Self::schema()
        };
        if let Some((_, existing)) = out.iter().find(|(existing, _)| *existing == name) {
            if *existing != schema {
                out.push((name.into_owned(), schema));
            }
            return;
        }
        out.push((name.into_owned(), schema));
        Self::schemas(out);
    }
}

/// `#[schema(value_type = Object)]`: `{"type": "object"}`, whatever the field's
/// Rust type is.
#[derive(Clone, Copy, Debug)]
pub struct Object;

/// A type with no component: fields inline its schema.
macro_rules! inline_schema {
    ($(impl[$($generics:tt)*] $ty:ty => $schema:expr;)+) => {$(
        impl<$($generics)*> ToSchema for $ty {
            fn schema() -> Schema {
                $schema
            }

            fn reference() -> Schema {
                Self::schema()
            }

            fn components(_out: &mut Vec<(String, Schema)>) {}
        }
    )+};
}

fn integer(format: Option<&str>, unsigned: bool) -> Schema {
    Schema {
        format: format.map(str::to_owned),
        minimum: unsigned.then(|| json!(0)),
        ..Schema::of_type(Type::Integer)
    }
}

fn number(format: &str) -> Schema {
    Schema {
        format: Some(format.to_owned()),
        ..Schema::of_type(Type::Number)
    }
}

fn formatted(format: &str) -> Schema {
    Schema {
        format: Some(format.to_owned()),
        ..Schema::string()
    }
}

inline_schema! {
    impl[] String => Schema::string();
    impl[] str => Schema::string();
    impl[] char => Schema::string();
    impl[] bool => Schema::of_type(Type::Boolean);
    impl[] i8 => integer(Some("int32"), false);
    impl[] i16 => integer(Some("int32"), false);
    impl[] i32 => integer(Some("int32"), false);
    impl[] i64 => integer(Some("int64"), false);
    impl[] i128 => integer(None, false);
    impl[] isize => integer(None, false);
    impl[] u8 => integer(Some("int32"), true);
    impl[] u16 => integer(Some("int32"), true);
    impl[] u32 => integer(Some("int32"), true);
    impl[] u64 => integer(Some("int64"), true);
    impl[] u128 => integer(None, true);
    impl[] usize => integer(None, true);
    impl[] f32 => number("float");
    impl[] f64 => number("double");
    impl[] Uuid => formatted("uuid");
    impl[Tz: TimeZone] DateTime<Tz> => formatted("date-time");
    impl[] NaiveDateTime => formatted("date-time");
    impl[] NaiveDate => formatted("date");
    impl[] NaiveTime => Schema::string();
    impl[] Value => Schema::default();
    impl[] serde_json::Map<String, Value> => Schema::map(Schema::string(), Schema::default());
    impl[] Object => Schema::object();
}

impl ToSchema for () {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("TupleUnit")
    }

    fn schema() -> Schema {
        Schema::default().default_value(Value::Null)
    }

    fn reference() -> Schema {
        Self::schema()
    }

    fn components(_out: &mut Vec<(String, Schema)>) {}
}

/// A wrapper that serializes as `T`: its schema and what it refers to are `T`'s.
macro_rules! transparent {
    ($(impl[$($generics:tt)*] $ty:ty => $inner:ty;)+) => {$(
        impl<$($generics)*> ToSchema for $ty {
            fn name() -> Cow<'static, str> {
                <$inner>::name()
            }

            fn schema() -> Schema {
                <$inner>::schema()
            }

            fn reference() -> Schema {
                <$inner>::reference()
            }

            fn schemas(out: &mut Vec<(String, Schema)>) {
                <$inner>::schemas(out);
            }

            fn components(out: &mut Vec<(String, Schema)>) {
                <$inner>::components(out);
            }
        }
    )+};
}

transparent! {
    impl[T: ToSchema + ?Sized] Box<T> => T;
    impl[T: ToSchema + ?Sized] Rc<T> => T;
    impl[T: ToSchema + ?Sized] Arc<T> => T;
    impl['a, T: ToSchema + ToOwned + ?Sized] Cow<'a, T> => T;
}

/// A reference is the referent's.
macro_rules! reference {
    ($(impl[$($generics:tt)*] $ty:ty;)+) => {$(
        impl<$($generics)*> ToSchema for $ty {
            fn name() -> Cow<'static, str> {
                T::name()
            }

            fn schema() -> Schema {
                T::schema()
            }

            fn reference() -> Schema {
                T::reference()
            }

            fn schemas(out: &mut Vec<(String, Schema)>) {
                T::schemas(out);
            }

            fn components(out: &mut Vec<(String, Schema)>) {
                T::components(out);
            }
        }
    )+};
}

reference! {
    impl['a, T: ToSchema + ?Sized] &'a T;
    impl['a, T: ToSchema + ?Sized] &'a mut T;
}

/// `Option<T>` on its own is `{"oneOf": [{"type": "null"}, T's schema]}`; as a
/// field it's what `T` says, nullable.
impl<T: ToSchema> ToSchema for Option<T> {
    fn name() -> Cow<'static, str> {
        Cow::Owned(format!("Option_{}", T::name()))
    }

    fn schema() -> Schema {
        Schema::one_of(vec![Schema::null(), T::schema()])
    }

    fn reference() -> Schema {
        T::reference().nullable()
    }

    fn schemas(out: &mut Vec<(String, Schema)>) {
        T::schemas(out);
    }

    fn components(out: &mut Vec<(String, Schema)>) {
        T::components(out);
    }
}

/// A list: `{"type": "array", "items": ..}`, with `T`'s schema on its own and
/// what `T` says as a field.
macro_rules! list {
    ($(impl[$($generics:tt)*] $ty:ty => $unique:expr;)+) => {$(
        impl<$($generics)*> ToSchema for $ty {
            fn name() -> Cow<'static, str> {
                let list = if ($unique as Option<bool>).is_some() { "Set" } else { "Vec" };
                Cow::Owned(format!("{list}_{}", T::name()))
            }

            fn schema() -> Schema {
                Schema {
                    unique_items: $unique,
                    ..Schema::array(T::schema())
                }
            }

            fn reference() -> Schema {
                Schema {
                    unique_items: $unique,
                    ..Schema::array(T::reference())
                }
            }

            fn schemas(out: &mut Vec<(String, Schema)>) {
                T::schemas(out);
            }

            fn components(out: &mut Vec<(String, Schema)>) {
                T::components(out);
            }
        }
    )+};
}

list! {
    impl[T: ToSchema] Vec<T> => None;
    impl[T: ToSchema] [T] => None;
    impl[T: ToSchema, const N: usize] [T; N] => None;
    impl[T: ToSchema] std::collections::LinkedList<T> => None;
    impl[T: ToSchema] std::collections::VecDeque<T> => None;
    impl[T: ToSchema, S] HashSet<T, S> => Some(true);
    impl[T: ToSchema] BTreeSet<T> => Some(true);
}

/// A map: `{"type": "object", "additionalProperties": .., "propertyNames": ..}`,
/// with `V`'s schema on its own and what `V` says as a field.
macro_rules! map {
    ($(impl[$($generics:tt)*] $ty:ty;)+) => {$(
        impl<$($generics)*> ToSchema for $ty {
            fn name() -> Cow<'static, str> {
                Cow::Owned(format!("Map_{}_{}", K::name(), V::name()))
            }

            fn schema() -> Schema {
                Schema::map(K::schema(), V::schema())
            }

            fn reference() -> Schema {
                Schema::map(K::reference(), V::reference())
            }

            fn schemas(out: &mut Vec<(String, Schema)>) {
                K::schemas(out);
                V::schemas(out);
            }

            fn components(out: &mut Vec<(String, Schema)>) {
                K::components(out);
                V::components(out);
            }
        }
    )+};
}

map! {
    impl[K: ToSchema, V: ToSchema] BTreeMap<K, V>;
    impl[K: ToSchema, V: ToSchema, S] HashMap<K, V, S>;
    impl[K: ToSchema, V: ToSchema, S] indexmap::IndexMap<K, V, S>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    struct Deal;

    impl ToSchema for Deal {
        fn schema() -> Schema {
            Schema::object()
        }
    }

    fn value<T: ToSchema + ?Sized>() -> Value {
        T::reference().to_value()
    }

    #[test]
    fn primitives_are_inline() {
        assert_eq!(value::<String>(), json!({"type": "string"}));
        assert_eq!(value::<&'static str>(), json!({"type": "string"}));
        assert_eq!(value::<Cow<'static, str>>(), json!({"type": "string"}));
        assert_eq!(
            value::<u64>(),
            json!({"type": "integer", "format": "int64", "minimum": 0})
        );
        assert_eq!(
            value::<i32>(),
            json!({"type": "integer", "format": "int32"})
        );
        assert_eq!(value::<usize>(), json!({"type": "integer", "minimum": 0}));
        assert_eq!(
            value::<f64>(),
            json!({"type": "number", "format": "double"})
        );
        assert_eq!(value::<Uuid>(), json!({"type": "string", "format": "uuid"}));
        assert_eq!(
            value::<DateTime<chrono::Utc>>(),
            json!({"type": "string", "format": "date-time"})
        );
        assert_eq!(value::<Value>(), json!({}));
    }

    #[test]
    fn options_are_nullable() {
        assert_eq!(
            value::<Option<bool>>(),
            json!({"type": ["boolean", "null"]})
        );
        assert_eq!(
            value::<Option<Option<String>>>(),
            json!({"type": ["string", "null"]})
        );
        assert_eq!(
            value::<Option<Deal>>(),
            json!({"oneOf": [{"$ref": "#/components/schemas/Deal"}, {"type": "null"}]})
        );
        assert_eq!(value::<Option<Value>>(), json!({}));
    }

    #[test]
    fn lists_and_maps_compose() {
        assert_eq!(
            value::<Vec<Deal>>(),
            json!({"type": "array", "items": {"$ref": "#/components/schemas/Deal"}})
        );
        assert_eq!(
            value::<BTreeSet<String>>(),
            json!({"type": "array", "items": {"type": "string"}, "uniqueItems": true})
        );
        assert_eq!(
            value::<BTreeMap<String, Deal>>(),
            json!({
                "type": "object",
                "additionalProperties": {"$ref": "#/components/schemas/Deal"},
                "propertyNames": {"type": "string"},
            })
        );
    }

    #[test]
    fn components_reach_through_wrappers_and_stop_at_primitives() {
        let mut out = Vec::new();
        <Option<Vec<BTreeMap<String, Box<Deal>>>>>::components(&mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "Deal");
        let mut out = Vec::new();
        <Vec<String>>::components(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn a_type_containing_itself_ends() {
        struct Tree;
        impl ToSchema for Tree {
            fn schema() -> Schema {
                Schema::object().property("children", <Vec<Tree>>::reference(), true)
            }
            fn schemas(out: &mut Vec<(String, Schema)>) {
                <Vec<Tree>>::components(out);
            }
        }
        let mut out = Vec::new();
        Tree::components(&mut out);
        assert_eq!(out.len(), 1);
    }
}
