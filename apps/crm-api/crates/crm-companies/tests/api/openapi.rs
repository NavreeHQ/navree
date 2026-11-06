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
        "CompanySize",
        "Contact",
        "ContactPage",
        "ContactInput",
        "ContactUpdateInput",
        "ContactStatus",
        "OwnerRef",
        "CompanyRef",
    ] {
        assert!(schemas[name].is_object(), "no `{name}` schema");
    }
    // The list queries are the list-query grammar, not schemas: the
    // operations' prose names the `filter[..]` parameters.
    for name in ["CompanyFilters", "CompanySort", "ContactFilters"] {
        assert!(schemas[name].is_null(), "`{name}` is no longer a schema");
    }
    for (path, parameters) in [
        (
            "/api/v1/companies",
            ["filter[owner_id]", "filter[industry]", "sort", "search"].as_slice(),
        ),
        (
            "/api/v1/contacts",
            [
                "filter[company_id]",
                "filter[status]",
                "filter[owner_id]",
                "search",
            ]
            .as_slice(),
        ),
        (
            "/api/v1/contacts/export",
            ["filter[...]", "search"].as_slice(),
        ),
    ] {
        let description = paths[path]["get"]["description"]
            .as_str()
            .unwrap_or_default();
        for parameter in parameters {
            assert!(
                description.contains(parameter),
                "{path} does not describe `{parameter}`: {description}"
            );
        }
    }
    // ... and the operations carry them as parameters too: `filter` (written
    // `filter[field]=value`), `sort` (companies only: contacts keep one order),
    // `search` and the paging.
    let parameter_names = |path: &str| -> Vec<String> {
        paths[path]["get"]["parameters"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter(|p| p["in"] == "query")
                    .filter_map(|p| p["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    for (path, expected) in [
        (
            "/api/v1/companies",
            ["filter", "sort", "search", "page", "per_page"].as_slice(),
        ),
        (
            "/api/v1/contacts",
            ["filter", "search", "page", "per_page"].as_slice(),
        ),
    ] {
        let names = parameter_names(path);
        for expected in expected {
            assert!(names.iter().any(|n| n == expected), "{path}: {names:?}");
        }
    }
    assert!(
        !parameter_names("/api/v1/contacts")
            .iter()
            .any(|n| n == "sort"),
        "contacts are listed in one order"
    );
    let companies_filter = paths["/api/v1/companies"]["get"]["parameters"]
        .as_array()
        .and_then(|list| list.iter().find(|p| p["name"] == "filter"))
        .expect("a filter parameter");
    assert_eq!(companies_filter["style"], "deepObject");
    assert!(companies_filter["schema"]["properties"]["industry"].is_object());
    // The export reads the filters but is not paged, and answers with a CSV file.
    assert_eq!(
        parameter_names("/api/v1/contacts/export"),
        ["filter", "search"].map(str::to_owned)
    );
    assert!(
        paths["/api/v1/contacts/export"]["get"]["responses"]["200"]["content"]["text/csv"]
            ["schema"]
            .is_object()
    );
    // The CRM's clients send a bearer token: the document has no cookie scheme.
    assert_eq!(
        doc["components"]["securitySchemes"]
            .as_object()
            .map(|schemes| schemes.keys().cloned().collect::<Vec<_>>()),
        Some(vec!["bearer".to_owned()])
    );
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
