//! SCIM PATCH (RFC 7644 §3.5.2), applied to a resource's JSON.
//!
//! Besides the RFC, it accepts what identity providers send in practice:
//! operation names in any case (`Replace`), path-less values whose keys are
//! paths (`{"name.givenName": "Ada"}`), `remove` on `members` with a value
//! array naming the members to remove, and `add` or `replace` with a value
//! filter that matches nothing (`emails[type eq "work"].value`), which adds
//! the element the filter describes.

use serde_json::{Map, Value};

use super::filter::{AttrPath, CompareOp, Filter, PatchPath, extension_schema, get_ci};

/// The `schemas` value of a PATCH request.
pub(crate) const PATCH_OP_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";

/// A PATCH operation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PatchOp {
    pub(crate) op: OpKind,
    pub(crate) path: Option<PatchPath>,
    pub(crate) value: Option<Value>,
}

/// The kind of a PATCH operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpKind {
    Add,
    Remove,
    Replace,
}

/// Why a PATCH request was refused, with its SCIM `scimType`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PatchError {
    pub(crate) scim_type: &'static str,
    pub(crate) detail: String,
}

impl PatchError {
    fn new(scim_type: &'static str, detail: impl Into<String>) -> Self {
        Self {
            scim_type,
            detail: detail.into(),
        }
    }
}

/// The operations of a PATCH request body.
pub(crate) fn parse(body: &Value) -> Result<Vec<PatchOp>, PatchError> {
    let Some(object) = body.as_object() else {
        return Err(PatchError::new("invalidSyntax", "the body must be a JSON object"));
    };
    if let Some(schemas) = get_ci(body, "schemas")
        && !schemas.as_array().is_some_and(|schemas| {
            schemas
                .iter()
                .any(|schema| schema.as_str() == Some(PATCH_OP_SCHEMA))
        })
    {
        return Err(PatchError::new(
            "invalidSyntax",
            format!("schemas must contain {PATCH_OP_SCHEMA}"),
        ));
    }
    let operations = object
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("Operations"))
        .map(|(_, value)| value)
        .and_then(Value::as_array)
        .ok_or_else(|| PatchError::new("invalidSyntax", "Operations must be an array"))?;
    let mut ops = Vec::with_capacity(operations.len());
    for operation in operations {
        let op = get_ci(operation, "op")
            .and_then(Value::as_str)
            .ok_or_else(|| PatchError::new("invalidSyntax", "each operation needs an op"))?;
        let op = match op.to_ascii_lowercase().as_str() {
            "add" => OpKind::Add,
            "remove" => OpKind::Remove,
            "replace" => OpKind::Replace,
            other => {
                return Err(PatchError::new(
                    "invalidSyntax",
                    format!("`{other}` is not a PATCH operation"),
                ));
            }
        };
        let path = match get_ci(operation, "path") {
            None | Some(Value::Null) => None,
            Some(Value::String(path)) if path.trim().is_empty() => None,
            Some(Value::String(path)) => Some(
                PatchPath::parse(path)
                    .map_err(|error| PatchError::new("invalidPath", error.0))?,
            ),
            Some(_) => return Err(PatchError::new("invalidPath", "path must be a string")),
        };
        let value = get_ci(operation, "value").cloned();
        match op {
            OpKind::Remove if path.is_none() => {
                return Err(PatchError::new("noTarget", "remove needs a path"));
            }
            OpKind::Add | OpKind::Replace if value.is_none() => {
                return Err(PatchError::new("invalidValue", "add and replace need a value"));
            }
            _ => {}
        }
        ops.push(PatchOp { op, path, value });
    }
    Ok(ops)
}

/// Applies `ops` to `resource` in order. Any failing operation fails the
/// whole PATCH (the caller keeps the unchanged resource).
pub(crate) fn apply(resource: &mut Value, ops: &[PatchOp]) -> Result<(), PatchError> {
    if !resource.is_object() {
        *resource = Value::Object(Map::new());
    }
    for op in ops {
        match &op.path {
            Some(path) => apply_at(resource, op.op, path, op.value.as_ref())?,
            None => {
                let Some(Value::Object(values)) = &op.value else {
                    return Err(PatchError::new(
                        "invalidValue",
                        "a PATCH operation without a path needs an object value",
                    ));
                };
                for (key, value) in values {
                    let path = PatchPath::parse(key).map_err(|error| {
                        PatchError::new("invalidPath", format!("`{key}`: {}", error.0))
                    })?;
                    apply_at(resource, op.op, &path, Some(value))?;
                }
            }
        }
    }
    Ok(())
}

fn apply_at(
    resource: &mut Value,
    op: OpKind,
    path: &PatchPath,
    value: Option<&Value>,
) -> Result<(), PatchError> {
    let attr = &path.attr;
    // A path naming an extension schema alone (`urn:...:enterprise:2.0:User`)
    // targets the extension object: each attribute of an object value is
    // an operation of its own, and `remove` drops the whole extension.
    if path.filter.is_none()
        && let Some(schema) = extension_schema(attr)
    {
        if op == OpKind::Remove {
            let root = object_mut(resource);
            if let Some(key) = key_ci(root, schema) {
                root.remove(&key);
            }
            return Ok(());
        }
        let Some(Value::Object(values)) = value else {
            return Err(PatchError::new(
                "invalidValue",
                format!("{schema} takes an object value"),
            ));
        };
        for (name, value) in values {
            let inner = PatchPath::parse(&format!("{schema}:{name}"))
                .map_err(|error| PatchError::new("invalidPath", error.0))?;
            apply_at(resource, op, &inner, Some(value))?;
        }
        return Ok(());
    }
    let container = container_mut(resource, attr.urn.as_deref());
    match (&path.filter, op) {
        (None, OpKind::Remove) => {
            remove_plain(container, attr, value);
            Ok(())
        }
        (None, _) => {
            let value = value.cloned().unwrap_or(Value::Null);
            set_plain(container, attr, op, value)
        }
        (Some(filter), OpKind::Remove) => {
            remove_filtered(container, attr, filter, path.sub.as_deref());
            Ok(())
        }
        (Some(filter), _) => {
            let value = value.cloned().unwrap_or(Value::Null);
            set_filtered(container, attr, filter, path.sub.as_deref(), value)
        }
    }
}

/// The object attributes of schema `urn` live in, created when missing.
fn container_mut<'a>(resource: &'a mut Value, urn: Option<&str>) -> &'a mut Map<String, Value> {
    let root = object_mut(resource);
    match urn {
        None => root,
        Some(urn) => {
            let key = key_ci(root, urn).unwrap_or_else(|| urn.to_owned());
            object_mut(root.entry(key).or_insert_with(|| Value::Object(Map::new())))
        }
    }
}

/// `value` as an object, replacing anything else with an empty one.
fn object_mut(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("just made an object")
}

/// The existing key of `map` that equals `name` without regard to case.
fn key_ci(map: &Map<String, Value>, name: &str) -> Option<String> {
    if map.contains_key(name) {
        return Some(name.to_owned());
    }
    map.keys().find(|key| key.eq_ignore_ascii_case(name)).cloned()
}

fn set_plain(
    container: &mut Map<String, Value>,
    attr: &AttrPath,
    op: OpKind,
    value: Value,
) -> Result<(), PatchError> {
    let key = key_ci(container, &attr.name).unwrap_or_else(|| attr.name.clone());
    match &attr.sub {
        Some(sub) => {
            let target = container
                .entry(key)
                .or_insert_with(|| Value::Object(Map::new()));
            match target {
                Value::Object(object) => {
                    let sub_key = key_ci(object, sub).unwrap_or_else(|| sub.clone());
                    object.insert(sub_key, value);
                    Ok(())
                }
                Value::Array(_) => Err(PatchError::new(
                    "invalidPath",
                    format!("`{attr}` names a sub-attribute of a multi-valued attribute without a value filter"),
                )),
                other => {
                    let mut object = Map::new();
                    object.insert(sub.clone(), value);
                    *other = Value::Object(object);
                    Ok(())
                }
            }
        }
        None => {
            match (container.get_mut(&key), op, value) {
                // add to a multi-valued attribute: append what is new.
                (Some(Value::Array(existing)), OpKind::Add, value) => {
                    let values = match value {
                        Value::Array(values) => values,
                        value => vec![value],
                    };
                    for value in values {
                        if !existing.contains(&value) {
                            existing.push(value);
                        }
                    }
                }
                // add or replace on a complex attribute: the given
                // sub-attributes replace theirs, the others stay.
                (Some(Value::Object(existing)), _, Value::Object(values)) => {
                    for (sub, value) in values {
                        let sub_key = key_ci(existing, &sub).unwrap_or(sub);
                        existing.insert(sub_key, value);
                    }
                }
                (_, _, value) => {
                    container.insert(key, value);
                }
            }
            Ok(())
        }
    }
}

fn remove_plain(container: &mut Map<String, Value>, attr: &AttrPath, value: Option<&Value>) {
    let Some(key) = key_ci(container, &attr.name) else {
        return;
    };
    match &attr.sub {
        Some(sub) => match container.get_mut(&key) {
            Some(Value::Object(object)) => {
                if let Some(sub_key) = key_ci(object, sub) {
                    object.remove(&sub_key);
                }
            }
            Some(Value::Array(items)) => {
                for item in items.iter_mut() {
                    if let Value::Object(object) = item
                        && let Some(sub_key) = key_ci(object, sub)
                    {
                        object.remove(&sub_key);
                    }
                }
            }
            _ => {}
        },
        None => {
            // `remove members` with a value array removes those members.
            if let (Some(Value::Array(remove)), Some(Value::Array(items))) =
                (value, container.get_mut(&key))
            {
                let gone: Vec<&Value> = remove
                    .iter()
                    .map(|item| get_ci(item, "value").unwrap_or(item))
                    .collect();
                items.retain(|item| {
                    let id = get_ci(item, "value").unwrap_or(item);
                    !gone.contains(&id)
                });
                return;
            }
            container.remove(&key);
        }
    }
}

fn remove_filtered(
    container: &mut Map<String, Value>,
    attr: &AttrPath,
    filter: &Filter,
    sub: Option<&str>,
) {
    let Some(key) = key_ci(container, &attr.name) else {
        return;
    };
    let Some(Value::Array(items)) = container.get_mut(&key) else {
        return;
    };
    match sub {
        Some(sub) => {
            for item in items.iter_mut().filter(|item| filter.matches(item)) {
                if let Value::Object(object) = item
                    && let Some(sub_key) = key_ci(object, sub)
                {
                    object.remove(&sub_key);
                }
            }
        }
        None => {
            items.retain(|item| !filter.matches(item));
            if items.is_empty() {
                container.remove(&key);
            }
        }
    }
}

fn set_filtered(
    container: &mut Map<String, Value>,
    attr: &AttrPath,
    filter: &Filter,
    sub: Option<&str>,
    value: Value,
) -> Result<(), PatchError> {
    let key = key_ci(container, &attr.name).unwrap_or_else(|| attr.name.clone());
    let items = container
        .entry(key)
        .or_insert_with(|| Value::Array(Vec::new()));
    if !items.is_array() {
        *items = Value::Array(Vec::new());
    }
    let items = items.as_array_mut().expect("just made an array");
    let mut matched = false;
    for item in items.iter_mut().filter(|item| filter.matches(item)) {
        matched = true;
        merge_into(item, sub, value.clone());
    }
    if matched {
        return Ok(());
    }
    // Nothing matched: add the element the filter describes, when it is a
    // plain `attr eq value` conjunction.
    let Some(mut element) = described_by(filter) else {
        return Err(PatchError::new(
            "noTarget",
            format!("no value of `{attr}` matches the filter"),
        ));
    };
    merge_into(&mut element, sub, value);
    items.push(element);
    Ok(())
}

/// Sets `sub` of `item` to `value`, or merges an object `value` into it.
fn merge_into(item: &mut Value, sub: Option<&str>, value: Value) {
    let object = object_mut(item);
    match (sub, value) {
        (Some(sub), value) => {
            let sub_key = key_ci(object, sub).unwrap_or_else(|| sub.to_owned());
            object.insert(sub_key, value);
        }
        (None, Value::Object(values)) => {
            for (sub, value) in values {
                let sub_key = key_ci(object, &sub).unwrap_or(sub);
                object.insert(sub_key, value);
            }
        }
        (None, value) => {
            object.insert("value".to_owned(), value);
        }
    }
}

/// The element a filter of `sub eq value` terms (joined by `and`)
/// describes: `type eq "work"` describes `{"type": "work"}`.
fn described_by(filter: &Filter) -> Option<Value> {
    fn collect(filter: &Filter, into: &mut Map<String, Value>) -> bool {
        match filter {
            Filter::Compare {
                path:
                    AttrPath {
                        urn: None,
                        name,
                        sub: None,
                    },
                op: CompareOp::Eq,
                value,
            } => {
                into.insert(name.clone(), value.clone());
                true
            }
            Filter::And(a, b) => collect(a, into) && collect(b, into),
            _ => false,
        }
    }
    let mut element = Map::new();
    collect(filter, &mut element).then_some(Value::Object(element))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ENTERPRISE: &str = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User";

    fn patch(resource: Value, operations: Value) -> Result<Value, PatchError> {
        let ops = parse(&json!({
            "schemas": [PATCH_OP_SCHEMA],
            "Operations": operations
        }))?;
        let mut resource = resource;
        apply(&mut resource, &ops)?;
        Ok(resource)
    }

    fn ada() -> Value {
        json!({
            "userName": "ada@acme.test",
            "active": true,
            "name": { "givenName": "Ada", "familyName": "Lovelace" },
            "emails": [{ "value": "ada@acme.test", "type": "work", "primary": true }]
        })
    }

    #[test]
    fn replace_sets_simple_and_merges_complex_attributes() {
        let user = patch(
            ada(),
            json!([
                { "op": "replace", "path": "active", "value": false },
                { "op": "Replace", "path": "name", "value": { "familyName": "King" } },
                { "op": "replace", "path": "name.middleName", "value": "Augusta" }
            ]),
        )
        .unwrap();
        assert_eq!(user["active"], json!(false));
        assert_eq!(
            user["name"],
            json!({ "givenName": "Ada", "familyName": "King", "middleName": "Augusta" })
        );
    }

    #[test]
    fn path_less_operations_take_an_object_whose_keys_are_paths() {
        let mut extension = Map::new();
        extension.insert(format!("{ENTERPRISE}:department"), json!("Mathematics"));
        extension.insert(ENTERPRISE.to_owned(), json!({ "employeeNumber": "1815" }));
        let user = patch(
            ada(),
            json!([
                { "op": "replace", "value": {
                    "active": false,
                    "name.givenName": "Augusta",
                    "title": "Analyst"
                } },
                { "op": "add", "value": Value::Object(extension) },
                { "op": "replace", "path": ENTERPRISE, "value": { "costCenter": "4130" } }
            ]),
        )
        .unwrap();
        assert_eq!(user["active"], json!(false));
        assert_eq!(user["name"]["givenName"], json!("Augusta"));
        assert_eq!(user["title"], json!("Analyst"));
        assert_eq!(user[ENTERPRISE]["department"], json!("Mathematics"));
        assert_eq!(user[ENTERPRISE]["employeeNumber"], json!("1815"));
        assert_eq!(user[ENTERPRISE]["costCenter"], json!("4130"));
        let user = patch(user, json!([{ "op": "remove", "path": ENTERPRISE }])).unwrap();
        assert!(user.get(ENTERPRISE).is_none());
    }

    #[test]
    fn add_appends_to_multi_valued_attributes_without_duplicates() {
        let user = patch(
            ada(),
            json!([{ "op": "add", "path": "emails", "value": [
                { "value": "ada@acme.test", "type": "work", "primary": true },
                { "value": "ada@home.test", "type": "home" }
            ] }]),
        )
        .unwrap();
        assert_eq!(user["emails"].as_array().unwrap().len(), 2);
        assert_eq!(user["emails"][1]["value"], json!("ada@home.test"));
    }

    #[test]
    fn filtered_paths_change_the_matching_values() {
        let user = patch(
            ada(),
            json!([{ "op": "replace", "path": "emails[type eq \"work\"].value", "value": "lovelace@acme.test" }]),
        )
        .unwrap();
        assert_eq!(
            user["emails"],
            json!([{ "value": "lovelace@acme.test", "type": "work", "primary": true }])
        );
        let user = patch(
            user,
            json!([{ "op": "replace", "path": "emails[type eq \"work\"]", "value": { "primary": false } }]),
        )
        .unwrap();
        assert_eq!(user["emails"][0]["primary"], json!(false));
        assert_eq!(user["emails"][0]["type"], json!("work"));
    }

    #[test]
    fn a_filter_that_matches_nothing_adds_the_element_it_describes() {
        let user = patch(
            ada(),
            json!([
                { "op": "add", "path": "phoneNumbers[type eq \"mobile\"].value", "value": "+44 20 7946 0958" },
                { "op": "replace", "path": "emails[type eq \"home\"].value", "value": "ada@home.test" }
            ]),
        )
        .unwrap();
        assert_eq!(
            user["phoneNumbers"],
            json!([{ "type": "mobile", "value": "+44 20 7946 0958" }])
        );
        assert_eq!(user["emails"][1], json!({ "type": "home", "value": "ada@home.test" }));
        let error = patch(
            ada(),
            json!([{ "op": "replace", "path": "emails[value co \"x\"].type", "value": "home" }]),
        )
        .unwrap_err();
        assert_eq!(error.scim_type, "noTarget");
    }

    #[test]
    fn remove_takes_attributes_sub_attributes_and_matching_values() {
        let user = patch(
            ada(),
            json!([
                { "op": "remove", "path": "name.familyName" },
                { "op": "remove", "path": "emails[type eq \"work\"]" },
                { "op": "remove", "path": "nickName" }
            ]),
        )
        .unwrap();
        assert_eq!(user["name"], json!({ "givenName": "Ada" }));
        assert!(user.get("emails").is_none());
        let user = patch(ada(), json!([{ "op": "remove", "path": "active" }])).unwrap();
        assert!(user.get("active").is_none());
    }

    #[test]
    fn group_members_are_added_and_removed_by_value() {
        let group = json!({
            "displayName": "Engineering",
            "members": [{ "value": "u1" }, { "value": "u2" }, { "value": "u3" }]
        });
        let group = patch(
            group,
            json!([
                { "op": "Add", "path": "members", "value": [{ "value": "u4" }, { "value": "u1" }] },
                { "op": "Remove", "path": "members[value eq \"u2\"]" },
                { "op": "Remove", "path": "members", "value": [{ "value": "u3" }] }
            ]),
        )
        .unwrap();
        assert_eq!(
            group["members"],
            json!([{ "value": "u1" }, { "value": "u4" }])
        );
        let group = patch(group, json!([{ "op": "remove", "path": "members" }])).unwrap();
        assert!(group.get("members").is_none());
        let group = patch(
            group,
            json!([{ "op": "replace", "path": "members", "value": [{ "value": "u9" }] }]),
        )
        .unwrap();
        assert_eq!(group["members"], json!([{ "value": "u9" }]));
    }

    #[test]
    fn attribute_names_match_without_regard_to_case() {
        let user = patch(
            ada(),
            json!([{ "op": "replace", "path": "NAME.GIVENNAME", "value": "Augusta" }]),
        )
        .unwrap();
        assert_eq!(user["name"]["givenName"], json!("Augusta"));
        assert!(user["name"].get("GIVENNAME").is_none());
    }

    #[test]
    fn malformed_requests_are_refused_with_their_scim_type() {
        let bad = |body: Value| parse(&body).unwrap_err().scim_type;
        assert_eq!(bad(json!([])), "invalidSyntax");
        assert_eq!(
            bad(json!({ "schemas": ["urn:x"], "Operations": [] })),
            "invalidSyntax"
        );
        assert_eq!(bad(json!({ "Operations": "x" })), "invalidSyntax");
        assert_eq!(bad(json!({ "Operations": [{ "path": "x" }] })), "invalidSyntax");
        assert_eq!(
            bad(json!({ "Operations": [{ "op": "move", "path": "x" }] })),
            "invalidSyntax"
        );
        assert_eq!(bad(json!({ "Operations": [{ "op": "remove" }] })), "noTarget");
        assert_eq!(
            bad(json!({ "Operations": [{ "op": "add", "path": "title" }] })),
            "invalidValue"
        );
        assert_eq!(
            bad(json!({ "Operations": [{ "op": "add", "path": "emails[", "value": 1 }] })),
            "invalidPath"
        );
        // A schemas list is optional; Operations is not.
        assert!(parse(&json!({ "Operations": [] })).unwrap().is_empty());
        let error = patch(
            ada(),
            json!([{ "op": "replace", "path": "emails.value", "value": "x" }]),
        )
        .unwrap_err();
        assert_eq!(error.scim_type, "invalidPath");
        let error = patch(ada(), json!([{ "op": "add", "value": "x" }])).unwrap_err();
        assert_eq!(error.scim_type, "invalidValue");
    }
}
