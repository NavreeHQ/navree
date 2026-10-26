//! API resource helpers: [`ApiResource`], [`resource_json`] and [`mask_fields`].

use serde::Serialize;
use serde_json::Value;

/// The member carrying a resource's subject type (what CASL's
/// `detectSubjectType: o => o.__type` reads in the browser).
pub const TYPE_FIELD: &str = "__type";

/// A JSON API resource with a constant subject type, sent to clients as
/// `"__type"` so front-end permission checks (`<Can I="update" this={deal}>`)
/// work on plain JSON objects.
///
/// ```
/// use navis_api::{ApiResource, resource_json};
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct DealResource {
///     id: u64,
///     title: String,
/// }
///
/// impl ApiResource for DealResource {
///     const TYPE: &'static str = "Deal";
/// }
///
/// let json = resource_json(&DealResource { id: 1, title: "Renewal".into() });
/// assert_eq!(json["__type"], "Deal");
/// ```
pub trait ApiResource: Serialize {
    /// The subject type, e.g. `Deal` (the name the access rules use).
    const TYPE: &'static str;
}

/// Serializes `resource` and adds `"__type": T::TYPE`.
///
/// A resource that does not serialize to a JSON object is returned as
/// serialized, without `__type`; one that fails to serialize (a map with
/// non-string keys, a failing `Serialize` impl) becomes `null` and the error
/// is logged.
pub fn resource_json<T: ApiResource>(resource: &T) -> Value {
    match serde_json::to_value(resource) {
        Ok(Value::Object(mut object)) => {
            object.insert(TYPE_FIELD.to_owned(), Value::String(T::TYPE.to_owned()));
            Value::Object(object)
        }
        Ok(other) => other,
        Err(error) => {
            tracing::error!(
                target: "navis::api",
                resource = T::TYPE,
                %error,
                "could not serialize an API resource"
            );
            Value::Null
        }
    }
}

/// Keeps only the `allowed` top-level fields of a resource, plus `id` and
/// `__type` (field-level access control: pass the fields the user may read,
/// e.g. from `Ability::permitted_fields`). `None` keeps every field.
///
/// Arrays are masked element by element; other values are returned as is.
///
/// ```
/// use navis_api::mask_fields;
/// use serde_json::json;
///
/// let deal = json!({"id": 1, "__type": "Deal", "title": "Renewal", "amount": 1200});
/// let masked = mask_fields(deal, Some(&["title".to_owned()]));
/// assert_eq!(masked, json!({"id": 1, "__type": "Deal", "title": "Renewal"}));
/// ```
pub fn mask_fields(value: Value, allowed: Option<&[String]>) -> Value {
    let Some(allowed) = allowed else {
        return value;
    };
    match value {
        Value::Object(mut object) => {
            object.retain(|key, _| {
                key == "id" || key == TYPE_FIELD || allowed.iter().any(|field| field == key)
            });
            Value::Object(object)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| mask_fields(item, Some(allowed)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[derive(Serialize)]
    struct DealResource {
        id: u32,
        title: String,
        amount: u64,
    }

    impl ApiResource for DealResource {
        const TYPE: &'static str = "Deal";
    }

    fn deal() -> DealResource {
        DealResource {
            id: 7,
            title: "Renewal".to_owned(),
            amount: 1200,
        }
    }

    fn fields(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn resource_json_adds_type() {
        let value = resource_json(&deal());
        assert_eq!(
            value,
            json!({"id": 7, "title": "Renewal", "amount": 1200, "__type": "Deal"})
        );
    }

    #[test]
    fn mask_fields_keeps_allowed_and_id() {
        let value = resource_json(&deal());
        let masked = mask_fields(value, Some(&fields(&["title"])));
        assert_eq!(
            masked,
            json!({"id": 7, "title": "Renewal", "__type": "Deal"})
        );
    }

    #[test]
    fn mask_fields_without_a_field_list_keeps_everything() {
        let value = resource_json(&deal());
        assert_eq!(mask_fields(value.clone(), None), value);
    }

    #[test]
    fn mask_fields_masks_each_item_of_a_list() {
        let list = json!([
            {"id": 1, "__type": "Deal", "title": "A", "amount": 1},
            {"id": 2, "__type": "Deal", "title": "B", "amount": 2},
        ]);
        let masked = mask_fields(list, Some(&fields(&["amount"])));
        assert_eq!(
            masked,
            json!([
                {"id": 1, "__type": "Deal", "amount": 1},
                {"id": 2, "__type": "Deal", "amount": 2},
            ])
        );
    }

    #[test]
    fn resource_json_leaves_non_objects_untouched() {
        #[derive(Serialize)]
        struct Code(&'static str);

        impl ApiResource for Code {
            const TYPE: &'static str = "Code";
        }

        assert_eq!(resource_json(&Code("x")), json!("x"));
    }
}
