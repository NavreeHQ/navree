//! Storage: one contract, run against the in-memory store and the
//! `settings` table.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use navree_db::Db;
use navree_settings::{
    DatabaseSettingsStore, MemorySettingsStore, ScopeKind, ScopeTarget, SettingsStore,
    StoredSetting,
};
use sea_orm::{ConnectionTrait, Statement};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::support::World;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn row(key: &str, scope: ScopeTarget, values: Value) -> StoredSetting {
    StoredSetting {
        key: key.to_owned(),
        scope,
        values: object(values),
        updated_at: Utc.with_ymd_and_hms(2026, 10, 27, 9, 30, 0).unwrap(),
        updated_by: None,
    }
}

/// The contract every store keeps.
pub async fn contract(store: &dyn SettingsStore) {
    let (acme, globex, ada) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let workspace = ScopeTarget::new(ScopeKind::Workspace, acme);
    let other = ScopeTarget::new(ScopeKind::Workspace, globex);
    let user = ScopeTarget::new(ScopeKind::User, ada);

    assert!(
        store
            .load(None, "portal", &[ScopeTarget::GLOBAL, workspace])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(store.load(None, "portal", &[]).await.unwrap().is_empty());

    let global = row(
        "portal",
        ScopeTarget::GLOBAL,
        json!({"page_size": 50, "welcome_text": "Hello"}),
    );
    store.save(None, &global).await.unwrap();
    let mut acme_row = row("portal", workspace, json!({"show_leaderboard": true}));
    acme_row.updated_by = Some(ada);
    store.save(None, &acme_row).await.unwrap();
    store
        .save(None, &row("portal", other, json!({"page_size": 10})))
        .await
        .unwrap();
    store
        .save(
            None,
            &row("notifications", workspace, json!({"daily_digest": true})),
        )
        .await
        .unwrap();
    store
        .save(
            None,
            &row("view-preferences", user, json!({"theme": "dark"})),
        )
        .await
        .unwrap();

    // Loads only the asked levels of the key.
    let mut loaded = store
        .load(None, "portal", &[workspace, ScopeTarget::GLOBAL])
        .await
        .unwrap();
    loaded.sort_by_key(|row| row.scope.kind);
    assert_eq!(loaded, [global.clone(), acme_row.clone()]);
    assert_eq!(acme_row.overridden_fields(), ["show_leaderboard"]);

    // Saving again replaces the row.
    let replaced = row(
        "portal",
        workspace,
        json!({"show_leaderboard": false, "welcome_text": "Hi"}),
    );
    store.save(None, &replaced).await.unwrap();
    assert_eq!(
        store.load(None, "portal", &[workspace]).await.unwrap(),
        std::slice::from_ref(&replaced)
    );

    // Lists by key, then level (global first), then id.
    let listed = store.list(None, None).await.unwrap();
    let order: Vec<(String, ScopeKind)> = listed
        .iter()
        .map(|row| (row.key.clone(), row.scope.kind))
        .collect();
    assert_eq!(
        order,
        [
            ("notifications".to_owned(), ScopeKind::Workspace),
            ("portal".to_owned(), ScopeKind::Global),
            ("portal".to_owned(), ScopeKind::Workspace),
            ("portal".to_owned(), ScopeKind::Workspace),
            ("view-preferences".to_owned(), ScopeKind::User),
        ]
    );
    let portal_ids: Vec<Uuid> = listed[2..4]
        .iter()
        .map(|row| row.scope.stored_id())
        .collect();
    let mut sorted = portal_ids.clone();
    sorted.sort();
    assert_eq!(portal_ids, sorted);
    assert_eq!(store.list(None, Some("portal")).await.unwrap().len(), 3);

    // Deletes one row; then every row of one level.
    assert!(store.delete(None, "portal", other).await.unwrap());
    assert!(!store.delete(None, "portal", other).await.unwrap());
    assert_eq!(
        store.delete_scope(None, workspace).await.unwrap(),
        ["notifications", "portal"]
    );
    assert!(
        store
            .delete_scope(None, workspace)
            .await
            .unwrap()
            .is_empty()
    );
    let left: Vec<String> = store
        .list(None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.key)
        .collect();
    assert_eq!(left, ["portal", "view-preferences"]);
}

#[tokio::test]
async fn the_memory_store_keeps_the_contract() {
    contract(&MemorySettingsStore::new()).await;
}

#[tokio::test]
async fn the_database_store_keeps_the_contract() {
    let world = World::new().await;
    let db = world.db().await;
    contract(&DatabaseSettingsStore::new(db)).await;
}

#[tokio::test]
async fn rows_keep_the_documented_columns() {
    let world = World::new().await;
    let db: Arc<Db> = world.db().await;
    let store = DatabaseSettingsStore::new(db.clone());
    let acme = Uuid::now_v7();
    store
        .save(
            None,
            &row(
                "portal",
                ScopeTarget::new(ScopeKind::Workspace, acme),
                json!({"welcome_text": "Hi", "page_size": 10}),
            ),
        )
        .await
        .unwrap();
    store
        .save(
            None,
            &row("portal", ScopeTarget::GLOBAL, json!({"page_size": 5})),
        )
        .await
        .unwrap();
    let backend = db.conn().get_database_backend();
    let rows = db
        .conn()
        .query_all_raw(Statement::from_string(
            backend,
            "SELECT \"key\", scope_type, scope_id, \"value\", overridden_fields FROM settings \
             ORDER BY scope_type",
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let global = &rows[0];
    assert_eq!(
        global.try_get::<String>("", "scope_type").unwrap(),
        "global"
    );
    assert_eq!(global.try_get::<Uuid>("", "scope_id").unwrap(), Uuid::nil());
    let workspace = &rows[1];
    assert_eq!(workspace.try_get::<String>("", "key").unwrap(), "portal");
    assert_eq!(workspace.try_get::<Uuid>("", "scope_id").unwrap(), acme);
    assert_eq!(
        workspace.try_get::<Value>("", "value").unwrap(),
        json!({"welcome_text": "Hi", "page_size": 10})
    );
    assert_eq!(
        workspace.try_get::<Value>("", "overridden_fields").unwrap(),
        json!(["page_size", "welcome_text"])
    );
}

#[tokio::test]
async fn an_unreadable_row_is_reported_not_guessed() {
    let world = World::new().await;
    let db = world.db().await;
    let store = DatabaseSettingsStore::new(db.clone());
    store
        .save(
            None,
            &row("portal", ScopeTarget::GLOBAL, json!({"page_size": 5})),
        )
        .await
        .unwrap();
    db.conn()
        .execute_unprepared("UPDATE settings SET scope_type = 'team'")
        .await
        .unwrap();
    let error = store.list(None, None).await.unwrap_err();
    assert!(
        error.to_string().contains("unknown scope `team`"),
        "{error}"
    );
}
