//! Dataloaders: one per operation, loads of the same tick batched; an
//! unregistered one is a programming error.

use navree_graphql::{Context, ContextExt, GraphqlSchema, ModuleBuilderGraphqlExt};
use navree_kernel::ModuleBuilder;
use navree_macros::{injectable, module, resolver};
use serde_json::json;
use uuid::Uuid;

use crate::support::{AuthorLoader, RecordingAudit, app, builder, insert_post};

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

/// A loader no module registered.
struct Unregistered;

#[injectable]
struct LonelyResolver;

#[resolver]
#[public]
impl LonelyResolver {
    #[query]
    async fn lonely(&self, ctx: &Context<'_>) -> bool {
        let _ = ctx.loader::<Unregistered>();
        true
    }
}

#[module(register = Self::wire)]
struct LonelyModule;

impl LonelyModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resolver::<LonelyResolver>();
    }
}

#[tokio::test]
#[should_panic(expected = "no dataloader graphql::loaders::Unregistered in this operation")]
async fn asking_for_an_unregistered_loader_panics_naming_it() {
    let app = builder(&RecordingAudit::default())
        .module(LonelyModule)
        .build()
        .await;
    let schema = app.get::<GraphqlSchema>().await;
    let _ = schema.execute("{ lonely }").await;
}
