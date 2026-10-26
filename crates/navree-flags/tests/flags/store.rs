//! The `FlagStore` conformance suite, run against the memory store and
//! the database store; plus the database store's transactions.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use navree_db::DatabaseModule;
use navree_flags::{
    FlagOverride, FlagState, FlagStore, FlagsModule, MemoryFlagStore, Rollout, ScopeType,
};
use navree_testing::TestApp;
use uuid::Uuid;

/// Every `FlagStore` behaves like this.
async fn conformance(store: &dyn FlagStore) {
    // Empty.
    assert!(store.states(None).await.unwrap().is_empty());
    assert_eq!(store.state(None, "new-pipeline").await.unwrap(), None);
    assert!(store.names(None).await.unwrap().is_empty());
    assert!(store.override_counts(None).await.unwrap().is_empty());

    // Global state: insert, then replace by name.
    let at = Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap();
    let actor = Uuid::now_v7();
    let mut state = FlagState {
        everyone: Some(true),
        rollout: Some(Rollout::percent(25).of_workspaces().on_plans(["pro"])),
        updated_at: Some(at),
        updated_by: Some(actor),
        ..FlagState::unset("new-pipeline")
    };
    store.save_state(None, &state).await.unwrap();
    assert_eq!(
        store.state(None, "new-pipeline").await.unwrap().as_ref(),
        Some(&state)
    );
    state.everyone = None;
    state.rollout = None;
    state.killed = true;
    store.save_state(None, &state).await.unwrap();
    assert_eq!(store.state(None, "new-pipeline").await.unwrap(), Some(state.clone()));
    let other = FlagState {
        updated_at: Some(at),
        ..FlagState::unset("ai-summaries")
    };
    store.save_state(None, &other).await.unwrap();
    let names: Vec<String> = store
        .states(None)
        .await
        .unwrap()
        .into_iter()
        .map(|state| state.name)
        .collect();
    assert_eq!(names, ["ai-summaries", "new-pipeline"]);

    // Overrides: insert, replace, list, count, remove.
    let (ada, grace, acme) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let entry = |flag: &str, scope_type, scope_id, active| FlagOverride {
        flag: flag.to_owned(),
        scope_type,
        scope_id,
        active,
    };
    store
        .set_override(None, &entry("new-pipeline", ScopeType::User, ada, true))
        .await
        .unwrap();
    store
        .set_override(None, &entry("new-pipeline", ScopeType::User, ada, false))
        .await
        .unwrap();
    store
        .set_override(None, &entry("new-pipeline", ScopeType::Workspace, acme, true))
        .await
        .unwrap();
    store
        .set_override(None, &entry("new-pipeline", ScopeType::User, grace, true))
        .await
        .unwrap();
    store
        .set_override(None, &entry("deal-export", ScopeType::User, ada, true))
        .await
        .unwrap();
    assert_eq!(
        store.overrides_of(None, ScopeType::User, ada).await.unwrap(),
        BTreeMap::from([
            ("deal-export".to_owned(), true),
            ("new-pipeline".to_owned(), false),
        ])
    );
    assert_eq!(
        store
            .overrides_of(None, ScopeType::Workspace, acme)
            .await
            .unwrap(),
        BTreeMap::from([("new-pipeline".to_owned(), true)])
    );
    assert!(
        store
            .overrides_of(None, ScopeType::Workspace, ada)
            .await
            .unwrap()
            .is_empty(),
        "a user id is not a workspace"
    );
    let listed = store.overrides(None, "new-pipeline").await.unwrap();
    assert_eq!(listed.len(), 3);
    assert_eq!(
        listed.iter().map(|o| o.scope_type).collect::<Vec<_>>(),
        [ScopeType::User, ScopeType::User, ScopeType::Workspace],
        "users first"
    );
    assert!(listed[0].scope_id < listed[1].scope_id, "then by id");
    assert_eq!(
        store.override_counts(None).await.unwrap(),
        BTreeMap::from([("deal-export".to_owned(), 1), ("new-pipeline".to_owned(), 3)])
    );
    assert!(
        store
            .remove_override(None, "new-pipeline", ScopeType::User, grace)
            .await
            .unwrap()
    );
    assert!(
        !store
            .remove_override(None, "new-pipeline", ScopeType::User, grace)
            .await
            .unwrap(),
        "already gone"
    );

    // Names, then purge.
    assert_eq!(
        store.names(None).await.unwrap(),
        BTreeSet::from([
            "ai-summaries".to_owned(),
            "deal-export".to_owned(),
            "new-pipeline".to_owned(),
        ])
    );
    store.purge(None, "new-pipeline").await.unwrap();
    assert_eq!(store.state(None, "new-pipeline").await.unwrap(), None);
    assert!(store.overrides(None, "new-pipeline").await.unwrap().is_empty());
    assert_eq!(
        store.overrides_of(None, ScopeType::User, ada).await.unwrap(),
        BTreeMap::from([("deal-export".to_owned(), true)]),
        "other flags keep their overrides"
    );
    assert_eq!(
        store.names(None).await.unwrap(),
        BTreeSet::from(["ai-summaries".to_owned(), "deal-export".to_owned()])
    );
}

async fn database_app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("flags.routes", false)
        .config("flags.admin_routes", false)
        .module(DatabaseModule::from_config())
        .module(FlagsModule::from_config())
        .build()
        .await
}

#[tokio::test]
async fn the_memory_store_conforms() {
    conformance(&MemoryFlagStore::new()).await;
}

#[tokio::test]
async fn the_database_store_conforms() {
    let app = database_app().await;
    let store: Arc<dyn FlagStore> = app.get::<dyn FlagStore>().await;
    conformance(&*store).await;
}

#[tokio::test]
async fn the_database_store_writes_in_the_unit_of_work() {
    let app = database_app().await;
    let store = app.get::<dyn FlagStore>().await;
    let db = app.get::<navree_db::Db>().await;
    let state = FlagState {
        everyone: Some(true),
        ..FlagState::unset("new-pipeline")
    };

    let uow = db.uow(None, None).await;
    store.save_state(Some(&uow), &state).await.unwrap();
    assert!(
        store
            .state(Some(&uow), "new-pipeline")
            .await
            .unwrap()
            .is_some(),
        "the unit of work sees its own write"
    );
    uow.rollback().await.unwrap();
    assert_eq!(store.state(None, "new-pipeline").await.unwrap(), None);

    let uow = db.uow(None, None).await;
    store.save_state(Some(&uow), &state).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(
        store.state(None, "new-pipeline").await.unwrap().unwrap().everyone,
        Some(true)
    );
}

#[tokio::test]
async fn the_database_store_needs_the_database_module() {
    let error = TestApp::builder()
        .config("flags.routes", false)
        .config("flags.admin_routes", false)
        .module(FlagsModule::from_config())
        .try_build()
        .await
        .expect_err("the boot fails");
    let message = error.to_string();
    assert!(message.contains("needs DatabaseModule"), "{message}");

    // The memory store needs no database.
    let app = TestApp::builder()
        .config("flags.store", "memory")
        .config("flags.routes", false)
        .config("flags.admin_routes", false)
        .module(FlagsModule::from_config())
        .build()
        .await;
    assert!(app.ctx().try_get::<dyn FlagStore>().await.is_some());
}
