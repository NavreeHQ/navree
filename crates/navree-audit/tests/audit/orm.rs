//! The audit trail as navree-orm models: `AuditRecord` is queried like any
//! model, `AuditFilter` gives the ORM query of its entries, and changes
//! saved through ORM models inside a unit of work are audited.

use navree_audit::{AuditFilter, AuditRecord};
use navree_contracts::{AuditEntry, AuditKind};
use navree_db::Db;
use navree_orm::Model as _;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::support;

/// The fixtures' `notes` table as an ORM model.
#[derive(navree_orm::Model, Clone, Debug, Default, Serialize)]
#[model(table = "notes")]
struct Note {
    id: Uuid,
    title: String,
    body: String,
}

#[tokio::test]
async fn audit_records_are_orm_models() {
    let app = support::app().await;
    let log = support::audit_log(&app).await;
    let actor = Uuid::now_v7();
    log.record(AuditEntry::new(AuditKind::Auth, "auth.login").actor(actor))
        .await
        .unwrap();
    log.record(AuditEntry::new(AuditKind::Security, "auth.failed"))
        .await
        .unwrap();
    log.flush().await;

    let db = app.get::<Db>().await;
    assert_eq!(AuditRecord::table_name(), "audit_logs");
    let by_actor = AuditRecord::query()
        .where_(AuditRecord::ACTOR_ID.eq(actor))
        .get(&*db)
        .await
        .unwrap();
    assert_eq!(by_actor.len(), 1);
    assert_eq!(by_actor[0].action, "auth.login");
    assert_eq!(by_actor[0].kind, "auth");
    assert_eq!(
        AuditRecord::query()
            .where_(AuditRecord::KIND.eq("security"))
            .count(&*db)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn a_filter_is_an_orm_query_of_its_entries() {
    let app = support::app().await;
    let log = support::audit_log(&app).await;
    for action in ["deal.updated", "company.updated", "deal.created", "updated"] {
        log.record(AuditEntry::new(AuditKind::Custom, action))
            .await
            .unwrap();
    }
    log.flush().await;

    let db = app.get::<Db>().await;
    let filter = AuditFilter {
        action: Some("updated".to_owned()),
        ..AuditFilter::default()
    };
    let mut actions: Vec<String> = filter
        .query()
        .get(&*db)
        .await
        .unwrap()
        .into_models()
        .into_iter()
        .map(|record| record.action)
        .collect();
    actions.sort();
    assert_eq!(actions, ["company.updated", "deal.updated", "updated"]);

    let filter = AuditFilter {
        kind: Some(AuditKind::Auth),
        ..AuditFilter::default()
    };
    assert_eq!(filter.query().count(&*db).await.unwrap(), 0);
}

#[tokio::test]
async fn changes_saved_through_orm_models_are_audited() {
    let app = support::app().await;
    let db = app.get::<Db>().await;
    let actor = Uuid::now_v7();

    let uow = db.uow(Some(actor), Some("req-orm".into())).await;
    let mut note = Note::create(
        &uow,
        Note {
            title: "a".to_owned(),
            body: "body of a".to_owned(),
            ..Note::default()
        },
    )
    .await
    .unwrap();
    note.title = "renamed".to_owned();
    note.save(&uow).await.unwrap();
    uow.commit().await.unwrap();

    let rows = support::audit_rows(&app).await;
    let actions: Vec<&str> = rows.iter().map(|row| row.action.as_str()).collect();
    assert_eq!(actions, ["note.created", "note.updated"]);
    let id = note.id.to_string();
    for row in &rows {
        assert_eq!(row.kind, "data");
        assert_eq!(row.actor_id, Some(actor));
        assert_eq!(row.subject_type.as_deref(), Some("Note"));
        assert_eq!(row.subject_id.as_deref(), Some(id.as_str()));
        assert_eq!(row.request_id.as_deref(), Some("req-orm"));
    }
    assert_eq!(
        rows[0].new_values,
        Some(json!({"id": id, "title": "a", "body": "body of a"}))
    );
    assert_eq!(rows[1].old_values, Some(json!({"title": "a"})));
    assert_eq!(rows[1].new_values, Some(json!({"title": "renamed"})));
}

#[tokio::test]
async fn orm_writes_rolled_back_leave_no_audit_row() {
    let app = support::app().await;
    let db = app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    Note::create(
        &uow,
        Note {
            title: "gone".to_owned(),
            ..Note::default()
        },
    )
    .await
    .unwrap();
    uow.rollback().await.unwrap();

    assert!(support::audit_rows(&app).await.is_empty());
    assert_eq!(support::count(&app, "notes").await, 0);
}
