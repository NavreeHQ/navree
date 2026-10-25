//! The session cookie: persistence across requests and cookie attributes.

use navree_crypto::Encrypter;
use navree_session::SessionOptions;
use navree_testing::cookie::SameSite;
use navree_testing::cookie::time::Duration;
use serde_json::json;

use crate::support::{self, COOKIE, XSRF};

#[tokio::test]
async fn session_persists_across_requests() {
    let app = support::app().await;
    let client = app.client();

    // No session cookie yet, so this first write is not CSRF-checked.
    let created = client
        .post("/api/probe/put")
        .json(&json!({ "key": "cart", "value": {"items": 3} }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let id = created["id"].as_str().unwrap().to_owned();
    assert!(client.cookie(COOKIE).is_some(), "the session cookie is set");

    let (same_id, value) = support::get(&client, "cart").await;
    assert_eq!(same_id, id, "the second request continues the session");
    assert_eq!(value, json!({"items": 3}));

    let (other_id, nothing) = support::get(&app.client(), "cart").await;
    assert_ne!(other_id, id, "another client has its own session");
    assert_eq!(nothing, json!(null));
}

#[tokio::test]
async fn cookie_is_encrypted_httponly_samesite() {
    let app = support::app().await;
    let client = app.client();

    let response = client
        .post("/api/probe/put")
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_status(200);
    let id = response.json_value()["id"].as_str().unwrap().to_owned();
    let cookie = response
        .set_cookie(COOKIE)
        .expect("the session cookie is set");

    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    assert_eq!(cookie.secure(), Some(true), "secure by default");
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.max_age(), Some(Duration::minutes(120)));
    assert_eq!(cookie.domain(), None);

    assert_eq!(id.len(), 40);
    assert!(
        !cookie.value().contains(&id),
        "the cookie does not carry the session id in clear"
    );
    let encrypter = app.get::<Encrypter>().await;
    assert_eq!(
        encrypter.decrypt_string(cookie.value()).unwrap(),
        id,
        "the cookie is the session id encrypted with the app key"
    );
}

#[tokio::test]
async fn cookie_attributes_follow_the_options() {
    let app = support::app_with(SessionOptions {
        cookie: "crm_session".into(),
        lifetime_minutes: 30,
        secure: false,
        same_site: navree_session::SameSite::Strict,
        domain: Some(".example.com".into()),
        ..SessionOptions::default()
    })
    .await;
    let response = app
        .client()
        .post("/api/probe/put")
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_status(200);
    assert!(response.set_cookie(COOKIE).is_none());
    let cookie = response.set_cookie("crm_session").expect("renamed cookie");
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Strict));
    assert_ne!(cookie.secure(), Some(true));
    assert_eq!(cookie.max_age(), Some(Duration::minutes(30)));
    assert_eq!(cookie.domain(), Some("example.com"));
}

#[tokio::test]
async fn a_tampered_cookie_starts_a_new_session() {
    let app = support::app().await;
    let client = app.client();
    let id = support::put(&client, "k", json!("v")).await;

    let mut forged = client.cookie(COOKIE).unwrap();
    let last = forged.pop().unwrap();
    forged.push(if last == 'A' { 'B' } else { 'A' });
    let other = app.client();
    other.set_cookie(COOKIE, &forged);
    let (other_id, value) = support::get(&other, "k").await;
    assert_ne!(other_id, id);
    assert_eq!(value, json!(null));

    let plain = app.client();
    plain.set_cookie(COOKIE, &id);
    let (plain_id, value) = support::get(&plain, "k").await;
    assert_ne!(plain_id, id, "an unencrypted id is not accepted");
    assert_eq!(value, json!(null));
}

#[tokio::test]
async fn a_request_that_does_not_touch_the_session_sets_no_cookie() {
    let app = support::app().await;
    let client = app.client();
    let response = client
        .get("/api/probe/untouched")
        .send()
        .await
        .assert_status(200);
    assert!(response.set_cookie(COOKIE).is_none());
    assert!(response.set_cookie(XSRF).is_none());
    let response = client
        .get("/api/probe/get/missing")
        .send()
        .await
        .assert_status(200);
    assert!(
        response.set_cookie(COOKIE).is_none(),
        "reading does not create a session"
    );
}

#[tokio::test]
async fn a_stale_duplicate_cookie_does_not_hide_the_session() {
    // E.g. a host-only cookie left over after `session.domain` changed.
    let app = support::app().await;
    let client = app.client();
    let id = support::put(&client, "k", json!("v")).await;
    let body = client
        .get("/api/probe/get/k")
        .header("cookie", "navree_session=v1.stale.cookie.value")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["id"], json!(id));
    assert_eq!(body["value"], json!("v"));
}
