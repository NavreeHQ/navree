//! `FlagsFake`: forcing flag values in tests without storage.

use std::sync::Arc;

use navree_access::{AccessModule, AccessOptions};
use navree_db::DatabaseModule;
use navree_flags::{FlagScope, FlagsFake, FlagsModule, Rollout};
use navree_testing::TestApp;
use serde_json::json;
use uuid::Uuid;

use crate::support::{AiSummaries, DealExport, DealsModule, NewPipeline};

/// An app with the deals routes (one of which needs a permission, so the
/// app has AccessModule and its database) and the fake.
async fn app_with(fake: &FlagsFake) -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("flags.store", "memory")
        .config("flags.admin_routes", false)
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .override_value(Arc::new(fake.flags()))
        .module(DealsModule)
        .module(FlagsModule::from_config())
        .build()
        .await
}

#[tokio::test]
async fn the_fake_answers_what_it_is_told_and_records_checks() {
    let fake = FlagsFake::new().on::<NewPipeline>().off::<AiSummaries>();
    let app = app_with(&fake).await;
    let rep = navree_testing::principal(Uuid::now_v7());

    app.acting_as(rep.clone())
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_status(200);
    app.acting_as(rep.clone())
        .get("/api/v1/deals/summary")
        .send()
        .await
        .assert_error(403, None);

    fake.assert_checked::<NewPipeline>();
    fake.assert_checked::<AiSummaries>();
    fake.assert_not_checked::<DealExport>();
    assert_eq!(fake.checked::<NewPipeline>(), 1);
    assert_eq!(
        fake.checks_of::<NewPipeline>(),
        [FlagScope::user(rep.user_id).scope()]
    );

    // Flags it was not told about resolve to their default.
    app.acting_as(rep.clone())
        .get("/api/v1/deals/export")
        .send()
        .await
        .assert_status(200);
    fake.assert_checked::<DealExport>();

    // Changed while the app runs.
    fake.set::<NewPipeline>(false);
    app.acting_as(rep)
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn writes_through_the_fake_change_its_answers() {
    let fake = FlagsFake::new();
    let flags = fake.flags();
    let anyone = FlagScope::user(Uuid::now_v7());
    assert!(!flags.active::<NewPipeline>(anyone).await.unwrap());
    flags
        .activate::<NewPipeline>(FlagScope::workspace(Uuid::now_v7()))
        .await
        .unwrap();
    assert!(flags.active::<NewPipeline>(anyone).await.unwrap());
    assert_eq!(flags.state::<NewPipeline>().await.unwrap().everyone, Some(true));
    flags.kill::<NewPipeline>().await.unwrap();
    assert!(!flags.active::<NewPipeline>(anyone).await.unwrap());
    flags.revive::<NewPipeline>().await.unwrap();
    assert!(!flags.active::<NewPipeline>(anyone).await.unwrap());
    flags
        .rollout::<NewPipeline>(Rollout::percent(100))
        .await
        .unwrap();
    assert!(!flags.active::<NewPipeline>(anyone).await.unwrap(), "ignored");
    assert_eq!(
        flags.all(anyone).await.unwrap().into_iter().collect::<Vec<_>>(),
        [("new-pipeline".to_owned(), false)]
    );
}

#[tokio::test]
async fn get_flags_works_with_the_fake() {
    let fake = FlagsFake::new().on::<AiSummaries>();
    let app = app_with(&fake).await;
    app.acting_as(navree_testing::principal(Uuid::now_v7()))
        .get("/api/flags")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ai-summaries", json!(true));
}

#[test]
#[should_panic(expected = "the flag `new-pipeline` was not checked")]
fn assert_checked_fails_for_an_unchecked_flag() {
    FlagsFake::new().assert_checked::<NewPipeline>();
}
