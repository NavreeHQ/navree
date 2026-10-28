//! The HTTP endpoint: POST only, GraphiQL in development, batching on
//! request, and the depth, complexity and introspection settings.

use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{RecordingAudit, app, builder};

async fn app_with(settings: &[(&str, Value)]) -> TestApp {
    let audit = RecordingAudit::default();
    let mut builder = builder(&audit);
    for (key, value) in settings {
        builder = builder.config(key, value.clone());
    }
    builder.build().await
}

#[tokio::test]
async fn graphiql_is_served_to_browsers_when_it_is_on() {
    let (app, _audit) = app().await;
    let response = app
        .client()
        .get("/graphql")
        .header("accept", "text/html,application/xhtml+xml")
        .send()
        .await
        .assert_status(200);
    let html = response.text();
    assert!(html.contains("GraphiQL"));
    assert!(html.contains(r#"data-subscriptions="/graphql/ws""#));
    let csp = response.header("content-security-policy").unwrap();
    assert!(csp.contains("script-src 'self';"), "{csp}");
    assert!(
        !csp.contains("unpkg") && !csp.contains("unsafe-eval"),
        "{csp}"
    );

    // Non-browsers get the 405 queries always get on GET.
    app.client().get("/graphql").send().await.assert_error(
        405,
        "Send GraphQL operations as POST requests with a JSON body",
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn graphiql_loads_only_files_the_app_serves() {
    let (app, _audit) = app().await;
    let html = app
        .client()
        .get("/graphql")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    let mut references = Vec::new();
    for attribute in ["src=\"", "href=\""] {
        for part in html.split(attribute).skip(1) {
            references.push(part.split('"').next().unwrap().to_owned());
        }
    }
    // GraphiQL, its stylesheet, React, React DOM and the bootstrap.
    assert_eq!(references.len(), 5, "{references:?}");
    for reference in &references {
        assert!(
            reference.starts_with("/graphql/assets/"),
            "{reference} is not served by the app"
        );
        let response = app.client().get(reference).send().await.assert_status(200);
        assert_eq!(
            response.header("cache-control"),
            Some("public, max-age=31536000, immutable"),
            "{reference}"
        );
        assert!(!response.bytes().is_empty(), "{reference}");
    }
    let script = app
        .client()
        .get("/graphql/assets/graphiql-3.8.3.min.js")
        .send()
        .await;
    assert_eq!(
        script.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert!(script.bytes().len() > 1_000_000, "GraphiQL is embedded");
    // Each library's license is served beside it; nothing else is.
    for license in [
        "graphiql-3.8.3.LICENSE.txt",
        "react-18.3.1.LICENSE.txt",
        "react-dom-18.3.1.LICENSE.txt",
    ] {
        let text = app
            .client()
            .get(&format!("/graphql/assets/{license}"))
            .send()
            .await
            .assert_status(200)
            .text();
        assert!(text.contains("MIT License"), "{license}");
    }
    app.client()
        .get("/graphql/assets/missing.js")
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn without_graphiql_a_get_is_a_405() {
    let app = app_with(&[("graphql.graphiql", json!(false))]).await;
    let response = app
        .client()
        .get("/graphql")
        .header("accept", "text/html")
        .send()
        .await
        .assert_error(405, None);
    assert_eq!(response.header("allow"), Some("POST"));
    // ... and its files are not served either.
    app.client()
        .get("/graphql/assets/graphiql-3.8.3.min.js")
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn queries_must_be_json() {
    let (app, _audit) = app().await;
    app.client()
        .post("/graphql")
        .header("content-type", "text/plain")
        .body(r#"{"query":"{ hello }"}"#)
        .send()
        .await
        .assert_error(415, None);
    let body = app
        .client()
        .post("/graphql")
        .header("content-type", "application/json")
        .body("not json")
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(body["errors"][0]["extensions"]["code"], "BAD_REQUEST");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn batches_need_batching() {
    let batch = json!([{ "query": "{ hello }" }, { "query": "{ postCount }" }]);
    let (app, _audit) = app().await;
    let body = app
        .client()
        .post("/graphql")
        .json(&batch)
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert!(
        body["errors"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("Batched requests are disabled"),
        "{body}"
    );
    app.shutdown().await.unwrap();

    let app = app_with(&[("graphql.batching", json!(true))]).await;
    app.client()
        .post("/graphql")
        .json(&batch)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("0.data.hello", json!("hello"))
        .assert_json_path("1.data.postCount", json!(0));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn depth_and_complexity_limits_reject_the_document() {
    let app = app_with(&[("graphql.depth_limit", json!(2))]).await;
    let body = app
        .client()
        .post("/graphql")
        .json(&json!({ "query": "{ posts { author { name } } }" }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert!(body.get("data").is_none());
    assert!(
        body["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("nested too deep"),
        "{body}"
    );
    app.shutdown().await.unwrap();

    let app = app_with(&[("graphql.complexity_limit", json!(2))]).await;
    let body = app
        .client()
        .post("/graphql")
        .json(&json!({ "query": "{ hello postCount posts { title } }" }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert!(
        body["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("too complex"),
        "{body}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn introspection_lists_only_the_public_schema() {
    let (app, _audit) = app().await;
    let body = app
        .client()
        .post("/graphql")
        .json(&json!({
            "query": "{ __schema { queryType { name } mutationType { name } subscriptionType { name } types { name } } hello }"
        }))
        .send()
        .await
        .json_value();
    assert_eq!(body["data"]["hello"], json!("hello"), "{body}");
    let schema = &body["data"]["__schema"];
    assert_eq!(schema["queryType"]["name"], json!("Query"));
    assert_eq!(schema["mutationType"]["name"], json!("Mutation"));
    assert_eq!(schema["subscriptionType"]["name"], json!("Subscription"));
    let names: Vec<&str> = schema["types"]
        .as_array()
        .expect("types")
        .iter()
        .map(|t| t["name"].as_str().expect("name"))
        .collect();
    for expected in ["Query", "Mutation", "Subscription", "Post"] {
        assert!(names.contains(&expected), "{expected} missing in {names:?}");
    }
    // Only the spec's own introspection types may start with `__`, and no
    // internal resolver part shows.
    let spec = [
        "__Schema",
        "__Type",
        "__TypeKind",
        "__Field",
        "__InputValue",
        "__EnumValue",
        "__Directive",
        "__DirectiveLocation",
    ];
    let leaked: Vec<&&str> = names
        .iter()
        .filter(|name| name.contains("Navree") || (name.starts_with("__") && !spec.contains(name)))
        .collect();
    assert!(leaked.is_empty(), "internal types leaked: {leaked:?}");
    // The root types keep their fields.
    let query = app
        .client()
        .post("/graphql")
        .json(&json!({ "query": "{ __type(name: \"Query\") { fields { name } } }" }))
        .send()
        .await
        .json_value();
    let fields: Vec<&str> = query["data"]["__type"]["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|f| f["name"].as_str().expect("name"))
        .collect();
    assert!(
        fields.contains(&"hello") && fields.contains(&"postCount"),
        "{fields:?}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn introspection_can_be_turned_off() {
    let introspect = json!({ "query": "{ __schema { queryType { name } } }" });
    let (app, _audit) = app().await;
    app.client()
        .post("/graphql")
        .json(&introspect)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.__schema.queryType.name", json!("Query"));
    app.shutdown().await.unwrap();

    let app = app_with(&[("graphql.introspection", json!(false))]).await;
    app.client()
        .post("/graphql")
        .json(&introspect)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.__schema", Value::Null);
    let body = app
        .client()
        .post("/graphql")
        .json(&json!({ "query": "{ __type(name: \"Post\") { name } hello }" }))
        .send()
        .await
        .json_value();
    assert_eq!(body["data"], json!({ "__type": null, "hello": "hello" }));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_endpoint_follows_graphql_path() {
    let app = app_with(&[("graphql.path", json!("/api/graph"))]).await;
    app.client()
        .post("/api/graph")
        .json(&json!({ "query": "{ hello }" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.hello", json!("hello"));
    app.client()
        .post("/graphql")
        .json(&json!({ "query": "{ hello }" }))
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.unwrap();
}
