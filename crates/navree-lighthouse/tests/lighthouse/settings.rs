//! Settings: what can change from the dashboard, who may change it, how a
//! change is checked and audited, and that secrets never show.

use std::sync::{Arc, Mutex};

use navree_contracts::{AuditEntry, AuditSink};
use navree_kernel::{BoxError, BoxFuture};
use navree_lighthouse::IssueFilter;
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::dashboard::sign_in;
use crate::releases::csrf_of;
use crate::support::{builder, flushed, lighthouse, token, user};

const SLACK: &str = "https://hooks.slack.test/services/T000/B000/SECRETPATH";
const SECRET: &str = "a-webhook-secret-of-at-least-32-bytes!";

/// An audit sink that keeps what it is given.
#[derive(Clone, Default)]
struct Audit(Arc<Mutex<Vec<AuditEntry>>>);

impl AuditSink for Audit {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
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

fn row<'a>(settings: &'a Value, key: &str) -> &'a Value {
    settings["settings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap_or_else(|| panic!("{key} in {settings}"))
}

#[tokio::test]
async fn the_settings_list_what_can_change_with_the_value_in_use_and_the_configured_one() {
    let app = builder()
        .config("lighthouse.thresholds.slow_query_ms", 450)
        .build()
        .await;
    people(&app).await;
    let viewer = token(&app, "viewer@acme.test").await;
    let settings: Value = app
        .client()
        .get("/_lighthouse/api/settings")
        .bearer(&viewer)
        .send()
        .await
        .assert_status(200)
        .json_value();
    let slow = row(&settings, "thresholds.slow_query_ms");
    assert_eq!(slow["current"], 450);
    assert_eq!(slow["configured"], 450);
    assert_eq!(slow["changed"], false);
    for key in [
        "thresholds.slow_request_ms",
        "sample.requests",
        "retention_days",
        "alerts.cooldown_minutes",
        "alerts.spike.minutes",
    ] {
        row(&settings, key);
    }
    // What is read-only says so and why.
    let fixed = settings["read_only"].as_array().unwrap();
    assert!(
        fixed.iter().any(|group| group["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["key"] == "lighthouse.path")),
        "{settings}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn secrets_never_show_on_the_settings_the_alerts_or_the_api() {
    let app = builder()
        .config("lighthouse.alerts.slack.webhook_url", SLACK)
        .config(
            "lighthouse.alerts.webhook",
            json!({ "url": "https://ops.acme.test/hooks/lh?token=URLTOKEN", "secret": SECRET }),
        )
        .config("lighthouse.alerts.email.to", "oncall@acme.test")
        .build()
        .await;
    people(&app).await;
    let client = sign_in(&app, "admin@acme.test").await;
    let mut seen = String::new();
    for path in [
        "/_lighthouse/settings",
        "/_lighthouse/alerts",
        "/_lighthouse/api/settings",
        "/_lighthouse/api/alerts/channels",
        "/_lighthouse/api/alerts/history",
    ] {
        let response = client.get(path).send().await;
        assert_eq!(response.status().as_u16(), 200, "{path}");
        seen.push_str(&response.text());
    }
    for secret in ["SECRETPATH", SECRET, "URLTOKEN", "T000/B000"] {
        assert!(!seen.contains(secret), "{secret} is shown");
    }
    // What is configured does show, as a fact and not as a value.
    assert!(seen.contains("ops.acme.test"));
    assert!(seen.contains("oncall@acme.test"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_change_needs_the_configure_ability() {
    let app = builder().build().await;
    people(&app).await;
    let viewer = token(&app, "viewer@acme.test").await;
    let client = app.client();
    let body = json!({ "values": { "retention_days": 30 } });
    client
        .put("/_lighthouse/api/settings")
        .json(&body)
        .send()
        .await
        .assert_status(401);
    client
        .put("/_lighthouse/api/settings")
        .bearer(&viewer)
        .json(&body)
        .send()
        .await
        .assert_status(403);
    assert_eq!(
        lighthouse(&app).await.options().retention_days,
        14,
        "nothing changed"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_change_through_the_cookie_needs_the_csrf_token() {
    let app = builder().build().await;
    people(&app).await;
    let client = sign_in(&app, "admin@acme.test").await;
    let page = client.get("/_lighthouse/settings").send().await.text();
    let csrf = csrf_of(&page);
    let body = json!({ "values": { "retention_days": 30 } });
    client
        .put("/_lighthouse/api/settings")
        .json(&body)
        .send()
        .await
        .assert_status(403);
    client
        .put("/_lighthouse/api/settings")
        .header("x-csrf-token", "forged")
        .json(&body)
        .send()
        .await
        .assert_status(403);
    let saved = client
        .put("/_lighthouse/api/settings")
        .header("x-csrf-token", &csrf)
        .json(&body)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(row(&saved, "retention_days")["current"], 30);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_change_applies_at_once_is_audited_and_can_be_undone() {
    let audit = Audit::default();
    let app = builder()
        .override_value::<dyn AuditSink>(Arc::new(audit.clone()))
        .build()
        .await;
    let admin = user(
        &app,
        "admin@acme.test",
        Some("lhadmin"),
        &["lighthouse.view", "lighthouse.configure"],
    )
    .await;
    let token = token(&app, "admin@acme.test").await;
    let client = app.client();

    // With the default threshold the demo route (it takes 30 ms) is not slow.
    client.get("/api/demo/slow").send().await.assert_status(200);
    let store = flushed(&app).await;
    assert!(
        store
            .issues(&IssueFilter::default())
            .await
            .unwrap()
            .data
            .is_empty()
    );

    let saved = client
        .put("/_lighthouse/api/settings")
        .bearer(&token)
        .json(&json!({ "values": { "thresholds.slow_request_ms": 10, "sample.requests": 1 } }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let slow = row(&saved, "thresholds.slow_request_ms");
    assert_eq!(slow["current"], 10);
    assert_eq!(slow["changed"], true);
    assert_eq!(slow["updated_by"], admin.to_string());
    assert_eq!(row(&saved, "sample.requests")["current"], 1.0);

    // The new threshold is in use, without a restart.
    client.get("/api/demo/slow").send().await.assert_status(200);
    let store = flushed(&app).await;
    let kinds: Vec<String> = store
        .issues(&IssueFilter::default())
        .await
        .unwrap()
        .data
        .iter()
        .map(|s| s.issue.kind.as_str().to_owned())
        .collect();
    assert!(kinds.contains(&"slow_request".to_owned()), "{kinds:?}");

    // Audited: who, what, from what to what.
    let entries = audit.0.lock().unwrap().clone();
    let entry = entries
        .iter()
        .find(|e| e.action == "lighthouse.settings.updated")
        .expect("an audit entry");
    assert_eq!(entry.actor_id, Some(admin));
    assert_eq!(
        entry.new_values.as_ref().unwrap()["thresholds.slow_request_ms"],
        10
    );
    assert_eq!(
        entry.old_values.as_ref().unwrap()["thresholds.slow_request_ms"],
        60_000
    );

    // Another process reads what was stored.
    lighthouse(&app).await.reload_settings().await.unwrap();

    // null returns a setting to what the configuration says.
    let reset = client
        .put("/_lighthouse/api/settings")
        .bearer(&token)
        .json(&json!({ "values": { "thresholds.slow_request_ms": null } }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let slow = row(&reset, "thresholds.slow_request_ms");
    assert_eq!(slow["current"], 60_000, "the suite's configured value");
    assert_eq!(slow["changed"], false);
    assert_eq!(
        row(&reset, "sample.requests")["changed"],
        true,
        "the other stays"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_bad_change_is_refused_whole_and_names_the_setting() {
    let app = builder().build().await;
    people(&app).await;
    let token = token(&app, "admin@acme.test").await;
    let client = app.client();
    for (values, names) in [
        (json!({ "retention_days": 0 }), "Keep data for"),
        (json!({ "sample.requests": 2 }), "Ordinary requests"),
        (json!({ "alerts.spike.minutes": "five" }), "Spike: window"),
        (json!({ "path": "/elsewhere" }), "path"),
        (
            json!({ "retention_days": 20, "sample.jobs": -1 }),
            "Ordinary jobs",
        ),
    ] {
        let response = client
            .put("/_lighthouse/api/settings")
            .bearer(&token)
            .json(&json!({ "values": values }))
            .send()
            .await;
        assert!(
            matches!(response.status().as_u16(), 400 | 422),
            "{values}: {}",
            response.status()
        );
        assert!(
            response.text().contains(names),
            "{values}: {}",
            response.text()
        );
    }
    let settings: Value = client
        .get("/_lighthouse/api/settings")
        .bearer(&token)
        .send()
        .await
        .json_value();
    assert_eq!(
        row(&settings, "retention_days")["current"],
        14,
        "a refused change changes nothing, not even its valid part"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_settings_page_is_read_only_without_the_ability_and_says_what_is_fixed() {
    let app = builder().build().await;
    people(&app).await;
    let viewer = sign_in(&app, "viewer@acme.test").await;
    let page = viewer
        .get("/_lighthouse/settings")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Slow query"));
    assert!(
        page.contains("lighthouse.configure"),
        "says what is missing"
    );
    assert!(!page.contains("data-action=\"save-settings\""));
    assert!(page.contains("disabled"));
    // What is set in code or configuration, and where.
    assert!(page.contains("Set in code or configuration"));
    assert!(page.contains("Mounted at"), "the path");
    assert!(page.contains("APP_LIGHTHOUSE__"), "how to set it");

    let admin = sign_in(&app, "admin@acme.test").await;
    let page = admin
        .get("/_lighthouse/settings")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("data-action=\"save-settings\""));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn settings_that_remove_the_alert_throttle_are_refused() {
    let app = builder().build().await;
    people(&app).await;
    let token = token(&app, "admin@acme.test").await;
    let client = app.client();
    for (values, names) in [
        (json!({ "alerts.cooldown_minutes": 0 }), "Cooldown"),
        (
            json!({ "alerts.spike.occurrences": 1 }),
            "Spike: occurrences",
        ),
    ] {
        let response = client
            .put("/_lighthouse/api/settings")
            .bearer(&token)
            .json(&json!({ "values": values }))
            .send()
            .await;
        assert_eq!(response.status().as_u16(), 400, "{values}");
        assert!(response.text().contains(names), "{}", response.text());
    }
    app.shutdown().await.unwrap();
}
