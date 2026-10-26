//! `graphql:schema [path] [--check]`.

use navree_graphql::GraphqlSchema;

use crate::support::app;

#[tokio::test]
async fn the_command_writes_the_sdl_and_checks_it() {
    let (app, _audit) = app().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("schema.graphql");
    let shown = path.to_str().unwrap();

    app.run_command("graphql:schema", &[shown]).await.unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written, app.get::<GraphqlSchema>().await.sdl());

    app.run_command("graphql:schema", &[shown, "--check"])
        .await
        .expect("the file is up to date");
    // A Windows checkout's line endings do not count as a difference.
    std::fs::write(&path, written.replace('\n', "\r\n")).unwrap();
    app.run_command("graphql:schema", &[shown, "--check"])
        .await
        .expect("CRLF is the same schema");

    std::fs::write(&path, written.replace("\thello: String!\n", "")).unwrap();
    let error = app
        .run_command("graphql:schema", &[shown, "--check"])
        .await
        .expect_err("the file is out of date")
        .to_string();
    assert!(error.contains("is out of date"), "{error}");
    assert!(error.contains("+\thello: String!"), "{error}");

    let missing = dir.path().join("missing.graphql");
    let error = app
        .run_command("graphql:schema", &[missing.to_str().unwrap(), "--check"])
        .await
        .expect_err("a missing file is out of date")
        .to_string();
    assert!(error.contains("+type Query {"), "{error}");

    let error = app
        .run_command("graphql:schema", &["--force"])
        .await
        .expect_err("unknown option")
        .to_string();
    assert!(error.contains("unknown option `--force`"), "{error}");
    app.shutdown().await.unwrap();
}
