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

/// Answers a report: the status and the body.
async fn report(app: &TestApp, deploy: &str, body: Value) -> (u16, Value) {
    let response = app
        .client()
        .post("/_lighthouse/api/releases")
        .bearer(deploy)
        .json(&body)
        .send()
        .await;
    let status = response.status().as_u16();
    (
        status,
        serde_json::from_str(&response.text()).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_report_is_stored_or_refused_with_a_reason_on_every_store() {
    let app = builder().build().await;
    operators(&app).await;
    let deploy = token(&app, "deploy@acme.test").await;
    let long = |n: usize| format!("https://x.example/{}", "a".repeat(n - 18));

    // Accepted everywhere, at the longest and the widest.
    for body in [
        json!({ "release": "ok-url", "url": long(255) }),
        json!({ "release": "ok-old", "deployed_at": "1971-01-01T00:00:00Z" }),
        json!({ "release": "ok-notes", "notes": "one\ntwo\tthree" }),
        json!({ "release": "\u{1F600}".repeat(200) }),
        json!({ "release": "ok-env", "environment": "\u{1F600}".repeat(100) }),
    ] {
        let (status, answer) = report(&app, &deploy, body.clone()).await;
        assert_eq!(status, 200, "{body}: {answer}");
    }
    // Refused everywhere, with what is wrong.
    for (body, says) in [
        (json!({ "release": "a", "url": long(256) }), "url"),
        (json!({ "release": "a", "url": long(500) }), "url"),
        (json!({ "release": "a", "commit": "ab\u{0}c" }), "commit"),
        (json!({ "release": "a", "notes": "ab\u{0}c" }), "notes"),
        (
            json!({ "release": "a", "environment": "ab\u{0}c" }),
            "environment",
        ),
        (json!({ "release": "a\u{0}" }), "release"),
        (
            json!({ "release": "a", "deployed_at": "2099-01-01T00:00:00Z" }),
            "deployed_at",
        ),
        (
            json!({ "release": "a", "deployed_at": "9999-12-31T23:59:59Z" }),
            "deployed_at",
        ),
        (
            json!({ "release": "a", "deployed_at": "1900-01-01T00:00:00Z" }),
            "deployed_at",
        ),
        (
            json!({ "release": "a", "deployed_at": "1970-01-01T00:00:00Z" }),
            "deployed_at",
        ),
    ] {
        let (status, answer) = report(&app, &deploy, body.clone()).await;
        assert_eq!(status, 400, "{body}: {answer}");
        assert!(answer.to_string().contains(says), "{body}: {answer}");
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn names_differing_only_in_case_are_two_releases_on_every_store() {
    let app = builder().build().await;
    operators(&app).await;
    let deploy = token(&app, "deploy@acme.test").await;
    let (status, first) = report(&app, &deploy, json!({ "release": "caseR", "notes": "a" })).await;
    assert_eq!(status, 200, "{first}");
    let (status, second) = report(&app, &deploy, json!({ "release": "CASER", "notes": "b" })).await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(second["name"], "CASER");
    assert_eq!(second["notes"], "b");
    let store = flushed(&app).await;
    let names: Vec<String> = store
        .releases(10)
        .await
        .unwrap()
        .into_iter()
        .map(|release| release.name)
        .collect();
    assert!(names.contains(&"caseR".to_owned()), "{names:?}");
    assert!(names.contains(&"CASER".to_owned()), "{names:?}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_report_past_the_newest_two_hundred_is_stored_and_answered() {
    let app = builder().build().await;
    operators(&app).await;
    let deploy = token(&app, "deploy@acme.test").await;
    for n in 0..201 {
        let (status, _) = report(&app, &deploy, json!({ "release": format!("n{n}") })).await;
        assert_eq!(status, 200);
    }
    let (status, answer) = report(
        &app,
        &deploy,
        json!({ "release": "ancient", "deployed_at": "1999-01-01T00:00:00Z" }),
    )
    .await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer["name"], "ancient");
    assert_eq!(answer["current"], false);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_process_back_on_an_older_release_makes_it_the_current_one() {
    let (url, _guard) = crate::servers::scratch();
    let boot = |release: &'static str| {
        let url = url.clone();
        async move {
            builder()
                .config("database.url", url)
                .config("database.migrate_on_boot", true)
                .config("database.max_connections", 4)
                .config("lighthouse.release", release)
                .build()
                .await
        }
    };
    let name_of_current = |app: TestApp| async move {
        let store = flushed(&app).await;
        let current = store.current_release().await.unwrap().map(|r| r.name);
        let listed: Vec<String> = store
            .releases(10)
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.current)
            .map(|r| r.name)
            .collect();
        app.shutdown().await.unwrap();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(current.as_deref(), Some(listed[0].as_str()));
        listed[0].clone()
    };
    assert_eq!(name_of_current(boot("r1").await).await, "r1");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    assert_eq!(name_of_current(boot("r2").await).await, "r2");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    // The rollback.
    assert_eq!(name_of_current(boot("r1").await).await, "r1");
}

#[tokio::test]
async fn a_new_report_is_current_after_an_old_one_with_a_date() {
    let app = builder().build().await;
    operators(&app).await;
    let deploy = token(&app, "deploy@acme.test").await;
    let soon = Utc::now() + TimeDelta::hours(1);
    let (status, answer) = report(
        &app,
        &deploy,
        json!({ "release": "soon", "deployed_at": soon }),
    )
    .await;
    assert_eq!(status, 200, "a clock a little ahead is allowed: {answer}");
    let (status, answer) = report(
        &app,
        &deploy,
        json!({ "release": "soon", "deployed_at": Utc::now() }),
    )
    .await;
    assert_eq!(status, 200, "{answer}");
    let (_, answer) = report(&app, &deploy, json!({ "release": "next" })).await;
    assert_eq!(answer["current"], true, "{answer}");
    app.shutdown().await.unwrap();
}

/// Statements the app runs while `f` does.
async fn statements_during<F: std::future::Future>(app: &TestApp, f: F) -> Vec<String> {
    use std::sync::{Arc, Mutex};
    let db = app.get::<navree_db::Db>().await;
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    db.on_query(move |query| log.lock().unwrap().push(query.sql.clone()));
    f.await;
    seen.lock().unwrap().clone()
}

#[tokio::test]
async fn only_the_releases_read_the_release_counts_and_they_never_fetch_the_issues() {
    let app = builder().build().await;
    operators(&app).await;
    boom(&app, 1).await;
    flushed(&app).await;
    let client = sign_in(&app, "viewer@acme.test").await;
    for page in [
        "/_lighthouse",
        "/_lighthouse/issues",
        "/_lighthouse/settings",
        "/_lighthouse/alerts",
        "/_lighthouse/queries",
    ] {
        let statements = statements_during(&app, async {
            client.get(page).send().await.assert_status(200);
        })
        .await;
        let counting: Vec<&String> = statements
            .iter()
            .filter(|sql| sql.contains("first_release") && sql.contains("lighthouse_issues"))
            .filter(|sql| !sql.contains("INSERT") && !sql.contains("UPDATE"))
            // A page listing issues reads the column along with the others.
            .filter(|sql| !sql.contains("fingerprint"))
            .collect();
        assert!(counting.is_empty(), "{page}: {counting:?}");
    }
    let statements = statements_during(&app, async {
        client
            .get("/_lighthouse/releases")
            .send()
            .await
            .assert_status(200);
    })
    .await;
    let counting: Vec<&String> = statements
        .iter()
        .filter(|sql| sql.contains("first_release") || sql.contains("regressed_in_release"))
        .filter(|sql| sql.contains("lighthouse_issues") && !sql.contains("INSERT"))
        .filter(|sql| !sql.contains("fingerprint"))
        .collect();
    assert!(!counting.is_empty());
    for sql in counting {
        assert!(sql.contains("GROUP BY") && sql.contains("COUNT"), "{sql}");
    }
    app.shutdown().await.unwrap();
}
