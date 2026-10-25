//! Request-scoped listeners: every emit (and every queued job run) is its own
//! unit of work with a fresh request scope.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use keel_contracts::{Dispatcher, JobContext, JobRegistration};
use keel_events::{EventBus, EventsModule, Listener, ModuleBuilderEventsExt, listener_job_name};
use keel_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, Keel, KernelError, Module, ModuleBuilder,
    ProviderKey, Resolver, Scope,
};

use crate::support::{CaptureDispatcher, Journal, UserRegistered};

/// Counts constructions of the request-scoped listener.
#[derive(Default)]
struct Built(AtomicUsize);

/// A request-scoped listener that records which construction handled the event.
struct PerUnit {
    number: usize,
    journal: Arc<Journal>,
    queued: bool,
}

impl Injectable for PerUnit {
    fn scope() -> Scope {
        Scope::Request
    }

    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Built>(), ProviderKey::of::<Journal>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let built = r.get::<Built>().await?;
            let journal = r.get::<Journal>().await?;
            let queued = r.get::<QueuedFlag>().await?.0;
            Ok(Self {
                number: built.0.fetch_add(1, Ordering::SeqCst) + 1,
                journal,
                queued,
            })
        })
    }
}

impl Listener<UserRegistered> for PerUnit {
    fn handle<'a>(
        &'a self,
        event: &'a UserRegistered,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            self.journal
                .push(format!("unit{}:{}", self.number, event.email));
            Ok(())
        })
    }

    fn queued(&self) -> bool {
        self.queued
    }
}

/// Whether `PerUnit` reports itself as queued.
struct QueuedFlag(bool);

struct ScopedModule {
    queued: bool,
}

impl Module for ScopedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(Built::default())
            .value(QueuedFlag(self.queued))
            .listen::<UserRegistered, PerUnit>();
    }
}

#[tokio::test]
async fn request_scoped_listener_is_built_per_emit() {
    let journal = Arc::new(Journal::default());
    let app = Keel::app()
        .module(EventsModule::for_root())
        .module(ScopedModule { queued: false })
        .override_value(journal.clone())
        .boot()
        .await
        .unwrap();

    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("ada")).await.unwrap();
    bus.emit(UserRegistered::new("bob")).await.unwrap();

    assert_eq!(journal.entries(), ["unit1:ada", "unit2:bob"]);
}

#[tokio::test]
async fn queued_request_scoped_listener_runs_in_its_own_scope() {
    let journal = Arc::new(Journal::default());
    let capture = Arc::new(CaptureDispatcher::default());
    let app = Keel::app()
        .module(EventsModule::for_root())
        .module(ScopedModule { queued: true })
        .override_value(journal.clone())
        .override_value(capture.clone() as Arc<dyn Dispatcher>)
        .boot()
        .await
        .unwrap();

    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("ada")).await.unwrap();

    let envelope = capture.envelopes().pop().expect("a dispatched job");
    let job_name = listener_job_name::<UserRegistered, PerUnit>();
    assert_eq!(envelope.job_type, job_name);
    let registration = app
        .ctx()
        .contributions::<JobRegistration>()
        .into_iter()
        .find(|c| c.value.name == job_name)
        .unwrap();
    let job = JobContext {
        app: app.ctx().clone(),
        job_id: "job-1".to_owned(),
        attempt: 1,
    };
    (registration.value.run)(envelope.payload, job)
        .await
        .unwrap();

    // Unit 1 decided to queue; unit 2 (the job run) handled the event.
    assert_eq!(journal.entries(), ["unit2:ada"]);
}
