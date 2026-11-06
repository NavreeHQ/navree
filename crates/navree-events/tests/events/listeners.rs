//! Sync listeners: delivery, order and error handling.

use std::sync::Arc;

use navree_events::{EventBus, EventsError, EventsModule, Listener, ModuleBuilderEventsExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree,
    Resolver,
};

use crate::support::{Journal, UserDeleted, UserRegistered, listener};

listener!(Welcome, "welcome", [UserRegistered => |e| e.email]);

struct UsersModule;

impl Module for UsersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, Welcome>();
    }
}

#[tokio::test]
async fn sync_listener_receives_event() {
    let journal = Arc::new(Journal::default());
    let app = Navree::app()
        .module(EventsModule::for_root())
        .module(UsersModule)
        .override_value(journal.clone())
        .boot()
        .await
        .unwrap();

    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("anu@example.com"))
        .await
        .unwrap();

    assert_eq!(journal.entries(), ["welcome:anu@example.com"]);
}

listener!(First, "first", [UserRegistered => |e| e.email]);
listener!(Second, "second", [UserRegistered => |e| e.email]);
listener!(Third, "third", [UserRegistered => |e| e.email]);

/// Registers `Second` before `First`: registration order, not type order.
struct AccountsModule;

impl Module for AccountsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, Second>()
            .listen::<UserRegistered, First>();
    }
}

/// Imports `AccountsModule`, so its listeners come after the imported ones.
struct BillingModule;

impl Module for BillingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccountsModule).listen::<UserRegistered, Third>();
    }
}

#[tokio::test]
async fn listeners_run_in_registration_order() {
    let journal = Arc::new(Journal::default());
    let app = Navree::app()
        .module(EventsModule::for_root())
        .module(BillingModule)
        .override_value(journal.clone())
        .boot()
        .await
        .unwrap();

    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("anu")).await.unwrap();
    bus.emit(UserRegistered::new("bob")).await.unwrap();

    assert_eq!(
        journal.entries(),
        [
            "second:anu",
            "first:anu",
            "third:anu",
            "second:bob",
            "first:bob",
            "third:bob"
        ]
    );
}

/// Fails every event.
struct Failing;

impl Injectable for Failing {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Failing) })
    }
}

impl Listener<UserRegistered> for Failing {
    fn handle<'a>(
        &'a self,
        _event: &'a UserRegistered,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async { Err("smtp is down".into()) })
    }
}

struct FlakyModule;

impl Module for FlakyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, First>()
            .listen::<UserRegistered, Failing>()
            .listen::<UserRegistered, Second>();
    }
}

#[tokio::test]
async fn error_in_one_listener_does_not_skip_others() {
    let journal = Arc::new(Journal::default());
    let app = Navree::app()
        .module(EventsModule::for_root())
        .module(FlakyModule)
        .override_value(journal.clone())
        .boot()
        .await
        .unwrap();

    let bus = app.ctx().get::<EventBus>().await.unwrap();
    let error = bus.emit(UserRegistered::new("anu")).await.unwrap_err();

    assert_eq!(journal.entries(), ["first:anu", "second:anu"]);
    let message = error.to_string();
    assert!(message.contains("user.registered"), "{message}");
    assert!(message.contains("Failing"), "{message}");
    assert!(message.contains("FlakyModule"), "{message}");
    assert!(message.contains("smtp is down"), "{message}");
    match error.downcast_ref::<EventsError>() {
        Some(EventsError::ListenersFailed { event, failures }) => {
            assert_eq!(*event, "user.registered");
            assert_eq!(failures.len(), 1);
            assert!(failures[0].listener.ends_with("Failing"));
            assert_eq!(failures[0].module, "FlakyModule");
            assert_eq!(failures[0].error.to_string(), "smtp is down");
        }
        other => panic!("expected ListenersFailed, got {other:?}"),
    }
}

/// Emits `UserDeleted` while handling `UserRegistered`.
struct Cascade;

impl Injectable for Cascade {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Cascade) })
    }
}

impl Listener<UserRegistered> for Cascade {
    fn handle<'a>(
        &'a self,
        _event: &'a UserRegistered,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let bus = ctx.get::<EventBus>().await?;
            bus.emit(UserDeleted { id: 1 }).await
        })
    }
}

listener!(Cleanup, "cleanup", [UserDeleted => |e| e.id]);

struct CascadeModule;

impl Module for CascadeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<UserRegistered, Cascade>()
            .listen::<UserRegistered, Welcome>()
            .listen::<UserDeleted, Cleanup>();
    }
}

#[tokio::test]
async fn listener_can_emit_events() {
    let journal = Arc::new(Journal::default());
    let app = Navree::app()
        .module(EventsModule::for_root())
        .module(CascadeModule)
        .override_value(journal.clone())
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();
    let mut deleted = bus.subscribe_json("user.deleted");

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        bus.emit(UserRegistered::new("anu")),
    )
    .await
    .expect("nested emit must not deadlock")
    .unwrap();

    assert_eq!(journal.entries(), ["cleanup:1", "welcome:anu"]);
    assert_eq!(deleted.try_recv().unwrap(), serde_json::json!({ "id": 1 }));
}
