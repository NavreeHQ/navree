//! `#[derive(ToSchema)]`: what each serde attribute and `#[schema(..)]` option
//! writes into the schema.

#![allow(dead_code)]

use std::borrow::Cow;
use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use navree_openapi::{Schema, ToSchema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

fn schema<T: ToSchema>() -> Value {
    T::schema().to_value()
}

fn components<T: ToSchema>() -> BTreeMap<String, Value> {
    let mut out = Vec::new();
    T::schemas(&mut out);
    out.into_iter()
        .map(|(name, schema)| (name, schema.to_value()))
        .collect()
}

/// An address.
#[derive(Serialize, ToSchema)]
struct Address {
    city: String,
}

/// A stage.
#[derive(ToSchema, Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    /// Variant docs stay out of a plain enum.
    NewLead,
    #[serde(rename = "gone")]
    Lost,
    #[serde(skip)]
    Hidden,
}

#[test]
fn a_struct_lists_its_fields_with_their_docs() {
    /// A deal.
    ///
    /// With two paragraphs.
    #[derive(ToSchema)]
    struct Deal {
        /// The id.
        id: Uuid,
        title: String,
        amount: Option<u64>,
        tags: Vec<String>,
        created_at: DateTime<Utc>,
        closes_on: Option<NaiveDate>,
        label: Cow<'static, str>,
        kind: &'static str,
        data: Value,
        extra: Option<Value>,
        counts: BTreeMap<String, u32>,
        maybe_tags: Option<Vec<String>>,
        twice: Option<Option<String>>,
    }
    assert_eq!(
        schema::<Deal>(),
        json!({
            "type": "object",
            "description": "A deal.\n\nWith two paragraphs.",
            "required": ["id", "title", "tags", "created_at", "label", "kind", "data", "counts"],
            "properties": {
                "id": {"type": "string", "format": "uuid", "description": "The id."},
                "title": {"type": "string"},
                "amount": {"type": ["integer", "null"], "format": "int64", "minimum": 0},
                "tags": {"type": "array", "items": {"type": "string"}},
                "created_at": {"type": "string", "format": "date-time"},
                "closes_on": {"type": ["string", "null"], "format": "date"},
                "label": {"type": "string"},
                "kind": {"type": "string"},
                "data": {},
                "extra": {},
                "counts": {
                    "type": "object",
                    "additionalProperties": {"type": "integer", "format": "int32", "minimum": 0},
                    "propertyNames": {"type": "string"},
                },
                "maybe_tags": {"type": ["array", "null"], "items": {"type": "string"}},
                "twice": {"type": ["string", "null"]},
            },
        })
    );
    assert_eq!(Deal::name(), "Deal");
    assert!(components::<Deal>().is_empty());
}

#[test]
fn a_field_of_a_component_type_refers_to_it_and_collects_it() {
    #[derive(ToSchema)]
    struct Company {
        /// Where it is.
        address: Address,
        billing: Option<Address>,
        /// Where else.
        other: Option<Address>,
        branches: Vec<Address>,
        by_name: BTreeMap<String, Address>,
    }
    assert_eq!(
        schema::<Company>()["properties"],
        json!({
            "address": {"$ref": "#/components/schemas/Address", "description": "Where it is."},
            "billing": {"oneOf": [{"$ref": "#/components/schemas/Address"}, {"type": "null"}]},
            "other": {"oneOf": [
                {"$ref": "#/components/schemas/Address", "description": "Where else."},
                {"type": "null"},
            ]},
            "branches": {"type": "array", "items": {"$ref": "#/components/schemas/Address"}},
            "by_name": {
                "type": "object",
                "additionalProperties": {"$ref": "#/components/schemas/Address"},
                "propertyNames": {"type": "string"},
            },
        })
    );
    let found = components::<Company>();
    assert_eq!(found.len(), 1);
    assert_eq!(found["Address"]["description"], "An address.");
}

#[test]
fn serde_decides_names_and_what_is_required() {
    #[derive(Serialize, Deserialize, ToSchema)]
    #[serde(rename_all = "camelCase")]
    struct Input {
        status_code: u16,
        #[serde(rename = "__type")]
        resource_type: String,
        #[serde(default)]
        with_default: String,
        #[serde(default = "seven")]
        with_function: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        skipped_if: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        skipped_list: Vec<String>,
        #[serde(skip)]
        hidden: String,
        #[serde(skip_serializing)]
        write_only: String,
        #[serde(skip_deserializing)]
        read_only: String,
    }
    let value = schema::<Input>();
    assert_eq!(value["required"], json!(["statusCode", "__type"]));
    let names: Vec<&String> = value["properties"].as_object().unwrap().keys().collect();
    assert_eq!(
        names,
        [
            "__type",
            "skippedIf",
            "skippedList",
            "statusCode",
            "withDefault",
            "withFunction",
        ]
    );
}

fn seven() -> u8 {
    7
}

#[test]
fn a_container_default_gives_each_field_its_default_value() {
    #[derive(Default, Serialize, Deserialize, ToSchema)]
    #[serde(default)]
    struct Options {
        limit: u32,
        name: Option<String>,
        tags: Vec<String>,
    }
    let value = schema::<Options>();
    assert_eq!(value.get("required"), None);
    assert_eq!(value["properties"]["limit"]["default"], json!(0));
    assert_eq!(value["properties"]["name"]["default"], json!(null));
    assert_eq!(value["properties"]["tags"]["default"], json!([]));
}

#[test]
fn deny_unknown_fields_closes_the_object() {
    #[derive(Serialize, Deserialize, ToSchema)]
    #[serde(deny_unknown_fields)]
    struct Closed {
        a: String,
    }
    assert_eq!(schema::<Closed>()["additionalProperties"], json!(false));
}

#[test]
fn flatten_is_all_of_and_a_flattened_map_is_additional_properties() {
    #[derive(Serialize, ToSchema)]
    struct Base {
        id: String,
    }
    #[derive(Serialize, ToSchema)]
    struct Extended {
        #[serde(flatten)]
        base: Base,
        name: String,
    }
    assert_eq!(
        schema::<Extended>(),
        json!({"allOf": [
            {"$ref": "#/components/schemas/Base"},
            {"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}}},
        ]})
    );
    assert!(components::<Extended>().contains_key("Base"));

    #[derive(Serialize, ToSchema)]
    struct Open {
        name: String,
        #[serde(flatten)]
        rest: BTreeMap<String, Value>,
    }
    let value = schema::<Open>();
    assert_eq!(value["additionalProperties"], json!({}));
    assert_eq!(value["required"], json!(["name"]));
    assert_eq!(value["properties"].as_object().unwrap().len(), 1);
}

#[test]
fn schema_options_on_a_field() {
    struct NoSchema;

    fn custom() -> Schema {
        Schema::string().description("From a function.")
    }

    #[derive(ToSchema)]
    struct Options {
        #[schema(value_type = String)]
        as_string: NoSchema,
        #[schema(value_type = Option<String>)]
        as_optional: NoSchema,
        #[schema(value_type = Object)]
        as_object: NoSchema,
        /// Docs on an object.
        #[schema(value_type = Option<Object>)]
        as_optional_object: NoSchema,
        #[schema(value_type = Address)]
        as_component: NoSchema,
        #[schema(value_type = String, format = Uuid, required = true)]
        formatted: Option<NoSchema>,
        #[schema(nullable = false)]
        never_null: Option<String>,
        #[schema(nullable = false)]
        never_null_ref: Option<Address>,
        #[schema(format = Password)]
        password: Option<String>,
        #[schema(format = "custom")]
        custom_format: String,
        #[schema(format = Date)]
        day: Vec<String>,
        #[schema(rename = "renamed", example = "Note")]
        with_example: String,
        #[schema(example = json!({"a": 1}))]
        json_example: Value,
        #[schema(minimum = 0, maximum = 100)]
        percent: i32,
        #[schema(minimum = 1)]
        at_least_one: u32,
        #[schema(title = "A title")]
        titled: String,
        #[schema(required = false)]
        not_required: String,
        #[schema(schema_with = custom)]
        with_function: NoSchema,
    }
    let value = schema::<Options>();
    let properties = &value["properties"];
    assert_eq!(properties["as_string"], json!({"type": "string"}));
    assert_eq!(
        properties["as_optional"],
        json!({"type": ["string", "null"]})
    );
    assert_eq!(properties["as_object"], json!({"type": "object"}));
    assert_eq!(
        properties["as_optional_object"],
        json!({"type": ["object", "null"], "description": "Docs on an object."})
    );
    assert_eq!(
        properties["as_component"],
        json!({"$ref": "#/components/schemas/Address"})
    );
    assert_eq!(
        properties["formatted"],
        json!({"type": "string", "format": "uuid"})
    );
    assert_eq!(properties["never_null"], json!({"type": "string"}));
    assert_eq!(
        properties["never_null_ref"],
        json!({"$ref": "#/components/schemas/Address"})
    );
    assert_eq!(
        properties["password"],
        json!({"type": ["string", "null"], "format": "password"})
    );
    assert_eq!(
        properties["custom_format"],
        json!({"type": "string", "format": "custom"})
    );
    assert_eq!(
        properties["day"],
        json!({"type": "array", "items": {"type": "string", "format": "date"}})
    );
    assert_eq!(
        properties["renamed"],
        json!({"type": "string", "example": "Note"})
    );
    assert_eq!(properties["json_example"], json!({"example": {"a": 1}}));
    assert_eq!(
        properties["percent"],
        json!({"type": "integer", "format": "int32", "minimum": 0, "maximum": 100})
    );
    assert_eq!(
        properties["at_least_one"],
        json!({"type": "integer", "format": "int32", "minimum": 1})
    );
    assert_eq!(properties["titled"]["title"], "A title");
    assert_eq!(
        properties["with_function"],
        json!({"type": "string", "description": "From a function."})
    );
    let required = value["required"].as_array().unwrap();
    assert!(required.contains(&json!("formatted")));
    assert!(!required.contains(&json!("never_null")));
    assert!(!required.contains(&json!("not_required")));
    assert!(required.contains(&json!("with_function")));
}

#[test]
fn inline_writes_the_schema_in_place() {
    #[derive(ToSchema)]
    struct Nested {
        reference: Address,
    }
    #[derive(ToSchema)]
    struct Outer {
        #[schema(inline)]
        plain: Nested,
        /// With its own docs.
        #[schema(inline)]
        described: Nested,
        /// Nullable.
        #[schema(inline)]
        maybe: Option<Nested>,
        #[schema(inline)]
        list: Vec<Nested>,
    }
    let nested = json!({
        "type": "object",
        "required": ["reference"],
        "properties": {"reference": {"$ref": "#/components/schemas/Address"}},
    });
    let value = schema::<Outer>();
    let properties = &value["properties"];
    assert_eq!(properties["plain"], nested);
    assert_eq!(
        properties["described"],
        json!({"allOf": [nested, {"type": "object", "description": "With its own docs."}]})
    );
    assert_eq!(
        properties["maybe"],
        json!({"oneOf": [nested, {"type": "null"}], "description": "Nullable."})
    );
    assert_eq!(
        properties["list"],
        json!({"type": "array", "items": nested})
    );
    let found = components::<Outer>();
    assert!(!found.contains_key("Nested"));
    assert!(found.contains_key("Address"));
}

#[test]
fn container_options() {
    /// Comes second.
    #[derive(ToSchema)]
    #[schema(as = PublicName, description = "Comes first.")]
    struct RustName {
        a: String,
    }
    assert_eq!(RustName::name(), "PublicName");
    assert_eq!(schema::<RustName>()["description"], "Comes first.");
    assert_eq!(
        <Option<RustName>>::reference().to_value(),
        json!({"oneOf": [{"$ref": "#/components/schemas/PublicName"}, {"type": "null"}]})
    );
}

#[test]
fn a_newtype_is_its_field() {
    /// Flags by name.
    #[derive(ToSchema)]
    struct Flags(BTreeMap<String, bool>);
    assert_eq!(
        schema::<Flags>(),
        json!({
            "type": "object",
            "description": "Flags by name.",
            "additionalProperties": {"type": "boolean"},
            "propertyNames": {"type": "string"},
        })
    );
    assert_eq!(
        Flags::reference().to_value(),
        json!({"$ref": "#/components/schemas/Flags"})
    );

    #[derive(ToSchema)]
    struct Pair(String, String);
    assert_eq!(
        schema::<Pair>(),
        json!({"type": "array", "items": {"type": "string"}, "maxItems": 2, "minItems": 2})
    );
}

#[test]
fn a_plain_enum_is_a_string_enum() {
    assert_eq!(
        schema::<Stage>(),
        json!({"type": "string", "description": "A stage.", "enum": ["new_lead", "gone"]})
    );
    assert_eq!(
        Stage::reference().to_value(),
        json!({"$ref": "#/components/schemas/Stage"})
    );
}

#[test]
fn enums_follow_serde_representations() {
    #[derive(Serialize, ToSchema)]
    #[serde(rename_all = "snake_case")]
    enum External {
        /// Nothing.
        Plain,
        /// One value.
        OneValue(String),
        /// Named.
        Named {
            /// The value.
            value: u8,
        },
    }
    assert_eq!(
        schema::<External>(),
        json!({"oneOf": [
            {"type": "string", "description": "Nothing.", "enum": ["plain"]},
            {
                "type": "object",
                "description": "One value.",
                "required": ["one_value"],
                "properties": {"one_value": {"type": "string", "description": "One value."}},
            },
            {
                "type": "object",
                "description": "Named.",
                "required": ["named"],
                "properties": {"named": {
                    "type": "object",
                    "description": "Named.",
                    "required": ["value"],
                    "properties": {"value": {
                        "type": "integer", "format": "int32", "description": "The value.", "minimum": 0,
                    }},
                }},
            },
        ]})
    );

    #[derive(Serialize, ToSchema)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Internal {
        /// Nothing.
        Plain,
        WithReference(Address),
        Named {
            value: String,
        },
    }
    assert_eq!(
        schema::<Internal>(),
        json!({"oneOf": [
            {
                "type": "object",
                "description": "Nothing.",
                "required": ["kind"],
                "properties": {"kind": {"type": "string", "enum": ["plain"]}},
            },
            {"allOf": [
                {"$ref": "#/components/schemas/Address"},
                {
                    "type": "object",
                    "required": ["kind"],
                    "properties": {"kind": {"type": "string", "enum": ["with_reference"]}},
                },
            ]},
            {
                "type": "object",
                "required": ["value", "kind"],
                "properties": {
                    "value": {"type": "string"},
                    "kind": {"type": "string", "enum": ["named"]},
                },
            },
        ]})
    );
    assert!(components::<Internal>().contains_key("Address"));

    #[derive(Serialize, ToSchema)]
    #[serde(tag = "type", content = "id", rename_all = "snake_case")]
    enum Adjacent {
        Everyone,
        Workspace(String),
    }
    assert_eq!(
        schema::<Adjacent>(),
        json!({"oneOf": [
            {
                "type": "object",
                "required": ["type"],
                "properties": {"type": {"type": "string", "enum": ["everyone"]}},
            },
            {
                "type": "object",
                "required": ["id", "type"],
                "properties": {
                    "id": {"type": "string"},
                    "type": {"type": "string", "enum": ["workspace"]},
                },
            },
        ]})
    );

    #[derive(Serialize, ToSchema)]
    #[serde(untagged)]
    enum Untagged {
        Reference(Address),
        Text(String),
        Named { a: String },
    }
    assert_eq!(
        schema::<Untagged>(),
        json!({"oneOf": [
            {"$ref": "#/components/schemas/Address"},
            {"type": "string"},
            {"type": "object", "required": ["a"], "properties": {"a": {"type": "string"}}},
        ]})
    );
}

#[test]
fn a_generic_type_refers_to_its_argument_without_collecting_it() {
    #[derive(ToSchema)]
    struct Page<T> {
        data: Vec<T>,
    }
    assert_eq!(Page::<Address>::name(), "Page");
    assert_eq!(
        schema::<Page<Address>>()["properties"]["data"]["items"],
        json!({"$ref": "#/components/schemas/Address"})
    );
    assert_eq!(
        schema::<Page<String>>()["properties"]["data"]["items"],
        json!({"type": "string"})
    );
    assert!(components::<Page<Address>>().is_empty());
}

#[test]
fn a_type_that_contains_itself_ends() {
    #[derive(ToSchema)]
    struct Tree {
        children: Vec<Tree>,
    }
    let mut out = Vec::new();
    Tree::components(&mut out);
    assert_eq!(out.len(), 1);
}

#[test]
fn a_raw_identifier_loses_its_prefix() {
    #[derive(ToSchema)]
    struct Keyword {
        r#type: String,
    }
    assert_eq!(schema::<Keyword>()["required"], json!(["type"]));
}

#[test]
fn a_hand_written_schema_is_a_component_by_its_rust_name() {
    struct Zone;
    impl ToSchema for Zone {
        fn schema() -> Schema {
            Schema::string()
        }
    }
    #[derive(ToSchema)]
    struct Place {
        zone: Zone,
    }
    assert_eq!(Zone::name(), "Zone");
    assert_eq!(
        schema::<Place>()["properties"]["zone"],
        json!({"$ref": "#/components/schemas/Zone"})
    );
    assert!(components::<Place>().contains_key("Zone"));
}
