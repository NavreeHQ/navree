//! The unit of work outside HTTP: lazy transaction, commit, rollback,
//! after-commit hooks and `Db::transaction`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use navree_db::{ChangeEvent, ChangeOp, Db, Uow};
use sea_orm::{ActiveModelTrait, DbErr, EntityTrait};
use uuid::Uuid;

use crate::fixtures::{count, new_note, note, notes_db};
use crate::support::TempDb;

#[tokio::test]
async fn uow_commits_writes_made_through_its_transaction() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    let tx = uow.tx().await.expect("tx");
    new_note("a").insert(&*tx).await.expect("insert");
    drop(tx);
    assert_eq!(count(&db, "notes").await, 0, "not visible before commit");
    uow.commit().await.expect("commit");
    assert_eq!(count(&db, "notes").await, 1);
}

#[tokio::test]
async fn uow_rollback_discards_writes() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    new_note("a")
        .insert(&*uow.tx().await.unwrap())
        .await
        .unwrap();
    uow.rollback().await.expect("rollback");
    assert_eq!(count(&db, "notes").await, 0);
}

#[tokio::test]
async fn uow_tx_is_shared_and_finished_uow_refuses_new_work() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    let first = uow.tx().await.unwrap();
    let second = uow.clone().tx().await.unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "one transaction per unit of work"
    );
    drop((first, second));
    uow.commit().await.unwrap();
    assert!(uow.tx().await.is_err(), "no transaction after commit");
    uow.commit().await.expect("a second commit is a no-op");
}

#[tokio::test]
async fn commit_fails_while_the_transaction_is_still_held() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    let held = uow.tx().await.unwrap();
    new_note("a").insert(&*held).await.unwrap();
    let error = uow.commit().await.unwrap_err();
    assert!(error.to_string().contains("still in use"), "{error}");
    drop(held);
    assert_eq!(count(&db, "notes").await, 0);
}

#[tokio::test]
async fn uow_carries_actor_and_request_id() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let actor = Uuid::now_v7();
    let uow = db.uow(Some(actor), Some("req-1".into())).await;
    assert_eq!(uow.actor(), Some(actor));
    assert_eq!(uow.request_id(), Some("req-1"));
    assert_eq!(uow.db().backend(), db.backend());
    assert!(
        !uow.lock_for_update(),
        "SQLite has no SELECT ... FOR UPDATE"
    );
}

#[tokio::test]
async fn after_commit_hooks_run_after_commit_only() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let runs = Arc::new(AtomicUsize::new(0));
    let hook = |runs: &Arc<AtomicUsize>| {
        let runs = runs.clone();
        move || -> navree_kernel::BoxFuture<'static, ()> {
            Box::pin(async move {
                runs.fetch_add(1, Ordering::SeqCst);
            })
        }
    };

    let committed = db.uow(None, None).await;
    committed.after_commit(hook(&runs));
    committed.after_commit(hook(&runs));
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    committed.commit().await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    let rolled_back = db.uow(None, None).await;
    rolled_back.after_commit(hook(&runs));
    rolled_back.rollback().await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn rollback_as_committed_keeps_nothing_but_runs_the_hooks() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let runs = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(AtomicUsize::new(usize::MAX));

    let uow = db.uow(None, None).await;
    new_note("a")
        .insert(&*uow.tx().await.unwrap())
        .await
        .unwrap();
    let (hook_runs, hook_seen, hook_db) = (runs.clone(), seen.clone(), db.clone());
    uow.after_commit(move || {
        Box::pin(async move {
            hook_runs.fetch_add(1, Ordering::SeqCst);
            // After the rollback: the note is gone for every connection.
            let notes = usize::try_from(count(&hook_db, "notes").await).unwrap();
            hook_seen.store(notes, Ordering::SeqCst);
        })
    });
    uow.rollback_as_committed().await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 1, "the hook ran");
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert_eq!(count(&db, "notes").await, 0, "nothing stays");
    assert!(uow.is_finished().await);
    uow.rollback_as_committed()
        .await
        .expect("ending a finished unit of work does nothing");
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    // A transaction still held: an error, and the hooks are dropped.
    let uow = db.uow(None, None).await;
    let held = uow.tx().await.unwrap();
    let hook_runs = runs.clone();
    uow.after_commit(move || {
        Box::pin(async move {
            hook_runs.fetch_add(1, Ordering::SeqCst);
        })
    });
    let error = uow.rollback_as_committed().await.unwrap_err();
    assert!(error.to_string().contains("still in use"), "{error}");
    drop(held);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn recorded_changes_are_kept_until_commit() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    uow.record_change(ChangeEvent {
        entity: "notes".into(),
        id: "1".into(),
        op: ChangeOp::Created,
        old: None,
        new: Some(serde_json::json!({"title": "a"})),
    });
    assert_eq!(uow.changes().len(), 1);
    uow.commit().await.unwrap();
    assert!(uow.changes().is_empty());
}

#[tokio::test]
async fn transaction_commits_on_ok_and_rolls_back_on_err() {
    let temp = TempDb::new();
    let db: Db = notes_db(&temp).await;

    let id = db
        .transaction(|uow| async move {
            let model = new_note("kept").insert(&*uow.tx().await?).await?;
            Ok(model.id)
        })
        .await
        .expect("commits");
    assert!(
        note::Entity::find_by_id(id)
            .one(db.conn())
            .await
            .unwrap()
            .is_some()
    );

    let error = db
        .transaction(|uow| async move {
            new_note("dropped").insert(&*uow.tx().await?).await?;
            Err::<(), _>(DbErr::Custom("nope".into()))
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("nope"));
    assert_eq!(count(&db, "notes").await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_read_then_write_units_of_work_queue_instead_of_failing() {
    use sea_orm::PaginatorTrait;

    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let mut tasks = Vec::new();
    for i in 0..12 {
        let db = db.clone();
        tasks.push(tokio::spawn(async move {
            db.transaction(|uow| async move {
                let tx = uow.tx().await?;
                // Read first, then write: a deferred SQLite transaction
                // fails here with SQLITE_BUSY when another writer committed.
                let seen = note::Entity::find().count(&*tx).await?;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                new_note(&format!("n{i}-{seen}")).insert(&*tx).await?;
                Ok(())
            })
            .await
        }));
    }
    for task in tasks {
        task.await.unwrap().expect("unit of work succeeds");
    }
    assert_eq!(count(&db, "notes").await, 12);
}

#[tokio::test]
async fn current_is_the_scoped_unit_of_work() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    assert!(Uow::current().is_none(), "no unit of work outside a scope");

    let uow = db.uow(None, Some("req-1".to_owned())).await;
    let seen = uow
        .scope(async { Uow::current().map(|current| current.request_id().map(str::to_owned)) })
        .await;
    assert_eq!(seen, Some(Some("req-1".to_owned())));
    assert!(Uow::current().is_none(), "the scope ends with the future");

    let inside = db
        .transaction(|uow| async move {
            let current = Uow::current().expect("Db::transaction scopes its unit of work");
            assert!(current.same_as(&uow));
            Ok(current.is_finished().await)
        })
        .await
        .unwrap();
    assert!(!inside, "unfinished while the closure runs");
}

#[tokio::test]
async fn is_finished_after_commit_or_rollback() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let committed = db.uow(None, None).await;
    assert!(!committed.is_finished().await);
    committed.commit().await.unwrap();
    assert!(committed.is_finished().await);

    let rolled_back = db.uow(None, None).await;
    rolled_back.tx().await.unwrap();
    rolled_back.rollback().await.unwrap();
    assert!(rolled_back.is_finished().await);
    assert!(!rolled_back.same_as(&committed));
}

/// `Db::reader` reads through the current unit of work's open transaction:
/// with a pool of one connection, a read while the unit of work holds that
/// connection would otherwise wait for the pool until the acquire timeout.
#[tokio::test]
async fn reader_uses_the_open_transaction_of_the_current_unit_of_work() {
    let temp = TempDb::new();
    let db = Db::connect(&navree_db::DbOptions {
        max_connections: 1,
        connect_timeout_secs: 1,
        ..temp.options()
    })
    .await
    .expect("connects");
    sea_orm_migration::MigrationTrait::up(
        &crate::fixtures::CreateNotes,
        &sea_orm_migration::SchemaManager::new(db.conn()),
    )
    .await
    .expect("creates tables");
    assert!(!db.reader().await.in_transaction(), "no unit of work");

    let started = std::time::Instant::now();
    let found = db
        .transaction(|uow| {
            let db = db.clone();
            async move {
                assert!(!db.reader().await.in_transaction(), "not begun yet");
                let tx = uow.tx().await?;
                new_note("uncommitted").insert(&*tx).await?;
                drop(tx);
                let reader = db.reader().await;
                assert!(reader.in_transaction());
                let found = note::Entity::find().all(&reader).await?;
                Ok(found.len())
            }
        })
        .await
        .expect("the read does not wait for a second connection");
    assert_eq!(found, 1, "sees the unit of work's own write");
    assert!(started.elapsed() < std::time::Duration::from_secs(2));

    // A finished unit of work reads from the pool again.
    let uow = db.uow(None, None).await;
    uow.tx().await.expect("tx");
    uow.rollback().await.expect("rollback");
    let in_tx = uow
        .scope(async { db.reader().await.in_transaction() })
        .await;
    assert!(!in_tx);
}
