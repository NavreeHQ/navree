//! `Repo<E>` on MySQL: writes through the unit of work with change events,
//! reads inside it, and `SELECT ... FOR UPDATE` row locks.

use std::time::Duration;

use navree_db::{ChangeOp, Repo};
use sea_orm::{ActiveValue::Set, DbErr, IntoActiveModel};
use serde_json::json;
use uuid::Uuid;

use crate::fixtures::{create_notes, new_note, note};
use crate::support::{Scratch, count};

type Notes = Repo<note::Entity>;

#[tokio::test]
async fn repo_records_change_events() {
    let Some(scratch) = Scratch::new("repo_records_change_events").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    let uow = db.uow(None, None).await;

    let created = Notes::insert(&uow, new_note("a")).await.expect("inserts");
    let mut changed = created.clone().into_active_model();
    changed.body = Set("changed".into());
    let updated = Notes::update(&uow, changed).await.expect("updates");
    assert_eq!(updated.body, "changed");
    Notes::delete(&uow, updated.clone()).await.expect("deletes");

    let changes = uow.changes();
    let id = created.id.to_string();
    assert_eq!(
        changes
            .iter()
            .map(|change| (change.entity.as_str(), change.id.as_str(), change.op))
            .collect::<Vec<_>>(),
        [
            ("notes", id.as_str(), ChangeOp::Created),
            ("notes", id.as_str(), ChangeOp::Updated),
            ("notes", id.as_str(), ChangeOp::Deleted),
        ]
    );
    assert_eq!(
        changes[0].new,
        Some(json!({"id": id, "title": "a", "body": "body of a"}))
    );
    assert_eq!(changes[1].old, Some(json!({"body": "body of a"})));
    assert_eq!(changes[1].new, Some(json!({"body": "changed"})));
    assert_eq!(
        changes[2].old,
        Some(json!({"id": id, "title": "a", "body": "changed"}))
    );
    uow.commit().await.unwrap();
    assert_eq!(count(db.conn(), "notes").await, 0);
}

#[tokio::test]
async fn repo_reads_inside_the_unit_of_work() {
    let Some(scratch) = Scratch::new("repo_reads_inside_the_unit_of_work").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    let uow = db.uow(None, None).await;
    let created = Notes::insert(&uow, new_note("a")).await.unwrap();

    let found = Notes::find(&uow, created.id).await.unwrap();
    assert_eq!(found.as_ref(), Some(&created), "sees uncommitted writes");
    assert_eq!(Notes::find(&uow, Uuid::now_v7()).await.unwrap(), None);
    let missing = Notes::find_or_fail(&uow, Uuid::now_v7()).await.unwrap_err();
    assert!(matches!(missing, DbErr::RecordNotFound(_)), "{missing:?}");
    assert_eq!(
        Notes::find_for_update(&uow, created.id).await.unwrap(),
        Some(created)
    );
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn repo_update_and_delete_of_missing_records_fail() {
    let Some(scratch) = Scratch::new("repo_update_and_delete_of_missing_records_fail").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    let uow = db.uow(None, None).await;
    let ghost = note::Model {
        id: Uuid::now_v7(),
        title: "ghost".into(),
        body: "boo".into(),
    };
    let error = Notes::update(&uow, ghost.clone().into_active_model())
        .await
        .unwrap_err();
    assert!(matches!(error, DbErr::RecordNotFound(_)), "{error:?}");
    let error = Notes::delete(&uow, ghost).await.unwrap_err();
    assert!(matches!(error, DbErr::RecordNotFound(_)), "{error:?}");
    assert!(uow.changes().is_empty());
    uow.rollback().await.unwrap();
}

/// MySQL reports the rows an `UPDATE` matched, not only those it changed
/// (SQLx asks for `CLIENT_FOUND_ROWS`), so an update writing the values a
/// row already has succeeds instead of failing as "not updated", and
/// records no change.
#[tokio::test]
async fn an_update_that_changes_nothing_succeeds() {
    let Some(scratch) = Scratch::new("an_update_that_changes_nothing_succeeds").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    let seeded = db.uow(None, None).await;
    let created = Notes::insert(&seeded, new_note("same")).await.unwrap();
    seeded.commit().await.unwrap();

    let uow = db.uow(None, None).await;
    let mut unchanged = created.clone().into_active_model();
    unchanged.title = Set(created.title.clone());
    unchanged.body = Set(created.body.clone());
    let updated = Notes::update(&uow, unchanged)
        .await
        .expect("the row matched, though nothing changed");
    assert_eq!(updated, created);
    assert!(uow.changes().is_empty(), "{:?}", uow.changes());
    uow.commit().await.unwrap();
}

/// A row read with `find_for_update` stays locked until its unit of work
/// ends: a second unit of work locking it waits, then reads the committed
/// change.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn find_for_update_makes_a_second_unit_of_work_wait() {
    let Some(scratch) = Scratch::new("find_for_update_makes_a_second_unit_of_work_wait").await
    else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    let seeded = db.uow(None, None).await;
    let created = Notes::insert(&seeded, new_note("contended")).await.unwrap();
    seeded.commit().await.unwrap();

    let first = db.uow(None, None).await;
    let locked = Notes::find_for_update(&first, created.id)
        .await
        .unwrap()
        .expect("found");

    let second_db = db.clone();
    let id = created.id;
    let second = tokio::spawn(async move {
        let uow = second_db.uow(None, None).await;
        let seen = Notes::find_for_update(&uow, id).await?;
        uow.commit().await?;
        Ok::<_, DbErr>(seen)
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !second.is_finished(),
        "the second unit of work waits for the lock"
    );

    let mut changed = locked.into_active_model();
    changed.body = Set("changed by the first".into());
    Notes::update(&first, changed).await.unwrap();
    first.commit().await.unwrap();

    let seen = tokio::time::timeout(Duration::from_secs(10), second)
        .await
        .expect("the lock is released at commit")
        .unwrap()
        .unwrap()
        .expect("found");
    assert_eq!(seen.body, "changed by the first");
}
