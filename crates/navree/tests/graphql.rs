//! The `graphql` feature through the facade, written the way the docs
//! write it: `navree::graphql::{ContextExt, Result, resolver}`, the
//! prelude's `Valid`, `GraphqlModule` and `m.resolver::<R>()`.
//!
//! Run with `cargo test -p navree --features graphql --test graphql`.

use async_graphql::{Context, InputObject, SimpleObject};
use garde::Validate;
use navree::graphql::{ContextExt, GraphqlSchema, Result, resolver};
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde_json::json;

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Greeting")]
struct GreetingObject {
    text: String,
    word_count: i32,
}

#[derive(Debug, InputObject, Validate)]
struct RenameInput {
    #[garde(length(chars, min = 2, max = 20))]
    name: String,
}

#[injectable]
struct GreetingResolver;

#[resolver]
impl GreetingResolver {
    /// A greeting.
    #[query]
    #[public]
    async fn greeting(&self, #[graphql(default = "world")] name: String) -> Result<GreetingObject> {
        let text = format!("Hello, {name}!");
        Ok(GreetingObject {
            word_count: text.split_whitespace().count() as i32,
            text,
        })
    }

    /// The viewer's id.
    #[query]
    #[authenticated]
    async fn me(&self, ctx: &Context<'_>) -> Result<String> {
        Ok(ctx.principal()?.user_id.to_string())
    }

    /// Echoes a validated name.
    #[mutation]
    #[authenticated]
    async fn rename(&self, input: Valid<RenameInput>) -> Result<String> {
        let Valid(RenameInput { name }) = input;
        Ok(name)
    }
}

#[module(register = Self::wire)]
struct GreetingModule;

impl GreetingModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resolver::<GreetingResolver>();
    }
}

#[tokio::test]
async fn resolvers_written_against_the_facade_serve_graphql() {
    let app = TestApp::builder()
        .module(GraphqlModule::from_config())
        .module(GreetingModule)
        .build()
        .await;
    app.client()
        .post("/graphql")
        .json(&json!({ "query": "{ greeting { text wordCount } }" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.greeting.text", json!("Hello, world!"))
        .assert_json_path("data.greeting.wordCount", json!(2));
    app.client()
        .post("/graphql")
        .json(&json!({ "query": "{ me }" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!(null))
        .assert_json_path("errors.0.extensions.code", json!("UNAUTHENTICATED"));

    let user = uuid::Uuid::now_v7();
    app.acting_as(principal(user))
        .post("/graphql")
        .json(&json!({ "query": "mutation { rename(input: { name: \"x\" }) }" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "errors.0.extensions.errors",
            json!({ "input.name": ["name must be longer than or equal to 2 characters"] }),
        );

    let schema = app.get::<GraphqlSchema>().await;
    let response = schema
        .execute(async_graphql::Request::new("{ me }").data(principal(user)))
        .await;
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "me": user.to_string() })
    );
    assert!(
        schema
            .sdl()
            .contains("greeting(name: String! = \"world\"): Greeting!")
    );
    app.shutdown().await.unwrap();
}
