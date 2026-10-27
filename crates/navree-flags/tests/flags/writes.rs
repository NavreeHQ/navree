//! Writes: units of work, `flag.changed` events, audit entries and the
//! cache.

use std::sync::Arc;
use std::time::Duration;

use navree_audit::{AuditLog, audit_log};
use navree_cache::{Cache, CacheStore, MemoryStore};
use navree_db::Db;
use navree_flags::{FlagScope, FlagStore, FlagsError, MemoryFlagStore, Rollout, ScopeType};
use sea_orm::sea_query::Query;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::json;
use uuid::Uuid;

use crate::support::{DealExport, NewPipeline, World, builder};

#[tokio::test]
async fn writes_join_the_current_unit_of_work() {
    let world = World::new().await;
    let flags = world.flags().await;
    let mut events = world.events(&["flag.changed"]).await;
    let acme = Uuid::now_v7();

    // Rolled back with the unit of work: no state, no event.
    let result: Result<(), FlagsError> = world
        .work(|_uow| {
            let flags = flags.clone();
            async move {
                flags
                    .activate::<NewPipeline>(FlagScope::workspace(acme))
                    .await?;
                Err(FlagsError::Other("the request failed".to_owned()))
            }
        })
        .await;
    assert!(result.is_err());
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    assert!(
        events.drain().is_empty(),
        "no event for a rolled-back change"
    );

    // Committed with it: the state and the event, after the commit.
    let actor = Uuid::now_v7();
    let db = world.db().await;
    let uow = db.uow(Some(actor), Some("req-1".to_owned())).await;
    uow.scope(flags.activate::<NewPipeline>(FlagScope::workspace(acme)))
        .await
        .unwrap();
    assert!(events.drain().is_empty(), "nothing before the commit");
    uow.commit().await.unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    let events = events.drain();
    assert_eq!(events.len(), 1, "{events:?}");
    let (name, payload) = &events[0];
    assert_eq!(name, "flag.changed");
    assert_eq!(
        payload,
        &json!({
            "flag": "new-pipeline",
            "change": "activated",
            "scope_type": "workspace",
            "scope_id": acme,
            "old": {"active": null},
            "new": {"active": true},
            "actor_id": actor,
        })
    );
}

#[tokio::test]
async fn writes_outside_a_unit_of_work_commit_on_their_own() {
    let world = World::new().await;
    let flags = world.flags().await;
    let mut events = world.events(&["flag.changed"]).await;
    flags.kill::<NewPipeline>().await.unwrap();
    let events = events.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1["change"], "killed");
    assert_eq!(events[0].1["old"], json!({"killed": false}));
    assert_eq!(events[0].1["new"], json!({"killed": true}));
    assert_eq!(events[0].1["actor_id"], json!(null));

    let state = flags.state::<NewPipeline>().await.unwrap();
    assert!(state.killed && state.updated_at.is_some());
    assert_eq!(state.updated_by, None);
}

#[tokio::test]
async fn every_change_is_audited_in_its_transaction() {
    let world = World::new().await;
    let flags = world.flags().await;
    let actor = Uuid::now_v7();
    let db = world.db().await;
    let uow = db.uow(Some(actor), Some("req-7".to_owned())).await;
    uow.scope(async {
        flags
            .rollout::<NewPipeline>(Rollout::percent(10).of_workspaces())
            .await
            .unwrap();
        flags
            .deactivate::<NewPipeline>(FlagScope::user(actor))
            .await
            .unwrap();
    })
    .await;
    uow.commit().await.unwrap();
    // A no-op change is not recorded.
    flags
        .forget::<DealExport>(FlagScope::everyone())
        .await
        .unwrap();

    world.app.get::<AuditLog>().await.flush().await;
    let entries = audit_log::Entity::find()
        .filter(audit_log::Column::Action.eq("flag.changed"))
        .order_by_asc(audit_log::Column::Id)
        .all(db.conn())
        .await
        .unwrap();
    assert_eq!(entries.len(), 2, "{entries:#?}");
    let rollout = &entries[0];
    assert_eq!(rollout.kind, "custom");
    assert_eq!(rollout.actor_id, Some(actor));
    assert_eq!(rollout.subject_type.as_deref(), Some("Flag"));
    assert_eq!(rollout.subject_id.as_deref(), Some("new-pipeline"));
    assert_eq!(rollout.request_id.as_deref(), Some("req-7"));
    assert_eq!(rollout.old_values, Some(json!({"rollout": null})));
    assert_eq!(
        rollout.new_values,
        Some(json!({"rollout": {"percent": 10, "of": "workspaces"}}))
    );
    assert_eq!(rollout.metadata["change"], "rollout_set");
    let off = &entries[1];
    assert_eq!(off.metadata["change"], "deactivated");
    assert_eq!(off.metadata["scope_type"], "user");
    assert_eq!(off.metadata["scope_id"], json!(actor));
    assert_eq!(off.new_values, Some(json!({"active": false})));
}

/// Writes straight to the table, as another app instance would.
async fn turn_on_behind_the_cache(db: &Db, workspace: Uuid) {
    db.conn()
        .execute(
            Query::insert()
                .into_table("feature_flag_overrides")
                .columns(["flag", "scope_type", "scope_id", "active"])
                .values_panic([
                    "new-pipeline".into(),
                    "workspace".into(),
                    workspace.into(),
                    true.into(),
                ]),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn checks_are_cached_and_changes_clear_the_cache() {
    let world = World::new().await;
    let flags = world.flags().await;
    let db = world.db().await;
    let (acme, globex) = (Uuid::now_v7(), Uuid::now_v7());

    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    turn_on_behind_the_cache(&db, acme).await;
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap(),
        "the cached overrides of acme still say nothing"
    );
    assert!(
        world
            .app
            .get::<Cache>()
            .await
            .has(&format!("flags:overrides:workspace:{acme}"))
            .await
            .unwrap()
    );

    // A change through Flags clears what it touched at once.
    flags
        .deactivate::<NewPipeline>(FlagScope::workspace(acme))
        .await
        .unwrap();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    flags
        .activate::<NewPipeline>(FlagScope::workspace(acme))
        .await
        .unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );

    // The global state is cached too.
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(globex))
            .await
            .unwrap()
    );
    db.conn()
        .execute(
            Query::insert()
                .into_table("feature_flags")
                .columns(["name", "everyone", "killed", "updated_at"])
                .values_panic([
                    "new-pipeline".into(),
                    true.into(),
                    false.into(),
                    chrono::Utc::now().into(),
                ]),
        )
        .await
        .unwrap();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(globex))
            .await
            .unwrap()
    );
    world.app.get::<Cache>().await.flush().await.unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::workspace(globex))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn other_instances_see_a_change_once_the_cache_expires() {
    let world = World::with(builder().config("flags.cache_ttl_secs", 1)).await;
    let flags = world.flags().await;
    let acme = Uuid::now_v7();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    turn_on_behind_the_cache(&*world.db().await, acme).await;
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn instances_sharing_a_cache_store_see_a_change_at_once() {
    // Two instances of one app: one flag store (the database) and one cache
    // store (what `cache.store = "redis"` gives every process).
    let flag_store: Arc<dyn FlagStore> = Arc::new(MemoryFlagStore::new());
    let cache_store: Arc<dyn CacheStore> = Arc::new(MemoryStore::new());
    let instance = || {
        builder()
            .config("flags.store", "memory")
            .config("flags.cache_ttl_secs", 3600)
            .override_value::<dyn FlagStore>(flag_store.clone())
            .override_value::<dyn CacheStore>(cache_store.clone())
    };
    let (one, two) = (World::with(instance()).await, World::with(instance()).await);
    let (flags_one, flags_two) = (one.flags().await, two.flags().await);
    let acme = FlagScope::workspace(Uuid::now_v7());

    // The second instance caches "off" for an hour...
    assert!(!flags_two.active::<NewPipeline>(acme).await.unwrap());
    assert!(two.app.get::<Cache>().await.has("flags:states").await.unwrap());

    // ...and a change on the first clears the shared entries, so the
    // second sees it on its next check.
    flags_one.activate::<NewPipeline>(acme).await.unwrap();
    assert!(flags_two.active::<NewPipeline>(acme).await.unwrap());
    flags_one.kill::<NewPipeline>().await.unwrap();
    assert!(!flags_two.active::<NewPipeline>(acme).await.unwrap());
}

#[tokio::test]
async fn a_zero_ttl_turns_the_cache_off() {
    let world = World::with(builder().config("flags.cache_ttl_secs", 0)).await;
    let flags = world.flags().await;
    let acme = Uuid::now_v7();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    turn_on_behind_the_cache(&*world.db().await, acme).await;
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    assert!(
        !world
            .app
            .get::<Cache>()
            .await
            .has("flags:states")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn reads_in_a_unit_of_work_see_its_writes_without_caching_them() {
    let world = World::new().await;
    let flags = world.flags().await;
    let acme = Uuid::now_v7();
    let db = world.db().await;
    let uow = db.uow(None, None).await;
    uow.scope(async {
        flags
            .activate::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap();
        assert!(
            flags
                .active::<NewPipeline>(FlagScope::workspace(acme))
                .await
                .unwrap(),
            "the write is visible in its own unit of work"
        );
    })
    .await;
    uow.rollback().await.unwrap();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap(),
        "the rolled-back state was not cached"
    );
}

#[tokio::test]
async fn the_memory_store_applies_changes_at_once() {
    let world = World::with(builder().config("flags.store", "memory")).await;
    let flags = world.flags().await;
    let store = world.app.get::<dyn FlagStore>().await;
    let mut events = world.events(&["flag.changed"]).await;
    let ada = Uuid::now_v7();
    flags
        .activate::<NewPipeline>(FlagScope::user(ada))
        .await
        .unwrap();
    assert!(
        store
            .overrides_of(None, ScopeType::User, ada)
            .await
            .unwrap()["new-pipeline"]
    );
    assert_eq!(events.drain().len(), 1, "emitted at once");
    world.app.get::<AuditLog>().await.flush().await;
    let audited = audit_log::Entity::find()
        .filter(audit_log::Column::Action.eq("flag.changed"))
        .all(world.db().await.conn())
        .await
        .unwrap();
    assert_eq!(audited.len(), 1);
}
