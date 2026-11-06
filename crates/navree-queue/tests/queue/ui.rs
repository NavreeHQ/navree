//! The fragment of an operation (`GET /operations/{id}/progress`) a Navree UI page follows:
//! the same visibility rules as the JSON route, and the states of an operation as a
//! region that stops asking once the operation is finished.
#![cfg(feature = "ui")]

use std::sync::Arc;

use navree_db::Db;
use navree_queue::{Operation, OperationRecord, QueueDriver, operation_topic};
use navree_realtime::Live;
use navree_testing::principal;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Broadcasts, Harness, harness_using, one_worker};

async fn app() -> Harness {
    harness_using(one_worker(QueueDriver::Memory), |builder| {
        builder.config("http.per_ip_rate_limit", Value::Null)
    })
    .await
}

async fn start(h: &Harness, operation: Operation) -> OperationRecord {
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let record = h.operations.start(&uow, operation).await.unwrap();
    uow.commit().await.unwrap();
    record
}

fn path(id: Uuid) -> String {
    format!("/operations/{id}/progress")
}

// A queued export shows progress, then a link to its file, and then asks no more.
#[tokio::test]
async fn a_queued_export_shows_progress_then_a_link() {
    let owner = Uuid::now_v7();
    let h = app().await;
    let op = start(&h, Operation::new("deals.export").owner(owner).total(10)).await;
    let get = || async {
        h.app
            .acting_as(principal(owner))
            .get(&path(op.id))
            .send()
            .await
            .assert_status(200)
            .text()
    };

    let waiting = get().await;
    assert!(
        waiting.contains(&format!(r#"id="operation-{}""#, op.id)),
        "{waiting}"
    );
    assert!(waiting.contains(r#"data-state="pending""#), "{waiting}");
    assert!(waiting.contains("data-ui-poll"), "{waiting}");

    h.operations
        .progress(op.id, 4, 10, "Writing rows")
        .await
        .unwrap();
    let running = get().await;
    assert!(running.contains(r#"data-state="running""#), "{running}");
    assert!(running.contains("Writing rows"), "{running}");
    assert!(running.contains("4 of 10"), "{running}");
    assert!(running.contains("data-ui-poll"), "{running}");

    h.operations
        .complete(op.id, json!({ "url": "/exports/deals.csv" }))
        .await
        .unwrap();
    let done = get().await;
    assert!(done.contains(r#"data-state="succeeded""#), "{done}");
    assert!(done.contains(r#"href="/exports/deals.csv""#), "{done}");
    assert!(
        !done.contains("data-ui-poll"),
        "a finished operation is not asked again: {done}"
    );
}

// A failed operation says why; the page of someone who may not see it is a 404, as for
// the JSON route.
#[tokio::test]
async fn a_failed_operation_says_why_and_a_stranger_gets_a_404() {
    let owner = Uuid::now_v7();
    let stranger = Uuid::now_v7();
    let h = app().await;
    let op = start(&h, Operation::new("deals.export").owner(owner)).await;
    h.operations
        .fail(op.id, "The file could not be made.")
        .await
        .unwrap();
    let page = h
        .app
        .acting_as(principal(owner))
        .get(&path(op.id))
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("The file could not be made."), "{page}");
    assert!(!page.contains("data-ui-poll"), "{page}");

    h.app
        .acting_as(principal(stranger))
        .get(&path(op.id))
        .send()
        .await
        .assert_error(404, None);
    h.app
        .acting_as(principal(owner))
        .get(&path(Uuid::now_v7()))
        .send()
        .await
        .assert_error(404, None);
    h.app
        .client()
        .get(&path(op.id))
        .send()
        .await
        .assert_status(401);
}

// With the live hub in the app (here a publisher that goes nowhere; the harness records what
// the queue broadcasts), a change of the operation is told to its owner's pages on the topic
// a page follows it by.
#[tokio::test]
async fn a_change_is_told_to_the_owners_live_pages() {
    let owner = Uuid::now_v7();
    let h = harness_using(one_worker(QueueDriver::Memory), |builder| {
        builder
            .config("http.per_ip_rate_limit", Value::Null)
            .override_value::<Live>(Arc::new(Live::new(Arc::new(Broadcasts::default()))))
    })
    .await;
    let op = start(&h, Operation::new("deals.export").owner(owner)).await;
    h.operations.progress(op.id, 1, 2, "Halfway").await.unwrap();
    let topic = operation_topic(op.id);
    let nudged = h.broadcasts.all().into_iter().any(|(channels, event, _)| {
        event == navree_realtime::NUDGE_EVENT
            && channels.iter().any(|channel| channel.contains(&topic))
    });
    assert!(nudged, "{:?}", h.broadcasts.all());
}
