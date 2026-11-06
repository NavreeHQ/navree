//! `RouteDoc`: query parameters, other responses and security added to the document.

use navree_api::{ApiError, ApiOptions, RouteDoc, openapi_document_with};
use navree_http::list::{Field, ListResource, ListSpec};
use navree_http::{AccessSpec, RouteMeta};
use serde_json::{Value, json};

struct Deals;

impl ListResource for Deals {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::one_of("stage", &["lead", "won"]),
        Field::uuid("owner_id"),
    ])
    .sortable(&["created_at"])
    .searchable(&["title"]);
}

fn route(method: &str, path: &str, name: Option<&str>, public: bool) -> RouteMeta {
    RouteMeta {
        method: method.to_owned(),
        full_path: path.to_owned(),
        name: name.map(str::to_owned),
        access: AccessSpec {
            public,
            authenticated: !public,
            ..AccessSpec::default()
        },
        ..RouteMeta::default()
    }
}

fn document(routes: &[RouteMeta], docs: &[RouteDoc], security: Option<&[String]>) -> Value {
    openapi_document_with(routes, docs, security, &ApiOptions::default()).expect("a document")
}

#[test]
fn a_list_route_documents_its_query() {
    let routes = [route("GET", "/api/v1/deals", None, false)];
    let doc = document(&routes, &[RouteDoc::get("/api/v1/deals").list::<Deals>()], None);

    let parameters = doc["paths"]["/api/v1/deals"]["get"]["parameters"]
        .as_array()
        .expect("parameters");
    let names: Vec<&str> = parameters.iter().filter_map(|p| p["name"].as_str()).collect();
    assert_eq!(
        names,
        ["filter", "sort", "search", "page", "per_page", "cursor", "limit"]
    );
    assert!(parameters.iter().all(|p| p["in"] == "query"));
    let filter = &parameters[0];
    assert_eq!(filter["style"], "deepObject");
    assert_eq!(
        filter["schema"]["properties"]["stage"]["oneOf"][0]["enum"],
        json!(["lead", "won"])
    );
    assert_eq!(
        filter["schema"]["properties"]["owner_id"]["oneOf"][0]["format"],
        "uuid"
    );
}

#[test]
fn a_route_that_picks_rows_without_paging_documents_filter_sort_and_search_only() {
    let routes = [route("GET", "/api/v1/deals/export", None, false)];
    let doc = document(
        &routes,
        &[RouteDoc::get("/api/v1/deals/export").filters::<Deals>()],
        None,
    );
    let names: Vec<&str> = doc["paths"]["/api/v1/deals/export"]["get"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(names, ["filter", "sort", "search"]);
}

#[test]
fn query_parameters_follow_the_path_parameters() {
    let routes = [route("GET", "/api/v1/deals/{id}/notes", None, false)];
    let doc = document(
        &routes,
        &[RouteDoc::get("/api/v1/deals/{id}/notes").list::<Deals>()],
        None,
    );
    let parameters = doc["paths"]["/api/v1/deals/{id}/notes"]["get"]["parameters"]
        .as_array()
        .unwrap();
    assert_eq!(parameters[0]["name"], "id");
    assert_eq!(parameters[0]["in"], "path");
    assert_eq!(parameters[1]["name"], "filter");
}

#[test]
fn a_doc_finds_its_route_by_name_or_by_method_and_path() {
    let routes = [
        route("GET", "/health/live", Some("health.live"), true),
        route("POST", "/api/v1/deals", None, false),
        route("GET", "/api/v1/deals", None, false),
    ];
    let docs = [
        RouteDoc::named("health.live").json_schema_response(200, "Up", json_object()),
        RouteDoc::get("/api/v1/deals").list::<Deals>(),
    ];
    let doc = document(&routes, &docs, None);

    assert!(doc["paths"]["/health/live"]["get"]["responses"]["200"]["content"]["application/json"]["schema"].is_object());
    // Only the GET of the path was described, not the POST.
    assert!(doc["paths"]["/api/v1/deals"]["get"]["parameters"].is_array());
    assert!(doc["paths"]["/api/v1/deals"]["post"].get("parameters").is_none());
}

fn json_object() -> navree_openapi::Schema {
    navree_openapi::Schema::object()
}

#[test]
fn a_response_that_is_not_json_is_documented_with_its_content_type() {
    let routes = [route("GET", "/api/v1/contacts/export", None, false)];
    let docs = [RouteDoc::get("/api/v1/contacts/export").text_response(
        200,
        "text/csv",
        "The contacts, one row each.",
    )];
    let doc = document(&routes, &docs, None);

    let ok = &doc["paths"]["/api/v1/contacts/export"]["get"]["responses"]["200"];
    assert_eq!(ok["description"], "The contacts, one row each.");
    assert_eq!(ok["content"]["text/csv"]["schema"], json!({"type": "string"}));
    assert!(ok["content"].get("application/json").is_none());
}

#[test]
fn another_success_status_replaces_the_default_one() {
    let routes = [route("GET", "/app/{key}", Some("realtime.connect"), true)];
    let docs = [RouteDoc::named("realtime.connect")
        .success_status(101)
        .empty_response(101, "Switching Protocols.")];
    let doc = document(&routes, &docs, None);

    let responses = doc["paths"]["/app/{key}"]["get"]["responses"]
        .as_object()
        .unwrap();
    assert_eq!(responses.keys().collect::<Vec<_>>(), ["101"]);
    assert_eq!(responses["101"]["description"], "Switching Protocols.");
}

#[test]
fn a_doc_can_add_a_response_with_a_type() {
    #[derive(navree_openapi::ToSchema)]
    #[allow(dead_code)]
    struct Receipt {
        number: String,
    }
    let routes = [route("POST", "/api/v1/deals/{id}/receipt", None, false)];
    let docs = [RouteDoc::post("/api/v1/deals/{id}/receipt").json_response::<Receipt>(201, "Created")];
    let doc = document(&routes, &docs, None);

    assert_eq!(
        doc["paths"]["/api/v1/deals/{id}/receipt"]["post"]["responses"]["201"]["content"]
            ["application/json"]["schema"]["$ref"],
        "#/components/schemas/Receipt"
    );
    assert_eq!(
        doc["components"]["schemas"]["Receipt"]["properties"]["number"]["type"],
        "string"
    );
}

#[test]
fn a_route_can_name_the_security_it_needs() {
    let routes = [
        route("GET", "/api/v1/deals", None, false),
        route("GET", "/metrics", Some("metrics"), true),
    ];
    let docs = [RouteDoc::named("metrics").security(&["bearer"])];
    let doc = document(&routes, &docs, None);

    assert_eq!(
        doc["paths"]["/metrics"]["get"]["security"],
        json!([{"bearer": []}])
    );
    // The route without a doc keeps both schemes.
    assert_eq!(
        doc["paths"]["/api/v1/deals"]["get"]["security"],
        json!([{"cookie": []}, {"bearer": []}])
    );

    let none = [RouteDoc::get("/api/v1/deals").security(&[])];
    let open = document(&routes[..1], &none, None);
    assert!(open["paths"]["/api/v1/deals"]["get"].get("security").is_none());
}

#[test]
fn an_app_that_uses_only_bearer_tokens_lists_no_cookie_scheme() {
    let routes = [
        route("GET", "/api/v1/deals", None, false),
        route("GET", "/health/live", Some("health.live"), true),
    ];
    let bearer = vec!["bearer".to_owned()];
    let doc = document(&routes, &[], Some(&bearer));

    assert_eq!(
        doc["paths"]["/api/v1/deals"]["get"]["security"],
        json!([{"bearer": []}])
    );
    let schemes = doc["components"]["securitySchemes"].as_object().unwrap();
    assert_eq!(schemes.keys().collect::<Vec<_>>(), ["bearer"]);
    // A public route stays open.
    assert!(doc["paths"]["/health/live"]["get"].get("security").is_none());
}

#[test]
fn a_doc_for_a_route_the_app_does_not_have_stops_the_boot() {
    let routes = [route("GET", "/api/v1/deals", None, false)];
    let error = openapi_document_with(
        &routes,
        &[RouteDoc::get("/api/v1/dealz").list::<Deals>()],
        None,
        &ApiOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(error, ApiError::UnknownRoute { .. }), "{error}");
    assert!(error.to_string().contains("GET /api/v1/dealz"), "{error}");

    let named = openapi_document_with(
        &routes,
        &[RouteDoc::named("deals.missing")],
        None,
        &ApiOptions::default(),
    )
    .unwrap_err();
    assert!(named.to_string().contains("deals.missing"), "{named}");

    // An optional doc is for a route another module may not mount.
    assert!(
        openapi_document_with(
            &routes,
            &[RouteDoc::named("deals.missing").optional()],
            None,
            &ApiOptions::default(),
        )
        .is_ok()
    );
}

#[test]
fn a_scheme_the_document_does_not_define_is_refused() {
    let routes = [route("GET", "/api/v1/deals", None, false)];
    let error = openapi_document_with(
        &routes,
        &[RouteDoc::get("/api/v1/deals").security(&["basic"])],
        None,
        &ApiOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(error, ApiError::UnknownSecurityScheme { .. }), "{error}");

    let app_wide = vec!["oauth".to_owned()];
    assert!(
        openapi_document_with(&routes, &[], Some(&app_wide), &ApiOptions::default()).is_err()
    );
}
