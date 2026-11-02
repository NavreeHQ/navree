//! Alerts: Slack and signed webhooks (through a fake HTTP client), the
//! cooldown, spikes, your own channels, queued delivery, and
//! `lighthouse:test-alerts`.

use std::sync::{Arc, Mutex};

use http::Method;
use navree_http_client::{FakeResponse, FakeServer, HttpClient};
use navree_kernel::{
    BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_lighthouse::{
    Alert, AlertChannel, AlertKind, IssueFilter, ModuleBuilderLighthouseExt, webhook_signature,
};
use navree_queue::{Queue, QueueModule};
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::{Value, json};

use crate::support::{builder, flushed, lighthouse};

const SLACK: &str = "https://hooks.slack.test/services/T000/B000/XXXX";
const WEBHOOK: &str = "https://ops.acme.test/hooks/lighthouse";
const SECRET: &str = "a-webhook-secret-of-at-least-32-bytes!";

fn with_channels(builder: TestAppBuilder) -> (TestAppBuilder, FakeServer) {
    let (client, server) = HttpClient::fake();
    server.on(Method::POST, &format!("{SLACK}*"), FakeResponse::new(200));
    server.on(Method::POST, &format!("{WEBHOOK}*"), FakeResponse::new(204));
    let builder = builder
        .config("lighthouse.alerts.slack.webhook_url", SLACK)
        .config(
            "lighthouse.alerts.webhook",
            json!({ "url": WEBHOOK, "secret": SECRET }),
        )
        .override_value(Arc::new(client));
    (builder, server)
}

async fn boom(app: &TestApp, n: u32) {
    app.client()
        .get(&format!("/api/demo/boom/{n}"))
        .send()
        .await
        .assert_status(500);
}

#[tokio::test]
async fn a_new_issue_goes_to_slack_and_the_signed_webhook() {
    let (builder, server) = with_channels(builder());
    let app = builder.build().await;
    boom(&app, 1).await;
    let store = flushed(&app).await;
    let issue = &store.issues(&IssueFilter::default()).await.unwrap().data[0].issue;

    let recorded = server.recorded();
    let slack = recorded
        .iter()
        .find(|request| request.url().as_str() == SLACK)
        .expect("a Slack message");
    let message: Value = slack.json().unwrap();
    assert!(
        message["text"]
            .as_str()
            .unwrap()
            .starts_with("New issue: invoice inv_1 not found")
    );
    assert_eq!(message["blocks"][1]["elements"][0]["url"], json!(issue.url));

    let hook = recorded
        .iter()
        .find(|request| request.url().as_str() == WEBHOOK)
        .expect("a webhook");
    let id = hook.header("webhook-id").unwrap();
    let timestamp: i64 = hook.header("webhook-timestamp").unwrap().parse().unwrap();
    assert_eq!(
        hook.header("webhook-signature").unwrap(),
        webhook_signature(SECRET.as_bytes(), id, timestamp, hook.body())
    );
    let body: Value = hook.json().unwrap();
    assert_eq!(body["type"], "lighthouse.new_issue");
    assert_eq!(body["issue"]["id"], json!(issue.id));
    assert_eq!(body["issue"]["culprit"], "GET /api/demo/boom/{n}");
    assert_eq!(body["release"], "r1");
    assert_eq!(body["environment"], "test");

    let alerts = store.alerts_for(Some(issue.id)).await.unwrap();
    assert_eq!(alerts.len(), 2);
    assert!(
        alerts
            .iter()
            .all(|alert| alert.status == "delivered" && alert.attempts == 1)
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_cooldown_holds_back_a_second_alert_of_a_kind() {
    let (builder, server) = with_channels(builder());
    let app = builder.build().await;
    boom(&app, 1).await;
    let store = flushed(&app).await;
    let issue = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .clone();
    server.assert_sent_count(2);
    // A regression within the cooldown is another kind: it is sent.
    store.resolve(issue.id, false, None).await.unwrap();
    boom(&app, 2).await;
    flushed(&app).await;
    server.assert_sent_count(4);
    // The same kind again within the cooldown is not.
    let lighthouse = lighthouse(&app).await;
    let issue = store.issue(issue.id).await.unwrap();
    let alert = Alert {
        id: uuid::Uuid::now_v7(),
        kind: AlertKind::Regression,
        issue,
        environment: "test".into(),
        release: "r1".into(),
        at: chrono::Utc::now(),
        test: false,
    };
    assert_eq!(lighthouse.raise_alert(alert).await.unwrap(), 0);
    server.assert_sent_count(4);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_spike_alerts_once_the_window_holds_enough_occurrences() {
    let (builder, server) = with_channels(
        builder()
            .config(
                "lighthouse.alerts.spike",
                json!({ "occurrences": 3, "minutes": 5 }),
            )
            .config("lighthouse.alerts.webhook.on", "spike"),
    );
    let app = builder.build().await;
    for n in 1..=3 {
        boom(&app, n).await;
    }
    flushed(&app).await;
    let hooks: Vec<Value> = server
        .recorded()
        .iter()
        .filter(|request| request.url().as_str() == WEBHOOK)
        .map(|request| request.json().unwrap())
        .collect();
    assert_eq!(hooks.len(), 1, "only the spike goes to the webhook");
    assert_eq!(hooks[0]["type"], "lighthouse.spike");
    assert_eq!(hooks[0]["issue"]["count"], 3);
    app.shutdown().await.unwrap();
}

/// A channel of the app's own.
#[derive(Default)]
struct Pager(Mutex<Vec<AlertKind>>);

impl Injectable for Pager {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Pager::default()) })
    }
}

impl AlertChannel for Pager {
    fn name(&self) -> &str {
        "pager"
    }

    fn wants(&self, alert: &Alert) -> bool {
        matches!(alert.kind, AlertKind::NewIssue | AlertKind::FailedJob)
    }

    fn send<'a>(&'a self, alert: &'a Alert) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(alert.kind);
            Ok(())
        })
    }
}

struct OpsModule;

impl Module for OpsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.alert_channel::<Pager>();
    }
}

#[tokio::test]
async fn your_own_channels_get_the_kinds_they_want() {
    let app = builder().module(OpsModule).build().await;
    boom(&app, 1).await;
    let store = flushed(&app).await;
    let pager = app.get::<Pager>().await;
    assert_eq!(*pager.0.lock().unwrap(), [AlertKind::NewIssue]);
    let alerts = store.alerts_for(None).await.unwrap();
    assert_eq!(alerts[0].channel, "pager");
    assert_eq!(alerts[0].status, "delivered");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn info_reports_and_ignored_issues_never_alert() {
    let app = builder().module(OpsModule).build().await;
    let lighthouse = lighthouse(&app).await;
    lighthouse.capture(
        navree_lighthouse::Report::message("cache warmed").level(navree_lighthouse::Level::Info),
    );
    let store = flushed(&app).await;
    assert_eq!(
        store
            .issues(&IssueFilter::default())
            .await
            .unwrap()
            .meta
            .total,
        1
    );
    assert!(app.get::<Pager>().await.0.lock().unwrap().is_empty());
    assert!(store.alerts_for(None).await.unwrap().is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn with_a_queue_alerts_are_lighthouse_alert_jobs() {
    let (builder, server) = with_channels(
        builder()
            .config("queue.driver", "memory")
            .module(QueueModule::from_config()),
    );
    let app = builder.build().await;
    boom(&app, 1).await;
    let store = flushed(&app).await;
    server.assert_nothing_sent();
    let alerts = store.alerts_for(None).await.unwrap();
    assert_eq!(alerts.len(), 2);
    assert!(alerts.iter().all(|alert| alert.status == "pending"));
    app.get::<Queue>().await.run_pending().await.unwrap();
    server.assert_sent_count(2);
    let alerts = store.alerts_for(None).await.unwrap();
    assert!(
        alerts.iter().all(|alert| alert.status == "delivered"),
        "{alerts:?}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_job_moving_to_the_failed_jobs_raises_failed_job() {
    let (builder, server) =
        with_channels(builder().config("lighthouse.alerts.webhook.on", "failed_job"));
    let app = builder.build().await;
    let telemetry = navree_telemetry::Telemetry::from_app(app.ctx()).await;
    telemetry.record(navree_telemetry::TelemetryRecord {
        at: chrono::Utc::now(),
        context: navree_telemetry::EventContext {
            job_id: Some("j9".into()),
            ..Default::default()
        },
        event: navree_telemetry::TelemetryEvent::JobFailed(navree_telemetry::JobFailed {
            queue: "default".into(),
            job_type: "invoices.sync".into(),
            job_id: "j9".into(),
            attempt: 5,
            duration: std::time::Duration::from_millis(3),
            error: "provider answered 503".into(),
            will_retry: false,
        }),
    });
    flushed(&app).await;
    let hooks: Vec<Value> = server
        .recorded()
        .iter()
        .filter(|request| request.url().as_str() == WEBHOOK)
        .map(|request| request.json().unwrap())
        .collect();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["type"], "lighthouse.failed_job");
    assert_eq!(hooks[0]["issue"]["kind"], "failed_job");
    assert_eq!(hooks[0]["issue"]["culprit"], "invoices.sync");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn email_alerts_go_through_the_mailer() {
    let app = builder()
        .config(
            "mail",
            json!({ "from": "Lighthouse <ops@acme.test>", "transport": "array" }),
        )
        .config(
            "lighthouse.alerts.email.to",
            "oncall@acme.test,cto@acme.test",
        )
        .module(navree_view::ViewModule::from_config())
        .module(navree_mail::MailModule::from_config())
        .build()
        .await;
    boom(&app, 1).await;
    flushed(&app).await;
    let mailer = app.get::<navree_mail::Mailer>().await;
    let sent = mailer
        .array_transport()
        .expect("the array transport")
        .sent();
    assert_eq!(sent.len(), 1);
    let message = &sent[0];
    assert_eq!(message.to, ["oncall@acme.test", "cto@acme.test"]);
    assert_eq!(
        message.subject,
        "[Lighthouse] New issue: invoice inv_1 not found (GET /api/demo/boom/{n})"
    );
    let text = message.text.as_deref().unwrap();
    assert!(
        text.contains("Open in Lighthouse: https://api.acme.test/_lighthouse/issues/"),
        "{text}"
    );
    assert!(
        message
            .html
            .as_deref()
            .unwrap()
            .contains("Open in Lighthouse")
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_alert_email_is_a_styled_page_with_a_button_and_nothing_external() {
    let app = builder()
        .config(
            "mail",
            json!({ "from": "Lighthouse <ops@acme.test>", "transport": "array" }),
        )
        .config("lighthouse.alerts.email.to", "oncall@acme.test")
        .module(navree_view::ViewModule::from_config())
        .module(navree_mail::MailModule::from_config())
        .build()
        .await;
    boom(&app, 1).await;
    flushed(&app).await;
    let mailer = app.get::<navree_mail::Mailer>().await;
    let sent = mailer.array_transport().unwrap().sent();
    let html = sent[0].html.as_deref().unwrap();
    assert!(html.starts_with("<!doctype html>"), "{html}");
    assert!(html.contains("<meta name=\"viewport\""), "{html}");
    assert!(html.contains("style=\""), "inline styles, as mail clients need");
    assert!(
        html.contains("href=\"https://api.acme.test/_lighthouse/issues/"),
        "the link is a button to the issue: {html}"
    );
    assert!(html.contains("Open in Lighthouse"));
    assert!(html.contains("invoice inv_1 not found"));
    assert!(html.contains("GET /api/demo/boom/{n}"));
    for external in ["<script", "<link", "@import", "src=\"http"] {
        assert!(!html.contains(external), "{external} in {html}");
    }
    // Text for clients that show no HTML stays complete.
    assert!(sent[0].text.as_deref().unwrap().contains("Open in Lighthouse: https://"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_failed_delivery_is_recorded() {
    let (client, server) = HttpClient::fake();
    server.on(Method::POST, &format!("{SLACK}*"), FakeResponse::new(500));
    let app = builder()
        .config("lighthouse.alerts.slack.webhook_url", SLACK)
        .override_value(Arc::new(client))
        .build()
        .await;
    boom(&app, 1).await;
    let store = flushed(&app).await;
    let alerts = store.alerts_for(None).await.unwrap();
    assert_eq!(alerts[0].status, "failed");
    assert!(
        alerts[0].error.as_deref().unwrap().contains("500"),
        "{alerts:?}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_alerts_go_through_every_channel_or_one() {
    let (builder, server) = with_channels(builder());
    let app = builder.build().await;
    let lighthouse = lighthouse(&app).await;
    let results = lighthouse.test_alerts(None).await.unwrap();
    let names: Vec<&str> = results.iter().map(|r| r.channel.as_str()).collect();
    assert_eq!(names, ["slack", "webhook"]);
    assert!(results.iter().all(|r| r.result.is_ok()));
    let body: Value = server
        .recorded()
        .iter()
        .find(|request| request.url().as_str() == WEBHOOK)
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(body["test"], true);
    let one = lighthouse.test_alerts(Some("slack")).await.unwrap();
    assert_eq!(one.len(), 1);
    let error = lighthouse.test_alerts(Some("sms")).await.unwrap_err();
    assert!(error.to_string().contains("slack, webhook"), "{error}");
    app.run_command("lighthouse:test-alerts", &["webhook"])
        .await
        .unwrap();
    assert!(store_alerts_empty(&app).await, "test alerts are not stored");
    app.shutdown().await.unwrap();
}

async fn store_alerts_empty(app: &TestApp) -> bool {
    flushed(app)
        .await
        .alerts_for(None)
        .await
        .unwrap()
        .is_empty()
}
