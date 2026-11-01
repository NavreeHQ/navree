//! Discriminators: which schemas have one, and the single-value property a
//! mapped schema gets so that TypeScript can tell the variants of a union
//! apart.

use std::collections::{HashMap, HashSet};

use super::json::Value;
use super::refs::{create_ref, parse_ref};

/// The discriminators of a document.
#[derive(Debug, Default)]
pub(crate) struct Discriminators {
    /// Each schema's discriminator object, by the pointer of the schema.
    pub(crate) objects: HashMap<String, Value>,
    /// The references whose schema already has its discriminator property.
    pub(crate) refs_handled: HashSet<String>,
}

/// How many references one reference may lead through.
pub(crate) const MAX_HOPS: usize = 64;

/// Where a reference leads: the keys from the root, following a chain of
/// references to the end. `None` when it does not lead anywhere.
pub(crate) fn resolve_keys(
    doc: &Value,
    reference: &str,
    visited: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Option<Vec<String>> {
    let keys = parse_ref(reference);
    if keys.is_empty() {
        return None;
    }
    let mut node = doc;
    for key in &keys {
        match node.get(key).filter(|next| next.truthy()) {
            Some(next) => node = next,
            None => {
                warn(warnings, format!("Could not resolve $ref \"{reference}\""));
                return None;
            }
        }
    }
    if let Some(next) = node
        .get("$ref")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
    {
        if visited.len() >= MAX_HOPS {
            warn(
                warnings,
                format!(
                    "Could not resolve $ref \"{reference}\": it leads through too many references"
                ),
            );
            return None;
        }
        if visited.iter().any(|seen| seen == next) {
            warn(
                warnings,
                format!("Could not resolve circular $ref \"{reference}\""),
            );
            return None;
        }
        visited.push(next.to_owned());
        return resolve_keys(doc, next, visited, warnings);
    }
    Some(keys)
}

/// The value at `keys`.
pub(crate) fn value_at<'a>(doc: &'a Value, keys: &[String]) -> Option<&'a Value> {
    keys.iter().try_fold(doc, |node, key| node.get(key))
}

fn value_at_mut<'a>(doc: &'a mut Value, keys: &[String]) -> Option<&'a mut Value> {
    let mut node = doc;
    for key in keys {
        node = match node {
            Value::Obj(_) => node.object_mut()?.get_mut(key)?,
            Value::Arr(_) => {
                let index = key.parse::<usize>().ok()?;
                node.array_mut()?.get_mut(index)?
            }
            _ => return None,
        };
    }
    Some(node)
}

/// Adds a warning once.
pub(crate) fn warn(warnings: &mut Vec<String>, message: String) {
    if warnings.len() < MAX_WARNINGS && !warnings.contains(&message) {
        warnings.push(message);
    }
}

/// How many warnings are kept.
const MAX_WARNINGS: usize = 50;

/// Every object of the document that `want` accepts, with the pointer of its
/// place, in document order (keys that are numbers first, as JavaScript lists
/// them).
fn collect(doc: &Value, want: &dyn Fn(&Value) -> bool) -> Vec<(String, Value)> {
    fn walk(
        node: &Value,
        path: &mut Vec<String>,
        want: &dyn Fn(&Value) -> bool,
        found: &mut Vec<(String, Value)>,
    ) {
        match node {
            Value::Arr(items) => {
                for (index, item) in items.iter().enumerate() {
                    path.push(index.to_string());
                    walk(item, path, want, found);
                    path.pop();
                }
            }
            Value::Obj(object) => {
                if want(node) {
                    let parts: Vec<&str> = path.iter().map(String::as_str).collect();
                    found.push((create_ref(&parts), node.clone()));
                }
                for (key, value) in object.js_entries() {
                    path.push(key.to_owned());
                    walk(value, path, want, found);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(doc, &mut Vec::new(), want, &mut found);
    found
}

fn property_name(discriminator: &Value) -> Option<&str> {
    discriminator
        .get("propertyName")
        .filter(|name| name.truthy())
        .and_then(Value::as_str)
}

/// The mapped values of a discriminator, as `(value, reference)`.
fn mapping(discriminator: &Value) -> Vec<(String, String)> {
    discriminator
        .get("mapping")
        .and_then(Value::as_object)
        .map(|mapping| {
            mapping
                .js_entries()
                .into_iter()
                .filter_map(|(value, target)| Some((value.to_owned(), target.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Finds the discriminators and gives each mapped schema its property.
pub(crate) fn scan(doc: &mut Value, warnings: &mut Vec<String>) -> Discriminators {
    let mut found = Discriminators::default();

    // First, every discriminator, and the schemas of a `oneOf` that it maps.
    let owners = collect(doc, &|node| {
        node.get("discriminator")
            .is_some_and(|discriminator| property_name(discriminator).is_some())
    });
    for (reference, owner) in owners {
        let Some(discriminator) = owner.get("discriminator").cloned() else {
            continue;
        };
        found
            .objects
            .insert(reference.clone(), discriminator.clone());
        let Some(one_of) = owner.get("oneOf").and_then(Value::as_array) else {
            continue;
        };
        // (reference, inferred value, defined values), in order of appearance.
        let mut mapped: Vec<(String, Option<String>, Option<Vec<String>>)> = Vec::new();
        let mut places: HashMap<String, usize> = HashMap::new();
        for item in one_of {
            let Some(target) = item.get("$ref").and_then(Value::as_str) else {
                continue;
            };
            let value = target.rsplit('/').next().unwrap_or("");
            if value.is_empty() {
                continue;
            }
            match places.get(target).and_then(|place| mapped.get_mut(*place)) {
                Some(entry) => entry.1 = Some(value.to_owned()),
                None => {
                    places.insert(target.to_owned(), mapped.len());
                    mapped.push((target.to_owned(), Some(value.to_owned()), None));
                }
            }
        }
        for (value, target) in mapping(&discriminator) {
            if target.is_empty() {
                continue;
            }
            let place = match places.get(&target) {
                Some(place) => *place,
                None => {
                    places.insert(target.clone(), mapped.len());
                    mapped.push((target.clone(), None, None));
                    mapped.len() - 1
                }
            };
            if let Some(entry) = mapped.get_mut(place) {
                if entry.2.is_none() {
                    *entry = (target.clone(), None, Some(Vec::new()));
                }
                if let Some(defined) = entry.2.as_mut() {
                    defined.push(value);
                }
            }
        }
        for (target, inferred, defined) in mapped {
            if found.refs_handled.contains(&target) {
                continue;
            }
            let values = match (defined, inferred) {
                (Some(defined), _) => defined,
                (None, Some(inferred)) => vec![inferred],
                (None, None) => continue,
            };
            if patch(doc, &target, &values, &discriminator, &reference, warnings) {
                found.refs_handled.insert(target);
            }
        }
    }

    // Second, the schemas that inherit from a discriminated one.
    let inheriting = collect(doc, &|node| {
        node.get("allOf")
            .is_some_and(|all| all.as_array().is_some())
    });
    for (reference, owner) in inheriting {
        let Some(all_of) = owner.get("allOf").and_then(Value::as_array) else {
            continue;
        };
        for item in all_of {
            if !item.has("$ref") {
                if let Some(discriminator) = item
                    .get("discriminator")
                    .filter(|discriminator| property_name(discriminator).is_some())
                {
                    found
                        .objects
                        .insert(reference.clone(), discriminator.clone());
                }
                continue;
            }
            let Some(parent) = item.get("$ref").and_then(Value::as_str) else {
                continue;
            };
            let Some(discriminator) = found.objects.get(parent).cloned() else {
                break;
            };
            let values: Vec<String> = mapping(&discriminator)
                .into_iter()
                .filter(|(_, target)| *target == reference)
                .map(|(value, _)| value)
                .collect();
            if !values.is_empty()
                && patch(doc, &reference, &values, &discriminator, parent, warnings)
            {
                found.refs_handled.insert(reference.clone());
            }
            found.objects.insert(reference.clone(), discriminator);
        }
    }
    found
}

/// A string schema whose only values are `values`.
fn discriminator_enum(values: &[String], previous: Option<&Value>) -> Value {
    let description = previous
        .and_then(|schema| schema.get("description"))
        .filter(|description| description.truthy())
        .map_or_else(
            || "discriminator enum property added by navree openapi:client".to_owned(),
            |description| {
                format!(
                    "{} (enum property replaced by navree openapi:client)",
                    description.js_string()
                )
            },
        );
    Value::object([
        ("type", Value::string("string")),
        (
            "enum",
            Value::Arr(std::rc::Rc::new(
                values.iter().map(|v| Value::string(v)).collect(),
            )),
        ),
        ("description", Value::string(&description)),
    ])
}

/// Adds the discriminator property, holding `values`, to the schema `target`
/// refers to. Whether the schema could take it.
fn patch(
    doc: &mut Value,
    target: &str,
    values: &[String],
    discriminator: &Value,
    discriminator_ref: &str,
    warnings: &mut Vec<String>,
) -> bool {
    let Some(name) = property_name(discriminator).map(str::to_owned) else {
        return false;
    };
    let keys = resolve_keys(doc, target, &mut Vec::new(), warnings);
    let node = keys.as_ref().and_then(|keys| value_at_mut(doc, keys));
    if let Some(node) = node {
        if node.get("allOf").is_some_and(Value::truthy)
            && let Some(object) = node.object_mut()
            && let Some(all_of) = object.get_mut("allOf").and_then(Value::array_mut)
        {
            all_of.push(Value::object([
                ("type", Value::string("object")),
                (
                    "required",
                    Value::Arr(std::rc::Rc::new(vec![Value::string(&name)])),
                ),
                (
                    "properties",
                    Value::object([(name.as_str(), discriminator_enum(values, None))]),
                ),
            ]));
            return true;
        }
        if node.get("type").and_then(Value::as_str) == Some("object")
            && let Some(object) = node.object_mut()
        {
            if !object.get("properties").is_some_and(Value::truthy) {
                object.set("properties", Value::object([]));
            }
            match object.get_mut("required") {
                Some(required) if required.truthy() => {
                    if let Some(list) = required.array_mut()
                        && !list.iter().any(|item| item.as_str() == Some(name.as_str()))
                    {
                        list.push(Value::string(&name));
                    }
                }
                _ => object.set(
                    "required",
                    Value::Arr(std::rc::Rc::new(vec![Value::string(&name)])),
                ),
            }
            if let Some(properties) = object.get_mut("properties").and_then(Value::object_mut) {
                let replacement = discriminator_enum(values, properties.get(&name));
                properties.set(&name, replacement);
            }
            return true;
        }
    }
    warn(
        warnings,
        format!(
            "Discriminator mapping has an invalid schema (neither an object schema nor an allOf array): {target} => {} (Discriminator: {discriminator_ref})",
            values.join(", ")
        ),
    );
    false
}
