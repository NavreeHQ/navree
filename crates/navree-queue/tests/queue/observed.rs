//! `Queue::push_observed`: jobs queued by a change observer join the commit
//! it observes, on every driver (a `QueueStore` driver queues them once the
//! commit succeeds).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_contracts::{Clock, FixedClock, JobEnvelope};
use navree_db::{
    ChangeEvent, ChangeObserver, ChangeOp, DatabaseModule, Db, DbError, ModuleBuilderDbExt, Uow,
};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Lazy, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_queue::{Queue, QueueDriver, QueueModule};
use navree_testing::TestApp;

use crate::support::{JobsModule, Journal, Record, options, start};

/// While set, [`QueueingObserver`] queues its job and then fails the commit.
#[derive(Default)]
struct Refuse(AtomicBool);

/// Queues one [`Record`] per change, labelled with the change's id.
struct QueueingObserver {
    queue: Lazy<Queue>,
    refuse: Arc<Refuse>,
}

impl Injectable for QueueingObserver {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Refuse>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(QueueingObserver {
                queue: r.lazy::<Queue>(),
                refuse: r.get::<Refuse>().await?,
            })
        })
    }
}

impl ChangeObserver for QueueingObserver {
    fn observe_with<'a>(
        &'a self,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            let queue = self
                .queue
                .get()
                .await
                .map_err(|e| DbError::invalid(e.to_string()))?;
            for change in changes {
                let envelope = JobEnvelope::of(&Record::new(change.id.clone()))
                    .map_err(|e| DbError::invalid(e.to_string()))?;
                queue
                    .push_observed(uow, envelope)
                    .await
                    .map_err(|e| DbError::invalid(e.to_string()))?;
            }
            if self.refuse.0.load(Ordering::SeqCst) {
                return Err(DbError::invalid("refused"));
            }
            Ok(())
        })
    }
}

struct ObservingModule;

impl Module for ObservingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(Refuse::default())
            .change_observer::<QueueingObserver>();
    }
}

fn change(id: &str) -> ChangeEvent {
    ChangeEvent {
        entity: "notes".into(),
        id: id.into(),
        op: ChangeOp::Created,
        old: None,
        new: None,
    }
}

async fn push_observed_joins_the_commit(driver: QueueDriver) {
    let journal = Arc::new(Journal::default());
    let mut builder = TestApp::builder();
    if driver == QueueDriver::Redis {
        // The `redis` driver name resolves the reference store, a store
        // behind the public `QueueStore` trait.
        builder = builder.module(crate::reference_store::NamedStoreModule(
            crate::reference_store::ReferenceStore::default(),
        ));
    }
    let app = crate::servers::database(builder)
        .module(DatabaseModule)
        .module(QueueModule::for_root(options(driver)))
        .module(JobsModule)
        .module(ObservingModule)
        .override_value(Arc::new(FixedClock::new(start())) as Arc<dyn Clock>)
        .override_value(journal.clone())
        .build()
        .await;
    let db = app.get::<Db>().await;
    let queue = app.get::<Queue>().await;

    let uow = db.uow(None, None).await;
    uow.record_change(change("committed"));
    uow.commit().await.expect("commits");
    assert_eq!(queue.pending_count("default").await.unwrap(), 1);

    app.get::<Refuse>().await.0.store(true, Ordering::SeqCst);
    let uow = db.uow(None, None).await;
    uow.record_change(change("refused"));
    uow.commit().await.expect_err("the observer refuses");
    assert_eq!(
        queue.pending_count("default").await.unwrap(),
        1,
        "a failed commit queues nothing"
    );

    queue.run_pending().await.expect("runs");
    assert_eq!(journal.labels(), ["committed"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn push_observed_joins_the_commit_on_the_database_driver() {
    push_observed_joins_the_commit(QueueDriver::Database).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn push_observed_joins_the_commit_on_the_memory_driver() {
    push_observed_joins_the_commit(QueueDriver::Memory).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn push_observed_joins_the_commit_on_a_queue_store_driver() {
    push_observed_joins_the_commit(QueueDriver::Redis).await;
}
