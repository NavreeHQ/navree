//! The overview's additions: sparklines in the figure tiles, time labels,
//! the release line, the top issues and the live panel; and that no page
//! needs an inline style or script.

use chrono::{TimeDelta, Utc};
use navree_lighthouse::ReleaseReport;
use navree_testing::TestApp;
use serde_json::Value;

use crate::dashboard::sign_in;
use crate::support::{builder, flushed, lighthouse, token, user};

async fn boom(app: &TestApp) {
    app.client()
        .get("/api/demo/boom/7")
        .send()
        .await
        .assert_status(500);
}

#[tokio::test]
async fn the_tiles_carry_sparklines_the_chart_time_labels_and_the_deploy() {
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

    let page = client.get("/_lighthouse?period=24h").send().await.text();
    assert_eq!(
        page.matches("class=\"tile-spark\"").count(),
        4,
        "requests, error rate, p95 and failed jobs have a history; open issues do not"
    );
    assert!(page.contains("id=\"spark-requests\" d=\"M0 "), "{page}");
    assert!(page.contains("<div class=\"axis\">"), "{page}");
    assert!(page.contains(">Now</span>"), "{page}");
    assert!(page.contains("class=\"release-line\""), "{page}");
    assert!(
        page.contains("Release <span class=\"mono\">r2</span> deployed"),
        "{page}"
    );
    assert!(page.contains("Times are UTC."));
    assert!(page.contains("p50 <span id=\"fig-p50\">"));
    assert!(page.contains("new in the last 24h"));

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
    // A deploy older than the period is not on the 1 hour chart's line.
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_deploy_outside_the_period_draws_no_line() {
    let app = builder().config("lighthouse.release", "").build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    lighthouse(&app)
        .await
        .report_release(ReleaseReport::new("old").deployed_at(Utc::now() - TimeDelta::days(3)))
        .await
        .unwrap();
    let client = sign_in(&app, "ops@acme.test").await;
    let day = client.get("/_lighthouse?period=24h").send().await.text();
    assert!(!day.contains("class=\"release-line\""));
    let week = client.get("/_lighthouse?period=7d").send().await.text();
    assert!(week.contains("class=\"release-line\""));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_live_panel_and_the_top_issues_show_what_just_happened() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    boom(&app).await;
    flushed(&app).await;
    let client = sign_in(&app, "ops@acme.test").await;
    let page = client.get("/_lighthouse").send().await.text();
    assert!(page.contains("aria-label=\"New occurrences\""));
    assert!(page.contains("id=\"feed\""));
    assert!(page.contains("id=\"stream-text\">Streaming"));
    assert!(page.contains("class=\"live-title\"") || page.contains("live-title"));
    assert!(page.contains("invoice inv_7 not found"));
    assert!(
        page.contains("GET /api/demo/boom/{n}")
            || page.contains("GET &#x2f;api&#x2f;demo&#x2f;boom")
    );
    // The top issues are a table of the open ones, with their kind.
    assert!(
        page.contains("<span class=\"kind error\"><i></i>Exception</span>"),
        "{page}"
    );
    // The script polls the overview's JSON for more.
    let script = client
        .get("/_lighthouse/assets/lighthouse.js")
        .send()
        .await
        .text();
    assert!(script.contains("/api/overview?period="));
    assert!(script.contains("setInterval(refresh, 10000)"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_empty_overview_says_so() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    let client = sign_in(&app, "ops@acme.test").await;
    let page = client.get("/_lighthouse").send().await.text();
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
    ] {
        let html = client.get(path).send().await.assert_status(200).text();
        assert!(!html.contains(" style=\""), "{path} has a style attribute");
        assert!(!html.contains("<style"), "{path} has a style element");
        for script in html.split("<script").skip(1) {
            assert!(
                script.trim_start().starts_with("src="),
                "{path} has inline script"
            );
        }
        assert!(!html.contains(" onclick="), "{path} has an inline handler");
    }
    app.shutdown().await.unwrap();
}
