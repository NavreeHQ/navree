//! Booting the module: options in code, validation, `store = "off"`, a
//! separate database, and the migration set.

use navree_db::{DatabaseModule, Db, Executor, Statement};
use navree_lighthouse::{
    IssueFilter, LIGHTHOUSE_MIGRATIONS, Lighthouse, LighthouseModule, LighthouseOptions,
    SampleRates, Thresholds,
};
use navree_testing::TestApp;

use crate::support::{DemoModule, builder, flushed};

#[tokio::test]
async fn for_root_takes_options_in_code() {
    let app = crate::servers::database(TestApp::builder())
        .module(DatabaseModule::from_config())
        .module(DemoModule)
        .module(LighthouseModule::for_root(LighthouseOptions {
            retention_days: 30,
            thresholds: Thresholds {
                slow_request_ms: 500,
                ..Thresholds::default()
            },
            dashboard: false,
            ..LighthouseOptions::default()
        }))
        .build()
        .await;
    let lighthouse = app.get::<Lighthouse>().await;
    assert_eq!(lighthouse.options().retention_days, 30);
    assert_eq!(lighthouse.options().thresholds.slow_request_ms, 500);
    app.client()
        .get("/api/demo/boom/1")
        .send()
        .await
        .assert_status(500);
    let store = flushed(&app).await;
    assert_eq!(
        store
            .issues(&IssueFilter::default())
            .await
            .unwrap()
            .meta
            .total,
        1
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_options_fail_the_boot_naming_the_key() {
    let error = crate::servers::database(TestApp::builder())
        .module(DatabaseModule::from_config())
        .module(LighthouseModule::for_root(LighthouseOptions {
            sample: SampleRates {
                requests: 1.5,
                ..SampleRates::default()
            },
            ..LighthouseOptions::default()
        }))
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot fails");
    assert!(
        error.to_string().contains("lighthouse.sample.requests"),
        "{error}"
    );

    let error = crate::servers::database(TestApp::builder())
        .config("lighthouse.alerts.email.on", "new_issue,sometimes")
        .module(DatabaseModule::from_config())
        .module(LighthouseModule::from_config())
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot fails");
    assert!(error.to_string().contains("unknown alert kind"), "{error}");
}

#[tokio::test]
async fn with_the_store_off_nothing_is_kept_and_nothing_is_mounted() {
    let app = builder().config("lighthouse.store", "off").build().await;
    app.client()
        .get("/api/demo/boom/1")
        .send()
        .await
        .assert_status(500);
    let lighthouse = app.get::<Lighthouse>().await;
    assert!(lighthouse.store().is_none());
    assert!(!lighthouse.is_recording());
    app.client()
        .get("/_lighthouse/login")
        .send()
        .await
        .assert_status(404);
    assert!(app.run_command("lighthouse:prune", &[]).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_separate_database_holds_the_tables() {
    let (url, _guard) = crate::servers::scratch();
    let app = builder()
        .config("lighthouse.database_url", url.clone())
        .build()
        .await;
    app.client()
        .get("/api/demo/boom/1")
        .send()
        .await
        .assert_status(500);
    let store = flushed(&app).await;
    assert_eq!(
        store
            .issues(&IssueFilter::default())
            .await
            .unwrap()
            .meta
            .total,
        1
    );
    let app_db = app.get::<Db>().await;
    let row = app_db
        .fetch_one(&Statement::text(
            "SELECT count(*) AS n FROM lighthouse_issues",
        ))
        .await
        .unwrap();
    let n: i64 = row.get("n").unwrap();
    assert_eq!(n, 0, "the app's own tables stay empty");
    assert_eq!(Executor::backend(store.db()), Executor::backend(&*app_db));
    assert!(
        !store.db().same_as(&app_db),
        "the store has its own database"
    );
    assert_eq!(LIGHTHOUSE_MIGRATIONS, "lighthouse");
    app.shutdown().await.unwrap();
}
