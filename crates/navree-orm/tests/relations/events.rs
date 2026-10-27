//! Lifecycle hooks of typed pivot models: attaching, updating and
//! detaching through them fire their model events.

use std::sync::{Arc, Mutex};

use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
use navree_orm::hooks::{Flow, ModelEvent, ModelHook};
use navree_orm::prelude::*;
use navree_orm::{ExecContext, Result};

use crate::fixtures::seed;
use crate::models::DealWatcher;
use crate::support::app;

/// The events seen, in order, as `"{event} {user_id}"`.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Notes every event of a watcher row but its retrieval.
struct Watch(Arc<Seen>);

impl Watch {
    fn note(&self, event: ModelEvent, record: &Record<DealWatcher>) {
        if event != ModelEvent::Retrieved {
            self.0
                .0
                .lock()
                .unwrap()
                .push(format!("{event} {}", record.user_id));
        }
    }
}

impl ModelHook<DealWatcher> for Watch {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        record: &'a mut Record<DealWatcher>,
    ) -> BoxFuture<'a, Result<Flow>> {
        Box::pin(async move {
            self.note(event, record);
            Ok(Flow::Continue)
        })
    }

    fn after<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        record: &'a mut Record<DealWatcher>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.note(event, record);
            Ok(())
        })
    }
}

struct Hooks(Arc<Seen>);

impl Module for Hooks {
    fn register(&self, m: &mut ModuleBuilder) {
        let seen = self.0.clone();
        m.multi(Factory::<dyn ModelHook<DealWatcher>>::new(move |_| {
            let seen = seen.clone();
            async move { Ok(Arc::new(Watch(seen)) as Arc<dyn ModelHook<DealWatcher>>) }
        }));
    }
}

#[tokio::test]
async fn typed_pivot_writes_fire_the_pivot_models_events() {
    let seen = Arc::new(Seen::default());
    let (_app, db, _clock) = app(vec![Box::new(Hooks(seen.clone()))]).await;
    let db = &*db;
    let crm = seed(db).await;
    let ada = crm.ada.id;

    crm.renewal
        .watchers()
        .attach_with(db, [(ada, [col("level").set("all")])])
        .await
        .unwrap();
    assert_eq!(
        seen.take(),
        [
            format!("saving {ada}"),
            format!("creating {ada}"),
            format!("created {ada}"),
            format!("saved {ada}"),
        ]
    );

    crm.renewal
        .watchers()
        .update_existing_pivot(db, ada, [col("level").set("mentions")])
        .await
        .unwrap();
    assert_eq!(
        seen.take(),
        [
            format!("saving {ada}"),
            format!("updating {ada}"),
            format!("updated {ada}"),
            format!("saved {ada}"),
        ]
    );
    // Nothing changed: no update, no events.
    crm.renewal
        .watchers()
        .update_existing_pivot(db, ada, [col("level").set("mentions")])
        .await
        .unwrap();
    assert!(seen.take().is_empty());

    crm.renewal.watchers().detach(db, [ada]).await.unwrap();
    assert_eq!(
        seen.take(),
        [format!("deleting {ada}"), format!("deleted {ada}")]
    );

    // Syncing attaches and detaches through the model as well.
    crm.renewal
        .watchers()
        .sync(db, [crm.bob.id])
        .await
        .unwrap();
    let bob = crm.bob.id;
    assert_eq!(
        seen.take(),
        [
            format!("saving {bob}"),
            format!("creating {bob}"),
            format!("created {bob}"),
            format!("saved {bob}"),
        ]
    );
}
