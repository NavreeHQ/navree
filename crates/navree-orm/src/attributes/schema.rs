//! OpenAPI schemas of models and their pages.
//!
//! [`Record<M>`] implements `ToSchema`, so a route documents a
//! record response with `navree_http::schema_fn::<Record<Deal>>`: the
//! schema has one property per serialized column under serde's key
//! (hidden columns left out, the visible list respected), typed from the
//! field or its cast (`Option` fields nullable, `skip_serializing_if`
//! fields not required), plus the appended accessors. A field or accessor
//! names its exact schema with `#[model(schema = Type)]` /
//! `#[accessor(schema = Type)]` (an enum's values, a nested object).
//!
//! Pages: [`SimplePage`] and [`CursorPage`] of any `ToSchema` type
//! implement `ToSchema`; navree-db's `Page` gets [`page_schema`] (a
//! `navree_http::SchemaFn`).
//!
//! ```
//! # use navree_orm::__doc_models::*;
//! use navree_openapi::ToSchema;
//!
//! assert_eq!(Record::<Tag>::name(), "Tag");
//! let schema = serde_json::to_value(Record::<Tag>::schema()).unwrap();
//! assert_eq!(schema["properties"]["name"], serde_json::json!({"type": "string"}));
//! assert_eq!(schema["required"], serde_json::json!(["id", "name", "color"]));
//! ```

use std::borrow::Cow;
use std::marker::PhantomData;

use navree_openapi::{Schema, ToSchema};
use serde_json::{Map, Value as Json, json};

use crate::attributes::serialize::output_key;
use crate::model::Model;
use crate::pagination::{CursorPage, SimplePage};
use crate::record::Record;
use crate::value::FieldKind;

/// The JSON Schema of a field kind (not nullable).
pub fn kind_schema(kind: FieldKind) -> Json {
    match kind {
        FieldKind::Bool => json!({"type": "boolean"}),
        FieldKind::Integer { bits, signed } => {
            let format = if bits <= 32 && signed {
                "int32"
            } else {
                "int64"
            };
            if signed {
                json!({"type": "integer", "format": format})
            } else {
                json!({"type": "integer", "format": format, "minimum": 0})
            }
        }
        FieldKind::Float { bits } => {
            json!({"type": "number", "format": if bits <= 32 { "float" } else { "double" }})
        }
        FieldKind::Decimal => json!({"type": "string", "format": "decimal"}),
        FieldKind::Text => json!({"type": "string"}),
        FieldKind::Bytes => {
            json!({"type": "array", "items": {"type": "integer", "format": "int32", "minimum": 0, "maximum": 255}})
        }
        FieldKind::Uuid => json!({"type": "string", "format": "uuid"}),
        FieldKind::Ulid => json!({"type": "string", "pattern": "^[0-9A-HJKMNP-TV-Z]{26}$"}),
        FieldKind::Date => json!({"type": "string", "format": "date"}),
        FieldKind::Time => json!({"type": "string", "format": "time"}),
        FieldKind::DateTime => json!({"type": "string", "format": "date-time"}),
        FieldKind::Url => json!({"type": "string", "format": "uri"}),
        FieldKind::Vector => {
            json!({"type": "array", "items": {"type": "number", "format": "float"}})
        }
        FieldKind::Json | FieldKind::Any => json!({}),
        // Kinds added later describe themselves as any value until mapped.
        #[allow(unreachable_patterns)]
        _ => json!({}),
    }
}

/// `schema` that also accepts `null`.
fn nullable(mut schema: Json) -> Json {
    if let Some(object) = schema.as_object_mut() {
        match object.get_mut("type") {
            Some(Json::String(single)) => {
                let single = std::mem::take(single);
                object.insert("type".to_owned(), json!([single, "null"]));
            }
            Some(Json::Array(types)) if !types.iter().any(|t| t == "null") => {
                types.push(json!("null"));
            }
            // Any value (and a list naming null) already includes null.
            _ => {}
        }
    }
    schema
}

/// The JSON Schema of a Rust type written as text (an accessor's return
/// type): common types map to their shape, anything else is any value.
pub fn rust_type_schema(text: &str) -> Json {
    let text = text.trim();
    let generic = |name: &str| {
        text.strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('<'))
            .and_then(|rest| rest.strip_suffix('>'))
    };
    if let Some(inner) = generic("Option") {
        return nullable(rust_type_schema(inner));
    }
    if let Some(inner) = generic("Vec")
        .or_else(|| generic("BTreeSet"))
        .or_else(|| generic("HashSet"))
    {
        return json!({"type": "array", "items": rust_type_schema(inner)});
    }
    let last = text.rsplit("::").next().unwrap_or(text);
    let kind = match last.trim_start_matches('&').trim_start_matches("'static ") {
        "bool" => FieldKind::Bool,
        "i8" => FieldKind::Integer {
            bits: 8,
            signed: true,
        },
        "i16" => FieldKind::Integer {
            bits: 16,
            signed: true,
        },
        "i32" => FieldKind::Integer {
            bits: 32,
            signed: true,
        },
        "i64" | "isize" => FieldKind::Integer {
            bits: 64,
            signed: true,
        },
        "u8" => FieldKind::Integer {
            bits: 8,
            signed: false,
        },
        "u16" => FieldKind::Integer {
            bits: 16,
            signed: false,
        },
        "u32" => FieldKind::Integer {
            bits: 32,
            signed: false,
        },
        "u64" | "usize" => FieldKind::Integer {
            bits: 64,
            signed: false,
        },
        "f32" => FieldKind::Float { bits: 32 },
        "f64" => FieldKind::Float { bits: 64 },
        "Decimal" => FieldKind::Decimal,
        "String" | "str" | "Cow<str>" | "Cow<'static, str>" => FieldKind::Text,
        "Uuid" => FieldKind::Uuid,
        "Ulid" => FieldKind::Ulid,
        "NaiveDate" => FieldKind::Date,
        "NaiveTime" => FieldKind::Time,
        "NaiveDateTime" | "DateTime<Utc>" | "DateTime<FixedOffset>" => FieldKind::DateTime,
        "Url" => FieldKind::Url,
        _ => FieldKind::Any,
    };
    kind_schema(kind)
}

/// A schema as JSON.
fn schema_json(schema: Schema) -> Json {
    serde_json::to_value(schema).unwrap_or_else(|_| json!({}))
}

/// The schema of `M`'s serialized records (see the module docs): the
/// model's hidden, visible and appended names as declared.
pub struct ModelSchema<M>(PhantomData<fn() -> M>);

impl<M: Model> ModelSchema<M> {
    /// The schema's name: the model's.
    pub fn name() -> Cow<'static, str> {
        Cow::Borrowed(M::meta().name())
    }

    /// The schema as JSON Schema.
    pub fn json() -> Json {
        let meta = M::meta();
        let keys = |names: &'static [&'static str]| -> Vec<String> {
            names
                .iter()
                .filter_map(|name| output_key(meta, name))
                .collect()
        };
        let hidden = keys(meta.hidden_names());
        let visible = keys(meta.visible_names());
        let shown = |key: &str| {
            (visible.is_empty() || visible.iter().any(|name| name == key))
                && !hidden.iter().any(|name| name == key)
        };
        let mut properties = Map::new();
        let mut required = Vec::new();
        let mut fields: Vec<&str> = Vec::new();
        for column in meta.all_columns() {
            if fields.contains(&column.field()) {
                continue;
            }
            fields.push(column.field());
            let Some(key) = column.serialized_name() else {
                continue;
            };
            if !shown(key) {
                continue;
            }
            let schema = match column.schema_override() {
                Some(schema) => schema_json(schema()),
                None if column.has_custom_serde() => json!({}),
                None if column.is_date_time() && meta.date_serializer().is_some() => json!({}),
                None => kind_schema(column.field_kind()),
            };
            let schema = if column.is_nullable() {
                nullable(schema)
            } else {
                schema
            };
            properties.insert(key.to_owned(), schema);
            if !column.is_serde_optional() {
                required.push(key.to_owned());
            }
        }
        for name in meta.appended_names() {
            if !shown(name) {
                continue;
            }
            let schema = M::accessors()
                .iter()
                .find(|accessor| accessor.name() == *name)
                .map_or_else(
                    || json!({}),
                    |accessor| match accessor.schema_override() {
                        Some(schema) => schema_json(schema()),
                        None => rust_type_schema(accessor.rust_type()),
                    },
                );
            properties.insert((*name).to_owned(), schema);
            required.push((*name).to_owned());
        }
        json!({"type": "object", "properties": properties, "required": required})
    }

    /// The schema in the document model's form.
    pub fn schema() -> Schema {
        to_schema(Self::json())
    }
}

/// JSON Schema in the document model's form.
fn to_schema(json: Json) -> Schema {
    Schema::from_value(json).unwrap_or_default()
}

/// A record's schema: the model's ([`ModelSchema`]), named after the model
/// (`Deal`).
impl<M: Model> ToSchema for Record<M> {
    fn name() -> Cow<'static, str> {
        ModelSchema::<M>::name()
    }

    fn schema() -> Schema {
        ModelSchema::<M>::schema()
    }
}

/// `{"$ref": "#/components/schemas/<T>"}`.
fn reference<T: ToSchema>() -> Json {
    json!({"$ref": format!("#/components/schemas/{}", T::name())})
}

/// The schemas a page of `T` refers to: `T` and what `T` refers to.
fn page_components<T: ToSchema>(schemas: &mut Vec<(String, Schema)>) {
    schemas.push((T::name().into_owned(), T::schema()));
    T::schemas(schemas);
}

fn count() -> Json {
    json!({"type": "integer", "format": "int64", "minimum": 0})
}

fn simple_page_json<T: ToSchema>() -> Json {
    json!({
        "type": "object",
        "required": ["data", "meta"],
        "properties": {
            "data": {"type": "array", "items": reference::<T>()},
            "meta": {
                "type": "object",
                "required": ["current_page", "per_page", "has_more"],
                "properties": {
                    "current_page": count(),
                    "per_page": count(),
                    "has_more": {"type": "boolean"},
                },
            },
        },
    })
}

fn cursor_page_json<T: ToSchema>() -> Json {
    json!({
        "type": "object",
        "required": ["data", "next_cursor", "prev_cursor"],
        "properties": {
            "data": {"type": "array", "items": reference::<T>()},
            "next_cursor": {"type": ["string", "null"]},
            "prev_cursor": {"type": ["string", "null"]},
        },
    })
}

fn page_json<T: ToSchema>() -> Json {
    json!({
        "type": "object",
        "required": ["data", "meta"],
        "properties": {
            "data": {"type": "array", "items": reference::<T>()},
            "meta": {
                "type": "object",
                "required": ["current_page", "per_page", "total", "last_page"],
                "properties": {
                    "current_page": count(),
                    "per_page": count(),
                    "total": count(),
                    "last_page": count(),
                },
            },
        },
    })
}

/// `{data: [T], meta: {current_page, per_page, has_more}}`, named
/// `<T>SimplePage`.
impl<T: ToSchema> ToSchema for SimplePage<T> {
    fn name() -> Cow<'static, str> {
        Cow::Owned(format!("{}SimplePage", T::name()))
    }

    fn schema() -> Schema {
        to_schema(simple_page_json::<T>())
    }

    fn schemas(schemas: &mut Vec<(String, Schema)>) {
        page_components::<T>(schemas);
    }
}

/// `{data: [T], next_cursor, prev_cursor}`, named `<T>CursorPage`.
impl<T: ToSchema> ToSchema for CursorPage<T> {
    fn name() -> Cow<'static, str> {
        Cow::Owned(format!("{}CursorPage", T::name()))
    }

    fn schema() -> Schema {
        to_schema(cursor_page_json::<T>())
    }

    fn schemas(schemas: &mut Vec<(String, Schema)>) {
        page_components::<T>(schemas);
    }
}

/// A page schema as `navree_http::SchemaFn` returns it: the name, the
/// schema, and the schemas it refers to under `x-navree-components`.
fn schema_fn_output<T: ToSchema>(name: String, mut schema: Json) -> (String, Json) {
    let mut referenced = Vec::new();
    page_components::<T>(&mut referenced);
    navree_openapi::__private::attach_components(&mut schema, referenced);
    (name, schema)
}

/// The schema of navree-db's `Page<T>` (`{data, meta: {current_page,
/// per_page, total, last_page}}`), named `<T>Page`, as a
/// `navree_http::SchemaFn`: `ApiMeta { response: Some(page_schema::<Record<Deal>>), .. }`.
///
/// ```
/// # use navree_orm::__doc_models::*;
/// use navree_orm::attributes::page_schema;
///
/// let (name, schema) = page_schema::<Record<Tag>>();
/// assert_eq!(name, "TagPage");
/// assert_eq!(schema["properties"]["data"]["items"]["$ref"], "#/components/schemas/Tag");
/// assert!(schema["x-navree-components"]["Tag"]["properties"]["color"].is_object());
/// ```
pub fn page_schema<T: ToSchema>() -> (String, Json) {
    schema_fn_output::<T>(format!("{}Page", T::name()), page_json::<T>())
}

/// The schema of [`SimplePage<T>`], as a `navree_http::SchemaFn`.
pub fn simple_page_schema<T: ToSchema>() -> (String, Json) {
    schema_fn_output::<T>(format!("{}SimplePage", T::name()), simple_page_json::<T>())
}

/// The schema of [`CursorPage<T>`], as a `navree_http::SchemaFn`.
pub fn cursor_page_schema<T: ToSchema>() -> (String, Json) {
    schema_fn_output::<T>(format!("{}CursorPage", T::name()), cursor_page_json::<T>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_rust_types_map_to_schemas() {
        assert_eq!(
            kind_schema(FieldKind::Integer {
                bits: 32,
                signed: true
            }),
            json!({"type": "integer", "format": "int32"})
        );
        assert_eq!(
            kind_schema(FieldKind::Integer {
                bits: 64,
                signed: false
            }),
            json!({"type": "integer", "format": "int64", "minimum": 0})
        );
        assert_eq!(
            nullable(kind_schema(FieldKind::DateTime)),
            json!({"type": ["string", "null"], "format": "date-time"})
        );
        assert_eq!(nullable(json!({})), json!({}));
        assert_eq!(
            rust_type_schema("Option<Vec<String>>"),
            json!({"type": ["array", "null"], "items": {"type": "string"}})
        );
        assert_eq!(
            rust_type_schema("rust_decimal::Decimal"),
            json!({"type": "string", "format": "decimal"})
        );
        assert_eq!(rust_type_schema("MyThing"), json!({}));
    }

    #[test]
    fn json_schemas_survive_the_document_model() {
        for schema in [
            json!({"type": "object", "properties": {"a": {"type": ["string", "null"], "format": "date-time"}}, "required": ["a"]}),
            json!({"type": "array", "items": {"type": "integer", "format": "int64"}}),
            simple_page_json::<Record<crate::__doc_models::Tag>>(),
        ] {
            assert_eq!(schema_json(to_schema(schema.clone())), schema);
        }
    }
}
