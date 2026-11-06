//! Gmail's push path with a real Pub/Sub server: a notification published to
//! the topic is pushed by Google's Pub/Sub emulator to the app's
//! `/api/mail/inbound/gmail` route, over HTTP, in the envelope Pub/Sub itself
//! writes (not the hand-written one of `mailboxes.rs`), and the route reads the
//! mailbox's history and receives the new messages.
//!
//! What this proves: the push envelope Pub/Sub sends is the one the route
//! decodes, the `?token=` of the push endpoint is checked, and a push that
//! the route answers with 2xx is acknowledged (Pub/Sub does not send it again).
//!
//! What stays a mock: Gmail itself (`users.watch`, the history and the
//! messages) and Microsoft Graph. Neither company offers an emulator, so
//! those calls are answered by the fake HTTP client, as in `mailboxes.rs`; a
//! run against a Microsoft 365 developer tenant and a Google Workspace test
//! domain needs accounts that only the owner can open.
//!
//! This runs in the integration workflow, which starts the emulator with
//! `scripts/integration-services.sh start mail` (variable
//! `NAVREE_TEST_PUBSUB_URL`); it is `#[ignore]`d in the other suites. Run it
//! with `-- --ignored`. A missing variable fails the test when
//! `NAVREE_TEST_REQUIRE_SERVICES=1` and prints `SKIPPED` otherwise.

use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use http::Method;
use navree_http_client::{FakeResponse, HttpClient, HttpClientOptions, SsrfPolicy};
use serde_json::{Value, json};

use crate::mailboxes::{GMAIL, Seen, gmail_watch, message, watches, with_mail};
use crate::support::harness;

const TOPIC: &str = "projects/acme-prod/topics/gmail-inbound";
const SUBSCRIPTION: &str = "projects/acme-prod/subscriptions/gmail-inbound-push";

/// The value of `variable`; `None` after a notice, or a panic when
/// `NAVREE_TEST_REQUIRE_SERVICES=1`.
fn service(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: {variable} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// A client for the emulator on the loopback interface.
fn client() -> HttpClient {
    HttpClient::new(HttpClientOptions {
        retries: 0,
        ssrf: SsrfPolicy {
            block_private: false,
            ..SsrfPolicy::default()
        },
        ..HttpClientOptions::default()
    })
    .expect("a client")
}

/// `PUT`s `body` at `path` on the emulator; it must answer 2xx.
async fn put(api: &str, path: &str, body: Value) {
    let response = client()
        .put(format!("{api}/v1/{path}"))
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|error| panic!("the emulator did not answer {path}: {error}"));
    assert!(
        response.is_success(),
        "PUT {path}: {} {}",
        response.status(),
        response.text()
    );
}

/// The name under which the machine running the tests is reachable from a
/// container (Docker Desktop provides it; the compose file maps it on Linux).
const HOST: &str = "host.docker.internal";

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn pubsub_pushes_a_gmail_notification_to_the_route_and_the_mailbox_is_read() {
    let test = "pubsub_pushes_a_gmail_notification_to_the_route_and_the_mailbox_is_read";
    let Some(pubsub) = service("NAVREE_TEST_PUBSUB_URL", test) else {
        return;
    };
    let seen = Seen::default();
    let h = harness(with_mail(&seen)).await;
    let watch = gmail_watch(&h).await;
    // Gmail's answers (the mock half): the history since the cursor and the
    // two messages it names.
    h.fake.api().on(
        Method::GET,
        &format!("{GMAIL}/users/ada@acme.test/history*"),
        FakeResponse::new(200).json(&json!({
            "history": [
                { "id": "1001", "messagesAdded": [{ "message": { "id": "m1" } }] },
                { "id": "1003", "messagesAdded": [{ "message": { "id": "m2" } }] },
            ],
            "historyId": "1005",
        })),
    );
    for (id, subject) in [("m1", "First"), ("m2", "Second")] {
        h.fake.api().on(
            Method::GET,
            &format!("{GMAIL}/users/ada@acme.test/messages/{id}*"),
            FakeResponse::new(200)
                .json(&json!({ "id": id, "raw": URL_SAFE_NO_PAD.encode(message(subject)) })),
        );
    }

    // The app serves its routes on a real port, on every interface, so the
    // emulator in its container can reach it.
    let router = navree_http::build_router(h.app.ctx())
        .await
        .expect("the router");
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();
    let server = tokio::spawn(async move {
        let _ = navree_http::serve(listener, router).await;
    });

    // The topic, and a push subscription that posts to the route.
    put(&pubsub, TOPIC, json!({})).await;
    put(
        &pubsub,
        SUBSCRIPTION,
        json!({
            "topic": TOPIC,
            "ackDeadlineSeconds": 10,
            "pushConfig": {
                "pushEndpoint": format!("http://{HOST}:{port}/api/mail/inbound/gmail?token=push-secret")
            }
        }),
    )
    .await;

    // Gmail publishes `{emailAddress, historyId}` base64-encoded.
    let data = json!({ "emailAddress": "ada@acme.test", "historyId": "1005" }).to_string();
    let published = client()
        .post(format!("{pubsub}/v1/{TOPIC}:publish"))
        .json(&json!({ "messages": [{ "data": STANDARD.encode(data) }] }))
        .send()
        .await
        .expect("the emulator answers");
    assert!(
        published.is_success(),
        "publish: {} {}",
        published.status(),
        published.text()
    );

    let deadline = Instant::now() + Duration::from_secs(30);
    while seen.lock().unwrap().len() < 2 {
        assert!(
            Instant::now() < deadline,
            "Pub/Sub did not push to the route: {:?}",
            seen.lock()
                .unwrap()
                .iter()
                .map(|m| m.subject.clone())
                .collect::<Vec<_>>()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    server.abort();

    let seen = seen.lock().unwrap().clone();
    let subjects: Vec<_> = seen.iter().map(|m| m.subject.clone().unwrap()).collect();
    assert_eq!(subjects, vec!["First", "Second"]);
    assert_eq!(seen[0].source, "gmail");
    assert_eq!(seen[0].provider_id.as_deref(), Some("m1"));
    assert_eq!(
        watches(&h)
            .await
            .find(watch.id)
            .await
            .unwrap()
            .cursor
            .as_deref(),
        Some("1005"),
        "the cursor moved to the history id of the push"
    );
}
