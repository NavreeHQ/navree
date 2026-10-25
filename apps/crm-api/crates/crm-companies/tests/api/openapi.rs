//! Every companies and contacts route is in the OpenAPI document with its
//! request and response schemas (the names crm-web's types use).

use serde_json::{Value, json};

use crate::support::crm;

/// The `$ref` of the JSON body at `pointer` in `operation`, if any.
fn body_ref(operation: &Value, pointer: &str) -> Option<String> {
    operation
        .pointer(pointer)
        .and_then(|content| content.pointer("/content/application~1json/schema/$ref"))
        .and_then(Value::as_str)
        .map(|reference| {
            reference
                .trim_start_matches("#/components/schemas/")
                .to_owned()
        })
}

/// `(path, method, tag, request schema, success status, response schema)`.
type Operation = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
    Option<&'static str>,
);

#[tokio::test]
async fn every_route_is_documented_with_its_schemas() {
    let crm = crm().await;
    let doc = crm
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let paths = &doc["paths"];

    let expected: &[Operation] = &[
        (
            "/api/v1/companies",
            "get",
            "Companies",
            None,
            "200",
            Some("CompanyPage"),
        ),
        (
            "/api/v1/companies",
            "post",
            "Companies",
            Some("CompanyInput"),
            "201",
            Some("Company"),
        ),
        (
            "/api/v1/companies/{id}",
            "get",
            "Companies",
            None,
            "200",
            Some("CompanyDetail"),
        ),
        (
            "/api/v1/companies/{id}",
            "patch",
            "Companies",
            Some("CompanyUpdateInput"),
            "200",
            Some("Company"),
        ),
        (
            "/api/v1/companies/{id}",
            "delete",
            "Companies",
            None,
            "204",
            None,
        ),
        (
            "/api/v1/contacts",
            "get",
            "Contacts",
            None,
            "200",
            Some("ContactPage"),
        ),
        (
            "/api/v1/contacts",
            "post",
            "Contacts",
            Some("ContactInput"),
            "201",
            Some("Contact"),
        ),
        (
            "/api/v1/contacts/{id}",
            "get",
            "Contacts",
            None,
            "200",
            Some("Contact"),
        ),
        (
            "/api/v1/contacts/{id}",
            "patch",
            "Contacts",
            Some("ContactUpdateInput"),
            "200",
            Some("Contact"),
        ),
        (
            "/api/v1/contacts/{id}",
            "delete",
            "Contacts",
            None,
            "204",
            None,
        ),
        (
            "/api/v1/contacts/export",
            "get",
            "Contacts",
            None,
            "200",
            None,
        ),
    ];
    for (path, method, tag, request, status, response) in expected {
        let operation = &paths[path][method];
        assert!(operation.is_object(), "{method} {path} is not documented");
        assert!(
            operation["summary"].as_str().is_some_and(|s| !s.is_empty()),
            "{method} {path} has no summary"
        );
        assert_eq!(operation["tags"], json!([tag]), "{method} {path}");
        assert_eq!(
            body_ref(operation, "/requestBody").as_deref(),
            *request,
            "{method} {path} request"
        );
        assert!(
            operation["responses"][status].is_object(),
            "{method} {path} has no {status} response: {operation}"
        );
        assert_eq!(
            body_ref(operation, &format!("/responses/{status}")).as_deref(),
            *response,
            "{method} {path} response"
        );
    }

    let schemas = &doc["components"]["schemas"];
    for name in [
        "Company",
        "CompanyDetail",
        "CompanyPage",
        "CompanyInput",
        "CompanyUpdateInput",
        "CompanySort",
        "CompanySize",
        "Contact",
        "ContactPage",
        "ContactInput",
        "ContactUpdateInput",
        "ContactFilters",
        "ContactStatus",
        "OwnerRef",
        "CompanyRef",
    ] {
        assert!(schemas[name].is_object(), "no `{name}` schema");
    }
    // `__type` is the CASL subject type.
    assert_eq!(
        schemas["Company"]["properties"]["__type"]["enum"],
        json!(["Company"])
    );
    assert_eq!(
        schemas["Contact"]["properties"]["__type"]["enum"],
        json!(["Contact"])
    );
    crm.shutdown().await;
}
