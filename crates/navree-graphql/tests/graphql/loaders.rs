//! Dataloaders: one per operation, loads of the same tick batched.

use serde_json::json;
use uuid::Uuid;

use crate::support::{AuthorLoader, app, insert_post};

#[tokio::test]
async fn loads_of_one_operation_are_batched_and_never_shared() {
    let (app, _audit) = app().await;
    let ada = Uuid::now_v7();
    let grace = Uuid::now_v7();
    insert_post(&app, "A", Some(ada), true).await;
    insert_post(&app, "B", Some(grace), true).await;
    insert_post(&app, "C", Some(ada), true).await;

    let body = app
        .client()
        .post("/graphql")
        .json(&json!({ "query": "{ posts { title author { id name } } }" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let posts = body["data"]["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 3);
    assert_eq!(posts[0]["author"]["id"], json!(ada));
    assert_eq!(posts[1]["author"]["id"], json!(grace));

    let loader = app.get::<AuthorLoader>().await;
    let mut expected = vec![ada, grace];
    expected.sort();
    assert_eq!(
        loader.batches(),
        vec![expected.clone()],
        "one batch, no duplicates"
    );

    // A second operation does not reuse the first one's results.
    app.client()
        .post("/graphql")
        .json(&json!({ "query": "{ posts { author { name } } }" }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(loader.batches(), vec![expected.clone(), expected]);
    app.shutdown().await.unwrap();
}
