//! `Repo<E>`: writes through the unit of work that record change events, and
//! change observers running inside the same transaction.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_db::{
    ChangeEvent, ChangeObserver, ChangeOp, DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt,
    Repo, Uow,
};
use navree_kernel::{
    BoxFuture, Injectable, Navree, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use sea_orm::sea_query::Query;
use sea_orm::{ActiveValue::Set, ConnectionTrait, DatabaseTransaction, DbErr, IntoActiveModel};
use serde_json::json;
use uuid::Uuid;

use crate::fixtures::{CreateNotes, count, new_note, note, notes_db};
use crate::support::TempDb;

type Notes = Repo<note::Entity>;

#[tokio::test]
async fn repo_records_change_events() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;

    let created = Notes::insert(&uow, new_note("a")).await.expect("insert");
    let mut changed = created.clone().into_active_model();
    changed.body = Set("changed".into());
    let updated = Notes::update(&uow, changed).await.expect("update");
    assert_eq!(updated.body, "changed");
    Notes::delete(&uow, updated.clone()).await.expect("delete");

    let changes = uow.changes();
    assert_eq!(changes.len(), 3);
    let id = created.id.to_string();

    assert_eq!(changes[0].entity, "notes");
    assert_eq!(changes[0].id, id);
    assert_eq!(changes[0].op, ChangeOp::Created);
    assert_eq!(changes[0].old, None);
    assert_eq!(
        changes[0].new,
        Some(json!({"id": id, "title": "a", "body": "body of a"}))
    );

    assert_eq!(changes[1].op, ChangeOp::Updated);
    assert_eq!(changes[1].old, Some(json!({"body": "body of a"})));
    assert_eq!(changes[1].new, Some(json!({"body": "changed"})));

    assert_eq!(changes[2].op, ChangeOp::Deleted);
    assert_eq!(
        changes[2].old,
        Some(json!({"id": id, "title": "a", "body": "changed"}))
    );
    assert_eq!(changes[2].new, None);

    uow.commit().await.unwrap();
    assert_eq!(count(&db, "notes").await, 0);
}

#[tokio::test]
async fn repo_find_reads_inside_the_unit_of_work() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
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
}

#[tokio::test]
async fn repo_update_and_delete_of_missing_records_fail() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
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
}

/// Makes [`FailingObserver`] fail.
#[derive(Default)]
struct FailSwitch(AtomicBool);

/// Writes one `audit` row per change, inside the unit of work's transaction.
struct AuditObserver;

impl Injectable for AuditObserver {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(AuditObserver) })
    }
}

impl ChangeObserver for AuditObserver {
    fn observe<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move {
            for change in changes {
                tx.execute(
                    Query::insert()
                        .into_table("audit")
                        .columns(["entity", "entity_id", "op", "actor"])
                        .values_panic([
                            change.entity.clone().into(),
                            change.id.clone().into(),
                            change.op.as_str().into(),
                            uow.actor().map(|a| a.to_string()).into(),
                        ]),
                )
                .await?;
            }
            Ok(())
        })
    }
}

/// Fails while the [`FailSwitch`] is on.
struct FailingObserver {
    switch: Arc<FailSwitch>,
}

impl Injectable for FailingObserver {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<FailSwitch>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(FailingObserver {
                switch: r.get::<FailSwitch>().await?,
            })
        })
    }
}

impl ChangeObserver for FailingObserver {
    fn observe<'a>(
        &'a self,
        _tx: &'a DatabaseTransaction,
        _uow: &'a Uow,
        _changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move {
            if self.switch.0.load(Ordering::SeqCst) {
                Err(DbErr::Custom("observer refused".into()))
            } else {
                Ok(())
            }
        })
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(CreateNotes))
            .value(FailSwitch::default())
            .change_observer::<AuditObserver>()
            .change_observer::<FailingObserver>();
    }
}

#[tokio::test]
async fn repo_records_change_events_and_observers_run_in_same_tx() {
    let temp = TempDb::new();
    let mut options = temp.options();
    options.migrate_on_boot = true;
    let app = Navree::app()
        .module(DatabaseModule::for_root(options))
        .module(NotesModule)
        .boot()
        .await
        .expect("boots");
    let db = app.ctx().get::<Db>().await.unwrap();
    let actor = Uuid::now_v7();

    let uow = db.uow(Some(actor), Some("req".into())).await;
    Notes::insert(&uow, new_note("a")).await.unwrap();
    Notes::insert(&uow, new_note("b")).await.unwrap();
    uow.commit().await.expect("commit");
    assert_eq!(count(&db, "notes").await, 2);
    assert_eq!(count(&db, "audit").await, 2, "one audit row per change");

    app.ctx()
        .get::<FailSwitch>()
        .await
        .unwrap()
        .0
        .store(true, Ordering::SeqCst);
    let uow = db.uow(Some(actor), None).await;
    Notes::insert(&uow, new_note("c")).await.unwrap();
    let error = uow.commit().await.unwrap_err();
    assert!(error.to_string().contains("observer refused"), "{error}");
    assert_eq!(count(&db, "notes").await, 2, "the change is rolled back");
    assert_eq!(
        count(&db, "audit").await,
        2,
        "the observer's audit row is rolled back with it"
    );
}

/// Wrongly tries to use the unit of work's own `tx()` from inside commit.
struct ReentrantObserver;

impl Injectable for ReentrantObserver {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ReentrantObserver) })
    }
}

impl ChangeObserver for ReentrantObserver {
    fn observe<'a>(
        &'a self,
        _tx: &'a DatabaseTransaction,
        uow: &'a Uow,
        _changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move { uow.tx().await.map(drop) })
    }
}

struct ReentrantModule;

impl Module for ReentrantModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(CreateNotes))
            .change_observer::<ReentrantObserver>();
    }
}

#[tokio::test]
async fn observer_using_uow_tx_gets_an_error_instead_of_a_deadlock() {
    let temp = TempDb::new();
    let mut options = temp.options();
    options.migrate_on_boot = true;
    let app = Navree::app()
        .module(DatabaseModule::for_root(options))
        .module(ReentrantModule)
        .boot()
        .await
        .expect("boots");
    let db = app.ctx().get::<Db>().await.unwrap();
    let uow = db.uow(None, None).await;
    Notes::insert(&uow, new_note("a")).await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), uow.commit())
        .await
        .expect("commit must not deadlock");
    let error = result.unwrap_err();
    assert!(error.to_string().contains("already committed"), "{error}");
    assert_eq!(count(&db, "notes").await, 0);
}
