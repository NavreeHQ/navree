//! `Valid<T>` arguments: validation rules before the field runs, with the
//! messages validation gives everywhere else.

use navree_testing::principal;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{app, insert_post, query, stored};

const RENAME: &str =
    "mutation($id: UUID!, $input: RenameInput!) { renamePost(id: $id, input: $input) { title } }";

#[tokio::test]
async fn a_broken_rule_fails_the_field_with_field_errors() {
    let (app, _audit) = app().await;
    let author = Uuid::now_v7();
    let post = insert_post(&app, "Original", Some(author), false).await;
    let body = app
        .acting_as(principal(author))
        .post("/graphql")
        .json(&query(
            RENAME,
            json!({ "id": post.id, "input": { "title": "Fine title", "position": -1 } }),
        ))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"], Value::Null);
    let error = &body["errors"][0];
    assert_eq!(error["message"], "position must not be less than 0");
    assert_eq!(error["path"], json!(["renamePost"]));
    assert_eq!(
        error["extensions"],
        json!({
            "code": "BAD_REQUEST",
            "statusCode": 400,
            "error": "Bad Request",
            "errors": { "input.position": ["position must not be less than 0"] },
        })
    );
    assert_eq!(stored(&app, post.id).await.title, "Original");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn every_message_is_reported_and_names_are_rust_paths() {
    let (app, _audit) = app().await;
    let author = Uuid::now_v7();
    let post = insert_post(&app, "Original", Some(author), false).await;
    let body = app
        .acting_as(principal(author))
        .post("/graphql")
        .json(&query(
            RENAME,
            json!({ "id": post.id, "input": { "title": "x", "tagNames": [] } }),
        ))
        .send()
        .await
        .json_value();
    let error = &body["errors"][0];
    assert_eq!(
        error["extensions"]["errors"],
        json!({
            "input.tag_names": ["tag_names must contain at least 1 elements"],
            "input.title": ["title must be longer than or equal to 2 characters"],
        })
    );
    assert_eq!(
        error["message"],
        "tag_names must contain at least 1 elements; title must be longer than or equal to 2 characters"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_valid_input_reaches_the_resolver() {
    let (app, _audit) = app().await;
    let author = Uuid::now_v7();
    let post = insert_post(&app, "Original", Some(author), false).await;
    app.acting_as(principal(author))
        .post("/graphql")
        .json(&query(
            RENAME,
            json!({ "id": post.id, "input": { "title": "Renamed", "position": 0 } }),
        ))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.renamePost.title", json!("Renamed"));
    assert_eq!(stored(&app, post.id).await.title, "Renamed");
    app.shutdown().await.unwrap();
}
