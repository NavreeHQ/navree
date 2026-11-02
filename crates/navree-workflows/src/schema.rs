//! JSON Schemas from `ToSchema` (the derive request bodies use),
//! for the block catalog an automation editor builds its forms from.

use navree_openapi::ToSchema;
use serde_json::{Map, Value, json};

/// A self-contained JSON Schema for `T`: its own schema (field doc
/// comments become descriptions) with the schemas it refers to under
/// `$defs` (`#/components/schemas/Stage` becomes `#/$defs/Stage`).
///
/// # Panics
///
/// When two different types under `T` have one schema name, which a `$ref`
/// couldn't tell apart; the message names both.
///
/// ```
/// use navree_workflows::json_schema;
///
/// #[derive(navree_openapi::ToSchema)]
/// #[allow(dead_code)]
/// struct SendEmail {
///     /// The template to render.
///     template: String,
/// }
///
/// let schema = json_schema::<SendEmail>();
/// assert_eq!(schema["type"], "object");
/// assert_eq!(schema["properties"]["template"]["description"], "The template to render.");
/// ```
pub fn json_schema<T: ToSchema>() -> Value {
    let mut schema = serde_json::to_value(T::schema()).unwrap_or_else(|_| json!({}));
    let referenced = navree_openapi::components_of::<T>();
    rewrite_refs(&mut schema);
    if !referenced.is_empty() {
        let defs: Map<String, Value> = referenced
            .into_iter()
            .map(|(name, component)| {
                let mut component = serde_json::to_value(component).unwrap_or_else(|_| json!({}));
                rewrite_refs(&mut component);
                (name, component)
            })
            .collect();
        if let Some(object) = schema.as_object_mut() {
            object.insert("$defs".to_owned(), Value::Object(defs));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(ToSchema)]
    #[allow(dead_code)]
    enum Stage {
        Lead,
        Won,
    }

    #[derive(ToSchema)]
    #[allow(dead_code)]
    struct MovedTo {
        /// The stage.
        stage: Stage,
    }

    #[test]
    fn referenced_schemas_are_inlined_under_defs() {
        let schema = json_schema::<MovedTo>();
        assert_eq!(schema["properties"]["stage"]["$ref"], "#/$defs/Stage");
        assert!(schema["$defs"]["Stage"].is_object());
    }
}
