//! The dashboard and its JSON API: access by ability, the app's own sign-in
//! (the session and its pages), the CSRF token of the dashboard's forms, and
//! the pages with the shell around them.

use navree_lighthouse::IssueFilter;
use navree_testing::{TestApp, TestClient, TestResponse};
use navree_ui::PAGE_CSP;
use serde_json::{Value, json};

use crate::support::{PASSWORD, builder, flushed, token, user};

async fn boom(app: &TestApp) {
    app.client()
        .get("/api/demo/boom/7")
        .send()
        .await
        .assert_status(500);
}

/// A page, asked for as a browser asks.
pub async fn html(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// A form posted as a page without JavaScript posts it: with the session's
/// CSRF token as a field.
pub async fn post_form(client: &TestClient, path: &str, fields: &[(&str, &str)]) -> TestResponse {
    let token = client
        .cookie("XSRF-TOKEN")
        .expect("a page handed out the token");
    let mut form = fields.to_vec();
    form.push(("_csrf", token.as_str()));
    client
        .post(path)
        .header("accept", "text/html")
        .form(&form)
        .send()
        .await
}

/// Signs `email` in on the app's sign-in page, as every page of the app does.
pub async fn sign_in(app: &TestApp, email: &str) -> TestClient {
    let client = app.client();
    html(&client, "/sign-in?next=%2F_lighthouse%2Fissues")
        .await
        .assert_status(200);
    let response = post_form(
        &client,
        "/sign-in",
        &[
            ("email", email),
            ("password", PASSWORD),
            ("next", "/_lighthouse/issues"),
        ],
    )
    .await;
    assert_eq!(response.status().as_u16(), 303, "{}", response.text());
    assert_eq!(response.header("location"), Some("/_lighthouse/issues"));
    client
}

#[tokio::test]
async fn guests_are_asked_to_sign_in_and_the_api_answers_401() {
    let app = builder().build().await;
    let client = app.client();
    let refused = html(&client, "/_lighthouse")
        .await
        .assert_status(401)
        .text();
    assert!(
        refused.contains("/sign-in?next=%2F_lighthouse"),
        "the page links to the app's sign-in and back: {refused}"
    );
    let deep = html(&client, "/_lighthouse/issues?status=resolved")
        .await
        .assert_status(401)
        .text();
    assert!(deep.contains("/sign-in?next="), "{deep}");
    // The live part of the overview and the JSON API refuse a guest too.
    html(&client, "/_lighthouse/overview/live")
        .await
        .assert_status(401);
    let api = client
        .get("/_lighthouse/api/issues")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(401);
    assert!(
        api.header("content-type").unwrap().contains("json"),
        "the API answers in JSON whatever the browser asks for"
    );
    // The dashboard has no sign-in of its own.
    html(&client, "/_lighthouse/login").await.assert_status(404);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn signing_in_opens_every_page_in_the_shell() {
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
        ("/_lighthouse?period=1h".to_owned(), "Requests per minute"),
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
        ("/_lighthouse/alerts".to_owned(), "Delivery history"),
        ("/_lighthouse/releases".to_owned(), "Report a release"),
        ("/_lighthouse/settings".to_owned(), "Changeable here"),
    ] {
        let page = html(&client, &path).await;
        assert_eq!(page.status().as_u16(), 200, "{path}: {}", page.text());
        let body = page.text();
        assert!(body.contains(text), "{path} lacks {text:?}");
        assert!(
            body.contains("ops@acme.test"),
            "{path} shows who is signed in"
        );
        // The navigation of the tool, on every page.
        for item in [
            "href=\"/_lighthouse/issues\"",
            "href=\"/_lighthouse/requests\"",
            "href=\"/_lighthouse/queries\"",
            "href=\"/_lighthouse/jobs\"",
            "href=\"/_lighthouse/outgoing\"",
            "href=\"/_lighthouse/alerts\"",
            "href=\"/_lighthouse/releases\"",
            "href=\"/_lighthouse/settings\"",
        ] {
            assert!(body.contains(item), "{path} lacks the link {item}");
        }
        // And a way out, which is the app's.
        assert!(body.contains("/sign-out"), "{path} can sign out");
        assert_eq!(page.header("content-security-policy"), Some(PAGE_CSP));
    }
    // A view-only user sees no buttons that change issues.
    let page = html(&client, &format!("/_lighthouse/issues/{}", issue.id))
        .await
        .text();
    assert!(!page.contains(&format!("/issues/{}/resolve", issue.id)));
    assert!(!page.contains(&format!("/issues/{}/ignore", issue.id)));
    // The trace id links to the trace backend (`observe.tracing.trace_url`).
    assert!(
        page.contains("https://grafana.acme.test/explore?traceId="),
        "{page}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_bad_period_is_a_400_page() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    let client = sign_in(&app, "ops@acme.test").await;
    html(&client, "/_lighthouse?period=2h")
        .await
        .assert_status(400);
    html(&client, "/_lighthouse/queries?period=soon")
        .await
        .assert_status(400);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_user_without_view_gets_the_forbidden_page() {
    let app = builder().build().await;
    user(&app, "sales@acme.test", Some("sales"), &["deals.read"]).await;
    boom(&app).await;
    flushed(&app).await;
    let client = sign_in(&app, "sales@acme.test").await;
    for path in [
        "/_lighthouse",
        "/_lighthouse/issues",
        "/_lighthouse/alerts",
        "/_lighthouse/overview/live",
    ] {
        let page = html(&client, path).await.assert_status(403);
        assert!(
            page.header("content-type")
                .unwrap()
                .starts_with("text/html"),
            "{path}"
        );
        assert!(
            !page.text().contains("invoice inv_7"),
            "{path} shows nothing of the issues"
        );
    }
    client
        .get("/_lighthouse/api/issues")
        .send()
        .await
        .assert_status(403);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn changes_through_the_session_need_its_csrf_token() {
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
    let page = html(&client, &format!("/_lighthouse/issues/{id}"))
        .await
        .assert_status(200)
        .text();
    let resolve = format!("/_lighthouse/issues/{id}/resolve");
    assert!(page.contains(&resolve), "the form is offered: {page}");
    assert!(page.contains("name=\"_csrf\""), "and carries the token");
    assert!(page.contains("method=\"post\""));

    // The form without its token is refused, and the issue stays open.
    client
        .post(&resolve)
        .header("accept", "text/html")
        .form(&[("comment", "fixed")])
        .send()
        .await
        .assert_status(403);
    // A wrong token is refused too.
    client
        .post(&resolve)
        .header("accept", "text/html")
        .form(&[("comment", "fixed"), ("_csrf", "forged")])
        .send()
        .await
        .assert_status(403);
    assert_eq!(
        store.issue_detail(id).await.unwrap().issue.status.as_str(),
        "open"
    );

    // With it, the issue is resolved and the browser goes back to the issue.
    let done = post_form(&client, &resolve, &[("comment", "fixed")])
        .await
        .assert_status(303);
    assert_eq!(
        done.header("location"),
        Some(format!("/_lighthouse/issues/{id}").as_str())
    );
    let detail = store.issue_detail(id).await.unwrap();
    assert_eq!(detail.issue.status.as_str(), "resolved");
    assert_eq!(detail.comment.as_deref(), Some("fixed"));

    // The JSON API takes the session's token as a header.
    let reopen = format!("/_lighthouse/api/issues/{id}/reopen");
    client.post(&reopen).send().await.assert_status(403);
    let token = client.cookie("XSRF-TOKEN").unwrap();
    let reopened: Value = client
        .post(&reopen)
        .header("x-csrf-token", &token)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(reopened["status"], "open");

    // Signing out is the app's: the session ends and the pages ask again.
    post_form(&client, "/sign-out", &[])
        .await
        .assert_status(303);
    html(&client, "/_lighthouse").await.assert_status(401);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_user_who_may_view_but_not_resolve_cannot_post_the_forms() {
    let app = builder().build().await;
    user(
        &app,
        "viewer@acme.test",
        Some("viewer"),
        &["lighthouse.view"],
    )
    .await;
    boom(&app).await;
    let store = flushed(&app).await;
    let id = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .id;
    let client = sign_in(&app, "viewer@acme.test").await;
    html(&client, &format!("/_lighthouse/issues/{id}"))
        .await
        .assert_status(200);
    post_form(
        &client,
        &format!("/_lighthouse/issues/{id}/ignore"),
        &[("duration", "forever")],
    )
    .await
    .assert_status(403);
    assert_eq!(
        store.issue_detail(id).await.unwrap().issue.status.as_str(),
        "open"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_issue_forms_resolve_ignore_and_reopen() {
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
    let status = |store: navree_lighthouse::LighthouseStore| async move {
        store
            .issue_detail(id)
            .await
            .unwrap()
            .issue
            .status
            .as_str()
            .to_owned()
    };

    post_form(
        &client,
        &format!("/_lighthouse/issues/{id}/resolve"),
        &[("in_next_release", "true"), ("comment", "  ")],
    )
    .await
    .assert_status(303);
    assert_eq!(status(store.clone()).await, "resolved_in_next_release");
    post_form(&client, &format!("/_lighthouse/issues/{id}/reopen"), &[])
        .await
        .assert_status(303);
    assert_eq!(status(store.clone()).await, "open");
    post_form(
        &client,
        &format!("/_lighthouse/issues/{id}/ignore"),
        &[("duration", "occurrences:10"), ("comment", "noisy")],
    )
    .await
    .assert_status(303);
    assert_eq!(status(store.clone()).await, "ignored");
    // The page of an ignored issue offers to reopen it, with the comment.
    let page = html(&client, &format!("/_lighthouse/issues/{id}"))
        .await
        .text();
    assert!(page.contains(&format!("/issues/{id}/reopen")), "{page}");
    assert!(page.contains("noisy"), "{page}");
    // A choice the page does not offer is refused.
    post_form(
        &client,
        &format!("/_lighthouse/issues/{id}/ignore"),
        &[("duration", "weeks:2")],
    )
    .await
    .assert_status(400);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_overview_has_a_part_a_region_can_fetch_again() {
    let app = builder().build().await;
    user(&app, "ops@acme.test", Some("ops"), &["lighthouse.view"]).await;
    boom(&app).await;
    flushed(&app).await;
    let client = sign_in(&app, "ops@acme.test").await;
    let page = html(&client, "/_lighthouse?period=24h").await.text();
    assert!(page.contains("id=\"lighthouse-live\""), "{page}");
    assert!(
        page.contains("data-ui-src=\"/_lighthouse/overview/live?period=24h\""),
        "{page}"
    );
    assert!(page.contains("data-ui-poll=\"10000\""), "{page}");
    let part = html(&client, "/_lighthouse/overview/live?period=24h")
        .await
        .assert_status(200)
        .text();
    assert!(part.contains("id=\"lighthouse-live\""), "{part}");
    assert!(part.contains("invoice inv_7 not found"), "{part}");
    assert!(!part.contains("<html"), "a part, not a page: {part}");
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
async fn super_admins_pass_and_the_old_assets_are_gone() {
    let app = builder().build().await;
    user(&app, "root@acme.test", Some("admin"), &[]).await;
    let root = token(&app, "root@acme.test").await;
    app.client()
        .get("/_lighthouse/api/issues")
        .bearer(&root)
        .send()
        .await
        .assert_status(200);
    // The pages' files are Navree UI's.
    for path in [
        "/_lighthouse/assets/lighthouse.css",
        "/_lighthouse/assets/lighthouse.js",
    ] {
        app.client().get(path).send().await.assert_status(404);
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_dashboard_can_move_or_be_turned_off() {
    let app = builder()
        .config("lighthouse.path", "/ops/lighthouse")
        .build()
        .await;
    let client = app.client();
    let moved = html(&client, "/ops/lighthouse")
        .await
        .assert_status(401)
        .text();
    assert!(
        moved.contains("/sign-in?next=%2Fops%2Flighthouse"),
        "{moved}"
    );
    html(&client, "/_lighthouse").await.assert_status(404);
    app.shutdown().await.unwrap();

    let app = builder()
        .config("lighthouse.dashboard", false)
        .build()
        .await;
    html(&app.client(), "/_lighthouse").await.assert_status(404);
    app.client()
        .get("/_lighthouse/api/issues")
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.unwrap();
}
