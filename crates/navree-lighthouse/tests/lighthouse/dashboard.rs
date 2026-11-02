//! The dashboard and its JSON API: access by ability, signing in, the
//! session cookie and its CSRF token, the pages and the assets.

use navree_lighthouse::{CONTENT_SECURITY_POLICY, IssueFilter};
use navree_testing::{TestApp, TestClient, TestResponse};
use serde_json::{Value, json};

use crate::support::{builder, flushed, token, user};

async fn boom(app: &TestApp) {
    app.client()
        .get("/api/demo/boom/7")
        .send()
        .await
        .assert_status(500);
}

/// The hidden `csrf` field of a sign-in form.
fn form_token(page: &TestResponse) -> String {
    let html = page.text();
    let start = html.find("name=\"csrf\" value=\"").expect("a csrf field") + 19;
    html[start..start + html[start..].find('"').unwrap()].to_owned()
}

/// Signs `email` in through the sign-in screen.
pub async fn sign_in(app: &TestApp, email: &str) -> TestClient {
    let client = app.client();
    let page = client
        .get("/_lighthouse/login")
        .send()
        .await
        .assert_status(200);
    let csrf = form_token(&page);
    let response = client
        .post("/_lighthouse/login")
        .form(&[
            ("email", email),
            ("password", "correct horse battery"),
            ("csrf", &csrf),
            ("next", "/_lighthouse/issues"),
        ])
        .send()
        .await;
    assert_eq!(response.status().as_u16(), 303, "{}", response.text());
    assert_eq!(response.header("location"), Some("/_lighthouse/issues"));
    assert!(client.cookie("lighthouse_session").is_some());
    client
}

#[tokio::test]
async fn guests_go_to_the_sign_in_screen_and_the_api_answers_401() {
    let app = builder().build().await;
    let client = app.client();
    let response = client.get("/_lighthouse").send().await.assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/_lighthouse/login?next=%2F_lighthouse")
    );
    client
        .get("/_lighthouse/api/issues")
        .send()
        .await
        .assert_status(401);
    let login = client
        .get("/_lighthouse/login")
        .send()
        .await
        .assert_status(200);
    assert!(login.text().contains("Sign in"));
    assert_eq!(
        login.header("content-security-policy"),
        Some(CONTENT_SECURITY_POLICY)
    );
    assert!(login.set_cookie("lighthouse_csrf").is_some());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn signing_in_opens_every_page() {
    let app = builder()
        .config(
            "observe.tracing.trace_url",
            "https://grafana.acme.test/explore?traceId={trace_id}",
        )
        .build()
        .await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    boom(&app).await;
    let store = flushed(&app).await;
    let issue = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .clone();
    let sample = store
        .issue_detail(issue.id)
        .await
        .unwrap()
        .latest
        .unwrap()
        .sample_id
        .unwrap();
    let client = sign_in(&app, "ops@acme.test").await;
    for (path, text) in [
        ("/_lighthouse".to_owned(), "New occurrences"),
        ("/_lighthouse?period=1h".to_owned(), "Requests / min"),
        ("/_lighthouse/issues".to_owned(), "invoice inv_7 not found"),
        (
            "/_lighthouse/issues?status=&sort=count".to_owned(),
            "invoice inv_7 not found",
        ),
        (format!("/_lighthouse/issues/{}", issue.id), "Latest event"),
        ("/_lighthouse/requests".to_owned(), "boom"),
        (
            "/_lighthouse/requests/route?endpoint=GET%20%2Fapi%2Fdemo%2Fboom%2F%7Bn%7D&period=1h"
                .to_owned(),
            "Slowest traces",
        ),
        (format!("/_lighthouse/traces/{sample}"), "Timeline"),
        ("/_lighthouse/queries".to_owned(), "Queries"),
        ("/_lighthouse/jobs".to_owned(), "Jobs"),
        ("/_lighthouse/outgoing".to_owned(), "Outgoing"),
    ] {
        let page = client.get(&path).send().await;
        assert_eq!(page.status().as_u16(), 200, "{path}: {}", page.text());
        let html = page.text();
        assert!(html.contains(text), "{path} lacks {text:?}");
        assert!(
            html.contains("ops@acme.test"),
            "{path} shows who is signed in"
        );
        assert_eq!(
            page.header("content-security-policy"),
            Some(CONTENT_SECURITY_POLICY)
        );
    }
    // A view-only user sees no buttons that change issues.
    let page = client
        .get(&format!("/_lighthouse/issues/{}", issue.id))
        .send()
        .await
        .text();
    assert!(!page.contains("data-action=\"resolve\""));
    // The trace id links to the trace backend (`observe.tracing.trace_url`).
    assert!(
        page.contains("https:&#x2f;&#x2f;grafana.acme.test&#x2f;explore?traceId="),
        "{page}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_user_without_view_gets_a_403_page() {
    let app = builder().build().await;
    user(&app, "sales@acme.test", Some("sales"), &["deals.read"]).await;
    let client = sign_in(&app, "sales@acme.test").await;
    let page = client
        .get("/_lighthouse/issues")
        .send()
        .await
        .assert_status(403);
    assert!(page.text().contains("lighthouse.view"));
    assert!(
        page.header("content-type")
            .unwrap()
            .starts_with("text/html")
    );
    client
        .get("/_lighthouse/api/issues")
        .send()
        .await
        .assert_status(403);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn wrong_passwords_and_stale_forms_are_refused() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    let client = app.client();
    let page = client.get("/_lighthouse/login").send().await;
    let csrf = form_token(&page);
    let wrong = client
        .post("/_lighthouse/login")
        .form(&[
            ("email", "ops@acme.test"),
            ("password", "nope"),
            ("csrf", csrf.as_str()),
            ("next", ""),
        ])
        .send()
        .await
        .assert_status(401);
    assert!(wrong.text().contains("do not match"));
    let stale = app
        .client()
        .post("/_lighthouse/login")
        .form(&[
            ("email", "ops@acme.test"),
            ("password", "correct horse battery"),
            ("csrf", "forged"),
            ("next", ""),
        ])
        .send()
        .await
        .assert_status(403);
    assert!(stale.text().contains("expired"));
    assert!(app.client().cookie("lighthouse_session").is_none());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_next_page_never_leaves_the_dashboard() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    let client = app.client();
    let page = client.get("/_lighthouse/login").send().await;
    let csrf = form_token(&page);
    let response = client
        .post("/_lighthouse/login")
        .form(&[
            ("email", "ops@acme.test"),
            ("password", "correct horse battery"),
            ("csrf", csrf.as_str()),
            ("next", "//evil.test/x"),
        ])
        .send()
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/_lighthouse"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn changes_through_the_cookie_need_the_csrf_token() {
    let app = builder().build().await;
    user(
        &app,
        "lead@acme.test",
        Some("lead"),
        &["lighthouse.view", "lighthouse.resolve"],
    )
    .await;
    boom(&app).await;
    let store = flushed(&app).await;
    let id = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .id;
    let client = sign_in(&app, "lead@acme.test").await;
    let page = client
        .get(&format!("/_lighthouse/issues/{id}"))
        .send()
        .await
        .text();
    assert!(page.contains("data-action=\"resolve\""));
    let start = page.find("data-csrf=\"").unwrap() + 11;
    let csrf = &page[start..start + page[start..].find('"').unwrap()];
    client
        .post(&format!("/_lighthouse/api/issues/{id}/resolve"))
        .json(&json!({}))
        .send()
        .await
        .assert_status(403);
    let resolved: Value = client
        .post(&format!("/_lighthouse/api/issues/{id}/resolve"))
        .header("x-csrf-token", csrf)
        .json(&json!({ "comment": "fixed" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(resolved["status"], "resolved");
    client
        .post("/_lighthouse/logout")
        .header("x-csrf-token", csrf)
        .send()
        .await
        .assert_status(204);
    assert!(client.cookie("lighthouse_session").is_none());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_api_accepts_access_tokens_and_checks_each_ability() {
    let app = builder().build().await;
    user(
        &app,
        "viewer@acme.test",
        Some("viewer"),
        &["lighthouse.view"],
    )
    .await;
    user(
        &app,
        "lead@acme.test",
        Some("lead"),
        &["lighthouse.view", "lighthouse.resolve"],
    )
    .await;
    boom(&app).await;
    let store = flushed(&app).await;
    let id = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .id;
    let viewer = token(&app, "viewer@acme.test").await;
    let lead = token(&app, "lead@acme.test").await;
    let client = app.client();

    let page = client
        .get("/_lighthouse/api/issues?status=open&sort=count")
        .bearer(&viewer)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["meta"]["total"], 1);
    assert_eq!(page["data"][0]["culprit"], "GET /api/demo/boom/{n}");
    assert_eq!(page["data"][0]["sparkline"].as_array().unwrap().len(), 24);
    let detail = client
        .get(&format!("/_lighthouse/api/issues/{id}"))
        .bearer(&viewer)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(detail["chain"], json!(["invoice inv_7 not found"]));
    for path in [
        "/_lighthouse/api/overview?period=1h".to_owned(),
        "/_lighthouse/api/routes".to_owned(),
        "/_lighthouse/api/routes/detail?endpoint=GET%20%2Fapi%2Fdemo%2Fboom%2F%7Bn%7D".to_owned(),
        "/_lighthouse/api/queries?period=7d".to_owned(),
        "/_lighthouse/api/jobs".to_owned(),
        "/_lighthouse/api/outgoing".to_owned(),
        "/_lighthouse/api/alerts".to_owned(),
        format!("/_lighthouse/api/issues/{id}/occurrences?per_page=5"),
    ] {
        client
            .get(&path)
            .bearer(&viewer)
            .send()
            .await
            .assert_status(200);
    }
    client
        .get("/_lighthouse/api/overview?period=2h")
        .bearer(&viewer)
        .send()
        .await
        .assert_status(400);
    client
        .get(&format!("/_lighthouse/api/issues/{}", uuid::Uuid::now_v7()))
        .bearer(&viewer)
        .send()
        .await
        .assert_status(404);

    client
        .post(&format!("/_lighthouse/api/issues/{id}/ignore"))
        .bearer(&viewer)
        .json(&json!({}))
        .send()
        .await
        .assert_status(403);
    let ignored = client
        .post(&format!("/_lighthouse/api/issues/{id}/ignore"))
        .bearer(&lead)
        .json(&json!({ "occurrences": 10, "comment": "noisy" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(ignored["status"], "ignored");
    client
        .post(&format!("/_lighthouse/api/issues/{id}/ignore"))
        .bearer(&lead)
        .json(&json!({ "occurrences": 0 }))
        .send()
        .await
        .assert_status(400);
    let reopened = client
        .post(&format!("/_lighthouse/api/issues/{id}/reopen"))
        .bearer(&lead)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(reopened["status"], "open");
    let next_release = client
        .post(&format!("/_lighthouse/api/issues/{id}/resolve"))
        .bearer(&lead)
        .json(&json!({ "in_next_release": true }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(next_release["status"], "resolved_in_next_release");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn super_admins_pass_and_the_assets_are_public() {
    let app = builder().build().await;
    user(&app, "root@acme.test", Some("admin"), &[]).await;
    let root = token(&app, "root@acme.test").await;
    app.client()
        .get("/_lighthouse/api/issues")
        .bearer(&root)
        .send()
        .await
        .assert_status(200);
    let css = app
        .client()
        .get("/_lighthouse/assets/lighthouse.css")
        .send()
        .await
        .assert_status(200);
    assert!(css.header("content-type").unwrap().starts_with("text/css"));
    assert!(css.text().contains("--accent: #3dff8f"));
    let js = app
        .client()
        .get("/_lighthouse/assets/lighthouse.js")
        .send()
        .await
        .assert_status(200);
    assert!(
        js.header("content-type")
            .unwrap()
            .starts_with("text/javascript")
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_dashboard_can_move_or_be_turned_off() {
    let app = builder()
        .config("lighthouse.path", "/ops/lighthouse")
        .build()
        .await;
    app.client()
        .get("/ops/lighthouse/login")
        .send()
        .await
        .assert_status(200);
    app.client()
        .get("/_lighthouse/login")
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.unwrap();

    let app = builder()
        .config("lighthouse.dashboard", false)
        .build()
        .await;
    app.client()
        .get("/_lighthouse/login")
        .send()
        .await
        .assert_status(404);
    app.client()
        .get("/_lighthouse/api/issues")
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.unwrap();
}
