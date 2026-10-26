//! Building the OpenAPI 3.1 document from the route table.

use std::collections::{BTreeMap, HashSet};

use axum::http::StatusCode;
use navree_http::{ERROR_CONTENT_TYPE, RouteMeta, SchemaFn, schema_fn};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::ToSchema;
use utoipa::openapi::schema::{ArrayBuilder, ObjectBuilder, OneOfBuilder, Schema, Type};

use crate::ApiOptions;

/// The HTTP methods an OpenAPI path item has operations for.
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// The extension member in which `navree_http::schema_fn` carries the schemas
/// a schema references.
const REFERENCED: &str = "x-navree-components";

/// The session cookie navree-session sets by default (`session.cookie`).
pub const DEFAULT_SESSION_COOKIE: &str = "navree_session";

/// Why a document cannot be built.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ApiError {
    /// Two different schemas share a component name, so a `$ref` to it
    /// would be wrong for one of them.
    #[error(
        "two different schemas are named `{name}` (referenced by {first} and by {second}); give one of the types a distinct name with #[schema(as = OtherName)]"
    )]
    SchemaConflict {
        /// The component name.
        name: String,
        /// The route that registered the name first (`GET /api/v1/deals`).
        first: String,
        /// The route whose schema differs.
        second: String,
    },
    /// The document refers to component schemas nothing registered.
    #[error("{}", missing_message(.missing))]
    MissingSchemas {
        /// Each missing schema name and the first route (or component) that
        /// refers to it.
        missing: Vec<(String, String)>,
    },
}

fn missing_message(missing: &[(String, String)]) -> String {
    let list: Vec<String> = missing
        .iter()
        .map(|(name, by)| {
            format!("`{name}` (referenced by {by}; register it with `m.api_schema::<{name}>()`)")
        })
        .collect();
    format!(
        "the OpenAPI document refers to schemas nothing registered: {}. utoipa does not collect the schemas of generic type arguments (`Note` in `Page<Note>`); register them in their module with navree_api::ModuleBuilderApiExt::api_schema",
        list.join(", ")
    )
}

/// The OpenAPI 3.1 document describing `routes` (usually
/// `navree_http::route_table`), with the default session cookie name in the
/// `cookie` security scheme.
///
/// - Paths are the mounted paths in OpenAPI form (`{*rest}` becomes
///   `{rest}`); every `{param}` is a required string path parameter.
/// - The route name is the `operationId`; [`ApiMeta`](navree_http::ApiMeta)
///   gives tags, summary, description, the JSON request body, the success
///   status (default 200) with its JSON response body, and the documented
///   error statuses, rendered as `application/json` responses with the
///   [`HttpError`] schema (Navree's JSON error shape).
/// - Non-public routes require the `cookie` or the `bearer` security
///   scheme and document 401; routes checked by the route authorizer
///   (roles, permissions, abilities) also document 403.
/// - Request and response schemas, and the schemas they reference, are
///   component schemas. A body schema whose name another, different schema
///   already holds is inlined.
/// - The route's specification extensions
///   ([`RouteMeta::extensions`](navree_http::RouteMeta::extensions), such
///   as `x-navree-flag`) are members of its operation.
///
/// # Errors
///
/// [`ApiError::SchemaConflict`] when two different referenced schemas share
/// a name; [`ApiError::MissingSchemas`] when the document refers to a
/// schema nothing registered (see
/// [`api_schema`](crate::ModuleBuilderApiExt::api_schema)).
pub fn openapi_document(routes: &[RouteMeta], options: &ApiOptions) -> Result<Value, ApiError> {
    build(routes, &[], options, DEFAULT_SESSION_COOKIE)
}

/// [`openapi_document`] with schemas registered by modules (and a label
/// for each) and the session cookie name of the app.
pub(crate) fn build(
    routes: &[RouteMeta],
    extra_schemas: &[(SchemaFn, String)],
    options: &ApiOptions,
    session_cookie: &str,
) -> Result<Value, ApiError> {
    let routes: Vec<(&RouteMeta, String)> = routes
        .iter()
        .filter_map(|route| method_of(route).map(|method| (route, method)))
        .collect();

    // First every referenced schema, so each `$ref` names the right one.
    let mut schemas = Schemas::default();
    for (f, label) in extra_schemas {
        let body = Body::collect(*f, label, &mut schemas)?;
        schemas.register(body.name, body.schema, label)?;
    }
    let error = Body::collect(schema_fn::<HttpError>, "error responses", &mut schemas)?;
    let mut bodies = Vec::with_capacity(routes.len());
    for (route, _) in &routes {
        let label = format!("{} {}", route.method, route.full_path);
        let api = route.api.as_ref();
        let mut collect = |f: Option<SchemaFn>| {
            f.map(|f| Body::collect(f, &label, &mut schemas))
                .transpose()
        };
        let request = collect(api.and_then(|api| api.request))?;
        let response = collect(api.and_then(|api| api.response))?;
        bodies.push((label, request, response));
    }

    let mut builder = Builder {
        schemas,
        error,
        operation_ids: HashSet::new(),
        tags: Vec::new(),
    };
    let mut paths = Map::new();
    let mut references = Vec::new();
    for ((route, method), (label, request, response)) in routes.into_iter().zip(bodies) {
        let (path, params) = openapi_path(&route.full_path);
        let operation = builder.operation(route, &label, &params, request, response);
        references.push((label, schema_refs(&operation)));
        if let Value::Object(item) = paths
            .entry(path)
            .or_insert_with(|| Value::Object(Map::new()))
        {
            item.insert(method, operation);
        }
    }

    builder.schemas.check_references(references)?;

    let mut document = Map::new();
    document.insert("openapi".into(), json!("3.1.0"));
    document.insert(
        "info".into(),
        json!({"title": options.title, "version": options.version}),
    );
    if !options.servers.is_empty() {
        let servers: Vec<Value> = options
            .servers
            .iter()
            .map(|url| json!({"url": url}))
            .collect();
        document.insert("servers".into(), Value::Array(servers));
    }
    if !builder.tags.is_empty() {
        let tags: Vec<Value> = builder
            .tags
            .iter()
            .map(|name| json!({"name": name}))
            .collect();
        document.insert("tags".into(), Value::Array(tags));
    }
    document.insert("paths".into(), Value::Object(paths));
    document.insert(
        "components".into(),
        json!({
            "schemas": builder.schemas.components,
            "securitySchemes": security_schemes(session_cookie),
        }),
    );
    Ok(Value::Object(document))
}

/// The lower-case OpenAPI method of a route, or `None` (logged) for methods
/// OpenAPI 3.1 has no operation for.
fn method_of(route: &RouteMeta) -> Option<String> {
    let method = route.method.to_ascii_lowercase();
    if METHODS.contains(&method.as_str()) {
        Some(method)
    } else {
        tracing::warn!(
            target: "navree::api",
            method = %route.method,
            path = %route.full_path,
            "OpenAPI has no operation for this HTTP method; the route is left out of the document"
        );
        None
    }
}

/// The `cookie` (first-party session, navree-session) and `bearer` (JWT
/// access token, navree-auth) security schemes.
fn security_schemes(session_cookie: &str) -> Value {
    json!({
        "cookie": {
            "type": "apiKey",
            "in": "cookie",
            "name": session_cookie,
            "description": "The session cookie of a first-party app (apps using navree-session). Unsafe methods (POST, PUT, PATCH, DELETE) also need the XSRF-TOKEN cookie's value in the X-XSRF-TOKEN header.",
        },
        "bearer": {
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT",
            "description": "A JWT access token from `POST /api/auth/login` or `/api/auth/refresh`: `Authorization: Bearer <token>`.",
        },
    })
}

/// The body of every Navree error response, in Navree's JSON error shape
/// (content type `application/json`). Documented as the `HttpError`
/// component schema.
///
/// ```json
/// {"statusCode": 404, "message": "Not Found", "error": "Not Found", "code": "not_found"}
/// {"statusCode": 400, "message": ["email must be a valid email address"],
///  "error": "Bad Request", "code": "validation_failed",
///  "errors": {"email": ["email must be a valid email address"]}}
/// ```
///
/// The request id is in the `X-Request-Id` response header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(description = "The JSON error shape every error response carries (`application/json`).")]
pub struct HttpError {
    /// The HTTP status code.
    pub status_code: u16,
    /// What went wrong: one message, or every validation message.
    #[schema(schema_with = message_schema)]
    pub message: HttpErrorMessage,
    /// The reason phrase of the status (`Not Found`, `Bad Request`).
    pub error: String,
    /// The machine-readable error code (`not_found`, `validation_failed`,
    /// `invalid_credentials`), stable across releases, for front ends to map
    /// to a translation key. Left out when `http.errors.code` is `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = code_schema)]
    pub code: Option<String>,
    /// Validation messages per field (400 validation errors only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<BTreeMap<String, Vec<String>>>,
}

/// The `message` of an [`HttpError`]: a string, or a list of strings (every
/// validation message).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HttpErrorMessage {
    /// One message (`"Not Found"`, `"Deal not found"`).
    One(String),
    /// Every validation message, in field order.
    Many(Vec<String>),
}

/// The schema of `code`: a string when present (never `null`).
fn code_schema() -> Schema {
    ObjectBuilder::new()
        .schema_type(Type::String)
        .description(Some(
            "The machine-readable error code (`not_found`, `validation_failed`, `invalid_credentials`), stable across releases, for front ends to map to a translation key. Left out when `http.errors.code` is false.",
        ))
        .examples([serde_json::json!("not_found")])
        .build()
        .into()
}

/// The schema of `message`: `string | string[]`.
fn message_schema() -> Schema {
    let string = || ObjectBuilder::new().schema_type(Type::String);
    OneOfBuilder::new()
        .item(string())
        .item(ArrayBuilder::new().items(string()))
        .description(Some(
            "What went wrong: one message, or every validation message.",
        ))
        .build()
        .into()
}

/// Describes operations, collecting component schemas, operation ids and tags.
struct Builder {
    schemas: Schemas,
    error: Body,
    operation_ids: HashSet<String>,
    tags: Vec<String>,
}

impl Builder {
    fn operation(
        &mut self,
        route: &RouteMeta,
        label: &str,
        params: &[String],
        request: Option<Body>,
        response: Option<Body>,
    ) -> Value {
        let api = route.api.as_ref();
        let mut operation = Map::new();
        if let Some(api) = api {
            if !api.tags.is_empty() {
                for tag in &api.tags {
                    if !self.tags.contains(tag) {
                        self.tags.push(tag.clone());
                    }
                }
                operation.insert("tags".into(), json!(api.tags));
            }
            if let Some(summary) = &api.summary {
                operation.insert("summary".into(), json!(summary));
            }
            if let Some(description) = &api.description {
                operation.insert("description".into(), json!(description));
            }
        }
        if let Some(name) = &route.name {
            if self.operation_ids.insert(name.clone()) {
                operation.insert("operationId".into(), json!(name));
            } else {
                tracing::warn!(
                    target: "navree::api",
                    name = %name,
                    path = %route.full_path,
                    "two routes share a name; only the first gets it as its operationId"
                );
            }
        }
        if !params.is_empty() {
            let parameters: Vec<Value> = params
                .iter()
                .map(|name| {
                    json!({
                        "name": name,
                        "in": "path",
                        "required": true,
                        "schema": {"type": "string"},
                    })
                })
                .collect();
            operation.insert("parameters".into(), Value::Array(parameters));
        }
        if let Some(request) = request {
            operation.insert(
                "requestBody".into(),
                json!({
                    "required": true,
                    "content": {"application/json": {"schema": self.schemas.use_body(request, label)}},
                }),
            );
        }
        operation.insert("responses".into(), self.responses(route, label, response));
        if !route.access.public {
            operation.insert("security".into(), json!([{"cookie": []}, {"bearer": []}]));
        }
        for (name, value) in &route.extensions {
            operation.insert(name.clone(), value.clone());
        }
        Value::Object(operation)
    }

    /// The success response, the documented errors, 401 for non-public
    /// routes and 403 for routes the route authorizer checks.
    fn responses(&mut self, route: &RouteMeta, label: &str, body: Option<Body>) -> Value {
        let api = route.api.as_ref();
        let success = api
            .map(|api| api.success_status)
            .filter(|status| *status != 0)
            .unwrap_or(200);
        let mut response = Map::new();
        response.insert("description".into(), json!(reason(success)));
        if let Some(body) = body {
            response.insert(
                "content".into(),
                json!({"application/json": {"schema": self.schemas.use_body(body, label)}}),
            );
        }
        let mut responses = Map::new();
        responses.insert(success.to_string(), Value::Object(response));

        let mut errors: Vec<u16> = api.map(|api| api.responses.clone()).unwrap_or_default();
        if !route.access.public {
            errors.push(401);
        }
        if route.access.needs_authorizer() {
            errors.push(403);
        }
        for status in errors.into_iter().filter(|status| *status != success) {
            let mut response = Map::new();
            response.insert("description".into(), json!(reason(status)));
            if status >= 400 {
                let error = self.schemas.use_body(self.error.clone(), "error responses");
                response.insert(
                    "content".into(),
                    json!({ERROR_CONTENT_TYPE: {"schema": error}}),
                );
            }
            responses.insert(status.to_string(), Value::Object(response));
        }
        Value::Object(responses)
    }
}

/// A request or response body schema, its referenced schemas already
/// registered.
#[derive(Clone)]
struct Body {
    name: String,
    schema: Value,
}

impl Body {
    /// Calls `f`, registers the schemas it references (for the route
    /// `label`) and returns the body schema.
    fn collect(f: SchemaFn, label: &str, schemas: &mut Schemas) -> Result<Self, ApiError> {
        let (name, mut schema) = f();
        for (referenced, component) in take_referenced(&mut schema) {
            schemas.register(referenced, component, label)?;
        }
        Ok(Self { name, schema })
    }
}

/// The component schemas, and for each the route that registered it.
#[derive(Default)]
struct Schemas {
    components: BTreeMap<String, Value>,
    origins: BTreeMap<String, String>,
}

impl Schemas {
    /// Registers a referenced schema: `$ref`s point at it by name, so a
    /// different schema with the same name is an error.
    fn register(&mut self, name: String, schema: Value, label: &str) -> Result<(), ApiError> {
        match self.components.get(&name) {
            Some(existing) if *existing == schema => Ok(()),
            Some(_) => Err(ApiError::SchemaConflict {
                first: self.origins.get(&name).cloned().unwrap_or_default(),
                second: label.to_owned(),
                name,
            }),
            None => {
                self.origins.insert(name.clone(), label.to_owned());
                self.components.insert(name, schema);
                Ok(())
            }
        }
    }

    /// A reference to `body` as a component, or the schema itself when a
    /// different schema already holds its name (e.g. `Page<Deal>` and
    /// `Page<Note>`, both named `Page`).
    fn use_body(&mut self, body: Body, label: &str) -> Value {
        let reference = json!({"$ref": format!("#/components/schemas/{}", body.name)});
        match self.components.get(&body.name) {
            Some(existing) if *existing == body.schema => reference,
            Some(_) => body.schema,
            None => {
                self.origins.insert(body.name.clone(), label.to_owned());
                self.components.insert(body.name, body.schema);
                reference
            }
        }
    }

    /// Checks that every schema the operations (`(label, refs)`) and the
    /// components refer to is a component.
    fn check_references(&self, operations: Vec<(String, Vec<String>)>) -> Result<(), ApiError> {
        let components = self.components.iter().map(|(name, schema)| {
            let origin = self.origins.get(name).cloned().unwrap_or_default();
            (
                format!("the `{name}` schema of {origin}"),
                schema_refs(schema),
            )
        });
        let mut missing: Vec<(String, String)> = Vec::new();
        for (by, refs) in operations.into_iter().chain(components) {
            for name in refs {
                if !self.components.contains_key(&name) && !missing.iter().any(|(n, _)| *n == name)
                {
                    missing.push((name, by.clone()));
                }
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(ApiError::MissingSchemas { missing })
        }
    }
}

/// The component schema names `value` refers to (`#/components/schemas/X`).
fn schema_refs(value: &Value) -> Vec<String> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                if let Some(name) = object
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|r| r.strip_prefix("#/components/schemas/"))
                {
                    out.push(name.to_owned());
                }
                object.values().for_each(|v| walk(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, &mut out);
    out
}

/// Removes the referenced schemas `navree_http::schema_fn` carries in
/// [`REFERENCED`] from `schema`, and from them, recursively.
fn take_referenced(schema: &mut Value) -> Vec<(String, Value)> {
    let Some(Value::Object(referenced)) = schema
        .as_object_mut()
        .and_then(|object| object.remove(REFERENCED))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, mut component) in referenced {
        let nested = take_referenced(&mut component);
        out.push((name, component));
        out.extend(nested);
    }
    out
}

/// The reason phrase of a status code, as a response description.
fn reason(status: u16) -> &'static str {
    StatusCode::from_u16(status)
        .ok()
        .and_then(|status| status.canonical_reason())
        .unwrap_or("Response")
}

/// The OpenAPI form of an Axum path (`{*rest}` becomes `{rest}`) and its
/// parameter names, in order.
fn openapi_path(path: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(path.len());
    let mut params = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        let name = rest[start + 1..start + len].trim_start_matches('*');
        out.push_str(&rest[..start]);
        out.push('{');
        out.push_str(name);
        out.push('}');
        params.push(name.to_owned());
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    (out, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axum_paths_become_openapi_paths() {
        assert_eq!(
            openapi_path("/api/deals"),
            ("/api/deals".to_owned(), vec![])
        );
        assert_eq!(
            openapi_path("/api/deals/{id}/notes/{note_id}"),
            (
                "/api/deals/{id}/notes/{note_id}".to_owned(),
                vec!["id".to_owned(), "note_id".to_owned()]
            )
        );
        assert_eq!(
            openapi_path("/files/{*path}"),
            ("/files/{path}".to_owned(), vec!["path".to_owned()])
        );
    }
}
