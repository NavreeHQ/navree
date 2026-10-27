//! When change events are prepared: as `Repo` writes when the app has a
//! change observer, and only when `uow.changes()` asks otherwise (an app
//! that nobody observes pays for no serialization).

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use navree_db::{
    ChangeEvent, ChangeObserver, ChangeOp, DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt,
    Repo, Uow,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver};
use sea_orm::{ActiveValue::Set, DatabaseTransaction, DbErr, IntoActiveModel};
use serde_json::json;
use uuid::Uuid;

use crate::fixtures::{CreateNotes, notes_db};
use crate::support::TempDb;

thread_local! {
    /// How many `counted` rows this thread serialized.
    static SERIALIZED: Cell<usize> = const { Cell::new(0) };
}

fn serialized() -> usize {
    SERIALIZED.get()
}

/// The `notes` table, with a model that counts its serializations.
mod counted {
    use sea_orm::entity::prelude::*;
    use serde::ser::SerializeStruct;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub title: String,
        pub body: String,
    }

    impl serde::Serialize for Model {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            super::SERIALIZED.set(super::SERIALIZED.get() + 1);
            let mut note = serializer.serialize_struct("Model", 3)?;
            note.serialize_field("id", &self.id)?;
            note.serialize_field("title", &self.title)?;
            note.serialize_field("body", &self.body)?;
            note.end()
        }
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

type Counted = Repo<counted::Entity>;

fn new_counted(title: &str) -> counted::ActiveModel {
    counted::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set(title.to_owned()),
        body: Set(format!("body of {title}")),
    }
}

/// Inserts, updates (twice: once without a change) and deletes a note.
async fn write_through(uow: &Uow) -> Uuid {
    let created = Counted::insert(uow, new_counted("a"))
        .await
        .expect("insert");
    let mut changed = created.clone().into_active_model();
    changed.body = Set("changed".into());
    let updated = Counted::update(uow, changed).await.expect("update");
    let mut same = updated.clone().into_active_model();
    same.body = Set(updated.body.clone());
    let unchanged = Counted::update(uow, same)
        .await
        .expect("update without a change");
    Counted::delete(uow, unchanged).await.expect("delete");
    created.id
}

#[tokio::test]
async fn without_observers_events_are_prepared_when_read() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let uow = db.uow(None, None).await;

    let before = serialized();
    let id = write_through(&uow).await.to_string();
    assert_eq!(
        serialized(),
        before,
        "nothing listens, nothing is serialized"
    );

    let changes = uow.changes();
    assert!(serialized() > before, "read: the events are prepared");
    let summary: Vec<_> = changes
        .iter()
        .map(|change| (change.op, change.old.clone(), change.new.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            (
                ChangeOp::Created,
                None,
                Some(json!({"id": id, "title": "a", "body": "body of a"}))
            ),
            (
                ChangeOp::Updated,
                Some(json!({"body": "body of a"})),
                Some(json!({"body": "changed"}))
            ),
            (
                ChangeOp::Deleted,
                Some(json!({"id": id, "title": "a", "body": "changed"})),
                None
            ),
        ],
        "an update that changed nothing records no event"
    );
    assert!(
        changes
            .iter()
            .all(|change| change.entity == "notes" && change.id == id)
    );
    assert_eq!(uow.changes(), changes, "read again, the same events");
    uow.commit().await.expect("commits");
}

/// Keeps every change it sees.
#[derive(Default)]
struct Seen(Mutex<Vec<ChangeEvent>>);

struct Recorder(Arc<Seen>);

impl Injectable for Recorder {
    fn dependencies() -> Vec<navree_kernel::ProviderKey> {
        vec![navree_kernel::ProviderKey::of::<Seen>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(Recorder(r.get::<Seen>().await?)) })
    }
}

impl ChangeObserver for Recorder {
    fn observe<'a>(
        &'a self,
        _tx: &'a DatabaseTransaction,
        _uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        self.0.0.lock().unwrap().extend_from_slice(changes);
        Box::pin(async { Ok(()) })
    }
}

struct Observed;

impl Module for Observed {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(CreateNotes))
            .value(Seen::default())
            .change_observer::<Recorder>();
    }
}

#[tokio::test]
async fn with_an_observer_events_are_prepared_as_records_are_written() {
    let temp = TempDb::new();
    let mut options = temp.options();
    options.migrate_on_boot = true;
    let app = Navree::app()
        .module(DatabaseModule::for_root(options))
        .module(Observed)
        .boot()
        .await
        .expect("boots");
    let db = app.ctx().get::<Db>().await.unwrap();
    let uow = db.uow(None, None).await;

    let before = serialized();
    Counted::insert(&uow, new_counted("b"))
        .await
        .expect("insert");
    assert_eq!(serialized(), before + 1, "serialized as it is written");
    uow.commit().await.expect("commits");

    let seen = app.ctx().get::<Seen>().await.unwrap();
    let seen = seen.0.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].op, ChangeOp::Created);
    assert_eq!(seen[0].new.as_ref().unwrap()["title"], "b");
}

/// An app's `Db` knows at boot whether anything observes its changes.
#[tokio::test]
async fn units_of_work_without_observers_still_commit_their_changes() {
    let temp = TempDb::new();
    let mut options = temp.options();
    options.migrate_on_boot = true;
    struct Unobserved;
    impl Module for Unobserved {
        fn register(&self, m: &mut ModuleBuilder) {
            m.migrations(MigrationSet::new("notes").migration(CreateNotes));
        }
    }
    let app = Navree::app()
        .module(DatabaseModule::for_root(options))
        .module(Unobserved)
        .boot()
        .await
        .expect("boots");
    let db = app.ctx().get::<Db>().await.unwrap();
    let uow = db.uow(None, None).await;
    let before = serialized();
    write_through(&uow).await;
    assert_eq!(serialized(), before);
    uow.commit().await.expect("commits");
    assert_eq!(serialized(), before, "committing needs no events");
    assert_eq!(crate::fixtures::count(&db, "notes").await, 0);
}
