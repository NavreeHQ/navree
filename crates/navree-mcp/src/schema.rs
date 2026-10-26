//! Tool input schemas and prompt arguments from `utoipa::ToSchema`, the
//! derive request bodies already use.

use serde_json::{Map, Value, json};

/// The JSON Schema of `T` for a tool's `inputSchema`: the schema utoipa
/// derives (field doc comments become descriptions), with the schemas it
/// refers to inlined under `$defs` (`#/components/schemas/Stage` becomes
/// `#/$defs/Stage`), so the document stands alone.
///
/// ```
/// use navree_mcp::input_schema;
///
/// #[derive(utoipa::ToSchema)]
/// #[allow(dead_code)]
/// struct ListDeals {
///     /// Words from the deal title.
///     query: Option<String>,
/// }
///
/// let schema = input_schema::<ListDeals>();
/// assert_eq!(schema["type"], "object");
/// assert_eq!(schema["properties"]["query"]["description"], "Words from the deal title.");
/// ```
pub fn input_schema<T: utoipa::ToSchema>() -> Value {
    let mut schema = serde_json::to_value(T::schema()).unwrap_or_else(|_| json!({}));
    let mut referenced = Vec::new();
    T::schemas(&mut referenced);
    if !referenced.is_empty() {
        let defs: Map<String, Value> = referenced
            .into_iter()
            .map(|(name, schema)| {
                let mut schema = serde_json::to_value(schema).unwrap_or_else(|_| json!({}));
                rewrite_refs(&mut schema);
                (name, schema)
            })
            .collect();
        rewrite_refs(&mut schema);
        if let Some(object) = schema.as_object_mut() {
            object.insert("$defs".to_owned(), Value::Object(defs));
        }
    }
    object_schema(schema)
}

/// The schema of a tool without arguments.
pub(crate) fn empty_object() -> Value {
    json!({ "type": "object", "properties": {} })
}

/// `schema` with `"type": "object"` (MCP requires an object schema).
fn object_schema(mut schema: Value) -> Value {
    match schema.as_object_mut() {
        Some(object) => {
            object
                .entry("type")
                .or_insert_with(|| Value::String("object".to_owned()));
            schema
        }
        None => empty_object(),
    }
}

/// `#/components/schemas/X` → `#/$defs/X`, everywhere in `value`.
fn rewrite_refs(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if key == "$ref"
                    && let Value::String(reference) = child
                    && let Some(name) = reference.strip_prefix("#/components/schemas/")
                {
                    *reference = format!("#/$defs/{name}");
                } else {
                    rewrite_refs(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(rewrite_refs),
        _ => {}
    }
}

/// One argument of a prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptArgument {
    /// The argument name (a property of the arguments struct).
    pub name: String,
    /// Its description (the field's doc comment).
    pub description: Option<String>,
    /// Whether the client must fill it in.
    pub required: bool,
}

/// The prompt arguments of an arguments struct: its properties, with their
/// descriptions and whether they are required.
pub(crate) fn prompt_arguments(schema: &Value) -> Vec<PromptArgument> {
    let required: Vec<&str> = schema["required"]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    schema["properties"]
        .as_object()
        .map(|properties| {
            properties
                .iter()
                .map(|(name, property)| PromptArgument {
                    name: name.clone(),
                    description: property["description"].as_str().map(str::to_owned),
                    required: required.contains(&name.as_str()),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Prompt arguments arrive as strings; turns those whose schema says
/// boolean, integer, number, array or object into JSON values so the
/// arguments struct deserializes. Unparsable values stay strings (and fail
/// validation with the field's message).
pub(crate) fn coerce_prompt_arguments(schema: &Value, arguments: Map<String, Value>) -> Value {
    let properties = schema["properties"].as_object();
    let coerced = arguments
        .into_iter()
        .map(|(name, value)| {
            let Value::String(text) = &value else {
                return (name, value);
            };
            let kind = properties
                .and_then(|properties| properties.get(&name))
                .map(schema_types)
                .unwrap_or_default();
            let parsed = if kind.iter().any(|t| t == "string") {
                None
            } else if kind
                .iter()
                .any(|t| matches!(t.as_str(), "boolean" | "integer" | "number" | "array" | "object"))
            {
                serde_json::from_str::<Value>(text).ok()
            } else {
                None
            };
            (name, parsed.unwrap_or(value))
        })
        .collect();
    Value::Object(coerced)
}

/// The `type` (a string or a list) of a property schema; `oneOf` members
/// count too (`Option<T>` with a `$ref`).
fn schema_types(schema: &Value) -> Vec<String> {
    let mut types: Vec<String> = match &schema["type"] {
        Value::String(t) => vec![t.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    for key in ["oneOf", "anyOf"] {
        if let Some(members) = schema[key].as_array() {
            types.extend(members.iter().flat_map(schema_types));
        }
    }
    types.retain(|t| t != "null");
    types
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(utoipa::ToSchema)]
    #[allow(dead_code)]
    enum Stage {
        Lead,
        Won,
    }

    #[derive(utoipa::ToSchema)]
    #[allow(dead_code)]
    struct ListDeals {
        /// Words from the deal title.
        query: Option<String>,
        /// Only deals in this stage.
        stage: Option<Stage>,
        /// Only the user's own deals.
        mine: bool,
        /// At most this many.
        limit: Option<u32>,
    }

    #[test]
    fn referenced_schemas_are_inlined_under_defs() {
        let schema = input_schema::<ListDeals>();
        assert_eq!(schema["type"], "object");
        assert!(
            schema.to_string().contains("#/$defs/Stage"),
            "the stage refers to $defs: {schema}"
        );
        assert!(!schema.to_string().contains("components"), "{schema}");
        assert!(schema["$defs"]["Stage"]["enum"].is_array(), "{schema}");
        assert_eq!(
            schema["properties"]["mine"]["description"],
            "Only the user's own deals."
        );
    }

    #[test]
    fn prompt_arguments_come_from_the_properties() {
        let arguments = prompt_arguments(&input_schema::<ListDeals>());
        let mine = arguments.iter().find(|a| a.name == "mine").unwrap();
        assert!(mine.required);
        assert_eq!(mine.description.as_deref(), Some("Only the user's own deals."));
        let query = arguments.iter().find(|a| a.name == "query").unwrap();
        assert!(!query.required);
    }

    #[test]
    fn string_arguments_are_coerced_by_schema_type() {
        let schema = input_schema::<ListDeals>();
        let arguments = serde_json::from_value(json!({
            "query": "42", "mine": "true", "limit": "5", "stage": "Lead", "extra": "x",
        }))
        .unwrap();
        assert_eq!(
            coerce_prompt_arguments(&schema, arguments),
            json!({ "query": "42", "mine": true, "limit": 5, "stage": "Lead", "extra": "x" })
        );
    }

    #[test]
    fn non_object_schemas_become_empty_objects() {
        assert_eq!(object_schema(json!(true)), empty_object());
        assert_eq!(object_schema(json!({ "properties": {} }))["type"], "object");
    }
}
