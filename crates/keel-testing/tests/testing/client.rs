//! `TestClient`: requests, JSON bodies, default headers, cookies, XSRF.

use keel_http::{HttpModule, HttpOptions};
use keel_testing::TestApp;
use serde_json::json;

use crate::support::{DemoModule, XSRF_RAW};

#[tokio::test]
async fn client_round_trip_json() {
    let app = TestApp::builder().module(DemoModule).build().await;

    let response = app
        .client()
        .post("/api/demo/echo")
        .json(&json!({ "name": "Acme", "tags": ["a", "b"] }))
        .send()
        .await;

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json_value(),
        json!({ "received": { "name": "Acme", "tags": ["a", "b"] } })
    );
}

#[tokio::test]
async fn cookies_persist_between_requests() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();

    for expected in 0..3 {
        let response = client.get("/api/demo/visits").send().await;
        assert_eq!(response.json_value()["visits"], expected);
    }
    assert_eq!(client.cookie("visits").as_deref(), Some("3"));

    let other = app.client();
    let response = other.get("/api/demo/visits").send().await;
    assert_eq!(
        response.json_value()["visits"],
        0,
        "every client has its own cookie jar"
    );
}

#[tokio::test]
async fn xsrf_helper_sends_header() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();

    let response = client.csrf_cookie().await;
    assert_eq!(response.status(), 204);
    assert_eq!(client.cookie("XSRF-TOKEN").as_deref(), Some(XSRF_RAW));

    let echoed = client.post("/api/demo/headers").xsrf().send().await;
    let headers = echoed.json_value();
    assert_eq!(
        headers["x-xsrf-token"], XSRF_RAW,
        "the raw cookie value, as a browser's JavaScript reads it"
    );
    assert!(
        headers["cookie"]
            .as_str()
            .is_some_and(|c| c.contains(&format!("XSRF-TOKEN={XSRF_RAW}"))),
        "the cookie travels too: {headers}"
    );

    let without = client.post("/api/demo/headers").send().await;
    assert!(
        without.json_value().get("x-xsrf-token").is_none(),
        "only requests that ask for it carry the header"
    );
}

#[tokio::test]
#[should_panic(expected = "no XSRF-TOKEN cookie")]
async fn xsrf_without_cookie_panics_with_a_hint() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let _ = app.client().post("/api/demo/headers").xsrf();
}

#[tokio::test]
async fn default_headers_accept_json_and_the_first_cors_origin() {
    let app = TestApp::builder()
        .module(DemoModule)
        .config(
            "http.cors.allowed_origins",
            json!(["http://localhost:3000", "https://other.example"]),
        )
        .build()
        .await;

    let response = app.client().get("/api/demo/headers").send().await;
    let headers = response.json_value();
    assert_eq!(headers["accept"], "application/json");
    assert_eq!(headers["origin"], "http://localhost:3000");
    assert_eq!(
        response.header("access-control-allow-origin"),
        Some("http://localhost:3000"),
        "the app answers with CORS headers"
    );
}

#[tokio::test]
async fn no_origin_header_without_cors_origins() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let headers = app
        .client()
        .get("/api/demo/headers")
        .send()
        .await
        .json_value();
    assert_eq!(headers["accept"], "application/json");
    assert!(headers.get("origin").is_none(), "{headers}");
}

#[tokio::test]
async fn http_options_from_for_root_are_used() {
    let mut options = HttpOptions::default();
    options.cors.allowed_origins = vec!["https://spa.example".to_owned()];
    let app = TestApp::builder()
        .module(HttpModule::for_root(options))
        .module(DemoModule)
        .build()
        .await;
    let headers = app
        .client()
        .get("/api/demo/headers")
        .send()
        .await
        .json_value();
    assert_eq!(headers["origin"], "https://spa.example");
}

#[tokio::test]
async fn explicit_headers_replace_the_defaults() {
    let app = TestApp::builder()
        .module(DemoModule)
        .config(
            "http.cors.allowed_origins",
            json!(["http://localhost:3000"]),
        )
        .build()
        .await;
    let headers = app
        .client()
        .get("/api/demo/headers")
        .header("accept", "text/html")
        .header("origin", "https://evil.example")
        .header("x-custom", "1")
        .send()
        .await
        .json_value();
    assert_eq!(headers["accept"], "text/html");
    assert_eq!(headers["origin"], "https://evil.example");
    assert_eq!(headers["x-custom"], "1");
}

#[tokio::test]
async fn bearer_sets_the_authorization_header() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let headers = app
        .client()
        .get("/api/demo/headers")
        .bearer("1|secret-token")
        .send()
        .await
        .json_value();
    assert_eq!(headers["authorization"], "Bearer 1|secret-token");
}

#[tokio::test]
async fn form_and_raw_bodies_set_their_content_type() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();

    let form = client
        .post("/api/demo/raw")
        .form(&[("socket_id", "1.2"), ("channel_name", "private-a b")])
        .send()
        .await
        .json_value();
    assert_eq!(form["content_type"], "application/x-www-form-urlencoded");
    assert_eq!(form["body"], "socket_id=1.2&channel_name=private-a+b");

    let raw = client
        .post("/api/demo/raw")
        .body("plain text")
        .header("content-type", "text/plain")
        .send()
        .await
        .json_value();
    assert_eq!(raw["content_type"], "text/plain");
    assert_eq!(raw["body"], "plain text");
}

#[tokio::test]
async fn every_verb_reaches_its_route() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();
    for (request, method) in [
        (client.get("/api/demo/method"), "GET"),
        (client.post("/api/demo/method"), "POST"),
        (client.put("/api/demo/method"), "PUT"),
        (client.patch("/api/demo/method"), "PATCH"),
        (client.delete("/api/demo/method"), "DELETE"),
    ] {
        assert_eq!(request.send().await.json_value()["method"], method);
    }
}

#[tokio::test]
async fn cookies_can_be_set_and_cleared_by_the_test_and_deleted_by_the_server() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();

    client.set_cookie("visits", "41");
    let response = client.get("/api/demo/visits").send().await;
    assert_eq!(response.json_value()["visits"], 41);

    let response = client.post("/api/demo/forget").send().await;
    assert_eq!(response.status(), 204);
    assert_eq!(client.cookie("visits"), None, "Max-Age=0 deletes");

    client.set_cookie("visits", "7");
    client.remove_cookie("visits");
    assert_eq!(client.cookie("visits"), None);

    client.set_cookie("a", "1");
    client.set_cookie("b", "2");
    client.clear_cookies();
    let headers = client.get("/api/demo/headers").send().await.json_value();
    assert!(headers.get("cookie").is_none(), "{headers}");
}

#[tokio::test]
async fn clones_share_the_cookie_jar() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();
    let clone = client.clone();
    client.get("/api/demo/visits").send().await;
    assert_eq!(clone.cookie("visits").as_deref(), Some("1"));
}

#[tokio::test]
async fn requests_come_from_a_loopback_address() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let response = app.client().get("/api/demo/ip").send().await;
    assert_eq!(response.json_value()["ip"], "127.0.0.1");
}

#[tokio::test]
async fn an_explicit_cookie_header_is_sent_with_the_jar_cookies() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();
    client.set_cookie("jar", "1");
    let headers = client
        .get("/api/demo/headers")
        .header("cookie", "explicit=2")
        .send()
        .await
        .json_value();
    assert_eq!(headers["cookie"], "explicit=2; jar=1");
}

#[tokio::test]
async fn query_appends_an_encoded_query_string() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();
    let response = client
        .get("/api/demo/query")
        .query(&[("page", "2"), ("q", "a b&c")])
        .send()
        .await;
    assert_eq!(response.json_value()["query"], "page=2&q=a+b%26c");

    let response = client
        .get("/api/demo/query?sort=name")
        .query(&[("page", "3")])
        .send()
        .await;
    assert_eq!(response.json_value()["query"], "sort=name&page=3");
}

#[tokio::test]
async fn requests_can_run_concurrently_on_spawned_tasks() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();
    let tasks: Vec<_> = (0..8)
        .map(|i| {
            let request = client.post("/api/demo/echo").json(&json!({ "i": i }));
            tokio::spawn(request.send())
        })
        .collect();
    for (i, task) in tasks.into_iter().enumerate() {
        let response = task.await.unwrap();
        assert_eq!(response.json_value()["received"]["i"], i);
    }
}
