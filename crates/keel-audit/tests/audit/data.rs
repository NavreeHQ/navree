//! Data changes recorded through the unit of work become `data` audit rows,
//! written by the `dyn ChangeObserver` inside the same transaction.

use std::sync::{Arc, Mutex};

use keel_audit::{AuditModule, audit_log};
use keel_db::{ChangeEvent, ChangeObserver, Db, ModuleBuilderDbExt, Repo, Uow};
use keel_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, IntoActiveModel,
    PaginatorTrait, QueryFilter,
};
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, audit_rows, count, new_note, note};

type Notes = Repo<note::Entity>;

/// What [`ProbeObserver`] saw inside the transaction, per change.
#[derive(Default)]
struct Probe {
    audit_rows_seen: Mutex<Vec<u64>>,
    fail: std::sync::atomic::AtomicBool,
}

/// Runs after the audit observer (its module imports `AuditModule`): counts
/// the audit rows of each change through the transaction it is given, and
/// fails when told to.
struct ProbeObserver(Arc<Probe>);

impl Injectable for ProbeObserver {
    fn dependencies() -> Vec<keel_kernel::ProviderKey> {
        vec![keel_kernel::ProviderKey::of::<Probe>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(ProbeObserver(r.get::<Probe>().await?)) })
    }
}

impl ChangeObserver for ProbeObserver {
    fn observe<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        _uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move {
            for change in changes {
                let seen = audit_log::Entity::find()
                    .filter(audit_log::Column::SubjectId.eq(change.id.clone()))
                    .count(tx)
                    .await?;
                self.0.audit_rows_seen.lock().unwrap().push(seen);
            }
            if self.0.fail.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(DbErr::Custom("probe failure".into()));
            }
            Ok(())
        })
    }
}

struct ProbeModule(Arc<Probe>);

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AuditModule)
            .value_arc(self.0.clone())
            .change_observer::<ProbeObserver>();
    }
}

#[tokio::test]
async fn data_change_writes_audit_row_in_same_tx() {
    let probe = Arc::new(Probe::default());
    let app = support::builder()
        .0
        .module(ProbeModule(probe.clone()))
        .build()
        .await;
    let db = app.get::<Db>().await;
    let actor = Uuid::now_v7();

    let uow = db.uow(Some(actor), Some("req-1".into())).await;
    let note = Notes::insert(&uow, new_note("a")).await.unwrap();
    assert_eq!(count(&app, "audit_logs").await, 0, "nothing before commit");
    uow.commit().await.unwrap();

    assert_eq!(
        *probe.audit_rows_seen.lock().unwrap(),
        vec![1],
        "a later observer sees the audit row inside the same transaction"
    );
    let rows = audit_rows(&app).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.kind, "data");
    assert_eq!(row.action, "note.created");
    assert_eq!(row.actor_id, Some(actor));
    assert_eq!(row.subject_type.as_deref(), Some("Note"));
    assert_eq!(row.subject_id, Some(note.id.to_string()));
    assert_eq!(row.old_values, None);
    assert_eq!(
        row.new_values,
        Some(json!({"id": note.id, "title": "a", "body": "body of a"}))
    );
    assert_eq!(row.request_id.as_deref(), Some("req-1"));
    assert_eq!(row.metadata, json!({}));
}

#[tokio::test]
async fn deletes_record_the_old_values() {
    let app = support::app().await;
    let db = app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    let note = Notes::insert(&uow, new_note("a")).await.unwrap();
    Notes::delete(&uow, note.clone()).await.unwrap();
    uow.commit().await.unwrap();

    let rows = audit_rows(&app).await;
    let actions: Vec<_> = rows.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions, ["note.created", "note.deleted"]);
    let deleted = &rows[1];
    assert_eq!(deleted.actor_id, None);
    assert_eq!(
        deleted.old_values,
        Some(json!({"id": note.id, "title": "a", "body": "body of a"}))
    );
    assert_eq!(deleted.new_values, None);
}

#[tokio::test]
async fn rolled_back_change_leaves_no_audit_row() {
    let probe = Arc::new(Probe::default());
    let app = support::builder()
        .0
        .module(ProbeModule(probe.clone()))
        .build()
        .await;
    let db = app.get::<Db>().await;

    // Rolled back by the caller: observers never run.
    let uow = db.uow(None, None).await;
    Notes::insert(&uow, new_note("a")).await.unwrap();
    uow.rollback().await.unwrap();

    // A later observer fails: the audit row it saw is rolled back too.
    probe.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    let uow = db.uow(None, None).await;
    Notes::insert(&uow, new_note("b")).await.unwrap();
    uow.commit().await.unwrap_err();
    assert_eq!(*probe.audit_rows_seen.lock().unwrap(), vec![1]);

    assert_eq!(count(&app, "notes").await, 0);
    assert_eq!(count(&app, "audit_logs").await, 0);
}

#[tokio::test]
async fn update_records_only_changed_fields() {
    let app = support::app().await;
    let db = app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    let note = Notes::insert(&uow, new_note("a")).await.unwrap();
    let mut changed = note.clone().into_active_model();
    changed.body = Set("changed".into());
    Notes::update(&uow, changed).await.unwrap();
    // A hand-recorded event carrying whole records is reduced to the
    // fields that differ.
    uow.record_change(ChangeEvent {
        entity: "notes".into(),
        id: note.id.to_string(),
        op: keel_db::ChangeOp::Updated,
        old: Some(json!({"id": note.id, "title": "a", "body": "changed"})),
        new: Some(json!({"id": note.id, "title": "b", "body": "changed", "extra": 1})),
    });
    uow.commit().await.unwrap();

    let rows = audit_rows(&app).await;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].action, "note.updated");
    assert_eq!(rows[1].old_values, Some(json!({"body": "body of a"})));
    assert_eq!(rows[1].new_values, Some(json!({"body": "changed"})));
    assert_eq!(
        rows[2].old_values,
        Some(json!({"title": "a", "extra": null}))
    );
    assert_eq!(rows[2].new_values, Some(json!({"title": "b", "extra": 1})));
}

#[tokio::test]
async fn update_without_changes_writes_no_row() {
    let app = support::app().await;
    let db = app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    let note = Notes::insert(&uow, new_note("a")).await.unwrap();
    uow.record_change(ChangeEvent {
        entity: "notes".into(),
        id: note.id.to_string(),
        op: keel_db::ChangeOp::Updated,
        old: Some(json!({"title": "a"})),
        new: Some(json!({"title": "a"})),
    });
    uow.commit().await.unwrap();

    let actions: Vec<_> = audit_rows(&app)
        .await
        .into_iter()
        .map(|r| r.action)
        .collect();
    assert_eq!(actions, ["note.created"]);
}
