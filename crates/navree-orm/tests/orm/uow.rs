//! Running inside navree-db's unit of work: one transaction, change events
//! for the observers (the audit trail), after-commit hooks, and `&Db`
//! joining the task's unit of work.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_db::{ChangeOp, Uow};
use navree_orm::Model;
use serde_json::json;

use crate::models::{Company, Deal, Stage};
use crate::support::{count_rows, sqlite};

#[tokio::test]
async fn writes_record_change_events_with_the_stored_values() {
    let t = sqlite().await;
    let uow = t.db.uow(None, Some("req-1".into())).await;
    let mut acme = Company::create(
        &uow,
        Company {
            name: "Acme".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    acme.name = "Acme Corp".into();
    acme.save(&uow).await.unwrap();
    let mut deal = Deal::create(&uow, Deal { stage: Stage::Qualified, ..Deal::new("Renewal", 10) })
        .await
        .unwrap();
    deal.delete(&uow).await.unwrap();
    acme.delete(&uow).await.unwrap();

    let changes = uow.changes();
    let ops: Vec<(String, ChangeOp)> = changes.iter().map(|c| (c.entity.clone(), c.op)).collect();
    assert_eq!(
        ops,
        [
            ("companies".to_owned(), ChangeOp::Created),
            ("companies".to_owned(), ChangeOp::Updated),
            ("deals".to_owned(), ChangeOp::Created),
            ("deals".to_owned(), ChangeOp::Deleted),
            ("companies".to_owned(), ChangeOp::Deleted),
        ]
    );
    let created = &changes[0];
    assert_eq!(created.id, "1");
    assert_eq!(created.new.as_ref().unwrap()["name"], "Acme");
    assert!(created.old.is_none());
    let updated = &changes[1];
    assert_eq!(updated.old.as_ref().unwrap()["name"], "Acme");
    assert_eq!(updated.new.as_ref().unwrap()["name"], "Acme Corp");
    assert_eq!(
        updated.new.as_ref().unwrap().as_object().unwrap().keys().collect::<Vec<_>>(),
        ["name", "updated_at"],
        "only the changed columns"
    );
    assert_eq!(changes[2].new.as_ref().unwrap()["stage"], json!("qualified"));
    assert_eq!(changes[3].id, deal.id.to_string());
    assert_eq!(changes[3].old.as_ref().unwrap()["title"], "Renewal");
    uow.commit().await.unwrap();
}

#[tokio::test]
async fn a_rolled_back_unit_of_work_leaves_nothing() {
    let t = sqlite().await;
    let uow = t.db.uow(None, None).await;
    Company::create(&uow, Company { name: "Ghost".into(), ..Company::default() })
        .await
        .unwrap();
    // Reads in the unit of work see its own writes.
    assert_eq!(Company::query().count(&uow).await.unwrap(), 1);
    uow.rollback().await.unwrap();
    assert_eq!(count_rows(&t.db, "companies").await, 0);
}

#[tokio::test]
async fn the_db_joins_the_current_unit_of_work() {
    let t = sqlite().await;
    let uow = t.db.uow(None, None).await;
    let db = t.db.clone();
    uow.scope(async {
        // Written through `&Db`, recorded in the scoped unit of work.
        Company::create(&db, Company { name: "Joined".into(), ..Company::default() })
            .await
            .unwrap();
        assert_eq!(Company::query().count(&db).await.unwrap(), 1, "reads see the open transaction");
    })
    .await;
    assert_eq!(uow.changes().len(), 1);
    uow.rollback().await.unwrap();
    assert_eq!(count_rows(&t.db, "companies").await, 0, "it was the unit of work's write");

    // Without a current unit of work, `&Db` writes at once (no events).
    Company::create(&t.db, Company { name: "Direct".into(), ..Company::default() })
        .await
        .unwrap();
    assert_eq!(count_rows(&t.db, "companies").await, 1);
}

#[tokio::test]
async fn after_commit_hooks_follow_orm_writes() {
    let t = sqlite().await;
    let ran = Arc::new(AtomicBool::new(false));
    let uow: Uow = t.db.uow(None, None).await;
    Company::create(&uow, Company { name: "Hooked".into(), ..Company::default() })
        .await
        .unwrap();
    let flag = ran.clone();
    uow.after_commit(move || {
        Box::pin(async move {
            flag.store(true, Ordering::SeqCst);
        })
    });
    assert!(!ran.load(Ordering::SeqCst));
    uow.commit().await.unwrap();
    assert!(ran.load(Ordering::SeqCst));
    assert_eq!(count_rows(&t.db, "companies").await, 1);
}

#[tokio::test]
async fn mass_writes_run_in_the_unit_of_work_without_events() {
    let t = sqlite().await;
    for name in ["A", "B"] {
        Company::create(&t.db, Company { name: name.into(), ..Company::default() })
            .await
            .unwrap();
    }
    let uow = t.db.uow(None, None).await;
    assert_eq!(Company::query().update(&uow, [Company::ACTIVE.set(true)]).await.unwrap(), 2);
    assert_eq!(Company::query().where_(Company::NAME.eq("A")).delete(&uow).await.unwrap(), 1);
    assert!(uow.changes().is_empty());
    uow.rollback().await.unwrap();
    assert_eq!(count_rows(&t.db, "companies").await, 2);
}
