//! When change events are prepared: as recorded writes run when the app
//! has a change observer, and only when `uow.changes()` asks otherwise (an
//! app that nobody observes pays for no serialization).

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use navree_db::{
    ChangeEvent, ChangeObserver, ChangeOp, DatabaseModule, Db, DbError, MigrationSet,
    ModuleBuilderDbExt, Uow, record,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver};
use serde::ser::SerializeStruct;
use serde_json::json;
use uuid::Uuid;

use crate::fixtures::{CreateNotes, notes_db};
use crate::support::TempDb;

thread_local! {
    /// How many `Counted` rows this thread serialized.
    static SERIALIZED: Cell<usize> = const { Cell::new(0) };
}

fn serialized() -> usize {
    SERIALIZED.get()
}

navree_db::record! {
    /// The `notes` table, as a record that counts its serializations.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Counted in "notes" {
        id: Uuid,
        title: String,
        body: String,
    }
}

impl serde::Serialize for Counted {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SERIALIZED.set(SERIALIZED.get() + 1);
        let mut note = serializer.serialize_struct("Counted", 3)?;
        note.serialize_field("id", &self.id)?;
        note.serialize_field("title", &self.title)?;
        note.serialize_field("body", &self.body)?;
        note.end()
    }
}

fn new_counted(title: &str) -> Counted {
    Counted {
        id: Uuid::now_v7(),
        title: title.to_owned(),
        body: format!("body of {title}"),
    }
}

/// Inserts, updates (twice: once without a change) and deletes a note.
async fn write_through(uow: &Uow) -> Uuid {
    let created = record::insert_recorded(uow, &new_counted("a"))
        .await
        .expect("insert");
    let changed = Counted {
        body: "changed".into(),
        ..created.clone()
    };
    let updated = record::update_recorded(uow, &created, &changed)
        .await
        .expect("update");
    let unchanged = record::update_recorded(uow, &updated, &updated.clone())
        .await
        .expect("update without a change");
    record::delete_recorded(uow, &unchanged)
        .await
        .expect("delete");
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
        _uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
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
    record::insert_recorded(&uow, &new_counted("b"))
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
