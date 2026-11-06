//! Every deal route is in the OpenAPI document with its schemas (crm-web
//! generates its API types from it).

use crm_testkit::Role;
use serde_json::{Value, json};

use crate::support::crm;

/// The `$ref` (or the schema) of `operation`'s JSON request body.
fn request_schema(operation: &Value) -> Value {
    operation["requestBody"]["content"]["application/json"]["schema"].clone()
}

/// The schema of `operation`'s `status` JSON response.
fn response_schema(operation: &Value, status: &str) -> Value {
    operation["responses"][status]["content"]["application/json"]["schema"].clone()
}

fn reference(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

#[tokio::test]
async fn openapi_documents_every_deal_route() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let document = rep
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let paths = &document["paths"];

    let operations = [
        ("/api/v1/deals", "get"),
        ("/api/v1/deals", "post"),
        ("/api/v1/deals/board", "get"),
        ("/api/v1/deals/{id}", "get"),
        ("/api/v1/deals/{id}", "patch"),
        ("/api/v1/deals/{id}", "delete"),
        ("/api/v1/deals/{id}/move", "post"),
    ];
    for (path, method) in operations {
        let operation = &paths[path][method];
        assert_eq!(operation["tags"], json!(["Deals"]), "{method} {path}");
        assert!(operation["summary"].is_string(), "{method} {path}");
    }

    let deals = &paths["/api/v1/deals"];
    assert_eq!(response_schema(&deals["get"], "200"), reference("DealPage"));
    assert_eq!(request_schema(&deals["post"]), reference("DealInput"));
    assert_eq!(response_schema(&deals["post"], "201"), reference("Deal"));
    let board = &paths["/api/v1/deals/board"]["get"];
    assert_eq!(response_schema(board, "200"), reference("DealBoard"));
    let deal = &paths["/api/v1/deals/{id}"];
    assert_eq!(response_schema(&deal["get"], "200"), reference("Deal"));
    assert_eq!(request_schema(&deal["patch"]), reference("DealUpdateInput"));
    assert_eq!(response_schema(&deal["patch"], "200"), reference("Deal"));
    assert!(deal["delete"]["responses"]["204"].is_object(), "{deal}");
    let moved = &paths["/api/v1/deals/{id}/move"]["post"];
    assert_eq!(request_schema(moved), reference("DealMoveInput"));
    assert_eq!(response_schema(moved, "200"), reference("Deal"));

    // The resource lists every contract field; required inputs are marked.
    let schemas = &document["components"]["schemas"];
    let mut fields: Vec<&str> = schemas["Deal"]["properties"]
        .as_object()
        .expect("Deal properties")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "__type",
            "amount_cents",
            "closed_at",
            "company",
            "company_id",
            "contact",
            "contact_id",
            "created_at",
            "currency",
            "expected_close_date",
            "id",
            "lost_reason",
            "owner",
            "owner_id",
            "position",
            "probability",
            "stage",
            "title",
            "updated_at",
        ]
    );
    assert_eq!(
        schemas["DealInput"]["required"],
        json!(["title", "company_id", "amount_cents"])
    );
    assert_eq!(schemas["DealMoveInput"]["required"], json!(["stage"]));
    assert_eq!(
        schemas["Deal"]["properties"]["__type"]["enum"],
        json!(["Deal"])
    );
    assert!(
        schemas.get("Stage").is_none(),
        "`Stage` is still a component"
    );
    for name in [
        "BoardStage",
        "DealStage",
        "CompanyRef",
        "ContactRef",
        "OwnerRef",
    ] {
        assert!(schemas[name].is_object(), "no `{name}` schema");
    }
    crm.shutdown().await;
}
