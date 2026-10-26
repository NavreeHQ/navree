//! Errors keep the JSON error shape in `extensions`, with each error's
//! HTTP status.

use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{app, query};

async fn failing(app: &TestApp, kind: &str) -> Value {
    app.client()
        .post("/graphql")
        .json(&query(
            "query($kind: String!) { failing(kind: $kind) }",
            json!({ "kind": kind }),
        ))
        .send()
        .await
        .assert_status(200)
        .json_value()
}

#[tokio::test]
async fn each_error_carries_its_status_code_and_reason() {
    let (app, _audit) = app().await;
    let cases = [
        ("missing", "NOT_FOUND", 404, "Not Found", "Not Found"),
        (
            "conflict",
            "CONFLICT",
            409,
            "Conflict",
            "That title is taken.",
        ),
        (
            "mapped",
            "CONFLICT",
            409,
            "Conflict",
            "That title is taken.",
        ),
        ("forbidden", "FORBIDDEN", 403, "Forbidden", "Forbidden"),
        (
            "boom",
            "INTERNAL_SERVER_ERROR",
            500,
            "Internal Server Error",
            "Internal server error",
        ),
        (
            "box",
            "INTERNAL_SERVER_ERROR",
            500,
            "Internal Server Error",
            "Internal server error",
        ),
        (
            "plain",
            "BAD_REQUEST",
            400,
            "Bad Request",
            "Pick another kind",
        ),
    ];
    for (kind, code, status, reason, message) in cases {
        let body = failing(&app, kind).await;
        assert_eq!(body["data"], Value::Null, "{kind}");
        let error = &body["errors"][0];
        assert_eq!(error["message"], message, "{kind}: {body}");
        assert_eq!(error["path"], json!(["failing"]), "{kind}");
        assert_eq!(
            error["extensions"],
            json!({ "code": code, "statusCode": status, "error": reason }),
            "{kind}"
        );
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn documents_that_do_not_run_are_400s_without_data() {
    let (app, _audit) = app().await;
    let body = app
        .client()
        .post("/graphql")
        .json(&json!({ "query": "{ hello" }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert!(body.get("data").is_none(), "{body}");
    assert_eq!(body["errors"][0]["extensions"]["code"], "BAD_REQUEST");
    assert_eq!(body["errors"][0]["extensions"]["statusCode"], 400);

    let body = app
        .client()
        .post("/graphql")
        .json(&json!({ "query": "{ nope }" }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert!(
        body["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown field \"nope\""),
        "{body}"
    );
    app.shutdown().await.unwrap();
}
