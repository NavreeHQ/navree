//! The unit of work outside HTTP: lazy transaction, commit, rollback,
//! after-commit hooks and `Db::transaction`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use keel_db::{ChangeEvent, ChangeOp, Db};
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
        move || -> keel_kernel::BoxFuture<'static, ()> {
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
