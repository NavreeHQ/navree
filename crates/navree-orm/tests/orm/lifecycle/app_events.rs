//! Typed application events per model event, through navree-events:
//! `#[model(dispatches(created = OrderPlaced, ..))]`.

use std::sync::{Arc, Mutex};

use navree_events::{EventsModule, Listener, ModuleBuilderEventsExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_orm::{Model, ModelLifecycle, Record, without_events};

use super::models::{Order, OrderPlaced, OrderRemoved, OrderSaving, create_tables};
use crate::support::{app, count_rows};

/// The events the app heard.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<String>>>);

impl Heard {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Hears order events; refuses to let orders named "refused" be saved.
struct OrderListener(Heard);

impl Injectable for OrderListener {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(OrderListener(Heard::clone(&*r.get::<Heard>().await?))) })
    }
}

impl Listener<OrderPlaced> for OrderListener {
    fn handle<'a>(
        &'a self,
        event: &'a OrderPlaced,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        self.0.0.lock().unwrap().push(format!(
            "placed {} for {}",
            event.reference, event.total_cents
        ));
        Box::pin(async { Ok(()) })
    }
}

impl Listener<OrderRemoved> for OrderListener {
    fn handle<'a>(
        &'a self,
        event: &'a OrderRemoved,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        self.0
            .0
            .lock()
            .unwrap()
            .push(format!("removed {}", event.reference));
        Box::pin(async { Ok(()) })
    }
}

impl Listener<OrderSaving> for OrderListener {
    fn handle<'a>(
        &'a self,
        event: &'a OrderSaving,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        let refused = event.reference == "refused";
        self.0
            .0
            .lock()
            .unwrap()
            .push(format!("saving {}", event.reference));
        Box::pin(async move {
            if refused {
                Err("orders named `refused` are refused".into())
            } else {
                Ok(())
            }
        })
    }
}

struct Orders(Heard);

impl Module for Orders {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(EventsModule)
            .value(self.0.clone())
            .listen::<OrderPlaced, OrderListener>()
            .on::<OrderRemoved, OrderListener>()
            .on::<OrderSaving, OrderListener>();
    }
}

fn order(reference: &str, total_cents: i64) -> Order {
    Order {
        reference: reference.into(),
        total_cents,
        ..Order::default()
    }
}

#[tokio::test]
async fn model_events_reach_the_app_as_typed_events_after_the_commit() {
    let heard = Heard::default();
    let (_app, db, _clock) = app(vec![Box::new(Orders(heard.clone()))]).await;
    create_tables(&db).await;

    let uow = db.uow(None, None).await;
    let mut placed = Order::create(&uow, order("A-1", 4200)).await.unwrap();
    assert_eq!(heard.take(), ["saving A-1"], "-ing events at once");
    uow.commit().await.unwrap();
    assert_eq!(
        heard.take(),
        ["placed A-1 for 4200"],
        "-ed events after the commit"
    );

    let uow = db.uow(None, None).await;
    Order::create(&uow, order("B-2", 100)).await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(
        heard.take(),
        ["saving B-2"],
        "never for a rolled-back write"
    );

    placed.delete(&*db).await.unwrap();
    assert_eq!(heard.take(), ["removed A-1"]);
}

#[tokio::test]
async fn a_failing_listener_of_an_ing_event_refuses_the_write() {
    let heard = Heard::default();
    let (_app, db, _clock) = app(vec![Box::new(Orders(heard.clone()))]).await;
    create_tables(&db).await;
    let error = Order::create(&*db, order("refused", 1)).await.unwrap_err();
    let message = error.to_string();
    assert!(
        message.starts_with("a Order hook failed: event `order.saving`:"),
        "{message}"
    );
    assert!(
        message.contains("orders named `refused` are refused"),
        "{message}"
    );
    assert_eq!(count_rows(&db, "orders").await, 0);
}

#[tokio::test]
async fn quiet_writes_and_apps_without_events_dispatch_nothing() {
    let heard = Heard::default();
    let (_app, db, _clock) = app(vec![Box::new(Orders(heard.clone()))]).await;
    create_tables(&db).await;
    Order::create_quietly(&*db, order("C-3", 1)).await.unwrap();
    without_events(async {
        let mut quiet = Record::new(order("D-4", 1));
        quiet.save(&*db).await.unwrap();
    })
    .await;
    assert!(heard.take().is_empty());

    // An app without EventsModule: the model still works.
    let (_plain, db, _clock) = app(Vec::new()).await;
    create_tables(&db).await;
    let created = Order::create(&*db, order("E-5", 1)).await.unwrap();
    assert!(created.exists());
}
