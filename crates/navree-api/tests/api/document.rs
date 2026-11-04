//! The OpenAPI document built from the route table.

use serde_json::{Value, json};

use crate::support::{boot, document};

fn path_params(operation: &Value) -> Vec<(String, Value)> {
    operation["parameters"]
        .as_array()
        .map(|params| {
            params
                .iter()
                .filter(|p| p["in"] == "path")
                .map(|p| {
                    (
                        p["name"].as_str().unwrap().to_owned(),
                        p["required"].clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn document_lists_paths_with_methods_and_params() {
    let app = boot().await;
    let doc = document(&app).await;

    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["info"]["title"], "Navree API");
    assert_eq!(doc["info"]["version"], "1.0.0");

    let paths = doc["paths"].as_object().expect("paths object");
    let mut listed: Vec<(&str, Vec<&str>)> = paths
        .iter()
        .map(|(path, item)| {
            let mut methods: Vec<&str> = item
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            methods.sort_unstable();
            (path.as_str(), methods)
        })
        .collect();
    listed.sort_unstable();
    assert_eq!(
        listed,
        [
            ("/api/v1/deals", vec!["get", "post"]),
            ("/api/v1/deals/{id}", vec!["get"]),
            ("/api/v1/deals/{id}/notes/{note_id}", vec!["delete"]),
            ("/files/{path}", vec!["get"]),
        ],
        "the docs routes themselves are not listed"
    );

    let show = &paths["/api/v1/deals/{id}"]["get"];
    assert_eq!(path_params(show), [("id".to_owned(), json!(true))]);
    assert_eq!(show["parameters"][0]["schema"]["type"], "string");
    assert_eq!(show["operationId"], "deals.show");
    assert_eq!(show["tags"], json!(["Deals"]));

    let delete = &paths["/api/v1/deals/{id}/notes/{note_id}"]["delete"];
    assert_eq!(
        path_params(delete),
        [
            ("id".to_owned(), json!(true)),
            ("note_id".to_owned(), json!(true))
        ]
    );
    assert!(delete.get("operationId").is_none(), "unnamed route");

    let files = &paths["/files/{path}"]["get"];
    assert_eq!(path_params(files), [("path".to_owned(), json!(true))]);

    let store = &paths["/api/v1/deals"]["post"];
    assert_eq!(store["summary"], "Create a deal");
    assert_eq!(store["description"], "Creates a deal owned by the caller.");
    assert!(path_params(store).is_empty());

    assert_eq!(doc["tags"], json!([{"name": "Deals"}]));
}

#[tokio::test]
async fn request_and_response_schemas_registered_as_components() {
    let app = boot().await;
    let doc = document(&app).await;

    let store = &doc["paths"]["/api/v1/deals"]["post"];
    assert_eq!(store["requestBody"]["required"], true);
    assert_eq!(
        store["requestBody"]["content"]["application/json"]["schema"],
        json!({"$ref": "#/components/schemas/CreateDeal"})
    );
    let created = &store["responses"]["201"];
    assert_eq!(created["description"], "Created");
    assert_eq!(
        created["content"]["application/json"]["schema"],
        json!({"$ref": "#/components/schemas/Deal"})
    );
    assert!(
        store["responses"].get("200").is_none(),
        "the declared success status replaces 200"
    );

    let show = &doc["paths"]["/api/v1/deals/{id}"]["get"];
    assert_eq!(
        show["responses"]["200"]["content"]["application/json"]["schema"],
        json!({"$ref": "#/components/schemas/Deal"})
    );
    let index = &doc["paths"]["/api/v1/deals"]["get"];
    assert_eq!(index["responses"]["200"], json!({"description": "OK"}));
    assert!(index.get("requestBody").is_none());

    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["CreateDeal"]["properties"]["title"]["type"],
        "string"
    );
    assert_eq!(schemas["Deal"]["properties"]["title"]["type"], "string");
    assert_eq!(
        schemas["Deal"]["properties"]["owner"]["$ref"],
        "#/components/schemas/Owner"
    );
    assert!(
        schemas["Deal"].get("x-navree-components").is_none(),
        "referenced schemas are hoisted, not left in the extension"
    );
    assert_eq!(schemas["Owner"]["properties"]["name"]["type"], "string");
}

#[tokio::test]
async fn non_public_routes_have_security() {
    let app = boot().await;
    let doc = document(&app).await;
    let paths = &doc["paths"];
    let both = json!([{"cookie": []}, {"bearer": []}]);

    assert_eq!(
        paths["/api/v1/deals/{id}"]["get"]["security"], both,
        "authenticated"
    );
    assert_eq!(
        paths["/api/v1/deals"]["post"]["security"], both,
        "permissions"
    );
    assert_eq!(
        paths["/api/v1/deals/{id}/notes/{note_id}"]["delete"]["security"], both,
        "roles"
    );
    assert!(
        paths["/api/v1/deals"]["get"].get("security").is_none(),
        "public"
    );
    assert!(
        paths["/files/{path}"]["get"].get("security").is_none(),
        "public"
    );
    assert!(
        doc.get("security").is_none(),
        "no document-wide requirement"
    );

    let schemes = &doc["components"]["securitySchemes"];
    assert_eq!(schemes["cookie"]["type"], "apiKey");
    assert_eq!(schemes["cookie"]["in"], "cookie");
    // MOD-001: the cookie navree-session sets by default, `__Host-` prefix and all.
    assert_eq!(schemes["cookie"]["name"], "__Host-navree_session");
    assert_eq!(schemes["bearer"]["type"], "http");
    assert_eq!(schemes["bearer"]["scheme"], "bearer");
    assert_eq!(
        schemes["bearer"]["bearerFormat"], "JWT",
        "navree-auth access tokens"
    );
    let description = schemes["bearer"]["description"].as_str().unwrap();
    assert!(
        description.contains("JWT access token") && !description.contains("personal access"),
        "{description}"
    );
}

fn error_response(description: &str) -> Value {
    json!({
        "description": description,
        "content": {
            "application/json": {"schema": {"$ref": "#/components/schemas/HttpError"}}
        }
    })
}

#[tokio::test]
async fn error_responses_reference_http_error_schema() {
    let app = boot().await;
    let doc = document(&app).await;
    let paths = &doc["paths"];

    let show = &paths["/api/v1/deals/{id}"]["get"]["responses"];
    assert_eq!(show["404"], error_response("Not Found"), "listed");
    assert_eq!(show["401"], error_response("Unauthorized"), "not public");
    assert!(show.get("403").is_none(), "no authorization rule to fail");

    let store = &paths["/api/v1/deals"]["post"]["responses"];
    assert_eq!(store["400"], error_response("Bad Request"));
    assert_eq!(store["401"], error_response("Unauthorized"));
    assert_eq!(store["403"], error_response("Forbidden"), "permissions");

    let delete = &paths["/api/v1/deals/{id}/notes/{note_id}"]["delete"]["responses"];
    assert_eq!(delete["403"], error_response("Forbidden"), "roles");

    let index = &paths["/api/v1/deals"]["get"]["responses"];
    assert_eq!(
        index.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["200"],
        "a public route without listed errors documents only its success"
    );

    let schemas = &doc["components"]["schemas"];
    assert!(schemas.get("Problem").is_none(), "no RFC 9457 schema");
    let error = &schemas["HttpError"];
    assert_eq!(error["type"], "object");
    let mut properties: Vec<&String> = error["properties"].as_object().unwrap().keys().collect();
    properties.sort();
    assert_eq!(
        properties,
        ["code", "error", "errors", "message", "statusCode"]
    );
    let mut required: Vec<&str> = error["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    required.sort_unstable();
    assert_eq!(
        required,
        ["error", "message", "statusCode"],
        "code (http.errors.code = false) and errors are optional"
    );
    assert_eq!(error["properties"]["statusCode"]["type"], "integer");
    assert_eq!(error["properties"]["error"]["type"], "string");
    let code = &error["properties"]["code"];
    assert_eq!(code["type"], "string", "a string when present: {code}");
    assert!(
        code["description"]
            .as_str()
            .is_some_and(|d| d.contains("translation")),
        "{code}"
    );
    assert_eq!(
        error["properties"]["message"]["oneOf"],
        json!([
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}},
        ]),
        "message: string | string[]: {error}"
    );
    let errors = &error["properties"]["errors"];
    assert_eq!(errors["additionalProperties"]["type"], "array", "{errors}");
    assert_eq!(errors["additionalProperties"]["items"]["type"], "string");
}

#[test]
fn route_extensions_become_members_of_the_operation() {
    use navree_api::{ApiOptions, openapi_document};
    use navree_http::RouteMeta;

    let mut flagged = RouteMeta {
        method: "GET".to_owned(),
        full_path: "/api/v1/deals/forecast".to_owned(),
        ..RouteMeta::default()
    };
    flagged.access.authenticated = true;
    flagged
        .extensions
        .insert("x-navree-flag".to_owned(), json!("new-pipeline"));
    let plain = RouteMeta {
        method: "GET".to_owned(),
        full_path: "/api/v1/deals".to_owned(),
        ..RouteMeta::default()
    };

    let doc = openapi_document(&[flagged, plain], &ApiOptions::default()).unwrap();
    let forecast = &doc["paths"]["/api/v1/deals/forecast"]["get"];
    assert_eq!(forecast["x-navree-flag"], "new-pipeline");
    assert!(forecast["responses"].is_object(), "the rest is unchanged");
    assert!(
        doc["paths"]["/api/v1/deals"]["get"]
            .get("x-navree-flag")
            .is_none()
    );
}

#[test]
fn http_error_describes_the_rendered_error_body() {
    use std::collections::BTreeMap;

    use navree_api::{HttpError, HttpErrorMessage};
    use navree_http::AppError;

    let not_found: HttpError = serde_json::from_value(AppError::not_found().body()).unwrap();
    assert_eq!(
        not_found,
        HttpError {
            status_code: 404,
            message: HttpErrorMessage::One("Not Found".to_owned()),
            error: "Not Found".to_owned(),
            code: Some("not_found".to_owned()),
            errors: None,
        }
    );

    // A body rendered with `http.errors.code = false` parses too.
    let without_code: HttpError = serde_json::from_value(serde_json::json!({
        "statusCode": 404, "message": "Not Found", "error": "Not Found",
    }))
    .unwrap();
    assert_eq!(without_code.code, None);
    assert!(
        serde_json::to_value(&without_code)
            .unwrap()
            .get("code")
            .is_none()
    );

    let errors = BTreeMap::from([(
        "email".to_owned(),
        vec!["email must be a valid email address".to_owned()],
    )]);
    let invalid = AppError::validation(errors.clone()).body();
    let parsed: HttpError = serde_json::from_value(invalid.clone()).unwrap();
    assert_eq!(
        parsed,
        HttpError {
            status_code: 400,
            message: HttpErrorMessage::Many(vec!["email must be a valid email address".to_owned()]),
            error: "Bad Request".to_owned(),
            code: Some("validation_failed".to_owned()),
            errors: Some(errors),
        }
    );
    assert_eq!(serde_json::to_value(&parsed).unwrap(), invalid);
}
