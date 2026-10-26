//! The schema: one `Query`, `Mutation` and `Subscription` type from every
//! module's resolvers, and the boot checks.

use async_graphql::Request;
use navree_graphql::{GraphqlModule, GraphqlSchema, ModuleBuilderGraphqlExt};
use navree_kernel::ModuleBuilder;
use navree_macros::{injectable, module, resolver};
use navree_testing::TestApp;
use serde_json::json;

use crate::support::{RecordingAudit, app, builder};

#[tokio::test]
async fn resolvers_from_every_module_share_the_roots() {
    let (app, _audit) = app().await;
    let schema = app.get::<GraphqlSchema>().await;
    let response = schema.execute("{ hello postCount viewer }").await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "hello": "hello", "postCount": 0, "viewer": null })
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_sdl_has_the_merged_roots_and_no_internal_types() {
    let (app, _audit) = app().await;
    let sdl = app.get::<GraphqlSchema>().await.sdl();
    for expected in [
        "type Query {",
        "hello: String!",
        "postCount: Int!",
        "post(id: UUID!): Post!",
        "type Mutation {",
        "renamePost(id: UUID!, input: RenameInput!): Post!",
        "type Subscription {",
        "postCreated(\n\t\t\"\"\"\n\t\tOnly titles starting with this",
        "input RenameInput {",
        "type Post {",
        "author: Author",
        "\"\"\"\n\tA greeting anyone may read.\n\t\"\"\"",
    ] {
        assert!(sdl.contains(expected), "missing {expected:?} in\n{sdl}");
    }
    assert!(!sdl.contains("__Navree"), "{sdl}");
    assert!(!sdl.contains("authorId"), "{sdl}");
    app.shutdown().await.unwrap();
}

#[injectable]
struct DuplicateResolver;

#[resolver]
impl DuplicateResolver {
    #[query]
    #[public]
    async fn hello(&self) -> &'static str {
        "again"
    }
}

#[module(register = Self::wire)]
struct DuplicateModule;

impl DuplicateModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resolver::<DuplicateResolver>();
    }
}

#[tokio::test]
async fn two_resolvers_with_the_same_root_field_fail_the_boot() {
    let audit = RecordingAudit::default();
    let error = builder(&audit)
        .module(DuplicateModule)
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot fails")
        .to_string();
    assert!(
        error.contains("two resolvers define the GraphQL query field `hello`"),
        "{error}"
    );
    assert!(
        error.contains("`PostResolver` (module `PostsModule`)"),
        "{error}"
    );
    assert!(
        error.contains("`DuplicateResolver` (module `DuplicateModule`)"),
        "{error}"
    );
}

#[injectable]
struct GuardedResolver;

#[resolver]
impl GuardedResolver {
    #[query]
    #[roles("admin")]
    async fn secret(&self) -> &'static str {
        "secret"
    }
}

#[module(register = Self::wire)]
struct GuardedModule;

impl GuardedModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resolver::<GuardedResolver>();
    }
}

#[tokio::test]
async fn a_field_with_roles_needs_a_route_authorizer() {
    let error = TestApp::builder()
        .module(GraphqlModule::from_config())
        .module(GuardedModule)
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot fails")
        .to_string();
    assert!(
        error.contains("GraphQL query `secret` of `GuardedResolver` (module `GuardedModule`)"),
        "{error}"
    );
    assert!(error.contains("add AccessModule"), "{error}");
}

#[tokio::test]
async fn batches_run_one_operation_after_the_other() {
    let (app, _audit) = app().await;
    let schema = app.get::<GraphqlSchema>().await;
    let batch =
        async_graphql::BatchRequest::Batch(vec![Request::new("{ hello }"), Request::new("{ me }")]);
    let async_graphql::BatchResponse::Batch(responses) = schema.execute_batch(batch).await else {
        panic!("a batch answers with a batch");
    };
    assert_eq!(responses.len(), 2);
    assert!(responses[0].errors.is_empty());
    assert_eq!(responses[1].errors[0].message, "Unauthorized");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn execute_stream_runs_subscriptions_without_http() {
    use async_graphql::futures_util::StreamExt;

    let (app, _audit) = app().await;
    let schema = app.get::<GraphqlSchema>().await;
    let me = uuid::Uuid::now_v7();
    let mut stream = schema.execute_stream(
        Request::new("subscription { postCreated(prefix: \"Hello\") { title } }")
            .data(navree_testing::principal(me)),
    );
    let first = tokio::spawn(async move { stream.next().await });
    let bus = app.get::<navree_events::EventBus>().await;
    for attempt in 0..100 {
        let title = if attempt % 2 == 0 {
            "Skipped"
        } else {
            "Hello there"
        };
        bus.emit(crate::support::PostCreated {
            id: uuid::Uuid::now_v7(),
            title: title.to_owned(),
        })
        .await
        .unwrap();
        if first.is_finished() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let response = first.await.unwrap().expect("one event");
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "postCreated": { "title": "Hello there" } })
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_app_without_resolvers_boots_with_an_empty_schema() {
    let app = TestApp::builder()
        .module(GraphqlModule::from_config())
        .build()
        .await;
    let response = app
        .get::<GraphqlSchema>()
        .await
        .execute(Request::new("{ nothing }"))
        .await;
    assert_eq!(response.errors.len(), 1);
    app.shutdown().await.unwrap();
}
