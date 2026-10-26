//! Every public item `content/docs/microservices/graphql.mdx` names, so a
//! rename fails the build: the module and its options (each config key),
//! the builder extension, `ContextExt`, `EventStream`, the dataloader
//! types, `GraphqlSchema`, the routes and the command.

use std::sync::Arc;

use navree_graphql::{
    Context, ContextExt, EventStream, GraphqlModule, GraphqlOptions, GraphqlResolver,
    GraphqlSchema, ModuleBuilderGraphqlExt, RequestLoader, Result, SharedLoader,
};
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::TestApp;

#[test]
fn every_config_key_is_an_option() {
    let GraphqlOptions {
        path,
        graphiql,
        introspection,
        depth_limit,
        complexity_limit,
        batching,
        subscriptions,
        connection_init_timeout_secs,
        keep_alive_secs,
    } = GraphqlOptions::default();
    assert_eq!(
        (path.as_str(), depth_limit, complexity_limit),
        ("/graphql", 16, 1000)
    );
    assert_eq!(
        (graphiql, introspection),
        (cfg!(debug_assertions), cfg!(debug_assertions))
    );
    assert!(!batching && subscriptions);
    assert_eq!((connection_init_timeout_secs, keep_alive_secs), (10, 15));
}

#[test]
fn the_module_has_its_three_forms() {
    fn module(_: impl Module) {}
    module(GraphqlModule);
    module(GraphqlModule::from_config());
    module(GraphqlModule::for_root(GraphqlOptions {
        depth_limit: 12,
        ..GraphqlOptions::default()
    }));
}

/// `ContextExt`'s methods, with the types the docs give them.
#[allow(dead_code)]
async fn context_ext(ctx: &Context<'_>) -> Result<()> {
    let _: &navree_kernel::AppContext = ctx.app()?;
    let _: navree_db::Uow = ctx.uow()?;
    let _: &navree_contracts::Principal = ctx.principal()?;
    let _: navree_auth::User = ctx.current_user().await?;
    let _: Arc<navree_access::Ability> = ctx.abilities().await?;
    let _: navree_workspace::CurrentWorkspace = ctx.workspace().await?;
    let _: &RequestLoader<()> = ctx.loader::<()>();
    let _: EventStream<Ping> = EventStream::new(ctx)?;
    Ok(())
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Ping;

impl navree_events::Event for Ping {
    const NAME: &'static str = "ping";
}

#[allow(dead_code)]
fn builder_extension<R: GraphqlResolver + navree_kernel::Injectable>(m: &mut ModuleBuilder) {
    m.resolver::<R>().dataloader::<R>();
}

#[allow(dead_code)]
fn shared_loader(loader: Arc<()>) -> SharedLoader<()> {
    SharedLoader::new(loader)
}

#[tokio::test]
async fn the_schema_routes_and_command_exist() {
    let app = TestApp::builder()
        .module(GraphqlModule::from_config())
        .build()
        .await;
    let schema = app.get::<GraphqlSchema>().await;
    let _: String = schema.sdl();
    let _: async_graphql::Response = schema
        .execute(async_graphql::Request::new("{ __typename }"))
        .await;
    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    let mounted: Vec<(String, String)> = routes
        .iter()
        .map(|route| (route.method.clone(), route.full_path.clone()))
        .collect();
    for (method, path) in [
        ("POST", "/graphql"),
        ("GET", "/graphql"),
        ("GET", "/graphql/ws"),
    ] {
        assert!(
            mounted.contains(&(method.to_owned(), path.to_owned())),
            "{method} {path} in {mounted:?}"
        );
    }
    assert!(routes.iter().all(|route| route.access.public));
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("schema.graphql");
    app.run_command("graphql:schema", &[file.to_str().unwrap()])
        .await
        .unwrap();
    app.run_command("graphql:schema", &[file.to_str().unwrap(), "--check"])
        .await
        .unwrap();
    app.shutdown().await.unwrap();
}
