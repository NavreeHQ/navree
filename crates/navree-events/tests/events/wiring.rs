//! Wiring: one listener for several events, and boot-time checks.

use std::sync::Arc;

use navree_events::{EventBus, EventsModule, ModuleBuilderEventsExt};
use navree_kernel::{KernelError, Module, ModuleBuilder, Navree};

use crate::support::{Journal, UserDeleted, UserRegistered, listener};

listener!(Auditor, "audit", [
    UserRegistered => |e| e.email,
    UserDeleted => |e| e.id,
]);

/// `listen` registers the provider once; `on` subscribes it to more events.
struct AuditModule;

impl Module for AuditModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, Auditor>()
            .on::<UserDeleted, Auditor>();
    }
}

#[tokio::test]
async fn one_listener_handles_several_events() {
    let journal = Arc::new(Journal::default());
    let app = Navree::app()
        .module(EventsModule::for_root())
        .module(AuditModule)
        .override_value(journal.clone())
        .boot()
        .await
        .unwrap();

    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("ada")).await.unwrap();
    bus.emit(UserDeleted { id: 7 }).await.unwrap();

    assert_eq!(journal.entries(), ["audit:ada", "audit:7"]);
}

listener!(Ghost, "ghost", [UserRegistered => |e| e.email]);

/// Subscribes `Ghost` without registering or importing it.
struct HauntedModule;

impl Module for HauntedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.on::<UserRegistered, Ghost>();
    }
}

#[tokio::test]
async fn listener_that_is_not_visible_fails_boot() {
    let errors = Navree::app()
        .module(EventsModule::for_root())
        .module(HauntedModule)
        .override_value(Arc::new(Journal::default()))
        .boot()
        .await
        .expect_err("boot must fail");

    assert!(
        errors.0.iter().any(|e| matches!(
            e,
            KernelError::MissingProvider { module: "HauntedModule", requested, .. }
                if requested.ends_with("Ghost")
        )),
        "{errors}"
    );
}

/// Subscribes the same listener to the same event twice.
struct EchoModule;

impl Module for EchoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, Auditor>()
            .on::<UserRegistered, Auditor>();
    }
}

#[tokio::test]
async fn subscribing_a_listener_twice_to_one_event_fails_boot() {
    let errors = Navree::app()
        .module(EventsModule::for_root())
        .module(EchoModule)
        .override_value(Arc::new(Journal::default()))
        .boot()
        .await
        .expect_err("boot must fail");

    assert!(
        errors.0.iter().any(|e| matches!(
            e,
            KernelError::DuplicateProvider { module: "EchoModule", key }
                if key.contains("Auditor") && key.contains("UserRegistered")
        )),
        "{errors}"
    );
}
