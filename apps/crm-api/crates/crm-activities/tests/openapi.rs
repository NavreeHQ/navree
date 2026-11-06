//! Every activities and dashboard route is documented in
//! `/api/openapi.json`, with the schema names crm-web's types use.

mod support;

use serde_json::{Value, json};

use support::crm;

/// The `$ref` target of a JSON response, if any.
fn response_ref(document: &Value, path: &str, method: &str, status: &str) -> Option<String> {
    document["paths"][path][method]["responses"][status]["content"]["application/json"]["schema"]
        ["$ref"]
        .as_str()
        .map(str::to_owned)
}

#[tokio::test]
async fn routes_are_documented_with_their_schemas() {
    let crm = crm().await;
    let document = crm
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let schema = |name: &str| format!("#/components/schemas/{name}");

    assert_eq!(
        response_ref(&document, "/api/v1/activities", "get", "200"),
        Some(schema("ActivityPage"))
    );
    // The list query is the list-query grammar: the prose names its
    // parameters.
    let description = document["paths"]["/api/v1/activities"]["get"]["description"]
        .as_str()
        .unwrap_or_default();
    for parameter in [
        "filter[subject_type]",
        "filter[subject_id]",
        "filter[kind]",
        "filter[assignee_id]",
        "filter[due]",
        "filter[completed]",
        "per_page",
    ] {
        assert!(
            description.contains(parameter),
            "the list does not describe `{parameter}`: {description}"
        );
    }
    assert_eq!(
        response_ref(&document, "/api/v1/activities", "post", "201"),
        Some(schema("Activity"))
    );
    assert_eq!(
        document["paths"]["/api/v1/activities"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"],
        json!(schema("ActivityInput"))
    );
    assert_eq!(
        response_ref(&document, "/api/v1/activities/{id}", "patch", "200"),
        Some(schema("Activity"))
    );
    assert_eq!(
        document["paths"]["/api/v1/activities/{id}"]["patch"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"],
        json!(schema("ActivityUpdateInput"))
    );
    assert!(
        document["paths"]["/api/v1/activities/{id}"]["delete"]["responses"]["204"].is_object(),
        "{}",
        document["paths"]["/api/v1/activities/{id}"]
    );
    assert_eq!(
        response_ref(&document, "/api/v1/activities/{id}/complete", "post", "200"),
        Some(schema("Activity"))
    );
    assert_eq!(
        response_ref(&document, "/api/v1/dashboard", "get", "200"),
        Some(schema("Dashboard"))
    );

    let components = &document["components"]["schemas"];
    let activity = &components["Activity"]["properties"];
    let required = components["Activity"]["required"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Every field is always sent (nullable ones as `null`).
    for field in [
        "__type",
        "id",
        "kind",
        "subject",
        "body",
        "due_at",
        "completed_at",
        "subject_type",
        "subject_id",
        "subject_label",
        "owner_id",
        "owner",
        "assignee_id",
        "assignee",
        "created_at",
    ] {
        assert!(activity[field].is_object(), "Activity.{field}: {activity}");
        assert!(
            required.contains(&json!(field)),
            "Activity.{field} is not required: {required:?}"
        );
    }
    let page = &components["ActivityPage"]["properties"];
    assert_eq!(page["data"]["items"]["$ref"], json!(schema("Activity")));
    for field in ["current_page", "per_page", "total", "last_page"] {
        assert!(
            page["meta"]["properties"][field].is_object(),
            "ActivityPage.meta.{field}: {page}"
        );
    }
    let dashboard = &components["Dashboard"]["properties"];
    for field in [
        "pipeline",
        "won_this_month",
        "open_deals",
        "tasks",
        "recent_activities",
    ] {
        assert!(
            dashboard[field].is_object(),
            "Dashboard.{field}: {dashboard}"
        );
    }
    // `__type` is the one value the CASL subject type is, and the subject type enum has the name
    // crm-web's types use.
    assert_eq!(
        activity["__type"]["enum"],
        json!(["Activity"]),
        "Activity.__type: {activity}"
    );
    assert!(components["ActivitySubjectType"]["enum"].is_array());
    assert!(
        components.get("SubjectType").is_none(),
        "`SubjectType` is still a component"
    );
    let update = &components["ActivityUpdateInput"];
    assert!(
        update["required"].as_array().is_none_or(Vec::is_empty),
        "every update field is optional: {update}"
    );
    // Only body, due_at and assignee_id may be cleared with null.
    let nullable = |field: &str| {
        let property = &update["properties"][field];
        property["type"]
            .as_array()
            .is_some_and(|types| types.contains(&json!("null")))
            || property["oneOf"]
                .as_array()
                .is_some_and(|options| options.contains(&json!({ "type": "null" })))
    };
    for field in ["body", "due_at", "assignee_id"] {
        assert!(nullable(field), "{field}: {update}");
    }
    for field in ["kind", "subject", "subject_type", "subject_id"] {
        assert!(!nullable(field), "{field}: {update}");
    }
    crm.shutdown().await;
}
