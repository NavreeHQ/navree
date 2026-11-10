//! The overview's additions: a line in the figure tiles, the chart with the
//! deploys on it, the top issues and the live panel; and that no page needs
//! an inline style or script.

use chrono::{TimeDelta, Utc};
use navree_lighthouse::ReleaseReport;
use navree_testing::TestApp;
use serde_json::Value;

use crate::dashboard::{html, sign_in};
use crate::support::{builder, flushed, lighthouse, token, user};

async fn boom(app: &TestApp) {
    app.client()
        .get("/api/demo/boom/7")
        .send()
        .await
        .assert_status(500);
}

#[tokio::test]
async fn the_tiles_carry_the_figures_the_chart_and_the_deploy() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    for _ in 0..3 {
        boom(&app).await;
    }
    app.client()
        .get("/api/demo/ok")
        .send()
        .await
        .assert_status(200);
    flushed(&app).await;
    lighthouse(&app)
        .await
        .report_release(ReleaseReport::new("r2").deployed_at(Utc::now() - TimeDelta::minutes(30)))
        .await
        .unwrap();
    let client = sign_in(&app, "ops@acme.test").await;

    let page = html(&client, "/_lighthouse?period=24h").await.text();
    for figure in [
        "Requests per minute",
        "Error rate",
        "p95 latency",
        "Failed jobs",
        "Open issues",
        "Pool wait",
        "Sampled out",
    ] {
        assert!(page.contains(figure), "{figure}: {page}");
    }
    assert!(page.contains("Requests and exceptions"), "{page}");
    assert!(page.contains("Times are UTC."));
    assert!(page.contains("new in 24h"));
    // The deploy is on the chart, with its name.
    assert!(page.contains("Release r2"), "{page}");

    // The series are real: a bucket per timeline point, with the traffic in the last.
    let token = token(&app, "ops@acme.test").await;
    for (period, buckets) in [("1h", 60), ("24h", 24), ("7d", 168)] {
        let overview: Value = app
            .client()
            .get(&format!("/_lighthouse/api/overview?period={period}"))
            .bearer(&token)
            .send()
            .await
            .assert_status(200)
            .json_value();
        let rpm = overview["series"]["requests_per_minute"]
            .as_array()
            .unwrap();
        assert_eq!(rpm.len(), buckets, "{period}");
        assert_eq!(
            overview["series"]["p95_ms"].as_array().unwrap().len(),
            buckets
        );
        assert_eq!(
            overview["series"]["error_rate"].as_array().unwrap().len(),
            buckets
        );
        assert_eq!(
            overview["series"]["failed_jobs"].as_array().unwrap().len(),
            buckets
        );
        assert!(
            rpm.iter().any(|v| v.as_f64().unwrap() > 0.0),
            "{period}: {rpm:?}"
        );
        assert_eq!(overview["new_issues"], 1);
        assert!(overview["p50_ms"].is_number());
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_deploy_outside_the_period_draws_no_line() {
    let app = builder().config("lighthouse.release", "").build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    // Something recorded, so the chart has a timeline to put a deploy on.
    boom(&app).await;
    flushed(&app).await;
    lighthouse(&app)
        .await
        .report_release(ReleaseReport::new("old").deployed_at(Utc::now() - TimeDelta::days(3)))
        .await
        .unwrap();
    let client = sign_in(&app, "ops@acme.test").await;
    let day = html(&client, "/_lighthouse?period=24h").await.text();
    assert!(!day.contains("Release old"), "{day}");
    let week = html(&client, "/_lighthouse?period=7d").await.text();
    assert!(week.contains("Release old"), "{week}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_live_panel_and_the_top_issues_show_what_just_happened() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    boom(&app).await;
    flushed(&app).await;
    let client = sign_in(&app, "ops@acme.test").await;
    let page = html(&client, "/_lighthouse").await.text();
    assert!(page.contains("New occurrences"));
    assert!(page.contains("Open issues"));
    assert!(page.contains("invoice inv_7 not found"));
    assert!(page.contains("GET /api/demo/boom/{n}"), "{page}");
    // The top issues are a table of the open ones, with their kind.
    assert!(page.contains("Exception"), "{page}");
    // The figures and the feed ask the server again on a timer.
    assert!(page.contains("data-ui-poll=\"10000\""), "{page}");
    assert!(
        page.contains("data-ui-src=\"/_lighthouse/overview/live?period=24h\""),
        "{page}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_empty_overview_says_so() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    let client = sign_in(&app, "ops@acme.test").await;
    let page = html(&client, "/_lighthouse").await.text();
    assert!(page.contains("Nothing yet. New occurrences appear here"));
    assert!(page.contains("No open issues."));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn no_page_needs_an_inline_style_or_script() {
    let app = builder().build().await;
    user(
        &app,
        "ops@acme.test",
        Some("ops"),
        &["lighthouse.view", "lighthouse.configure"],
    )
    .await;
    boom(&app).await;
    flushed(&app).await;
    let client = sign_in(&app, "ops@acme.test").await;
    for path in [
        "/_lighthouse",
        "/_lighthouse?period=7d",
        "/_lighthouse/alerts",
        "/_lighthouse/releases",
        "/_lighthouse/settings",
        "/_lighthouse/issues",
        "/_lighthouse/requests",
        "/_lighthouse/queries",
        "/_lighthouse/jobs",
        "/_lighthouse/outgoing",
    ] {
        let page = html(&client, path).await.assert_status(200);
        let body = page.text();
        assert!(!body.contains(" style=\""), "{path} has a style attribute");
        assert!(!body.contains("<style"), "{path} has a style element");
        for script in body.split("<script").skip(1) {
            let tag = script.split('>').next().unwrap_or_default();
            assert!(tag.contains(" src="), "{path} has inline script: {tag}");
        }
        assert!(!body.contains(" onclick="), "{path} has an inline handler");
    }
    app.shutdown().await.unwrap();
}
