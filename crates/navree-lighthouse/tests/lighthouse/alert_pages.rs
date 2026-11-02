//! The Alerts page and its API: the channels, their kinds, muting them,
//! testing them and the delivery history.

use std::sync::Arc;

use http::Method;
use navree_http_client::{FakeResponse, FakeServer, HttpClient};
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::{Value, json};

use crate::dashboard::sign_in;
use crate::releases::csrf_of;
use crate::support::{builder, flushed, token, user};

const SLACK: &str = "https://hooks.slack.test/services/T000/B000/XXXX";
const WEBHOOK: &str = "https://ops.acme.test/hooks/lighthouse";
const SECRET: &str = "a-webhook-secret-of-at-least-32-bytes!";

fn channels(status: u16) -> (TestAppBuilder, FakeServer) {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        &format!("{SLACK}*"),
        FakeResponse::new(status),
    );
    server.on(Method::POST, &format!("{WEBHOOK}*"), FakeResponse::new(204));
    let builder = builder()
        .config("lighthouse.alerts.slack.webhook_url", SLACK)
        .config(
            "lighthouse.alerts.webhook",
            json!({ "url": WEBHOOK, "secret": SECRET }),
        )
        .override_value(Arc::new(client));
    (builder, server)
}

async fn people(app: &TestApp) {
    user(
        app,
        "viewer@acme.test",
        Some("viewer"),
        &["lighthouse.view"],
    )
    .await;
    user(
        app,
        "admin@acme.test",
        Some("lhadmin"),
        &["lighthouse.view", "lighthouse.configure"],
    )
    .await;
}

/// A second, different issue: another route that fails.
async fn charge(app: &TestApp) {
    app.client()
        .post("/api/demo/charge")
        .send()
        .await
        .assert_status(500);
}

async fn boom(app: &TestApp, n: u32) {
    app.client()
        .get(&format!("/api/demo/boom/{n}"))
        .send()
        .await
        .assert_status(500);
}

fn sent_to(server: &FakeServer, url: &str) -> usize {
    server
        .recorded()
        .iter()
        .filter(|request| request.url().as_str() == url)
        .count()
}

fn channel<'a>(list: &'a Value, name: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("{name} in {list}"))
}

#[tokio::test]
async fn the_page_lists_the_channels_with_their_kinds_and_the_delivery_history() {
    let (builder, _server) = channels(200);
    let app = builder.build().await;
    people(&app).await;
    boom(&app, 1).await;
    flushed(&app).await;
    let client = sign_in(&app, "viewer@acme.test").await;
    let page = client
        .get("/_lighthouse/alerts")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Channels"));
    assert!(page.contains("slack") && page.contains("webhook"));
    assert!(page.contains("New issue") && page.contains("Regression") && page.contains("Spike"));
    assert!(page.contains("Delivery history"));
    assert!(page.contains("invoice inv_1 not found"));
    assert!(page.contains("delivered"));
    assert!(page.contains("Cooldown"), "the throttling that applies");
    // Create and edit are not offered, and the page says why.
    assert!(page.contains("secrets"), "{page}");
    // Read-only for someone who may not configure.
    assert!(!page.contains("data-action=\"mute\""));

    let list: Value = client
        .get("/_lighthouse/api/alerts/channels")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let slack = channel(&list, "slack");
    assert_eq!(slack["kinds"], json!(["new_issue", "regression", "spike"]));
    assert_eq!(slack["muted"], false);
    let history: Value = client
        .get("/_lighthouse/api/alerts/history")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(history["meta"]["total"], 2);
    assert_eq!(history["data"][0]["title"], "invoice inv_1 not found");
    assert_eq!(history["data"][0]["culprit"], "GET /api/demo/boom/{n}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn without_channels_the_page_says_how_to_add_one() {
    let app = builder().build().await;
    people(&app).await;
    let client = sign_in(&app, "viewer@acme.test").await;
    let page = client
        .get("/_lighthouse/alerts")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("No alert channel is configured"), "{page}");
    assert!(page.contains("lighthouse.alerts"));
    assert!(page.contains("No alerts yet"), "the empty history");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn muting_a_channel_stops_its_alerts_but_not_the_others_until_it_is_unmuted() {
    let (builder, server) = channels(200);
    let app = builder.build().await;
    people(&app).await;
    let admin = token(&app, "admin@acme.test").await;
    let client = app.client();

    let muted = client
        .put("/_lighthouse/api/alerts/channels/slack")
        .bearer(&admin)
        .json(&json!({ "muted": true, "minutes": 60 }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(muted["muted"], true);
    assert!(muted["muted_until"].is_string());

    boom(&app, 1).await;
    flushed(&app).await;
    assert_eq!(sent_to(&server, SLACK), 0, "muted");
    assert_eq!(
        sent_to(&server, WEBHOOK),
        1,
        "the other channel still gets it"
    );

    client
        .put("/_lighthouse/api/alerts/channels/slack")
        .bearer(&admin)
        .json(&json!({ "muted": false }))
        .send()
        .await
        .assert_status(200);
    charge(&app).await;
    flushed(&app).await;
    assert_eq!(sent_to(&server, SLACK), 1, "unmuted");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_channel_can_be_given_other_kinds_and_returned_to_its_own() {
    let (builder, server) = channels(200);
    let app = builder.build().await;
    people(&app).await;
    let admin = token(&app, "admin@acme.test").await;
    let client = app.client();
    let changed = client
        .put("/_lighthouse/api/alerts/channels/webhook")
        .bearer(&admin)
        .json(&json!({ "on": ["spike", "failed_job"] }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(changed["kinds"], json!(["spike", "failed_job"]));
    assert_eq!(
        changed["configured_kinds"],
        json!(["new_issue", "regression", "spike"])
    );
    boom(&app, 1).await;
    flushed(&app).await;
    assert_eq!(
        sent_to(&server, WEBHOOK),
        0,
        "a new issue is not among its kinds"
    );
    assert_eq!(sent_to(&server, SLACK), 1);

    let back = client
        .put("/_lighthouse/api/alerts/channels/webhook")
        .bearer(&admin)
        .json(&json!({ "on": null }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(back["kinds"], json!(["new_issue", "regression", "spike"]));
    charge(&app).await;
    flushed(&app).await;
    assert_eq!(sent_to(&server, WEBHOOK), 1);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_test_goes_through_the_one_channel_even_when_it_is_muted_and_hides_secrets_in_errors() {
    let (builder, server) = channels(200);
    let app = builder.build().await;
    people(&app).await;
    let admin = token(&app, "admin@acme.test").await;
    let client = app.client();
    client
        .put("/_lighthouse/api/alerts/channels/slack")
        .bearer(&admin)
        .json(&json!({ "muted": true }))
        .send()
        .await
        .assert_status(200);
    let result = client
        .post("/_lighthouse/api/alerts/channels/slack/test")
        .bearer(&admin)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(result["channel"], "slack");
    assert_eq!(result["ok"], true);
    assert_eq!(sent_to(&server, SLACK), 1);
    assert_eq!(sent_to(&server, WEBHOOK), 0, "only the one asked for");
    app.shutdown().await.unwrap();

    let (builder, _server) = channels(500);
    let app = builder.build().await;
    people(&app).await;
    let admin = token(&app, "admin@acme.test").await;
    let result = app
        .client()
        .post("/_lighthouse/api/alerts/channels/slack/test")
        .bearer(&admin)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(result["ok"], false);
    let error = result["error"].as_str().unwrap();
    assert!(error.contains("500"), "{error}");
    assert!(!error.contains("XXXX"), "{error}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn changing_and_testing_channels_needs_the_configure_ability_and_a_known_channel() {
    let (builder, server) = channels(200);
    let app = builder.build().await;
    people(&app).await;
    let viewer = token(&app, "viewer@acme.test").await;
    let admin = token(&app, "admin@acme.test").await;
    let client = app.client();
    client
        .put("/_lighthouse/api/alerts/channels/slack")
        .json(&json!({ "muted": true }))
        .send()
        .await
        .assert_status(401);
    client
        .put("/_lighthouse/api/alerts/channels/slack")
        .bearer(&viewer)
        .json(&json!({ "muted": true }))
        .send()
        .await
        .assert_status(403);
    client
        .post("/_lighthouse/api/alerts/channels/slack/test")
        .bearer(&viewer)
        .send()
        .await
        .assert_status(403);
    assert_eq!(sent_to(&server, SLACK), 0);
    client
        .put("/_lighthouse/api/alerts/channels/carrier-pigeon")
        .bearer(&admin)
        .json(&json!({ "muted": true }))
        .send()
        .await
        .assert_status(404);
    let bad = client
        .put("/_lighthouse/api/alerts/channels/slack")
        .bearer(&admin)
        .json(&json!({ "on": ["new_issue", "sometimes"] }))
        .send()
        .await;
    assert!(
        matches!(bad.status().as_u16(), 400 | 422),
        "{}",
        bad.status()
    );
    // Through the cookie, the CSRF token is needed.
    let web = sign_in(&app, "admin@acme.test").await;
    let csrf = csrf_of(&web.get("/_lighthouse/alerts").send().await.text());
    web.put("/_lighthouse/api/alerts/channels/slack")
        .json(&json!({ "muted": true }))
        .send()
        .await
        .assert_status(403);
    web.put("/_lighthouse/api/alerts/channels/slack")
        .header("x-csrf-token", &csrf)
        .json(&json!({ "muted": true }))
        .send()
        .await
        .assert_status(200);
    let page = web.get("/_lighthouse/alerts").send().await.text();
    assert!(page.contains("data-action=\"mute\""), "the controls show");
    assert!(page.contains("Muted"), "{page}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_alerts_are_throttled_per_channel_and_audited() {
    use navree_contracts::{AuditEntry, AuditSink};
    use navree_kernel::{BoxError, BoxFuture};
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Audit(Arc<Mutex<Vec<AuditEntry>>>);
    impl AuditSink for Audit {
        fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
            self.0.lock().unwrap().push(entry);
            Box::pin(async { Ok(()) })
        }
    }

    let audit = Audit::default();
    let (builder, server) = channels(200);
    let app = builder
        .override_value::<dyn AuditSink>(Arc::new(audit.clone()))
        .build()
        .await;
    people(&app).await;
    let admin = token(&app, "admin@acme.test").await;
    let client = app.client();
    let mut statuses = Vec::new();
    for _ in 0..50 {
        statuses.push(
            client
                .post("/_lighthouse/api/alerts/channels/slack/test")
                .bearer(&admin)
                .send()
                .await
                .status()
                .as_u16(),
        );
    }
    let sent = sent_to(&server, SLACK);
    assert_eq!(sent, 5, "at most five a minute: {statuses:?}");
    assert_eq!(statuses.iter().filter(|s| **s == 429).count(), 45);
    let audited = audit
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|entry| entry.action == "lighthouse.alert_channel.tested")
        .count();
    assert_eq!(audited, 5, "every test that went out is audited");
    // The limit is per channel.
    client
        .post("/_lighthouse/api/alerts/channels/webhook/test")
        .bearer(&admin)
        .send()
        .await
        .assert_status(200);
    assert_eq!(sent_to(&server, WEBHOOK), 1);
    app.shutdown().await.unwrap();
}
