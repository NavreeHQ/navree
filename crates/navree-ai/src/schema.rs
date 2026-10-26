//! JSON Schemas for structured output and tool arguments, from utoipa's
//! `ToSchema` (the derive the OpenAPI document uses), and the adjustments
//! each provider needs.

use serde_json::{Map, Value, json};
use utoipa::ToSchema;

/// The schema name of a wrapped non-object value (`{"value": ...}`).
pub(crate) const WRAPPED: &str = "value";

/// A self-contained JSON Schema for `T`: its own schema with the types it
/// refers to under `$defs` (`#/$defs/Stage`).
pub fn json_schema<T: ToSchema>() -> Value {
    let mut components = Vec::new();
    T::schemas(&mut components);
    let mut schema = serde_json::to_value(T::schema()).unwrap_or_else(|_| json!({}));
    rewrite_refs(&mut schema);
    if !components.is_empty() {
        let mut defs = Map::new();
        for (name, component) in components {
            let mut component = serde_json::to_value(component).unwrap_or_else(|_| json!({}));
            rewrite_refs(&mut component);
            defs.insert(name, component);
        }
        if let Value::Object(map) = &mut schema {
            map.insert("$defs".to_owned(), Value::Object(defs));
        }
    }
    schema
}

/// `#/components/schemas/X` → `#/$defs/X`, everywhere in `schema`.
fn rewrite_refs(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref")
                && let Some(name) = reference.strip_prefix("#/components/schemas/")
            {
                *reference = format!("#/$defs/{name}");
            }
            for value in map.values_mut() {
                rewrite_refs(value);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(rewrite_refs),
        _ => {}
    }
}

/// Whether `schema` describes a JSON object (what providers require at the
/// root of structured output).
pub(crate) fn is_object(schema: &Value) -> bool {
    match schema.get("type") {
        Some(Value::String(kind)) => kind == "object",
        Some(_) => false,
        None => schema.get("properties").is_some(),
    }
}

/// `schema` as an object: unchanged when it is one, else
/// `{"value": schema}` (unwrap the answer with [`unwrap_value`]).
pub(crate) fn wrap_non_object(schema: Value) -> (Value, bool) {
    if is_object(&schema) {
        return (schema, false);
    }
    let mut schema = schema;
    let defs = schema
        .as_object_mut()
        .and_then(|map| map.remove("$defs"));
    let mut wrapped = json!({
        "type": "object",
        "properties": { WRAPPED: schema },
        "required": [WRAPPED],
    });
    if let Some(defs) = defs {
        wrapped["$defs"] = defs;
    }
    (wrapped, true)
}

/// The value inside a wrapped answer.
pub(crate) fn unwrap_value(mut value: Value) -> Value {
    match value.get_mut(WRAPPED) {
        Some(inner) => inner.take(),
        None => value,
    }
}

/// String formats Anthropic's structured outputs accept.
const ANTHROPIC_FORMATS: [&str; 10] = [
    "date-time", "time", "date", "duration", "email", "hostname", "uri", "ipv4", "ipv6", "uuid",
];

/// Keywords Anthropic's structured outputs reject (they are checked on the
/// answer instead, by deserialization and `.validated()`).
const ANTHROPIC_UNSUPPORTED: [&str; 12] = [
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minItems",
    "maxItems",
    "uniqueItems",
    "examples",
    "example",
];

/// `schema` adjusted for Anthropic's structured outputs: every object gets
/// `additionalProperties: false`, unsupported constraints and formats are
/// dropped, and `"type": [X, "null"]` becomes `anyOf`.
pub(crate) fn for_anthropic(mut schema: Value) -> Value {
    fn visit(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for key in ANTHROPIC_UNSUPPORTED {
                    map.remove(key);
                }
                if let Some(Value::String(format)) = map.get("format")
                    && !ANTHROPIC_FORMATS.contains(&format.as_str())
                {
                    map.remove("format");
                }
                if let Some(Value::Array(types)) = map.get("type").cloned() {
                    map.remove("type");
                    let mut variants: Vec<Value> = Vec::new();
                    for kind in types {
                        let mut variant = Map::new();
                        variant.insert("type".to_owned(), kind.clone());
                        if kind != "null" {
                            for key in ["properties", "required", "items", "enum", "format"] {
                                if let Some(value) = map.remove(key) {
                                    variant.insert(key.to_owned(), value);
                                }
                            }
                        }
                        variants.push(Value::Object(variant));
                    }
                    map.insert("anyOf".to_owned(), Value::Array(variants));
                }
                let object = map.get("type").and_then(Value::as_str) == Some("object")
                    || map.contains_key("properties");
                if object {
                    map.insert("additionalProperties".to_owned(), Value::Bool(false));
                }
                for value in map.values_mut() {
                    visit(value);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(visit),
            _ => {}
        }
    }
    visit(&mut schema);
    schema
}

/// `schema` with every `$ref` replaced by its definition and no `$defs`
/// (for providers that read only inline schemas). Self-referencing types
/// stop inlining at a depth of 16.
pub(crate) fn inline_refs(schema: &Value) -> Value {
    fn visit(value: &Value, defs: &Map<String, Value>, depth: usize) -> Value {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(reference)) = map.get("$ref")
                    && let Some(name) = reference.strip_prefix("#/$defs/")
                    && let Some(definition) = defs.get(name)
                {
                    if depth > 16 {
                        return json!({});
                    }
                    let mut inlined = visit(definition, defs, depth + 1);
                    // Keep sibling keywords (a description next to the $ref).
                    if let Value::Object(target) = &mut inlined {
                        for (key, value) in map {
                            if key != "$ref" {
                                target.insert(key.clone(), visit(value, defs, depth));
                            }
                        }
                    }
                    return inlined;
                }
                Value::Object(
                    map.iter()
                        .filter(|(key, _)| key.as_str() != "$defs")
                        .map(|(key, value)| (key.clone(), visit(value, defs, depth)))
                        .collect(),
                )
            }
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| visit(item, defs, depth)).collect())
            }
            other => other.clone(),
        }
    }
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    visit(schema, &defs, 0)
}

/// The schema of a tool that takes no arguments.
pub(crate) fn empty_object() -> Value {
    json!({ "type": "object", "properties": {} })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    /// How a customer email reads.
    #[allow(dead_code)]
    #[derive(Debug, Deserialize, ToSchema)]
    #[serde(rename_all = "snake_case")]
    enum Sentiment {
        Positive,
        Neutral,
        AtRisk,
    }

    #[allow(dead_code)]
    #[derive(Debug, Deserialize, ToSchema)]
    struct Triage {
        sentiment: Sentiment,
        /// One sentence for the account owner.
        reason: String,
        /// How sure, from 0 to 100.
        #[schema(minimum = 0, maximum = 100)]
        confidence: Option<u8>,
    }

    #[test]
    fn schemas_carry_their_definitions_and_descriptions() {
        let schema = json_schema::<Triage>();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["sentiment"]["$ref"], "#/$defs/Sentiment");
        assert_eq!(
            schema["properties"]["reason"]["description"],
            "One sentence for the account owner."
        );
        assert_eq!(
            schema["$defs"]["Sentiment"]["enum"],
            json!(["positive", "neutral", "at_risk"])
        );
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("reason")));
        assert!(!required.contains(&json!("confidence")));
    }

    #[test]
    fn anthropic_schemas_close_objects_and_drop_constraints() {
        let schema = for_anthropic(json_schema::<Triage>());
        assert_eq!(schema["additionalProperties"], false);
        let confidence = &schema["properties"]["confidence"];
        let text = confidence.to_string();
        assert!(!text.contains("minimum") && !text.contains("maximum"), "{text}");
        assert!(!text.contains("\"format\":\"int32\""), "{text}");
        // `["integer", "null"]` became anyOf.
        assert!(confidence.get("anyOf").is_some(), "{confidence}");
    }

    #[test]
    fn refs_inline() {
        let schema = inline_refs(&json_schema::<Triage>());
        assert!(schema.get("$defs").is_none());
        assert_eq!(
            schema["properties"]["sentiment"]["enum"],
            json!(["positive", "neutral", "at_risk"])
        );
    }

    #[test]
    fn non_objects_are_wrapped() {
        let (schema, wrapped) = wrap_non_object(json_schema::<Sentiment>());
        assert!(wrapped);
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["value"]));
        assert_eq!(unwrap_value(json!({ "value": "neutral" })), json!("neutral"));
        let (_, wrapped) = wrap_non_object(json_schema::<Triage>());
        assert!(!wrapped);
    }
}
