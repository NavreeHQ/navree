//! Entries written while a user impersonates another record the
//! impersonator: data changes and `record_in` take it from the unit of
//! work, `record` from the task's current unit of work, and the list
//! filters by it.

use navree_audit::{AuditFilter, AuditLog};
use navree_contracts::{AuditEntry, AuditKind};
use navree_db::{Db, PageQuery, Repo, Uow};
use uuid::Uuid;

use crate::support::{self, audit_log, audit_rows, new_note, note};

type Notes = Repo<note::Entity>;

#[tokio::test]
async fn data_changes_of_an_impersonated_unit_of_work_record_the_impersonator() {
    let app = support::app().await;
    let db = app.get::<Db>().await;
    let (user, admin) = (Uuid::now_v7(), Uuid::now_v7());

    let uow = db.impersonated_uow(user, admin, Some("req-1".into())).await;
    Notes::insert(&uow, new_note("seen by support"))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let plain = db.uow(Some(user), None).await;
    Notes::insert(&plain, new_note("by the user"))
        .await
        .unwrap();
    plain.commit().await.unwrap();

    let rows = audit_rows(&app).await;
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].action, "note.created");
    assert_eq!(rows[0].actor_id, Some(user));
    assert_eq!(rows[0].impersonator_id, Some(admin));
    assert_eq!(rows[1].impersonator_id, None);
}

#[tokio::test]
async fn sink_entries_take_the_impersonator_of_the_current_unit_of_work() {
    let app = support::app().await;
    let db = app.get::<Db>().await;
    let log = audit_log(&app).await;
    let (user, admin, other) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());

    let uow = db.impersonated_uow(user, admin, None).await;
    let log_in_scope = log.clone();
    uow.scope(async move {
        log_in_scope
            .record(AuditEntry::new(AuditKind::Custom, "deal.exported").actor(user))
            .await
            .unwrap();
        // An explicit impersonator wins.
        log_in_scope
            .record(
                AuditEntry::new(AuditKind::Custom, "deal.shared")
                    .actor(user)
                    .impersonator(other),
            )
            .await
            .unwrap();
    })
    .await;
    uow.rollback().await.unwrap();
    // Outside any unit of work: none.
    log.record(AuditEntry::new(AuditKind::Auth, "auth.login").actor(user))
        .await
        .unwrap();
    log.flush().await;

    let rows = audit_rows(&app).await;
    let by_action = |action: &str| {
        rows.iter()
            .find(|row| row.action == action)
            .unwrap_or_else(|| panic!("{action} in {rows:?}"))
            .impersonator_id
    };
    assert_eq!(by_action("deal.exported"), Some(admin));
    assert_eq!(by_action("deal.shared"), Some(other));
    assert_eq!(by_action("auth.login"), None);
}

#[tokio::test]
async fn record_in_defaults_to_the_impersonator_of_the_unit_of_work() {
    let app = support::app().await;
    let db = app.get::<Db>().await;
    let log = app.get::<AuditLog>().await;
    let (user, admin) = (Uuid::now_v7(), Uuid::now_v7());

    let uow: Uow = db.impersonated_uow(user, admin, None).await;
    log.record_in(&uow, AuditEntry::new(AuditKind::Custom, "deal.won"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let rows = audit_rows(&app).await;
    assert_eq!(rows[0].actor_id, Some(user));
    assert_eq!(rows[0].impersonator_id, Some(admin));
}

#[tokio::test]
async fn the_list_filters_by_impersonator() {
    let app = support::app().await;
    let db = app.get::<Db>().await;
    let log = app.get::<AuditLog>().await;
    let (user, admin) = (Uuid::now_v7(), Uuid::now_v7());

    let uow = db.impersonated_uow(user, admin, None).await;
    Notes::insert(&uow, new_note("a")).await.unwrap();
    uow.commit().await.unwrap();
    let uow = db.uow(Some(user), None).await;
    Notes::insert(&uow, new_note("b")).await.unwrap();
    uow.commit().await.unwrap();

    let page = log
        .list(
            &AuditFilter {
                impersonator_id: Some(admin),
                ..AuditFilter::default()
            },
            PageQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].impersonator_id, Some(admin));
}
