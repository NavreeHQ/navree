//! Queued listeners: dispatched as jobs through `dyn Dispatcher`.

use std::sync::Arc;

use navree_contracts::{Dispatcher, JobContext, JobRegistration};
use navree_events::{EventBus, EventsModule, ModuleBuilderEventsExt};
use navree_kernel::{Navree, Module, ModuleBuilder};
use serde_json::json;

use crate::support::{CaptureDispatcher, Journal, UserRegistered, listener};

listener!(Audit, "audit", [UserRegistered => |e| e.email]);
listener!(SendWelcomeMail, "mail", queued, [UserRegistered => |e| e.email]);

struct MailingModule;

impl Module for MailingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, SendWelcomeMail>()
            .listen::<UserRegistered, Audit>();
    }
}

#[tokio::test]
async fn queued_listener_dispatches_job_and_job_runs_listener() {
    let journal = Arc::new(Journal::default());
    let capture = Arc::new(CaptureDispatcher::default());
    let app = Navree::app()
        .module(EventsModule::for_root())
        .module(MailingModule)
        .override_value(journal.clone())
        .override_value(capture.clone() as Arc<dyn Dispatcher>)
        .boot()
        .await
        .unwrap();

    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("ada")).await.unwrap();

    // The queued listener did not run inline; the sync one did.
    assert_eq!(journal.entries(), ["audit:ada"]);
    let envelopes = capture.envelopes();
    assert_eq!(envelopes.len(), 1);
    let expected_job = format!(
        "events.listener.user.registered.{}",
        std::any::type_name::<SendWelcomeMail>()
    );
    assert_eq!(envelopes[0].job_type, expected_job);
    assert_eq!(envelopes[0].payload, json!({ "email": "ada" }));
    assert_eq!(envelopes[0].delay_secs, None);

    // The worker side: the contributed job registration runs the listener.
    let registration = app
        .ctx()
        .contributions::<JobRegistration>()
        .into_iter()
        .find(|c| c.value.name == expected_job)
        .expect("a job registration for the queued listener");
    let job = JobContext {
        app: app.ctx().clone(),
        job_id: "job-1".to_owned(),
        attempt: 1,
    };
    (registration.value.run)(envelopes[0].payload.clone(), job)
        .await
        .unwrap();

    assert_eq!(journal.entries(), ["audit:ada", "mail:ada"]);
}

#[tokio::test]
async fn queued_listener_runs_inline_without_dispatcher() {
    let journal = Arc::new(Journal::default());
    let app = Navree::app()
        .module(EventsModule::for_root())
        .module(MailingModule)
        .override_value(journal.clone())
        .boot()
        .await
        .unwrap();

    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("ada")).await.unwrap();

    assert_eq!(journal.entries(), ["mail:ada", "audit:ada"]);
}
