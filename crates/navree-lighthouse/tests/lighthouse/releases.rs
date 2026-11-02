//! Releases: the ones processes note at boot, the ones a deploy reports,
//! the issues each introduced and the ones that came back in it, and the
//! release the header shows.

use chrono::{TimeDelta, Utc};
use navree_lighthouse::{IssueFilter, ReleaseReport};
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::dashboard::sign_in;
use crate::support::{builder, flushed, lighthouse, token, user};

async fn boom(app: &TestApp, n: u32) {
    app.client()
        .get(&format!("/api/demo/boom/{n}"))
        .send()
        .await
        .assert_status(500);
}

/// The CSRF token a signed-in page carries.
pub fn csrf_of(html: &str) -> String {
    let start = html.find("data-csrf=\"").expect("a signed-in page") + 11;
    html[start..start + html[start..].find('"').unwrap()].to_owned()
}

async fn operators(app: &TestApp) {
    user(
        app,
        "viewer@acme.test",
        Some("viewer"),
        &["lighthouse.view"],
    )
    .await;
    user(
        app,
        "deploy@acme.test",
        Some("deploy"),
        &["lighthouse.view", "lighthouse.configure"],
    )
    .await;
}

#[tokio::test]
async fn a_process_notes_the_release_it_runs_when_it_boots() {
    let app = builder().build().await;
    operators(&app).await;
    let viewer = token(&app, "viewer@acme.test").await;
    let releases: Value = app
        .client()
        .get("/_lighthouse/api/releases")
        .bearer(&viewer)
        .send()
        .await
        .assert_status(200)
        .json_value();
    let releases = releases.as_array().unwrap();
    assert_eq!(releases.len(), 1, "{releases:?}");
    assert_eq!(releases[0]["name"], "r1");
    assert_eq!(releases[0]["environment"], "test");
    assert_eq!(releases[0]["source"], "process");
    assert_eq!(releases[0]["current"], true);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn no_release_set_means_no_release_listed_and_no_made_up_one_in_the_header() {
    let app = builder().config("lighthouse.release", "").build().await;
    operators(&app).await;
    let client = sign_in(&app, "viewer@acme.test").await;
    let page = client.get("/_lighthouse").send().await.text();
    assert!(!page.contains("unknown"), "{page}");
    assert!(page.contains("test"), "the environment still shows");
    let releases = client
        .get("/_lighthouse/releases")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(
        releases.contains("No release has been reported"),
        "{releases}"
    );
    assert!(releases.contains("POST"), "it says how to report one");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_deploy_reports_the_real_deploy_time_and_the_newest_deploy_is_the_one_in_the_header() {
    let app = builder().build().await;
    operators(&app).await;
    let deploy = token(&app, "deploy@acme.test").await;
    let at = Utc::now() - TimeDelta::minutes(18);

    // The process booted a moment ago with the label r1; the deploy knows
    // better when r1 went out.
    let summary = app
        .client()
        .post("/_lighthouse/api/releases")
        .bearer(&deploy)
        .json(&json!({
            "release": "r1",
            "commit": "3f2a9c1",
            "url": "https://git.acme.test/crm/commit/3f2a9c1",
            "notes": "Stage validation",
            "deployed_at": at,
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(summary["name"], "r1");
    assert_eq!(summary["environment"], "test", "the app's own environment");
    assert_eq!(summary["source"], "api");
    assert_eq!(summary["commit"], "3f2a9c1");
    assert_eq!(summary["introduced"], 0);

    let client = sign_in(&app, "viewer@acme.test").await;
    let page = client.get("/_lighthouse").send().await.text();
    assert!(page.contains("test · r1"), "{page}");
    assert!(page.contains("deployed 18 min ago"), "{page}");
    let list = client
        .get("/_lighthouse/releases")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(list.contains("Stage validation"));
    assert!(list.contains("3f2a9c1"));
    assert!(list.contains("git.acme.test"), "the link to the changes");

    // A later deploy is the current release; the earlier one stays listed.
    let again = app
        .client()
        .post("/_lighthouse/api/releases")
        .bearer(&deploy)
        .json(&json!({ "release": "r2", "notes": "Hotfix" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(again["current"], true);
    let page = client.get("/_lighthouse").send().await.text();
    assert!(page.contains("test · r2"), "{page}");
    // Reporting r1 again updates it; what it did not say stays.
    let r1 = app
        .client()
        .post("/_lighthouse/api/releases")
        .bearer(&deploy)
        .json(&json!({ "release": "r1", "notes": "Stage validation, v2" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(r1["notes"], "Stage validation, v2");
    assert_eq!(r1["commit"], "3f2a9c1");
    assert_eq!(r1["current"], false);
    let all: Value = app
        .client()
        .get("/_lighthouse/api/releases")
        .bearer(&deploy)
        .send()
        .await
        .json_value();
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert_eq!(all[0]["name"], "r2", "newest deploy first");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn reporting_a_release_needs_the_configure_ability_and_a_valid_report() {
    let app = builder().build().await;
    operators(&app).await;
    let viewer = token(&app, "viewer@acme.test").await;
    let deploy = token(&app, "deploy@acme.test").await;
    let client = app.client();
    client
        .post("/_lighthouse/api/releases")
        .json(&json!({ "release": "r9" }))
        .send()
        .await
        .assert_status(401);
    client
        .post("/_lighthouse/api/releases")
        .bearer(&viewer)
        .json(&json!({ "release": "r9" }))
        .send()
        .await
        .assert_status(403);
    for bad in [
        json!({ "release": "" }),
        json!({ "release": "r9", "url": "javascript:alert(1)" }),
        json!({ "release": "r9", "commit": "has spaces" }),
        json!({ "release": "r9", "surprise": true }),
    ] {
        let response = client
            .post("/_lighthouse/api/releases")
            .bearer(&deploy)
            .json(&bad)
            .send()
            .await;
        assert!(
            matches!(response.status().as_u16(), 400 | 422),
            "{bad}: {}",
            response.status()
        );
    }
    let all: Value = client
        .get("/_lighthouse/api/releases")
        .bearer(&deploy)
        .send()
        .await
        .json_value();
    assert_eq!(all.as_array().unwrap().len(), 1, "nothing bad was kept");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn issues_introduced_and_regressed_are_counted_per_release() {
    let app = builder().build().await;
    operators(&app).await;
    boom(&app, 1).await;
    boom(&app, 1).await;
    let store = flushed(&app).await;
    let issue = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .clone();
    store.resolve(issue.id, false, None).await.unwrap();
    boom(&app, 1).await;
    flushed(&app).await;

    let deploy = token(&app, "deploy@acme.test").await;
    let releases: Value = app
        .client()
        .get("/_lighthouse/api/releases")
        .bearer(&deploy)
        .send()
        .await
        .json_value();
    let r1 = &releases[0];
    assert_eq!(r1["name"], "r1");
    assert_eq!(r1["introduced"], 1);
    assert_eq!(r1["regressed"], 1, "{r1}");

    let client = sign_in(&app, "viewer@acme.test").await;
    let page = client
        .get("/_lighthouse/releases?release=r1")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Introduced in r1"), "{page}");
    assert!(page.contains("Came back in r1"), "{page}");
    assert!(page.contains("invoice inv_1 not found"), "{page}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn code_can_report_its_release_too() {
    let app = builder().build().await;
    let lighthouse = lighthouse(&app).await;
    let summary = lighthouse
        .report_release(
            ReleaseReport::new("r2")
                .commit("abc")
                .environment("staging"),
        )
        .await
        .unwrap();
    assert_eq!(summary.environment, "staging");
    assert!(
        !summary.current,
        "another environment is not this app's release"
    );
    assert!(
        lighthouse
            .report_release(ReleaseReport::new(" "))
            .await
            .is_err()
    );
    app.shutdown().await.unwrap();
}
