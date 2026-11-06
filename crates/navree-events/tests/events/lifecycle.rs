//! The bus and the app's lifetime.

use std::sync::Arc;

use navree_events::{EventBus, EventsError, EventsModule};
use navree_kernel::Navree;

use crate::support::UserRegistered;

/// Any value registered in the app: while the app's container is alive, it
/// holds a reference to it.
struct Probe;

#[tokio::test]
async fn app_is_released_after_graceful_shutdown() {
    let probe = Arc::new(Probe);
    let app = Navree::app()
        .module(EventsModule::for_root())
        .override_value(probe.clone())
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("anu")).await.unwrap();
    drop(bus);

    app.shutdown().await.unwrap();

    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "the bus must not keep the app's container alive after shutdown"
    );
}

#[tokio::test]
async fn emit_after_shutdown_fails() {
    let app = Navree::app()
        .module(EventsModule::for_root())
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();

    app.shutdown().await.unwrap();
    let error = bus.emit(UserRegistered::new("anu")).await.unwrap_err();

    assert!(
        matches!(
            error.downcast_ref::<EventsError>(),
            Some(EventsError::ShutDown)
        ),
        "{error}"
    );
}

#[tokio::test]
async fn app_is_released_when_dropped_without_shutdown() {
    let probe = Arc::new(Probe);
    let app = Navree::app()
        .module(EventsModule::for_root())
        .override_value(probe.clone())
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();
    bus.emit(UserRegistered::new("anu")).await.unwrap();
    drop(bus);

    drop(app);

    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "the bus must not keep a dropped app's container alive"
    );
}

#[tokio::test]
async fn emit_after_the_app_is_dropped_fails() {
    let app = Navree::app()
        .module(EventsModule::for_root())
        .boot()
        .await
        .unwrap();
    let bus = app.ctx().get::<EventBus>().await.unwrap();

    drop(app);
    let error = bus.emit(UserRegistered::new("anu")).await.unwrap_err();

    assert!(
        matches!(
            error.downcast_ref::<EventsError>(),
            Some(EventsError::ShutDown)
        ),
        "{error}"
    );
}
