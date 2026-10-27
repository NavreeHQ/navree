//! `ctx.current_user()` (navree-auth) and `ctx.workspace()`
//! (navree-workspace).

use navree_access::{AccessModule, AccessOptions};
use navree_auth::{AuthModule, AuthService};
use navree_crypto::CryptoModule;
use navree_db::{DatabaseModule, Db};
use navree_graphql::{Context, ContextExt, GraphqlModule, ModuleBuilderGraphqlExt, Result};
use navree_kernel::ModuleBuilder;
use navree_macros::{injectable, module, resolver};
use navree_testing::{TestApp, principal};
use navree_workspace::{DemoWorkspaces, NewWorkspace, WorkspaceModule, Workspaces};
use serde_json::{Value, json};
use uuid::Uuid;

#[injectable]
struct IdentityResolver;

#[resolver]
#[authenticated]
impl IdentityResolver {
    /// The viewer's email address.
    #[query]
    async fn my_email(&self, ctx: &Context<'_>) -> Result<String> {
        Ok(ctx.current_user().await?.email)
    }

    /// The current workspace's name.
    #[query]
    async fn workspace_name(&self, ctx: &Context<'_>) -> Result<String> {
        let workspace = ctx.workspace().await?;
        Ok(workspace.workspace().name.clone())
    }

    /// A mutation inside the current workspace (writes nothing).
    #[mutation]
    async fn touch_workspace(&self, ctx: &Context<'_>) -> Result<String> {
        let workspace = ctx.workspace().await?;
        Ok(workspace.workspace().slug.clone())
    }
}

#[module(register = Self::wire)]
struct IdentityModule;

impl IdentityModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resolver::<IdentityResolver>();
    }
}

async fn app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(AuthModule::from_config())
        .module(WorkspaceModule::from_config())
        .module(GraphqlModule::from_config())
        .module(IdentityModule)
        .fake_auth()
        .build()
        .await
}

async fn user(app: &TestApp, name: &str, email: &str) -> Uuid {
    app.get::<AuthService>()
        .await
        .create_user(name, email, "correct horse battery")
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn current_user_is_the_viewers_record() {
    let app = app().await;
    let ada = user(&app, "Ada", "ada@navree.test").await;
    app.acting_as(principal(ada))
        .post("/graphql")
        .json(&json!({ "query": "{ myEmail }" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.myEmail", json!("ada@navree.test"));

    // A principal whose user is gone is not signed in.
    app.acting_as(principal(Uuid::now_v7()))
        .post("/graphql")
        .json(&json!({ "query": "{ myEmail }" }))
        .send()
        .await
        .assert_json_path("errors.0.extensions.code", json!("UNAUTHENTICATED"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn workspace_comes_from_the_request_and_checks_the_membership() {
    let app = app().await;
    let ada = user(&app, "Ada", "ada@navree.test").await;
    let grace = user(&app, "Grace", "grace@navree.test").await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let workspace = app
        .get::<Workspaces>()
        .await
        .create(&uow, ada, NewWorkspace::named("Acme"))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    app.acting_as(principal(ada))
        .post("/graphql")
        .header("x-workspace", workspace.slug.as_str())
        .json(&json!({ "query": "{ workspaceName }" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.workspaceName", json!("Acme"));

    app.acting_as(principal(grace))
        .post("/graphql")
        .header("x-workspace", workspace.slug.as_str())
        .json(&json!({ "query": "{ workspaceName }" }))
        .send()
        .await
        .assert_json_path("errors.0.extensions.code", json!("FORBIDDEN"));

    app.acting_as(principal(ada))
        .post("/graphql")
        .json(&json!({ "query": "{ workspaceName }" }))
        .send()
        .await
        .assert_json_path("errors.0.extensions.code", json!("BAD_REQUEST"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn read_only_demos_answer_queries_and_refuse_mutations() {
    let app = app().await;
    let ada = user(&app, "Ada", "ada@navree.test").await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let demo = app
        .get::<DemoWorkspaces>()
        .await
        .create(&uow, ada, NewWorkspace::named("Acme demo"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let client = app.acting_as(principal(ada));

    // A query is a POST that reads.
    let read = client
        .post("/graphql")
        .header("x-workspace", demo.slug.as_str())
        .json(&json!({ "query": "{ workspaceName }" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.workspaceName", json!("Acme demo"));
    assert_eq!(read.header("x-navree-demo"), Some("read_only"));
    client
        .post("/graphql")
        .header("x-workspace", demo.slug.as_str())
        .json(&json!({ "query": "mutation { touchWorkspace }" }))
        .send()
        .await
        .assert_json_path("errors.0.extensions.code", json!("FORBIDDEN"))
        .assert_json_path("errors.0.message", json!("Demo workspaces are read-only."));
    app.shutdown().await.unwrap();
}
