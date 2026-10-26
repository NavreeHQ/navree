//! `/api/notifications`: the signed-in user's notifications and read state.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use navis_contracts::{Clock, FixedClock};
use navis_notify::{Channel, Notifier, NotifyOptions};
use navis_testing::{TestApp, principal};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{self, DealWon, TestUser};

/// An app whose clock moves one minute per notification; alice gets three
/// notifications (`first`, `second`, `third`) and bob one.
async fn seeded() -> (TestApp, TestUser, TestUser, Vec<Uuid>, Uuid) {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap(),
    ));
    let app = support::builder(NotifyOptions::default())
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;
    let (alice, bob) = (TestUser::new(), TestUser::new());
    let mut ids = Vec::new();
    for title in ["first", "second", "third"] {
        let id = notifier
            .send(&alice, DealWon::via(&[Channel::Database], title))
            .await
            .unwrap();
        ids.push(id);
        clock.advance(Duration::from_secs(60));
    }
    let bobs = notifier
        .send(&bob, DealWon::via(&[Channel::Database], "bob's"))
        .await
        .unwrap();
    (app, alice, bob, ids, bobs)
}

fn ids_of(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_owned())
        .collect()
}

fn strings(ids: &[Uuid]) -> Vec<String> {
    ids.iter().map(Uuid::to_string).collect()
}

#[tokio::test]
async fn endpoints_list_count_and_mark_read_only_own() {
    let (app, alice, bob, ids, bobs) = seeded().await;
    let as_alice = app.acting_as(principal(alice.id));
    let as_bob = app.acting_as(principal(bob.id));

    // Newest first, only alice's, in the list shape.
    let list = as_alice
        .get("/api/notifications")
        .send()
        .await
        .assert_status(200);
    let body = list.json_value();
    assert_eq!(ids_of(&body), strings(&[ids[2], ids[1], ids[0]]));
    assert_eq!(
        body["data"][2],
        json!({
            "id": ids[0],
            "type": "deal_won",
            "data": DealWon::via(&[], "first").data(),
            "read_at": null,
            "created_at": "2026-10-01T09:00:00Z",
        })
    );
    assert_eq!(
        body["meta"],
        json!({"current_page": 1, "per_page": 15, "total": 3, "last_page": 1})
    );
    as_alice
        .get("/api/notifications/unread-count")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("count", json!(3));

    // Someone else's notification is not found, and stays unread.
    as_alice
        .post(&format!("/api/notifications/{bobs}/read"))
        .send()
        .await
        .assert_error(404, None);
    as_bob
        .get("/api/notifications/unread-count")
        .send()
        .await
        .assert_json_path("count", json!(1));

    // Marking one read.
    as_alice
        .post(&format!("/api/notifications/{}/read", ids[0]))
        .send()
        .await
        .assert_status(204);
    as_alice
        .get("/api/notifications/unread-count")
        .send()
        .await
        .assert_json_path("count", json!(2));
    let unread = as_alice
        .get("/api/notifications?unread=true")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(ids_of(&unread), strings(&[ids[2], ids[1]]));
    let all = as_alice.get("/api/notifications").send().await.json_value();
    assert_eq!(all["data"][2]["read_at"], json!("2026-10-01T09:03:00Z"));

    // Marking all read leaves bob's alone.
    as_alice
        .post("/api/notifications/read-all")
        .send()
        .await
        .assert_status(204);
    as_alice
        .get("/api/notifications/unread-count")
        .send()
        .await
        .assert_json_path("count", json!(0));
    as_bob
        .get("/api/notifications/unread-count")
        .send()
        .await
        .assert_json_path("count", json!(1));
    let bob_list = as_bob.get("/api/notifications").send().await.json_value();
    assert_eq!(ids_of(&bob_list), strings(&[bobs]));
}

#[tokio::test]
async fn marking_read_twice_keeps_the_first_read_time() {
    let (app, alice, _bob, ids, _bobs) = seeded().await;
    let as_alice = app.acting_as(principal(alice.id));
    let path = format!("/api/notifications/{}/read", ids[1]);

    as_alice.post(&path).send().await.assert_status(204);
    let clock = app.get::<dyn Clock>().await;
    let first_read = support::rows(&app).await[1].read_at;
    assert_eq!(first_read, Some(clock.now()));
    as_alice.post(&path).send().await.assert_status(204);

    assert_eq!(support::rows(&app).await[1].read_at, first_read);
}

#[tokio::test]
async fn list_pages_and_validates_the_unread_flag() {
    let (app, alice, _bob, ids, _bobs) = seeded().await;
    let as_alice = app.acting_as(principal(alice.id));

    let page = as_alice
        .get("/api/notifications?per_page=2&page=2")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(ids_of(&page), strings(&[ids[0]]));
    assert_eq!(page["meta"]["last_page"], json!(2));

    as_alice
        .get("/api/notifications?unread=maybe")
        .send()
        .await
        .assert_validation_error("unread")
        .assert_error(400, "unread must be a boolean value");
    let all = as_alice
        .get("/api/notifications?unread=false")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(all["meta"]["total"], json!(3));
}

#[tokio::test]
async fn a_malformed_id_is_not_found() {
    let (app, alice, _bob, _ids, _bobs) = seeded().await;

    app.acting_as(principal(alice.id))
        .post("/api/notifications/not-a-uuid/read")
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn guests_are_unauthenticated() {
    let (app, ..) = seeded().await;
    let guest = app.client();

    for path in ["/api/notifications", "/api/notifications/unread-count"] {
        guest
            .get(path)
            .send()
            .await
            .assert_error(401, "Unauthorized");
    }
    guest
        .post("/api/notifications/read-all")
        .send()
        .await
        .assert_error(401, "Unauthorized");
}

#[tokio::test]
async fn routes_can_be_turned_off() {
    let app = support::builder(NotifyOptions {
        routes: false,
        ..NotifyOptions::default()
    })
    .build()
    .await;

    app.acting_as(principal(Uuid::now_v7()))
        .get("/api/notifications")
        .send()
        .await
        .assert_status(404);
}
