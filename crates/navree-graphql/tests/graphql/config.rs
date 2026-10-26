//! `GraphqlModule::for_root`, `from_config`, the plain module and the
//! `APP_GRAPHQL__*` environment variables.

use navree_config::ConfigSources;
use navree_graphql::{GraphqlModule, GraphqlOptions, GraphqlSchema};
use navree_testing::TestApp;

#[tokio::test]
async fn for_root_takes_the_options() {
    let app = TestApp::builder()
        .module(GraphqlModule::for_root(GraphqlOptions {
            depth_limit: 12,
            ..GraphqlOptions::default()
        }))
        .build()
        .await;
    assert_eq!(app.get::<GraphqlOptions>().await.depth_limit, 12);
    assert_eq!(app.get::<GraphqlSchema>().await.options().depth_limit, 12);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_options_fail_the_boot() {
    let error = TestApp::builder()
        .module(GraphqlModule::for_root(GraphqlOptions {
            path: "graphql".to_owned(),
            ..GraphqlOptions::default()
        }))
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("graphql.path"), "{error}");

    let error = TestApp::builder()
        .config("graphql.complexity_limit", 0)
        .module(GraphqlModule::from_config())
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("graphql.complexity_limit"), "{error}");
}

#[tokio::test]
async fn from_config_and_the_plain_module_read_the_graphql_section() {
    for plain in [false, true] {
        let builder = TestApp::builder()
            .config("graphql.path", "/gql")
            .config("graphql.batching", true);
        let builder = if plain {
            builder.module(GraphqlModule)
        } else {
            builder.module(GraphqlModule::from_config())
        };
        let app = builder.build().await;
        let options = app.get::<GraphqlOptions>().await;
        assert_eq!(options.path, "/gql");
        assert!(options.batching);
        assert_eq!(options.complexity_limit, 1000, "the defaults fill the rest");
        app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn environment_variables_override_the_section() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_GRAPHQL__INTROSPECTION", "false"),
            ("APP_GRAPHQL__COMPLEXITY_LIMIT", "300"),
        ],
    );
    let app = TestApp::builder()
        .sources(sources)
        .module(GraphqlModule::from_config())
        .build()
        .await;
    let options = app.get::<GraphqlOptions>().await;
    assert!(!options.introspection);
    assert_eq!(options.complexity_limit, 300);
    app.shutdown().await.unwrap();
}
