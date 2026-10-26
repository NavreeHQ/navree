//! Entries recorded through `dyn AuditSink` / `AuditLog::record` (own
//! connection, outside the request transaction) and `AuditLog::record_in`
//! (inside it).

use std::sync::Arc;
use std::time::Duration;

use navis_audit::AuditLog;
use navis_contracts::{AuditEntry, AuditKind, AuditSink};
use navis_db::{Db, Repo};
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, audit_log, audit_rows, count, new_note, note};

type Notes = Repo<note::Entity>;

#[tokio::test]
async fn security_entry_survives_rollback() {
    let app = support::app().await;
    let db = app.get::<Db>().await;
    let sink = app.get::<dyn AuditSink>().await;
    let actor = Uuid::now_v7();

    // A request's unit of work holds its transaction (on SQLite, the write
    // lock) while the denial is recorded.
    let uow = db.uow(Some(actor), Some("req-9".into())).await;
    Notes::insert(&uow, new_note("a")).await.unwrap();
    let entry = AuditEntry::new(AuditKind::Security, "access.denied")
        .actor(actor)
        .subject("Note", "42")
        .request_id("req-9")
        .ip("127.0.0.1")
        .user_agent("tests")
        .metadata(json!({"route": "/api/notes/{id}"}));
    tokio::time::timeout(Duration::from_secs(2), sink.record(entry))
        .await
        .expect("recording does not wait for the open transaction")
        .unwrap();
    uow.rollback().await.unwrap();
    audit_log(&app).await.flush().await;

    assert_eq!(count(&app, "notes").await, 0);
    let rows = audit_rows(&app).await;
    assert_eq!(rows.len(), 1, "only the security entry: {rows:?}");
    let row = &rows[0];
    assert_eq!(row.kind, "security");
    assert_eq!(row.action, "access.denied");
    assert_eq!(row.actor_id, Some(actor));
    assert_eq!(row.subject_type.as_deref(), Some("Note"));
    assert_eq!(row.subject_id.as_deref(), Some("42"));
    assert_eq!(row.request_id.as_deref(), Some("req-9"));
    assert_eq!(row.ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(row.user_agent.as_deref(), Some("tests"));
    assert_eq!(row.metadata, json!({"route": "/api/notes/{id}"}));
}

#[tokio::test]
async fn the_sink_is_the_audit_log() {
    let app = support::app().await;
    let sink = app.get::<dyn AuditSink>().await;
    let log = app.get::<AuditLog>().await;
    assert!(std::ptr::addr_eq(Arc::as_ptr(&sink), Arc::as_ptr(&log)));
}

#[tokio::test]
async fn entries_are_written_in_order() {
    let app = support::app().await;
    let log = audit_log(&app).await;
    for i in 0..50 {
        log.record(AuditEntry::new(AuditKind::Auth, format!("auth.login.{i}")))
            .await
            .unwrap();
    }
    log.flush().await;
    let actions: Vec<_> = audit_rows(&app)
        .await
        .into_iter()
        .map(|r| r.action)
        .collect();
    let expected: Vec<_> = (0..50).map(|i| format!("auth.login.{i}")).collect();
    assert_eq!(actions, expected);
}

#[tokio::test]
async fn graceful_shutdown_drains_the_queue() {
    let (url, _guard) = navis_testing::temp_sqlite_url();
    let app = support::builder()
        .0
        .config("database.url", url.clone())
        .build()
        .await;
    let log = audit_log(&app).await;
    for i in 0..20 {
        log.record(AuditEntry::new(AuditKind::Auth, format!("auth.login.{i}")))
            .await
            .unwrap();
    }
    drop(log);
    app.shutdown().await.unwrap();

    let db = Db::connect(&navis_db::DbOptions {
        url,
        ..navis_db::DbOptions::default()
    })
    .await
    .unwrap();
    use sea_orm::{EntityTrait, PaginatorTrait};
    let written = navis_audit::audit_log::Entity::find()
        .count(db.conn())
        .await
        .unwrap();
    assert_eq!(written, 20);
}

#[tokio::test]
async fn record_in_commits_and_rolls_back_with_the_unit_of_work() {
    let app = support::app().await;
    let db = app.get::<Db>().await;
    let log = audit_log(&app).await;
    let actor = Uuid::now_v7();

    let uow = db.uow(Some(actor), Some("req-1".into())).await;
    log.record_in(&uow, AuditEntry::new(AuditKind::Custom, "deal.won"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(count(&app, "audit_logs").await, 0);

    let uow = db.uow(Some(actor), Some("req-2".into())).await;
    log.record_in(
        &uow,
        AuditEntry::new(AuditKind::Custom, "deal.won").subject("Deal", "7"),
    )
    .await
    .unwrap();
    uow.commit().await.unwrap();
    let rows = audit_rows(&app).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "custom");
    assert_eq!(
        rows[0].actor_id,
        Some(actor),
        "defaults to the unit of work's actor"
    );
    assert_eq!(rows[0].request_id.as_deref(), Some("req-2"));
}

#[tokio::test]
async fn disabled_audit_writes_nothing() {
    let app = support::builder()
        .0
        .config("audit.enabled", false)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let log = audit_log(&app).await;

    let uow = db.uow(None, None).await;
    Notes::insert(&uow, new_note("a")).await.unwrap();
    log.record_in(&uow, AuditEntry::new(AuditKind::Custom, "x"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    log.record(AuditEntry::new(AuditKind::Security, "y"))
        .await
        .unwrap();
    log.flush().await;

    assert_eq!(count(&app, "notes").await, 1);
    assert_eq!(count(&app, "audit_logs").await, 0);
}
