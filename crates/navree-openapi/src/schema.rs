//! [`Schema`]: one JSON Schema (2020-12, the dialect of OpenAPI 3.1).

use indexmap::IndexMap;
use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// A JSON Schema type name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// `"object"`.
    Object,
    /// `"string"`.
    String,
    /// `"integer"`.
    Integer,
    /// `"number"`.
    Number,
    /// `"boolean"`.
    Boolean,
    /// `"array"`.
    Array,
    /// `"null"`.
    Null,
}

impl Type {
    /// The name in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Array => "array",
            Self::Null => "null",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "object" => Self::Object,
            "string" => Self::String,
            "integer" => Self::Integer,
            "number" => Self::Number,
            "boolean" => Self::Boolean,
            "array" => Self::Array,
            "null" => Self::Null,
            _ => return None,
        })
    }
}

/// The `type` keyword: one name, or several (`["string", "null"]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaType {
    /// `"type": "string"`.
    One(Type),
    /// `"type": ["string", "null"]`.
    Many(Vec<Type>),
}

impl SchemaType {
    fn to_value(&self) -> Value {
        match self {
            Self::One(ty) => Value::from(ty.as_str()),
            Self::Many(types) => Value::Array(
                types
                    .iter()
                    .map(|ty| Value::from(ty.as_str()))
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(name) => Type::parse(name).map(Self::One),
            Value::Array(names) => names
                .iter()
                .map(|name| name.as_str().and_then(Type::parse))
                .collect::<Option<Vec<_>>>()
                .map(Self::Many),
            _ => None,
        }
    }
}

/// A keyword whose value is a schema or a boolean (`items`,
/// `additionalProperties`).
#[derive(Clone, Debug, PartialEq)]
pub enum Subschema {
    /// `true` allows anything, `false` nothing.
    Bool(bool),
    /// A schema.
    Schema(Box<Schema>),
}

impl Subschema {
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(allowed) => Some(Self::Bool(*allowed)),
            Value::Object(_) => Schema::from_value(value.clone())
                .ok()
                .map(|schema| Self::Schema(Box::new(schema))),
            _ => None,
        }
    }
}

impl Serialize for Subschema {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Bool(allowed) => serializer.serialize_bool(*allowed),
            Self::Schema(schema) => schema.serialize(serializer),
        }
    }
}

/// A value that can't be a schema: anything but a JSON object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotASchema;

impl std::fmt::Display for NotASchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a schema must be a JSON object")
    }
}

impl std::error::Error for NotASchema {}

/// The Rust type a component came from, for the error when two types share a
/// name. It is not part of the schema: it is never written, and two schemas
/// are equal whatever it says.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RustType(pub Option<&'static str>);

impl PartialEq for RustType {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// One JSON Schema. Every field is a keyword; [`extra`](Schema::extra) holds
/// the ones without a field, so reading a schema and writing it again loses
/// nothing but the order of its keywords.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schema {
    #[doc(hidden)]
    pub rust_type: RustType,
    /// `$ref`.
    pub reference: Option<String>,
    /// `oneOf`.
    pub one_of: Vec<Schema>,
    /// `anyOf`.
    pub any_of: Vec<Schema>,
    /// `allOf`.
    pub all_of: Vec<Schema>,
    /// `type`.
    pub schema_type: Option<SchemaType>,
    /// `title`.
    pub title: Option<String>,
    /// `format`.
    pub format: Option<String>,
    /// `items`.
    pub items: Option<Subschema>,
    /// `prefixItems`.
    pub prefix_items: Vec<Schema>,
    /// `description`.
    pub description: Option<String>,
    /// `default`.
    pub default: Option<Value>,
    /// `enum`.
    pub enum_values: Vec<Value>,
    /// `const`.
    pub const_value: Option<Value>,
    /// `required`, in the order the fields are declared.
    pub required: Vec<String>,
    /// `properties`, in the order the fields are declared.
    pub properties: IndexMap<String, Schema>,
    /// `additionalProperties`.
    pub additional_properties: Option<Subschema>,
    /// `propertyNames`.
    pub property_names: Option<Box<Schema>>,
    /// `deprecated`.
    pub deprecated: Option<bool>,
    /// `example`.
    pub example: Option<Value>,
    /// `examples`.
    pub examples: Vec<Value>,
    /// `writeOnly`.
    pub write_only: Option<bool>,
    /// `readOnly`.
    pub read_only: Option<bool>,
    /// `maximum`.
    pub maximum: Option<Value>,
    /// `minimum`.
    pub minimum: Option<Value>,
    /// `maxLength`.
    pub max_length: Option<u64>,
    /// `minLength`.
    pub min_length: Option<u64>,
    /// `pattern`.
    pub pattern: Option<String>,
    /// `maxItems`.
    pub max_items: Option<u64>,
    /// `minItems`.
    pub min_items: Option<u64>,
    /// `uniqueItems`.
    pub unique_items: Option<bool>,
    /// Every other keyword.
    pub extra: Map<String, Value>,
}

impl Schema {
    /// A schema of one type.
    pub fn of_type(ty: Type) -> Self {
        Self {
            schema_type: Some(SchemaType::One(ty)),
            ..Self::default()
        }
    }

    /// `{"type": "object"}`.
    pub fn object() -> Self {
        Self::of_type(Type::Object)
    }

    /// `{"type": "string"}`.
    pub fn string() -> Self {
        Self::of_type(Type::String)
    }

    /// `{"type": "null"}`.
    pub fn null() -> Self {
        Self::of_type(Type::Null)
    }

    /// `{"type": "array", "items": items}`.
    pub fn array(items: Schema) -> Self {
        Self {
            items: Some(Subschema::Schema(Box::new(items))),
            ..Self::of_type(Type::Array)
        }
    }

    /// `{"type": "object", "additionalProperties": values, "propertyNames": keys}`.
    pub fn map(keys: Schema, values: Schema) -> Self {
        Self {
            additional_properties: Some(Subschema::Schema(Box::new(values))),
            property_names: Some(Box::new(keys)),
            ..Self::object()
        }
    }

    /// `{"$ref": "#/components/schemas/<name>"}`.
    pub fn reference_to(name: &str) -> Self {
        Self {
            reference: Some(format!("#/components/schemas/{name}")),
            ..Self::default()
        }
    }

    /// `{"oneOf": items}`.
    pub fn one_of(items: Vec<Schema>) -> Self {
        Self {
            one_of: items,
            ..Self::default()
        }
    }

    /// `{"allOf": items}`.
    pub fn all_of(items: Vec<Schema>) -> Self {
        Self {
            all_of: items,
            ..Self::default()
        }
    }

    /// `{"not": {}}`: no value is allowed, as for an enum without variants.
    pub fn never() -> Self {
        let mut schema = Self::default();
        schema
            .extra
            .insert("not".to_owned(), Value::Object(Map::new()));
        schema
    }

    /// `{"type": "string", "enum": values}`.
    pub fn string_enum<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            enum_values: values
                .into_iter()
                .map(|value| Value::String(value.into()))
                .collect(),
            ..Self::string()
        }
    }

    /// Adds a property, and with `required` lists it in `required`.
    #[must_use]
    pub fn property(mut self, name: &str, schema: Schema, required: bool) -> Self {
        self.properties.insert(name.to_owned(), schema);
        if required {
            self.required.push(name.to_owned());
        }
        self
    }

    /// Sets `description`.
    #[must_use]
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Sets `example`.
    #[must_use]
    pub fn example(mut self, value: Value) -> Self {
        self.example = Some(value);
        self
    }

    /// Sets `examples`.
    #[must_use]
    pub fn examples(mut self, values: Vec<Value>) -> Self {
        self.examples = values;
        self
    }

    /// `"readOnly": true`: only in responses.
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.read_only = Some(true);
        self
    }

    /// `"writeOnly": true`: only in requests.
    #[must_use]
    pub fn write_only(mut self) -> Self {
        self.write_only = Some(true);
        self
    }

    /// Sets `default`.
    #[must_use]
    pub fn default_value(mut self, value: Value) -> Self {
        self.default = Some(value);
        self
    }

    /// Sets `title`.
    #[must_use]
    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.title = Some(text.into());
        self
    }

    /// Sets `deprecated`.
    #[must_use]
    pub fn deprecated(mut self) -> Self {
        self.deprecated = Some(true);
        self
    }

    /// Sets `additionalProperties` to `false`.
    #[must_use]
    pub fn no_additional_properties(mut self) -> Self {
        self.additional_properties = Some(Subschema::Bool(false));
        self
    }

    /// Sets `format`, on the items of an array or the values of a map, whose
    /// elements are what the format describes.
    #[must_use]
    pub fn format(self, format: impl Into<String>) -> Self {
        let format = format.into();
        self.at_elements(|schema| schema.format = Some(format))
    }

    /// Sets `minimum`, on the items of an array or the values of a map.
    #[must_use]
    pub fn minimum(self, value: Value) -> Self {
        self.at_elements(|schema| schema.minimum = Some(value))
    }

    /// Sets `maximum`, on the items of an array or the values of a map.
    #[must_use]
    pub fn maximum(self, value: Value) -> Self {
        self.at_elements(|schema| schema.maximum = Some(value))
    }

    /// `null` is also valid: `type` gains `"null"`; a schema without a type
    /// becomes `{"oneOf": [schema, {"type": "null"}]}`. A schema that allows
    /// anything already allows `null`.
    #[must_use]
    pub fn nullable(mut self) -> Self {
        match self.schema_type.take() {
            Some(SchemaType::One(Type::Null)) => {
                self.schema_type = Some(SchemaType::One(Type::Null));
                self
            }
            Some(SchemaType::One(ty)) => {
                self.schema_type = Some(SchemaType::Many(vec![ty, Type::Null]));
                self
            }
            Some(SchemaType::Many(mut types)) => {
                if !types.contains(&Type::Null) {
                    types.push(Type::Null);
                }
                self.schema_type = Some(SchemaType::Many(types));
                self
            }
            None if self.is_nullable_union() || self.allows_anything() => self,
            None => Self::one_of(vec![self, Self::null()]),
        }
    }

    /// The description a field's doc comment gives: on the reference when the
    /// schema is a nullable reference (`{"oneOf": [{"$ref": ..}, null]}`),
    /// otherwise on the schema.
    #[must_use]
    pub fn described(mut self, text: impl Into<String>) -> Self {
        if self.is_nullable_union()
            && let Some(first) = self.one_of.first_mut()
            && first.reference.is_some()
        {
            first.description = Some(text.into());
            return self;
        }
        self.description = Some(text.into());
        self
    }

    /// The schema as JSON.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Reads a schema from JSON. A keyword whose value isn't of the kind the
    /// keyword has stays in [`extra`](Schema::extra), so every JSON object is
    /// read and written back.
    ///
    /// # Errors
    ///
    /// [`NotASchema`] when `value` isn't a JSON object.
    pub fn from_value(value: Value) -> Result<Self, NotASchema> {
        let Value::Object(object) = value else {
            return Err(NotASchema);
        };
        let mut schema = Self::default();
        for (keyword, value) in object {
            if !schema.read_keyword(&keyword, &value) {
                schema.extra.insert(keyword, value);
            }
        }
        Ok(schema)
    }

    /// `{"oneOf": [.., {"type": "null"}]}`.
    fn is_nullable_union(&self) -> bool {
        self.one_of
            .iter()
            .any(|item| item.schema_type == Some(SchemaType::One(Type::Null)))
    }

    /// No keyword that constrains a value.
    fn allows_anything(&self) -> bool {
        self.reference.is_none()
            && self.one_of.is_empty()
            && self.any_of.is_empty()
            && self.all_of.is_empty()
            && self.enum_values.is_empty()
            && self.const_value.is_none()
            && self.properties.is_empty()
            && self.items.is_none()
    }

    fn at_elements(mut self, set: impl FnOnce(&mut Schema)) -> Self {
        fn apply(schema: &mut Schema, set: impl FnOnce(&mut Schema)) {
            if let Some(Subschema::Schema(items)) = &mut schema.items {
                apply(items, set);
            } else if let Some(Subschema::Schema(values)) = &mut schema.additional_properties {
                apply(values, set);
            } else {
                set(schema);
            }
        }
        apply(&mut self, set);
        self
    }

    /// Takes `keyword` into its field; `false` when it has none, the value
    /// isn't of its kind, or the field couldn't write it back (an empty list).
    fn read_keyword(&mut self, keyword: &str, value: &Value) -> bool {
        fn text(slot: &mut Option<String>, value: &Value) -> bool {
            value.as_str().is_some_and(|text| {
                *slot = Some(text.to_owned());
                true
            })
        }
        fn flag(slot: &mut Option<bool>, value: &Value) -> bool {
            value.as_bool().is_some_and(|flag| {
                *slot = Some(flag);
                true
            })
        }
        fn count(slot: &mut Option<u64>, value: &Value) -> bool {
            value.as_u64().is_some_and(|count| {
                *slot = Some(count);
                true
            })
        }
        fn number(slot: &mut Option<Value>, value: &Value) -> bool {
            value.is_number() && {
                *slot = Some(value.clone());
                true
            }
        }
        fn list(slot: &mut Vec<Schema>, value: &Value) -> bool {
            let Some(items) = value.as_array().filter(|items| !items.is_empty()) else {
                return false;
            };
            let Some(schemas) = items
                .iter()
                .map(|item| Schema::from_value(item.clone()).ok())
                .collect::<Option<Vec<_>>>()
            else {
                return false;
            };
            *slot = schemas;
            true
        }
        fn values(slot: &mut Vec<Value>, value: &Value) -> bool {
            value
                .as_array()
                .filter(|values| !values.is_empty())
                .is_some_and(|values| {
                    slot.clone_from(values);
                    true
                })
        }
        fn sub(slot: &mut Option<Subschema>, value: &Value) -> bool {
            Subschema::from_value(value).is_some_and(|sub| {
                *slot = Some(sub);
                true
            })
        }

        match keyword {
            "$ref" => text(&mut self.reference, value),
            "oneOf" => list(&mut self.one_of, value),
            "anyOf" => list(&mut self.any_of, value),
            "allOf" => list(&mut self.all_of, value),
            "type" => SchemaType::from_value(value).is_some_and(|ty| {
                self.schema_type = Some(ty);
                true
            }),
            "title" => text(&mut self.title, value),
            "format" => text(&mut self.format, value),
            "items" => sub(&mut self.items, value),
            "prefixItems" => list(&mut self.prefix_items, value),
            "description" => text(&mut self.description, value),
            "default" => {
                self.default = Some(value.clone());
                true
            }
            "enum" => values(&mut self.enum_values, value),
            "const" => {
                self.const_value = Some(value.clone());
                true
            }
            "required" => value
                .as_array()
                .filter(|names| !names.is_empty())
                .and_then(|names| {
                    names
                        .iter()
                        .map(|name| name.as_str().map(str::to_owned))
                        .collect::<Option<Vec<_>>>()
                })
                .is_some_and(|names| {
                    self.required = names;
                    true
                }),
            "properties" => value
                .as_object()
                .filter(|properties| !properties.is_empty())
                .and_then(|properties| {
                    properties
                        .iter()
                        .map(|(name, schema)| {
                            Schema::from_value(schema.clone())
                                .ok()
                                .map(|schema| (name.clone(), schema))
                        })
                        .collect::<Option<IndexMap<_, _>>>()
                })
                .is_some_and(|properties| {
                    self.properties = properties;
                    true
                }),
            "additionalProperties" => sub(&mut self.additional_properties, value),
            "propertyNames" => Schema::from_value(value.clone()).is_ok_and(|schema| {
                self.property_names = Some(Box::new(schema));
                true
            }),
            "deprecated" => flag(&mut self.deprecated, value),
            "example" => {
                self.example = Some(value.clone());
                true
            }
            "examples" => values(&mut self.examples, value),
            "writeOnly" => flag(&mut self.write_only, value),
            "readOnly" => flag(&mut self.read_only, value),
            "maximum" => number(&mut self.maximum, value),
            "minimum" => number(&mut self.minimum, value),
            "maxLength" => count(&mut self.max_length, value),
            "minLength" => count(&mut self.min_length, value),
            "pattern" => text(&mut self.pattern, value),
            "maxItems" => count(&mut self.max_items, value),
            "minItems" => count(&mut self.min_items, value),
            "uniqueItems" => flag(&mut self.unique_items, value),
            _ => false,
        }
    }
}

impl Serialize for Schema {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        fn some<M: SerializeMap, T: Serialize>(
            map: &mut M,
            key: &str,
            value: Option<&T>,
        ) -> Result<(), M::Error> {
            match value {
                Some(value) => map.serialize_entry(key, value),
                None => Ok(()),
            }
        }
        fn many<M: SerializeMap, T: Serialize>(
            map: &mut M,
            key: &str,
            values: &[T],
        ) -> Result<(), M::Error> {
            if values.is_empty() {
                Ok(())
            } else {
                map.serialize_entry(key, values)
            }
        }

        let mut map = serializer.serialize_map(None)?;
        some(&mut map, "$ref", self.reference.as_ref())?;
        many(&mut map, "oneOf", &self.one_of)?;
        many(&mut map, "anyOf", &self.any_of)?;
        many(&mut map, "allOf", &self.all_of)?;
        some(
            &mut map,
            "type",
            self.schema_type.as_ref().map(SchemaType::to_value).as_ref(),
        )?;
        some(&mut map, "title", self.title.as_ref())?;
        some(&mut map, "format", self.format.as_ref())?;
        some(&mut map, "items", self.items.as_ref())?;
        many(&mut map, "prefixItems", &self.prefix_items)?;
        some(&mut map, "description", self.description.as_ref())?;
        some(&mut map, "default", self.default.as_ref())?;
        many(&mut map, "enum", &self.enum_values)?;
        some(&mut map, "const", self.const_value.as_ref())?;
        many(&mut map, "required", &self.required)?;
        if !self.properties.is_empty() {
            map.serialize_entry("properties", &self.properties)?;
        }
        some(
            &mut map,
            "additionalProperties",
            self.additional_properties.as_ref(),
        )?;
        some(&mut map, "propertyNames", self.property_names.as_ref())?;
        some(&mut map, "deprecated", self.deprecated.as_ref())?;
        some(&mut map, "example", self.example.as_ref())?;
        many(&mut map, "examples", &self.examples)?;
        some(&mut map, "writeOnly", self.write_only.as_ref())?;
        some(&mut map, "readOnly", self.read_only.as_ref())?;
        some(&mut map, "maximum", self.maximum.as_ref())?;
        some(&mut map, "minimum", self.minimum.as_ref())?;
        some(&mut map, "maxLength", self.max_length.as_ref())?;
        some(&mut map, "minLength", self.min_length.as_ref())?;
        some(&mut map, "pattern", self.pattern.as_ref())?;
        some(&mut map, "maxItems", self.max_items.as_ref())?;
        some(&mut map, "minItems", self.min_items.as_ref())?;
        some(&mut map, "uniqueItems", self.unique_items.as_ref())?;
        for (keyword, value) in &self.extra {
            map.serialize_entry(keyword, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Schema {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_value(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn nullable_adds_null_to_a_type_or_wraps_a_reference() {
        assert_eq!(
            Schema::string().nullable().to_value(),
            json!({"type": ["string", "null"]})
        );
        assert_eq!(
            Schema::reference_to("Deal").nullable().to_value(),
            json!({"oneOf": [{"$ref": "#/components/schemas/Deal"}, {"type": "null"}]})
        );
        assert_eq!(Schema::default().nullable().to_value(), json!({}));
        let twice = Schema::reference_to("Deal").nullable().nullable();
        assert_eq!(twice, Schema::reference_to("Deal").nullable());
    }

    #[test]
    fn a_description_goes_on_the_reference_of_a_nullable_reference() {
        assert_eq!(
            Schema::reference_to("Deal")
                .nullable()
                .described("The deal.")
                .to_value(),
            json!({"oneOf": [
                {"$ref": "#/components/schemas/Deal", "description": "The deal."},
                {"type": "null"},
            ]})
        );
    }

    #[test]
    fn format_and_limits_go_on_the_elements() {
        assert_eq!(
            Schema::array(Schema::string()).format("uuid").to_value(),
            json!({"type": "array", "items": {"type": "string", "format": "uuid"}})
        );
    }

    #[test]
    fn every_json_object_is_read_and_written_back() {
        let value = json!({
            "type": ["string", 7],
            "enum": "not a list",
            "required": [],
            "oneOf": [],
            "x-navree-flag": {"name": "beta"},
            "properties": {"a": "not a schema"},
            "$ref": 3,
        });
        let schema = Schema::from_value(value.clone()).unwrap();
        assert_eq!(schema.to_value(), value);
    }

    #[test]
    fn a_schema_survives_json() {
        let value = json!({
            "type": "object",
            "description": "A deal.",
            "required": ["id"],
            "properties": {
                "id": {"type": "string", "format": "uuid"},
                "stage": {"oneOf": [{"$ref": "#/components/schemas/Stage"}, {"type": "null"}]},
                "tags": {"type": "array", "items": {"type": "string"}, "uniqueItems": true},
            },
            "additionalProperties": false,
        });
        let schema: Schema = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&schema).unwrap(), value);
    }

    #[test]
    fn only_an_object_is_a_schema() {
        assert_eq!(Schema::from_value(json!([1])), Err(NotASchema));
        assert_eq!(Schema::from_value(json!(true)), Err(NotASchema));
    }
}
