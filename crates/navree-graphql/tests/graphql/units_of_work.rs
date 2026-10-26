//! Each mutation field runs in a unit of work of its own; query fields
//! share one that commits nothing.

use std::time::Duration;

use navree_events::EventBus;
use navree_testing::principal;
use serde_json::json;
use uuid::Uuid;

use crate::support::{PostCreated, app, insert_post, query, stored};

#[tokio::test]
async fn a_mutation_commits_when_it_returns_ok_and_rolls_back_otherwise() {
    let (app, _audit) = app().await;
    let author = Uuid::now_v7();
    let post = insert_post(&app, "Original", Some(author), false).await;
    let body = app
        .acting_as(principal(author))
        .post("/graphql")
        .json(&query(
            "mutation($id: UUID!) {
                first: renamePost(id: $id, input: { title: \"One\" }) { title }
                second: renamePost(id: $id, input: { title: \"fail-after-write\" }) { title }
            }",
            json!({ "id": post.id }),
        ))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["errors"][0]["path"], json!(["second"]), "{body}");
    assert_eq!(body["errors"][0]["message"], "failed after writing");
    // The first mutation committed on its own; the second rolled back.
    assert_eq!(stored(&app, post.id).await.title, "One");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn work_queued_after_commit_runs_once_the_mutation_commits() {
    let (app, _audit) = app().await;
    let mut created = app
        .get::<EventBus>()
        .await
        .subscribe_json(<PostCreated as navree_events::Event>::NAME);
    let me = Uuid::now_v7();
    let body = app
        .acting_as(principal(me))
        .post("/graphql")
        .json(&json!({ "query": "mutation { createPost(title: \"Hello\") { id title } }" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let id = body["data"]["createPost"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let event = tokio::time::timeout(Duration::from_secs(5), created.recv())
        .await
        .expect("the event in time")
        .unwrap();
    assert_eq!(event["id"], id.as_str());
    let id = Uuid::parse_str(&id).unwrap();
    assert_eq!(stored(&app, id).await.author_id, Some(me));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn query_fields_commit_nothing() {
    let (app, _audit) = app().await;
    insert_post(&app, "Public", None, true).await;
    for _ in 0..3 {
        app.client()
            .post("/graphql")
            .json(&json!({ "query": "{ posts { title } postCount }" }))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("data.postCount", json!(1));
    }
    app.shutdown().await.unwrap();
}
