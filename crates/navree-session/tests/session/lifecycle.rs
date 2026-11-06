//! Session lifecycle: regenerate, invalidate, flush, remove and expiry.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::{Clock, FixedClock, SessionStore};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{self, COOKIE, XSRF};

/// An app whose sessions see `clock`.
async fn app_with_clock(clock: Arc<FixedClock>) -> TestApp {
    support::base()
        .override_value::<dyn Clock>(clock)
        .module(SessionModule::for_root(SessionOptions::default()))
        .build()
        .await
}

fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        "2026-10-25T09:00:00Z".parse().expect("a valid timestamp"),
    ))
}

#[tokio::test]
async fn regenerate_changes_id_keeps_data() {
    let app = support::app().await;
    let store = app.get::<dyn SessionStore>().await;
    let client = app.client();
    let id = support::put(&client, "cart", json!([1, 2])).await;
    let old_token = client.cookie(XSRF).unwrap();

    let response = client
        .post("/api/probe/regenerate")
        .xsrf()
        .send()
        .await
        .assert_status(200);
    let body = response.json_value();
    assert_eq!(body["old"], json!(id));
    let new_id = body["new"].as_str().unwrap().to_owned();
    assert_ne!(new_id, id);
    assert_eq!(new_id.len(), 40);

    let (current, value) = support::get(&client, "cart").await;
    assert_eq!(current, new_id, "the client follows the new id");
    assert_eq!(value, json!([1, 2]), "the data is kept");

    assert!(
        store.load(&id).await.unwrap().is_none(),
        "the old record is deleted"
    );
    assert!(store.load(&new_id).await.unwrap().is_some());

    // The CSRF token rotates with the id.
    let new_token = response.set_cookie(XSRF).expect("a new XSRF-TOKEN cookie");
    assert_ne!(new_token.value(), old_token);
    client
        .post("/api/probe/write")
        .header("x-xsrf-token", old_token.as_str())
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    client
        .post("/api/probe/write")
        .xsrf()
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn old_session_cookie_is_dead_after_regenerate() {
    let app = support::app().await;
    let client = app.client();
    support::put(&client, "user", json!("anu")).await;
    let old_cookie = client.cookie(COOKIE).unwrap();
    client
        .post("/api/probe/regenerate")
        .xsrf()
        .send()
        .await
        .assert_status(200);

    let attacker = app.client();
    attacker.set_cookie(COOKIE, &old_cookie);
    let (_, value) = support::get(&attacker, "user").await;
    assert_eq!(
        value,
        json!(null),
        "a fixated or stolen old id is worthless"
    );
}

#[tokio::test]
async fn invalidate_clears() {
    let app = support::app().await;
    let store = app.get::<dyn SessionStore>().await;
    let client = app.client();
    let id = support::put(&client, "user", json!("anu")).await;
    let old_token = client.cookie(XSRF).unwrap();

    let response = client
        .post("/api/probe/invalidate")
        .xsrf()
        .send()
        .await
        .assert_status(200);
    let new_id = response.json_value()["new"].as_str().unwrap().to_owned();
    assert_ne!(new_id, id);

    let (current, value) = support::get(&client, "user").await;
    assert_eq!(current, new_id);
    assert_eq!(value, json!(null), "every value is gone");
    assert!(
        store.load(&id).await.unwrap().is_none(),
        "the old record is deleted"
    );

    // A fresh token comes with the fresh session, so the app keeps working.
    let new_token = response.set_cookie(XSRF).expect("a new XSRF-TOKEN cookie");
    assert_ne!(new_token.value(), old_token);
    client
        .post("/api/probe/write")
        .xsrf()
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn flush_removes_values_but_keeps_the_id() {
    let app = support::app().await;
    let client = app.client();
    let id = support::put(&client, "a", json!(1)).await;
    support::put(&client, "b", json!(2)).await;

    let body = client
        .post("/api/probe/flush")
        .xsrf()
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["id"], json!(id));
    assert_eq!(support::get(&client, "a").await, (id.clone(), Value::Null));
    assert_eq!(support::get(&client, "b").await, (id, Value::Null));
    client
        .post("/api/probe/write")
        .xsrf()
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn remove_returns_the_value() {
    let app = support::app().await;
    let client = app.client();
    support::put(&client, "a", json!({"x": 1})).await;

    let body = client
        .post("/api/probe/remove/a")
        .xsrf()
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["removed"], json!({"x": 1}));
    assert_eq!(support::get(&client, "a").await.1, Value::Null);

    let body = client
        .post("/api/probe/remove/a")
        .xsrf()
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["removed"], Value::Null);
}

#[tokio::test]
async fn expired_session_is_new() {
    let clock = clock();
    let app = app_with_clock(clock.clone()).await;
    let store = app.get::<dyn SessionStore>().await;
    let client = app.client();
    let id = support::put(&client, "user", json!("anu")).await;

    clock.advance(Duration::from_secs(121 * 60));
    let (current, value) = support::get(&client, "user").await;
    assert_ne!(current, id, "an expired session is replaced");
    assert_eq!(value, Value::Null);
    assert!(
        store.load(&id).await.unwrap().is_none(),
        "the expired record is deleted"
    );
}

#[tokio::test]
async fn activity_extends_the_session() {
    let clock = clock();
    let app = app_with_clock(clock.clone()).await;
    let client = app.client();
    let id = support::put(&client, "user", json!("anu")).await;

    // Within a minute of the last write nothing is written again.
    clock.advance(Duration::from_secs(30));
    let response = client.get("/api/probe/get/user").send().await;
    assert!(
        response.set_cookie(COOKIE).is_none(),
        "no write within a minute"
    );

    // Later requests extend the session (record and cookie).
    clock.advance(Duration::from_secs(100 * 60));
    let response = client.get("/api/probe/get/user").send().await;
    assert!(
        response.set_cookie(COOKIE).is_some(),
        "the cookie is renewed"
    );
    assert!(response.set_cookie(XSRF).is_some(), "with the XSRF cookie");

    clock.advance(Duration::from_secs(100 * 60));
    let (current, value) = support::get(&client, "user").await;
    assert_eq!(current, id, "200 minutes after login, still active");
    assert_eq!(value, json!("anu"));
}

// "remember me" keeps the session, and its cookies, for the
// lifetime the session set itself, past `session.lifetime_minutes`.
#[tokio::test]
async fn a_remembered_session_outlives_the_configured_lifetime() {
    let clock = clock();
    let app = app_with_clock(clock.clone()).await;
    let store = app.get::<dyn SessionStore>().await;
    let client = app.client();
    let id = support::put(&client, "user", json!("anu")).await;
    let response = client
        .post("/api/probe/remember")
        .xsrf()
        .send()
        .await
        .assert_status(200);
    let days = i64::try_from(support::REMEMBER_DAYS).unwrap();
    let cookie = response.set_cookie(COOKIE).expect("the cookie is renewed");
    assert_eq!(
        cookie.max_age(),
        Some(navree_testing::cookie::time::Duration::days(days)),
        "the session cookie lasts as long as the session"
    );
    let xsrf = response.set_cookie(XSRF).expect("with the XSRF cookie");
    assert_eq!(
        xsrf.max_age(),
        Some(navree_testing::cookie::time::Duration::days(days))
    );
    let record = store.load(&id).await.unwrap().expect("the record");
    assert_eq!(
        record.expires_at,
        Some(clock.now() + chrono::TimeDelta::days(days)),
        "the record lives as long"
    );

    // Five idle hours: past the configured two, within the thirty days.
    clock.advance(Duration::from_secs(5 * 3600));
    let (current, value) = support::get(&client, "user").await;
    assert_eq!(current, id, "the remembered session is still there");
    assert_eq!(value, json!("anu"));

    // Thirty-one idle days: gone.
    clock.advance(Duration::from_secs(31 * 24 * 3600));
    let (current, value) = support::get(&client, "user").await;
    assert_ne!(current, id);
    assert_eq!(value, Value::Null);
}

// A session that never set a lifetime keeps the configured one.
#[tokio::test]
async fn a_session_without_its_own_lifetime_keeps_the_configured_one() {
    let clock = clock();
    let app = app_with_clock(clock.clone()).await;
    let client = app.client();
    client.csrf_cookie().await;
    let response = client
        .post("/api/probe/put")
        .xsrf()
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.set_cookie(COOKIE).unwrap().max_age(),
        Some(navree_testing::cookie::time::Duration::minutes(120))
    );
}

#[tokio::test]
async fn changes_before_an_error_response_are_kept() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    for _ in 0..2 {
        client
            .post("/api/probe/fail")
            .xsrf()
            .send()
            .await
            .assert_status(400);
    }
    assert_eq!(support::get(&client, "attempts").await.1, json!(2));
}
