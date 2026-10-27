//! `MongoModule` in an app: options from `database.mongodb` or given, the
//! connection, the `mongodb` readiness check and telemetry. The first
//! tests need no server.

use navree_contracts::{HealthCheck, HealthStatus};
use navree_mongo::{MongoDb, MongoModule, MongoOptions};
use navree_orm::{Model, col, table};
use navree_telemetry::{EventKind, TelemetryRecorder};
use navree_testing::TestApp;

use crate::models::Company;
use crate::support::{TestMongo, UNREACHABLE};

async fn mongodb_health(app: &TestApp) -> HealthStatus {
    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    let check = checks
        .iter()
        .find(|check| check.name() == "mongodb")
        .expect("the mongodb readiness check");
    check.check().await
}

fn unreachable_options() -> MongoOptions {
    MongoOptions {
        url: "mongodb://navree:s3cret@127.0.0.1:1/navree_unreachable".into(),
        connect_timeout_ms: 300,
        server_selection_timeout_ms: 300,
        ..MongoOptions::default()
    }
}

#[tokio::test]
async fn the_app_boots_without_a_server_and_says_it_is_down() {
    let app = TestApp::builder()
        .module(MongoModule::for_root(unreachable_options()))
        .build()
        .await;
    let mongo = app.get::<MongoDb>().await;
    assert_eq!(mongo.database().name(), "navree_unreachable");
    match mongodb_health(&app).await {
        HealthStatus::Down(message) => {
            assert!(message.contains("does not answer"), "{message}");
            assert!(
                !message.contains("s3cret"),
                "the password stays out: {message}"
            );
        }
        other => panic!("expected Down, got {other:?}"),
    }
    let error = Company::query().count(&*mongo).await.unwrap_err();
    assert!(error.to_string().starts_with("mongodb: "), "{error}");
}

#[tokio::test]
async fn options_come_from_the_database_mongodb_section() {
    let app = TestApp::builder()
        .config("database.mongodb.url", UNREACHABLE)
        .config("database.mongodb.database", "crm")
        .config("database.mongodb.max_pool_size", 4)
        .config("database.mongodb.slow_query_ms", 250)
        .config("app.name", "crm-api")
        .module(MongoModule)
        .build()
        .await;
    let options = app.get::<MongoOptions>().await;
    assert_eq!(options.url, UNREACHABLE);
    assert_eq!(options.max_pool_size, Some(4));
    assert_eq!(options.slow_query_ms, 250);
    let mongo = app.get::<MongoDb>().await;
    assert_eq!(mongo.database().name(), "crm");
    assert_eq!(
        mongo.options().app_name.as_deref(),
        Some("crm-api"),
        "the server logs the app's name"
    );
}

#[tokio::test]
async fn invalid_options_fail_the_boot() {
    let error = TestApp::builder()
        .config("database.mongodb.url", "postgres://localhost/crm")
        .module(MongoModule::from_config())
        .try_build()
        .await
        .err()
        .expect("the boot fails");
    let message = error.to_string();
    assert!(message.contains("url"), "{message}");
    let error = TestApp::builder()
        .module(MongoModule::for_root(MongoOptions {
            read_preference: Some("anywhere".into()),
            ..unreachable_options()
        }))
        .try_build()
        .await
        .err()
        .expect("the boot fails");
    assert!(
        error.to_string().contains("invalid MongoDB options"),
        "{error}"
    );
}

#[tokio::test]
async fn the_app_reports_commands_without_their_values() {
    let Some(t) = TestMongo::connect("the_app_reports_commands_without_their_values").await else {
        return;
    };
    let recorder = TelemetryRecorder::only([EventKind::QueryExecuted]);
    let app = TestApp::builder()
        .module(recorder.module())
        .module(MongoModule::for_root(t.options()))
        .build()
        .await;
    assert_eq!(mongodb_health(&app).await, HealthStatus::Up);
    let mongo = app.get::<MongoDb>().await;
    let count = table("companies")
        .where_(col("name").eq("Top secret"))
        .count(&*mongo)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let queries = recorder.queries();
    let query = queries
        .iter()
        .find(|query| query.sql.starts_with("db.companies.countDocuments("))
        .expect("the count was reported");
    assert_eq!(query.sql, r#"db.companies.countDocuments({"name": ?})"#);
    assert!(!query.failed);
    assert_eq!(query.rows, Some(1));
    assert!(query.connection.contains(&t.name), "{}", query.connection);
    assert!(
        queries
            .iter()
            .all(|query| !query.sql.contains("Top secret"))
    );
    t.cleanup().await;
}
