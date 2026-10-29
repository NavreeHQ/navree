//! The unit of work outside HTTP: lazy transaction, commit, rollback,
//! after-commit hooks and `Db::transaction`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use navree_db::{ChangeEvent, ChangeOp, Db, DbError, ErrorKind, Record, Uow, record};
use uuid::Uuid;

use crate::fixtures::{Note, count, create_notes, new_note, notes_db};
use crate::support::TempDb;

#[tokio::test]
async fn uow_commits_writes_made_through_its_transaction() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    assert!(!uow.has_open_transaction().await, "lazy");
    record::insert(&uow, &new_note("a")).await.expect("insert");
    assert!(uow.has_open_transaction().await);
    assert_eq!(count(&db, "notes").await, 0, "not visible before commit");
    uow.commit().await.expect("commit");
    assert_eq!(count(&db, "notes").await, 1);
}

#[tokio::test]
async fn uow_rollback_discards_writes() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    record::insert(&uow, &new_note("a")).await.unwrap();
    uow.rollback().await.expect("rollback");
    assert_eq!(count(&db, "notes").await, 0);
}

#[tokio::test]
async fn uow_has_one_transaction_and_finished_uow_refuses_new_work() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    record::insert(&uow, &new_note("a")).await.unwrap();
    // A clone writes in the same transaction: it sees the first write.
    let clone = uow.clone();
    assert_eq!(Note::select().count(&clone).await.unwrap(), 1);
    assert_eq!(count(&db, "notes").await, 0, "one uncommitted transaction");
    uow.commit().await.unwrap();
    let error = record::insert(&uow, &new_note("b")).await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::TransactionDone, "{error}");
    uow.commit().await.expect("a second commit is a no-op");
}

#[tokio::test]
async fn savepoints_nest_inside_the_unit_of_work() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;
    record::insert(&uow, &new_note("kept")).await.unwrap();
    let savepoint = uow.savepoint().await.unwrap();
    record::insert(&savepoint, &new_note("undone"))
        .await
        .unwrap();
    savepoint.rollback().await.unwrap();
    let savepoint = uow.savepoint().await.unwrap();
    record::insert(&savepoint, &new_note("released"))
        .await
        .unwrap();
    savepoint.commit().await.unwrap();
    uow.commit().await.unwrap();
    let mut titles: Vec<String> = Note::select()
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .map(|note| note.title)
        .collect();
    titles.sort();
    assert_eq!(titles, ["kept", "released"]);
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
    assert!(
        !uow.set_local("navree.workspace", "w").await.unwrap(),
        "SQLite has no transaction-local settings"
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
    record::insert(&uow, &new_note("a")).await.unwrap();
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
            let note = new_note("kept");
            record::insert(&uow, &note).await?;
            Ok(note.id)
        })
        .await
        .expect("commits");
    assert!(record::find::<Note, _>(&db, id).await.unwrap().is_some());

    let error = db
        .transaction(|uow| async move {
            record::insert(&uow, &new_note("dropped")).await?;
            Err::<(), _>(DbError::invalid("nope"))
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("nope"));
    assert_eq!(count(&db, "notes").await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_read_then_write_units_of_work_queue_instead_of_failing() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let mut tasks = Vec::new();
    for i in 0..12 {
        let db = db.clone();
        tasks.push(tokio::spawn(async move {
            db.transaction(|uow| async move {
                // Read first, then write: a deferred SQLite transaction
                // fails here with SQLITE_BUSY when another writer committed.
                let seen = Note::select().count(&uow).await?;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                record::insert(&uow, &new_note(&format!("n{i}-{seen}"))).await?;
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
    Note::select().count(&rolled_back).await.unwrap();
    rolled_back.rollback().await.unwrap();
    assert!(rolled_back.is_finished().await);
    assert!(!rolled_back.same_as(&committed));
}

/// `Db::read_executor` reads through the current unit of work's open
/// transaction, and so does `Db` itself: with a pool of one connection, a
/// read while the unit of work holds that connection would otherwise wait
/// for the pool until the acquire timeout.
#[tokio::test]
async fn reads_use_the_open_transaction_of_the_current_unit_of_work() {
    let temp = TempDb::new();
    let db = Db::connect(&navree_db::DbOptions {
        max_connections: 1,
        connect_timeout_secs: 1,
        ..temp.options()
    })
    .await
    .expect("connects");
    create_notes(&db).await;
    assert!(
        !db.read_executor().await.in_transaction(),
        "no unit of work"
    );

    let started = std::time::Instant::now();
    let found = db
        .transaction(|uow| {
            let db = db.clone();
            async move {
                assert!(!db.read_executor().await.in_transaction(), "not begun yet");
                record::insert(&uow, &new_note("uncommitted")).await?;
                let reader = db.read_executor().await;
                assert!(reader.in_transaction());
                let found = Note::select().all(&reader).await?;
                let through_db = Note::select().count(&db).await?;
                Ok((found.len(), through_db))
            }
        })
        .await
        .expect("the read does not wait for a second connection");
    assert_eq!(found, (1, 1), "sees the unit of work's own write");
    assert!(started.elapsed() < std::time::Duration::from_secs(2));

    // A finished unit of work reads from the pool again.
    let uow = db.uow(None, None).await;
    Note::select().count(&uow).await.expect("begins");
    uow.rollback().await.expect("rollback");
    let in_tx = uow
        .scope(async { db.read_executor().await.in_transaction() })
        .await;
    assert!(!in_tx);
}
