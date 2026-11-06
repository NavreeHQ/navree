//! SCIM resources: a client's User or Group JSON read into the normalized
//! [`DirectoryUser`] and [`DirectoryGroup`], resources rendered back with
//! their `id` and `meta`, attribute projection, and the discovery
//! documents (`ServiceProviderConfig`, `ResourceTypes`, `Schemas`).
//!
//! The resource a client writes is kept as it wrote it (without `id`,
//! `meta`, `groups` and `password`), so it reads back unchanged; the
//! normalized user is derived from it on every write.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::ScimError;
use super::filter::{ENTERPRISE_USER_SCHEMA, GROUP_SCHEMA, USER_SCHEMA, get_ci};
use crate::user::{ContactValue, DirectoryGroup, DirectoryUser};

/// The `schemas` of a list response.
pub(crate) const LIST_RESPONSE_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
/// The `schemas` of an error.
pub(crate) const ERROR_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
const SERVICE_PROVIDER_CONFIG_SCHEMA: &str =
    "urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig";
const RESOURCE_TYPE_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:ResourceType";
const SCHEMA_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:Schema";

/// Attributes the server owns: never stored from a client's resource.
const SERVER_OWNED: [&str; 3] = ["id", "meta", "groups"];

/// Top-level user attributes with a field of their own.
const USER_FIELDS: [&str; 10] = [
    "schemas",
    "externalId",
    "userName",
    "active",
    "name",
    "displayName",
    "emails",
    "phoneNumbers",
    "title",
    "locale",
];

/// A client's resource without the attributes the server owns, and
/// without `password` (never stored).
pub(crate) fn stored_resource(input: &Value) -> Result<Value, ScimError> {
    let Some(object) = input.as_object() else {
        return Err(ScimError::invalid_syntax(
            "the resource must be a JSON object",
        ));
    };
    let kept: Map<String, Value> = object
        .iter()
        .filter(|(key, _)| {
            !SERVER_OWNED
                .iter()
                .any(|owned| key.eq_ignore_ascii_case(owned))
                && !key.eq_ignore_ascii_case("password")
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Ok(Value::Object(kept))
}

/// The normalized user of a stored User resource. `external_id` is the
/// client's `externalId`, or `None` when it sent none (the caller uses
/// the SCIM id).
pub(crate) fn user_from_resource(resource: &Value) -> Result<(DirectoryUser, bool), ScimError> {
    let user_name = string(resource, "userName")?
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| ScimError::invalid_value("userName is required"))?;
    let external_id = string(resource, "externalId")?.filter(|id| !id.is_empty());
    let has_external_id = external_id.is_some();
    let active = match get_ci(resource, "active") {
        None | Some(Value::Null) => true,
        Some(value) => {
            boolean(value).ok_or_else(|| ScimError::invalid_value("active must be a boolean"))?
        }
    };
    let mut attributes = BTreeMap::new();
    let (given_name, family_name) = match get_ci(resource, "name") {
        None | Some(Value::Null) => (None, None),
        Some(Value::Object(name)) => {
            let mut rest = name.clone();
            let given = take_string(&mut rest, "givenName");
            let family = take_string(&mut rest, "familyName");
            if !rest.is_empty() {
                attributes.insert("name".to_owned(), Value::Object(rest));
            }
            (given, family)
        }
        Some(_) => return Err(ScimError::invalid_value("name must be an object")),
    };
    let mut user = DirectoryUser {
        external_id: external_id.unwrap_or_default(),
        user_name,
        active,
        given_name,
        family_name,
        display_name: string(resource, "displayName")?,
        emails: contacts(resource, "emails")?,
        phone_numbers: contacts(resource, "phoneNumbers")?,
        title: string(resource, "title")?,
        locale: match string(resource, "locale")? {
            Some(locale) => Some(locale),
            None => string(resource, "preferredLanguage")?,
        },
        ..DirectoryUser::default()
    };
    if let Some(object) = resource.as_object() {
        for (key, value) in object {
            if key.eq_ignore_ascii_case(ENTERPRISE_USER_SCHEMA) {
                let Value::Object(extension) = value else {
                    return Err(ScimError::invalid_value(format!(
                        "{ENTERPRISE_USER_SCHEMA} must be an object"
                    )));
                };
                let mut rest = extension.clone();
                user.employee_number = take_string(&mut rest, "employeeNumber");
                user.department = take_string(&mut rest, "department");
                user.organization = take_string(&mut rest, "organization");
                user.manager_id = match take(&mut rest, "manager") {
                    Some(Value::Object(manager)) => manager
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("value"))
                        .and_then(|(_, v)| v.as_str())
                        .filter(|v| !v.is_empty())
                        .map(str::to_owned),
                    Some(Value::String(id)) if !id.is_empty() => Some(id),
                    _ => None,
                };
                if !rest.is_empty() {
                    attributes.insert(ENTERPRISE_USER_SCHEMA.to_owned(), Value::Object(rest));
                }
            } else if !USER_FIELDS
                .iter()
                .any(|field| key.eq_ignore_ascii_case(field))
            {
                attributes.insert(key.clone(), value.clone());
            }
        }
    }
    user.time_zone = attributes
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("timezone"))
        .and_then(|(_, value)| value.as_str())
        .map(str::to_owned);
    user.attributes = attributes;
    Ok((user, has_external_id))
}

/// The normalized group of a stored Group resource, and its members' SCIM
/// ids (`members[].value`).
pub(crate) fn group_from_resource(
    resource: &Value,
) -> Result<(DirectoryGroup, Vec<String>), ScimError> {
    let display_name = string(resource, "displayName")?
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| ScimError::invalid_value("displayName is required"))?;
    let members = match get_ci(resource, "members") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(members)) => {
            let mut ids = Vec::new();
            for member in members {
                let id = match member {
                    Value::Object(_) => get_ci(member, "value").and_then(Value::as_str),
                    Value::String(id) => Some(id.as_str()),
                    _ => None,
                }
                .ok_or_else(|| ScimError::invalid_value("each member needs a value"))?;
                if !ids.iter().any(|known| known == id) {
                    ids.push(id.to_owned());
                }
            }
            ids
        }
        Some(_) => return Err(ScimError::invalid_value("members must be an array")),
    };
    let mut attributes = BTreeMap::new();
    if let Some(object) = resource.as_object() {
        for (key, value) in object {
            if ![
                "schemas",
                "externalId",
                "displayName",
                "members",
                "description",
            ]
            .iter()
            .any(|field| key.eq_ignore_ascii_case(field))
            {
                attributes.insert(key.clone(), value.clone());
            }
        }
    }
    let group = DirectoryGroup {
        external_id: string(resource, "externalId")?.unwrap_or_default(),
        display_name,
        description: string(resource, "description")?,
        email: None,
        members: Vec::new(),
        attributes,
    };
    Ok((group, members))
}

/// What a rendered resource carries besides the stored attributes.
pub(crate) struct Meta<'a> {
    pub(crate) id: &'a str,
    pub(crate) resource_type: &'static str,
    pub(crate) created: chrono::DateTime<chrono::Utc>,
    pub(crate) last_modified: chrono::DateTime<chrono::Utc>,
    pub(crate) location: String,
    pub(crate) version: &'a str,
}

/// A stored resource with its `id`, `meta` and (for users) `groups`, its
/// `schemas` completed.
pub(crate) fn render(stored: &Value, meta: &Meta<'_>, extra: Vec<(&'static str, Value)>) -> Value {
    let mut object = stored.as_object().cloned().unwrap_or_default();
    let core = if meta.resource_type == "Group" {
        GROUP_SCHEMA
    } else {
        USER_SCHEMA
    };
    // The core schema, then each extension the resource carries.
    let mut schemas: Vec<Value> = vec![Value::String(core.to_owned())];
    for (key, value) in &object {
        if key.len() > 4 && key[..4].eq_ignore_ascii_case("urn:") && value.is_object() {
            schemas.push(Value::String(key.clone()));
        }
    }
    object.retain(|key, _| !key.eq_ignore_ascii_case("schemas"));
    let mut rendered = Map::new();
    rendered.insert("schemas".to_owned(), Value::Array(schemas));
    rendered.insert("id".to_owned(), Value::String(meta.id.to_owned()));
    rendered.extend(object);
    for (key, value) in extra {
        rendered.insert(key.to_owned(), value);
    }
    rendered.insert(
        "meta".to_owned(),
        json!({
            "resourceType": meta.resource_type,
            "created": meta.created.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "lastModified": meta.last_modified.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "location": meta.location,
            "version": format!("W/\"{}\"", meta.version),
        }),
    );
    Value::Object(rendered)
}

/// Which attributes a response carries (`attributes`,
/// `excludedAttributes`): top-level names and `attr.sub` paths, matched
/// without regard to case. `id` and `schemas` are always returned.
#[derive(Clone, Debug, Default)]
pub(crate) struct Projection {
    attributes: Vec<String>,
    excluded: Vec<String>,
}

impl Projection {
    pub(crate) fn new(attributes: Option<&str>, excluded: Option<&str>) -> Self {
        let list = |text: Option<&str>| -> Vec<String> {
            text.map(|text| {
                text.split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(|name| strip_core_urn(name).to_owned())
                    .collect()
            })
            .unwrap_or_default()
        };
        Self {
            attributes: list(attributes),
            excluded: list(excluded),
        }
    }

    /// Whether `name` is excluded (`excludedAttributes=members`).
    pub(crate) fn excludes(&self, name: &str) -> bool {
        self.excluded
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(name))
    }

    pub(crate) fn apply(&self, resource: Value) -> Value {
        let Value::Object(mut object) = resource else {
            return resource;
        };
        let always = |key: &str| key == "id" || key == "schemas";
        if !self.attributes.is_empty() {
            let mut kept = Map::new();
            for (key, value) in object {
                if always(&key) {
                    kept.insert(key, value);
                    continue;
                }
                let wanted: Vec<Option<&str>> = self
                    .attributes
                    .iter()
                    .filter_map(|path| {
                        let (name, sub) = split_path(path);
                        name.eq_ignore_ascii_case(&key).then_some(sub)
                    })
                    .collect();
                if wanted.is_empty() {
                    continue;
                }
                if wanted.iter().any(Option::is_none) {
                    kept.insert(key, value);
                } else {
                    let subs: Vec<&str> = wanted.into_iter().flatten().collect();
                    kept.insert(key, keep_subs(value, &subs));
                }
            }
            object = kept;
        }
        for path in &self.excluded {
            let (name, sub) = split_path(path);
            let Some(key) = object
                .keys()
                .find(|key| key.eq_ignore_ascii_case(name) && !always(key))
                .cloned()
            else {
                continue;
            };
            match sub {
                None => {
                    object.remove(&key);
                }
                Some(sub) => {
                    if let Some(value) = object.get_mut(&key) {
                        drop_sub(value, sub);
                    }
                }
            }
        }
        Value::Object(object)
    }
}

fn strip_core_urn(path: &str) -> &str {
    for urn in [USER_SCHEMA, GROUP_SCHEMA] {
        if path.len() > urn.len()
            && path[..urn.len()].eq_ignore_ascii_case(urn)
            && path.as_bytes()[urn.len()] == b':'
        {
            return &path[urn.len() + 1..];
        }
    }
    path
}

fn split_path(path: &str) -> (&str, Option<&str>) {
    if path.len() > 4 && path[..4].eq_ignore_ascii_case("urn:") {
        // An extension's attribute: the URN is the top-level key.
        if let Some(at) = path.rfind(':') {
            let urn = &path[..at];
            if urn.eq_ignore_ascii_case(ENTERPRISE_USER_SCHEMA) {
                return (urn, Some(&path[at + 1..]));
            }
        }
        return (path, None);
    }
    match path.split_once('.') {
        Some((name, sub)) => (name, Some(sub)),
        None => (path, None),
    }
}

fn keep_subs(value: Value, subs: &[&str]) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .filter(|(key, _)| subs.iter().any(|sub| sub.eq_ignore_ascii_case(key)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| keep_subs(item, subs))
                .collect(),
        ),
        other => other,
    }
}

fn drop_sub(value: &mut Value, sub: &str) {
    match value {
        Value::Object(object) => object.retain(|key, _| !key.eq_ignore_ascii_case(sub)),
        Value::Array(items) => items.iter_mut().for_each(|item| drop_sub(item, sub)),
        _ => {}
    }
}

/// A list response.
pub(crate) fn list_response(total: usize, start_index: usize, resources: Vec<Value>) -> Value {
    json!({
        "schemas": [LIST_RESPONSE_SCHEMA],
        "totalResults": total,
        "startIndex": start_index,
        "itemsPerPage": resources.len(),
        "Resources": resources,
    })
}

/// `GET /ServiceProviderConfig`.
pub(crate) fn service_provider_config(base: &str, max_results: usize) -> Value {
    json!({
        "schemas": [SERVICE_PROVIDER_CONFIG_SCHEMA],
        "patch": { "supported": true },
        "bulk": { "supported": false, "maxOperations": 0, "maxPayloadSize": 0 },
        "filter": { "supported": true, "maxResults": max_results },
        "changePassword": { "supported": false },
        "sort": { "supported": false },
        "etag": { "supported": false },
        "authenticationSchemes": [{
            "type": "oauthbearertoken",
            "name": "OAuth Bearer Token",
            "description": "The directory connection's bearer token, sent as Authorization: Bearer <token>",
            "primary": true
        }],
        "meta": {
            "resourceType": "ServiceProviderConfig",
            "location": format!("{base}/ServiceProviderConfig")
        }
    })
}

/// `GET /ResourceTypes`.
pub(crate) fn resource_types(base: &str) -> Vec<Value> {
    vec![
        json!({
            "schemas": [RESOURCE_TYPE_SCHEMA],
            "id": "User",
            "name": "User",
            "endpoint": "/Users",
            "description": "User Account",
            "schema": USER_SCHEMA,
            "schemaExtensions": [{ "schema": ENTERPRISE_USER_SCHEMA, "required": false }],
            "meta": { "resourceType": "ResourceType", "location": format!("{base}/ResourceTypes/User") }
        }),
        json!({
            "schemas": [RESOURCE_TYPE_SCHEMA],
            "id": "Group",
            "name": "Group",
            "endpoint": "/Groups",
            "description": "Group",
            "schema": GROUP_SCHEMA,
            "meta": { "resourceType": "ResourceType", "location": format!("{base}/ResourceTypes/Group") }
        }),
    ]
}

/// `GET /Schemas`: the User, Group and enterprise user schemas, with the
/// attributes this server reads.
pub(crate) fn schemas(base: &str) -> Vec<Value> {
    let attr = |name: &str, kind: &str, description: &str| {
        json!({
            "name": name, "type": kind, "multiValued": false, "description": description,
            "required": false, "caseExact": false, "mutability": "readWrite",
            "returned": "default", "uniqueness": "none"
        })
    };
    let with = |mut value: Value, key: &str, field: Value| {
        value[key] = field;
        value
    };
    let multi_contact = |name: &str, description: &str| {
        with(
            with(
                attr(name, "complex", description),
                "multiValued",
                json!(true),
            ),
            "subAttributes",
            json!([
                attr("value", "string", "The value"),
                attr("type", "string", "work, home, mobile or other"),
                attr("primary", "boolean", "Whether this is the primary value"),
            ]),
        )
    };
    let user_attributes = vec![
        with(
            with(
                attr("userName", "string", "The unique sign-in name"),
                "required",
                json!(true),
            ),
            "uniqueness",
            json!("server"),
        ),
        with(
            attr("name", "complex", "The person's name"),
            "subAttributes",
            json!([
                attr("formatted", "string", "The full name"),
                attr("familyName", "string", "The family name"),
                attr("givenName", "string", "The given name"),
                attr("middleName", "string", "The middle name"),
                attr("honorificPrefix", "string", "The honorific prefix"),
                attr("honorificSuffix", "string", "The honorific suffix"),
            ]),
        ),
        attr("displayName", "string", "The name to show"),
        attr("nickName", "string", "The casual name"),
        attr("title", "string", "The job title"),
        attr("userType", "string", "The relationship to the organization"),
        attr("preferredLanguage", "string", "The preferred language"),
        attr("locale", "string", "The locale"),
        attr("timezone", "string", "The time zone"),
        attr("active", "boolean", "Whether the account is active"),
        multi_contact("emails", "Email addresses"),
        multi_contact("phoneNumbers", "Phone numbers"),
        with(
            with(
                with(
                    attr("groups", "complex", "The groups the user belongs to"),
                    "multiValued",
                    json!(true),
                ),
                "mutability",
                json!("readOnly"),
            ),
            "subAttributes",
            json!([
                attr("value", "string", "The group's id"),
                attr("display", "string", "The group's name"),
                attr("$ref", "reference", "The group's URI"),
            ]),
        ),
    ];
    let group_attributes = vec![
        with(
            attr("displayName", "string", "The group's name"),
            "required",
            json!(true),
        ),
        with(
            with(
                attr("members", "complex", "The group's members"),
                "multiValued",
                json!(true),
            ),
            "subAttributes",
            json!([
                attr("value", "string", "The member's id"),
                attr("display", "string", "The member's name"),
                attr("$ref", "reference", "The member's URI"),
                attr("type", "string", "User"),
            ]),
        ),
    ];
    let enterprise_attributes = vec![
        attr("employeeNumber", "string", "The employee number"),
        attr("costCenter", "string", "The cost center"),
        attr("organization", "string", "The organization"),
        attr("division", "string", "The division"),
        attr("department", "string", "The department"),
        with(
            attr("manager", "complex", "The user's manager"),
            "subAttributes",
            json!([
                attr("value", "string", "The manager's id"),
                attr("displayName", "string", "The manager's name"),
                attr("$ref", "reference", "The manager's URI"),
            ]),
        ),
    ];
    [
        (USER_SCHEMA, "User", "User Account", user_attributes),
        (GROUP_SCHEMA, "Group", "Group", group_attributes),
        (
            ENTERPRISE_USER_SCHEMA,
            "EnterpriseUser",
            "Enterprise User",
            enterprise_attributes,
        ),
    ]
    .into_iter()
    .map(|(id, name, description, attributes)| {
        json!({
            "schemas": [SCHEMA_SCHEMA],
            "id": id,
            "name": name,
            "description": description,
            "attributes": attributes,
            "meta": { "resourceType": "Schema", "location": format!("{base}/Schemas/{id}") }
        })
    })
    .collect()
}

// -- reading values -----------------------------------------------------------

fn string(resource: &Value, key: &str) -> Result<Option<String>, ScimError> {
    match get_ci(resource, key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(ScimError::invalid_value(format!("{key} must be a string"))),
    }
}

/// A boolean, also from the strings `"true"` and `"false"` in any case
/// (some clients send `"False"`).
pub(crate) fn boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(text) if text.eq_ignore_ascii_case("true") => Some(true),
        Value::String(text) if text.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn take(object: &mut Map<String, Value>, key: &str) -> Option<Value> {
    let found = object
        .keys()
        .find(|k| k.eq_ignore_ascii_case(key))
        .cloned()?;
    object.remove(&found)
}

fn take_string(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    match take(object, key) {
        Some(Value::String(value)) => Some(value),
        Some(Value::Null) | None => None,
        Some(other) => {
            // Not a string: keep it where the client put it.
            object.insert(key.to_owned(), other);
            None
        }
    }
}

fn contacts(resource: &Value, key: &str) -> Result<Vec<ContactValue>, ScimError> {
    let items = match get_ci(resource, key) {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(items)) => items,
        Some(_) => return Err(ScimError::invalid_value(format!("{key} must be an array"))),
    };
    let mut values = Vec::new();
    for item in items {
        let value = get_ci(item, "value")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let Some(value) = value else {
            continue;
        };
        values.push(ContactValue {
            value: value.to_owned(),
            kind: get_ci(item, "type")
                .and_then(Value::as_str)
                .map(str::to_owned),
            primary: get_ci(item, "primary").and_then(boolean).unwrap_or(false),
        });
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn entra_user() -> Value {
        json!({
            "schemas": [USER_SCHEMA, ENTERPRISE_USER_SCHEMA],
            "externalId": "8f6c9a1e-5d2b-4c3a-9e8f-7a6b5c4d3e2f",
            "userName": "anu@acme.test",
            "active": "True",
            "displayName": "Anu Lakhotia",
            "title": "Principal engineer",
            "preferredLanguage": "en-GB",
            "password": "Secret!123",
            "id": "client-chosen",
            "meta": { "resourceType": "User" },
            "name": { "givenName": "Anu", "familyName": "Lakhotia", "formatted": "Anu Lakhotia" },
            "emails": [{ "primary": "true", "type": "work", "value": "anu@acme.test" }],
            "phoneNumbers": [{ "type": "mobile", "value": "+44 20 7946 0958" }],
            "addresses": [{ "type": "work", "country": "GB" }],
            ENTERPRISE_USER_SCHEMA: {
                "employeeNumber": "1815",
                "department": "Engineering",
                "costCenter": "4130",
                "manager": { "value": "26118915-6090-4610-87e4-49d8ca9f808d" }
            }
        })
    }

    #[test]
    fn a_client_resource_is_kept_without_server_owned_attributes_or_passwords() {
        let stored = stored_resource(&entra_user()).unwrap();
        assert!(stored.get("password").is_none());
        assert!(stored.get("id").is_none());
        assert!(stored.get("meta").is_none());
        assert_eq!(stored["userName"], json!("anu@acme.test"));
        assert!(stored_resource(&json!([1])).is_err());
    }

    #[test]
    fn users_are_normalized_with_every_attribute_kept() {
        let stored = stored_resource(&entra_user()).unwrap();
        let (user, has_external_id) = user_from_resource(&stored).unwrap();
        assert!(has_external_id);
        assert_eq!(user.external_id, "8f6c9a1e-5d2b-4c3a-9e8f-7a6b5c4d3e2f");
        assert_eq!(user.user_name, "anu@acme.test");
        assert!(user.active);
        assert_eq!(user.given_name.as_deref(), Some("Anu"));
        assert_eq!(user.family_name.as_deref(), Some("Lakhotia"));
        assert_eq!(user.display_name.as_deref(), Some("Anu Lakhotia"));
        assert_eq!(user.primary_email(), Some("anu@acme.test"));
        assert!(user.emails[0].primary);
        assert_eq!(user.primary_phone(), Some("+44 20 7946 0958"));
        assert_eq!(user.title.as_deref(), Some("Principal engineer"));
        assert_eq!(user.locale.as_deref(), Some("en-GB"));
        assert_eq!(user.employee_number.as_deref(), Some("1815"));
        assert_eq!(user.department.as_deref(), Some("Engineering"));
        assert_eq!(
            user.manager_id.as_deref(),
            Some("26118915-6090-4610-87e4-49d8ca9f808d")
        );
        assert_eq!(
            user.attributes["addresses"],
            json!([{ "type": "work", "country": "GB" }])
        );
        assert_eq!(
            user.attributes["name"],
            json!({ "formatted": "Anu Lakhotia" })
        );
        assert_eq!(
            user.attributes[ENTERPRISE_USER_SCHEMA],
            json!({ "costCenter": "4130" })
        );
        assert_eq!(user.attributes["preferredLanguage"], json!("en-GB"));
        assert!(!user.attributes.contains_key("userName"));
    }

    #[test]
    fn user_resources_are_checked() {
        let err = |resource: Value| user_from_resource(&resource).unwrap_err();
        assert_eq!(err(json!({})).detail, "userName is required");
        assert_eq!(
            err(json!({ "userName": " " })).detail,
            "userName is required"
        );
        assert_eq!(
            err(json!({ "userName": "a", "active": "maybe" })).detail,
            "active must be a boolean"
        );
        assert_eq!(
            err(json!({ "userName": 7 })).detail,
            "userName must be a string"
        );
        assert_eq!(
            err(json!({ "userName": "a", "emails": "a@b" })).detail,
            "emails must be an array"
        );
        let (user, has_external_id) =
            user_from_resource(&json!({ "userName": "gauri", "active": false })).unwrap();
        assert!(!has_external_id);
        assert!(!user.active);
        assert_eq!(user.external_id, "");
        let (user, _) = user_from_resource(&json!({
            "userName": "arun",
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User": { "manager": "m-1" }
        }))
        .unwrap();
        assert_eq!(user.manager_id.as_deref(), Some("m-1"));
    }

    #[test]
    fn groups_are_normalized_with_their_member_ids() {
        let (group, members) = group_from_resource(&json!({
            "schemas": [GROUP_SCHEMA],
            "displayName": "Engineering",
            "externalId": "grp-eng",
            "description": "Builders",
            "members": [{ "value": "u1", "display": "Anu" }, { "value": "u2" }, { "value": "u1" }]
        }))
        .unwrap();
        assert_eq!(group.display_name, "Engineering");
        assert_eq!(group.external_id, "grp-eng");
        assert_eq!(group.description.as_deref(), Some("Builders"));
        assert_eq!(members, vec!["u1", "u2"]);
        assert!(group.members.is_empty());
        assert_eq!(
            group_from_resource(&json!({ "members": [] }))
                .unwrap_err()
                .detail,
            "displayName is required"
        );
        assert_eq!(
            group_from_resource(&json!({ "displayName": "x", "members": [{}] }))
                .unwrap_err()
                .detail,
            "each member needs a value"
        );
    }

    #[test]
    fn rendered_resources_carry_id_meta_and_their_schemas() {
        let stored = stored_resource(&entra_user()).unwrap();
        let at = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
        let meta = Meta {
            id: "2819c223-7f76-453a-919d-413861904646",
            resource_type: "User",
            created: at,
            last_modified: at,
            location: "https://app.acme.test/scim/v2/Users/2819c223-7f76-453a-919d-413861904646"
                .to_owned(),
            version: "abc123",
        };
        let rendered = render(&stored, &meta, vec![("groups", json!([]))]);
        assert_eq!(
            rendered["schemas"],
            json!([USER_SCHEMA, ENTERPRISE_USER_SCHEMA])
        );
        assert_eq!(
            rendered["id"],
            json!("2819c223-7f76-453a-919d-413861904646")
        );
        assert_eq!(rendered["meta"]["resourceType"], json!("User"));
        assert_eq!(
            rendered["meta"]["created"],
            json!("2026-10-27T08:00:00.000Z")
        );
        assert_eq!(rendered["meta"]["version"], json!("W/\"abc123\""));
        assert_eq!(rendered["groups"], json!([]));
        assert_eq!(rendered["userName"], json!("anu@acme.test"));
        assert!(rendered.get("password").is_none());
    }

    #[test]
    fn projections_keep_or_drop_attributes() {
        let user = json!({
            "schemas": [USER_SCHEMA],
            "id": "u1",
            "userName": "anu",
            "name": { "givenName": "Anu", "familyName": "Lakhotia" },
            "emails": [{ "value": "a@acme.test", "type": "work" }],
            "meta": { "resourceType": "User" }
        });
        let only = Projection::new(Some("userName,name.givenName"), None).apply(user.clone());
        assert_eq!(
            only,
            json!({
                "schemas": [USER_SCHEMA],
                "id": "u1",
                "userName": "anu",
                "name": { "givenName": "Anu" }
            })
        );
        let core =
            Projection::new(Some(&format!("{USER_SCHEMA}:userName")), None).apply(user.clone());
        assert_eq!(core["userName"], json!("anu"));
        assert!(core.get("name").is_none());
        let without = Projection::new(None, Some("emails.type,meta,id")).apply(user);
        assert_eq!(without["emails"], json!([{ "value": "a@acme.test" }]));
        assert!(without.get("meta").is_none());
        assert_eq!(without["id"], json!("u1"));
        assert!(Projection::new(None, Some("Members")).excludes("members"));
    }

    #[test]
    fn discovery_documents_describe_the_server() {
        let config = service_provider_config("https://app.acme.test/scim/v2", 200);
        assert_eq!(config["patch"]["supported"], json!(true));
        assert_eq!(config["bulk"]["supported"], json!(false));
        assert_eq!(config["filter"]["maxResults"], json!(200));
        assert_eq!(
            config["meta"]["location"],
            json!("https://app.acme.test/scim/v2/ServiceProviderConfig")
        );
        let types = resource_types("/scim/v2");
        assert_eq!(types[0]["endpoint"], json!("/Users"));
        assert_eq!(types[1]["schema"], json!(GROUP_SCHEMA));
        let schemas = schemas("/scim/v2");
        assert_eq!(schemas.len(), 3);
        assert_eq!(schemas[0]["id"], json!(USER_SCHEMA));
        assert_eq!(schemas[0]["attributes"][0]["name"], json!("userName"));
        assert_eq!(schemas[0]["attributes"][0]["required"], json!(true));
        let list = list_response(3, 1, vec![json!({}), json!({})]);
        assert_eq!(list["totalResults"], json!(3));
        assert_eq!(list["itemsPerPage"], json!(2));
        assert_eq!(list["schemas"], json!([LIST_RESPONSE_SCHEMA]));
    }
}
