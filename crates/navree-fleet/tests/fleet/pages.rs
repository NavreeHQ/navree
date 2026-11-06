//! The dashboard's pages, opened the way an app's people open them: on the
//! browser session, signed in through the app's own sign-in page. A guest is
//! sent to sign in, the pages draw from the same data as the JSON API, and
//! every write is a form that carries the session's CSRF token and needs the
//! `operate` ability.

use navree_access::{AbilityCtx, AccessModule, AccessOptions, AccessService, Rules};
use navree_auth::{AuthModule, AuthPages, AuthService};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::Db;
use navree_fleet::{CompletedJob, MetricKind, MetricSnapshot, REDACTED};
use navree_queue::AttemptOutcome;
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestClient, TestResponse};
use navree_ui::UiModule;
use serde_json::{Value, json};

use crate::support::{Harness, Record, SendDigest, builder, fail_exports};

const PASSWORD: &str = "correct horse battery";

/// Only the permissions stored for a user count.
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

/// An app as a team runs it: sessions, sign-in pages, the toolkit, and Fleet's
/// dashboard on.
async fn site() -> Harness {
    Harness::new(
        builder()
            .config("fleet.dashboard", true)
            .config("app.key", generate_key())
            .config("hashing.memory_kib", 1024)
            .config("hashing.iterations", 1)
            .config("http.per_ip_rate_limit", Value::Null)
            .module(CryptoModule)
            .module(SessionModule::for_root(SessionOptions::default()))
            .module(AuthModule::from_config())
            .module(AccessModule::for_root(AccessOptions {
                abilities,
                ..AccessOptions::default()
            }))
            .module(UiModule)
            .module(AuthPages::new().app_name("Acme Operations")),
    )
    .await
}

async fn get(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// Posts `form` with the session's CSRF token in `_csrf`, as a page without
/// script does.
async fn post(client: &TestClient, path: &str, form: &[(&str, &str)]) -> TestResponse {
    let token = client
        .cookie("XSRF-TOKEN")
        .expect("a page handed out the token");
    let mut form = form.to_vec();
    form.push(("_csrf", token.as_str()));
    client
        .post(path)
        .header("accept", "text/html")
        .form(&form)
        .send()
        .await
}

/// A person with `permissions`, signed in on a browser of their own.
async fn member(h: &Harness, name: &str, email: &str, permissions: &[&str]) -> TestClient {
    let user = h
        .app
        .get::<AuthService>()
        .await
        .create_user(name, email, PASSWORD)
        .await
        .unwrap();
    let access = h.app.get::<AccessService>().await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    for permission in permissions {
        access
            .grant_to_user(&uow, user.id, permission)
            .await
            .unwrap();
    }
    uow.commit().await.unwrap();
    let browser = h.app.client();
    get(&browser, "/sign-in").await.assert_status(200);
    post(
        &browser,
        "/sign-in",
        &[("email", email), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    browser
}

/// Someone who may look.
async fn viewer(h: &Harness) -> TestClient {
    member(
        h,
        "Gurpreet Kaur",
        "gurpreet@example.in",
        &["fleet.view"],
    )
    .await
}

/// Someone who may look and operate.
async fn operator(h: &Harness) -> TestClient {
    member(
        h,
        "Harjinder Singh",
        "harjinder@example.in",
        &["fleet.view", "fleet.operate"],
    )
    .await
}

#[tokio::test]
async fn a_guest_is_sent_to_sign_in_from_every_page() {
    let h = site().await;
    let guest = h.app.client();
    for path in [
        "/_fleet",
        "/_fleet/supervisors",
        "/_fleet/jobs",
        "/_fleet/failed",
        "/_fleet/metrics",
    ] {
        let page = get(&guest, path).await.assert_status(401).text();
        assert!(
            page.contains("/sign-in?next=%2F_fleet"),
            "{path} links to sign in and back: {page}"
        );
        assert!(!page.contains("Queue workers"), "{path} shows no data");
    }
    // The sign-in page is the app's own, and signing in comes back.
    let form = get(&guest, "/sign-in?next=%2F_fleet%2Ffailed")
        .await
        .assert_status(200)
        .text();
    assert!(form.contains("Acme Operations"), "{form}");
}

#[tokio::test]
async fn someone_without_the_ability_is_refused_a_page_not_sent_to_sign_in() {
    let h = site().await;
    let nobody = member(&h, "Jaswinder Kaur", "jaswinder@example.in", &[]).await;
    let page = get(&nobody, "/_fleet").await.assert_status(403).text();
    assert!(!page.contains("Failed past 7 days"), "{page}");
    let operator_only = member(
        &h,
        "Kuldeep Singh",
        "kuldeep@example.in",
        &["fleet.operate"],
    )
    .await;
    get(&operator_only, "/_fleet/failed")
        .await
        .assert_status(403);
}

#[tokio::test]
async fn the_overview_draws_the_shell_the_figures_and_the_failed_jobs() {
    let h = site().await;
    h.queue.dispatch(Record::new("ledger")).await.unwrap();
    fail_exports(&h, 2).await;
    let browser = operator(&h).await;
    let page = get(&browser, "/_fleet").await.assert_status(200).text();

    // The shell: the tool's name, its pages, and the signed-in person.
    assert!(page.contains("Fleet"), "{page}");
    for href in [
        "href=\"/_fleet/supervisors\"",
        "href=\"/_fleet/jobs\"",
        "href=\"/_fleet/failed\"",
        "href=\"/_fleet/metrics\"",
    ] {
        assert!(page.contains(href), "{href} in the sidebar: {page}");
    }
    assert!(page.contains("Harjinder Singh"), "the user's menu: {page}");
    assert!(page.contains("action=\"/sign-out\""), "{page}");
    assert!(page.contains(">Overview</h1>"), "{page}");

    // The figures and the workload of the default supervisor.
    for text in [
        "Status",
        "Inactive",
        "Jobs per minute",
        "Failed past 7 days",
        "Longest wait",
        "No master running",
        "default",
    ] {
        assert!(page.contains(text), "{text}: {page}");
    }

    // The newest failed jobs, each with its own forms.
    assert!(page.contains("contacts.export"), "{page}");
    assert!(page.contains("the mail server is down"), "{page}");
    assert!(
        page.contains("action=\"/_fleet/failed/retry\""),
        "retry all: {page}"
    );
    let failed = h.fleet.failed_jobs(None).await.unwrap();
    for job in &failed {
        for action in ["retry", "forget"] {
            let form = format!("action=\"/_fleet/failed/{}/{action}\"", job.id);
            assert!(page.contains(&form), "{form}: {page}");
        }
    }

    // Forms carry the session's token; the part that changes redraws itself;
    // nothing is inline.
    assert!(page.contains("name=\"_csrf\""), "{page}");
    assert!(page.contains("data-ui-poll=\"5000\""), "{page}");
    assert!(page.contains("data-ui-src=\"/_fleet\""), "{page}");
    assert!(!page.contains("<script>"), "no inline script: {page}");
    assert!(!page.contains(" style=\""), "no style attribute: {page}");
    // The columns a phone can do without are marked, so it keeps the rest.
    assert!(page.contains("data-hide-mobile"), "{page}");
}

#[tokio::test]
async fn every_page_draws_with_the_data_behind_the_api() {
    let h = site().await;
    let pending = h
        .queue
        .dispatch(SendDigest {
            user: "ada".into(),
            api_token: "tok_123".into(),
        })
        .await
        .unwrap();
    fail_exports(&h, 1).await;
    let failed = h.fleet.failed_jobs(None).await.unwrap().remove(0);
    h.fleet
        .record(
            None,
            &[CompletedJob {
                job_id: "done-1".into(),
                queue: "default".into(),
                job_type: "test.record".into(),
                attempt: 1,
                outcome: AttemptOutcome::Succeeded,
                runtime_ms: 12,
                finished_at: h.fleet.now(),
            }],
        )
        .await
        .unwrap();
    h.fleet
        .record_snapshots(
            Some("m1"),
            &[MetricSnapshot {
                at: h.fleet.now() - chrono::TimeDelta::minutes(2),
                kind: MetricKind::Queue,
                name: "mail".into(),
                processed: 30,
                failed: 0,
                runtime_avg_ms: 20,
                runtime_p95_ms: 45,
                pending: Some(1),
                wait_ms: Some(3_000),
            }],
        )
        .await
        .unwrap();
    let browser = viewer(&h).await;

    let supervisors = get(&browser, "/_fleet/supervisors")
        .await
        .assert_status(200)
        .text();
    assert!(supervisors.contains(">Supervisors</h1>"), "{supervisors}");
    assert!(
        supervisors.contains("No master has reported"),
        "{supervisors}"
    );

    // Pending jobs, the other tab, and a search.
    let jobs = get(&browser, "/_fleet/jobs").await.assert_status(200).text();
    assert!(jobs.contains("mail.digest"), "{jobs}");
    assert!(jobs.contains(&format!("href=\"/_fleet/jobs/{pending}\"")), "{jobs}");
    assert!(jobs.contains("Pending (1)"), "{jobs}");
    assert!(jobs.contains("Completed (1)"), "{jobs}");
    let completed = get(&browser, "/_fleet/jobs?tab=completed")
        .await
        .assert_status(200)
        .text();
    assert!(completed.contains("test.record"), "{completed}");
    assert!(completed.contains("succeeded"), "{completed}");
    assert!(completed.contains("12 ms"), "{completed}");
    let searched = get(&browser, "/_fleet/jobs?search=nothing-like-it")
        .await
        .assert_status(200)
        .text();
    assert!(searched.contains("Nothing here."), "{searched}");

    // One pending job, its payload redacted.
    let job = get(&browser, &format!("/_fleet/jobs/{pending}"))
        .await
        .assert_status(200)
        .text();
    assert!(!job.contains("tok_123"), "{job}");
    assert!(job.contains(REDACTED), "{job}");
    get(&browser, "/_fleet/jobs/missing")
        .await
        .assert_status(404);

    // Failed jobs: the list, a search, one with its error and payload.
    let list = get(&browser, "/_fleet/failed")
        .await
        .assert_status(200)
        .text();
    assert!(list.contains("contacts.export"), "{list}");
    assert!(
        list.contains(&format!("href=\"/_fleet/failed/{}\"", failed.id)),
        "{list}"
    );
    let none = get(&browser, "/_fleet/failed?search=nothing-like-it")
        .await
        .assert_status(200)
        .text();
    assert!(none.contains("No failed job matches."), "{none}");
    let one = get(&browser, &format!("/_fleet/failed/{}", failed.id))
        .await
        .assert_status(200)
        .text();
    assert!(one.contains("the mail server is down"), "{one}");
    assert!(!one.contains("hunter2"), "the password is redacted: {one}");
    assert!(one.contains(REDACTED), "{one}");
    assert!(one.contains("contact-0"), "{one}");
    get(&browser, "/_fleet/failed/missing")
        .await
        .assert_status(404);

    // Metrics: charts for the queue that has snapshots, on each period.
    for period in ["hour", "day", "week", "year"] {
        let metrics = get(&browser, &format!("/_fleet/metrics?period={period}"))
            .await
            .assert_status(200)
            .text();
        assert!(metrics.contains(">Metrics</h1>"), "{metrics}");
    }
    let metrics = get(&browser, "/_fleet/metrics").await.text();
    assert!(metrics.contains("<svg"), "charts are drawn on the server");
    assert!(metrics.contains("Throughput"), "{metrics}");
    assert!(metrics.contains("Runtime"), "{metrics}");
    assert!(metrics.contains("Wait"), "{metrics}");
    assert!(!metrics.contains("<script>"), "{metrics}");
}

#[tokio::test]
async fn a_write_without_the_session_token_is_refused() {
    let h = site().await;
    fail_exports(&h, 1).await;
    let id = h.fleet.failed_jobs(None).await.unwrap().remove(0).id;
    let operator = operator(&h).await;
    operator
        .post(&format!("/_fleet/failed/{id}/retry"))
        .header("accept", "text/html")
        .form(&[("next", "/_fleet")])
        .send()
        .await
        .assert_status(403);
    operator
        .post("/_fleet/pause")
        .header("accept", "text/html")
        .form(&[("next", "/_fleet")])
        .send()
        .await
        .assert_status(403);
    assert_eq!(h.fleet.failed_jobs(None).await.unwrap().len(), 1);
    assert!(h.fleet.paused().await.unwrap().is_empty());
}

#[tokio::test]
async fn an_operator_retries_and_forgets_and_comes_back_to_the_page() {
    let h = site().await;
    fail_exports(&h, 3).await;
    let ids: Vec<String> = h
        .fleet
        .failed_jobs(None)
        .await
        .unwrap()
        .into_iter()
        .map(|job| job.id)
        .collect();
    let operator = operator(&h).await;

    // Retry from a page that names where to come back to.
    let back = post(
        &operator,
        &format!("/_fleet/failed/{}/retry", ids[0]),
        &[("next", "/_fleet")],
    )
    .await
    .assert_status(303);
    assert_eq!(back.header("location"), Some("/_fleet"));
    assert_eq!(h.queue.pending_count("exports").await.unwrap(), 1);
    assert_eq!(h.fleet.failed_jobs(None).await.unwrap().len(), 2);

    // Without a `next`, or with one that leaves the site, it comes back to
    // the failed jobs.
    let back = post(
        &operator,
        &format!("/_fleet/failed/{}/forget", ids[1]),
        &[("next", "//evil.example")],
    )
    .await
    .assert_status(303);
    assert_eq!(back.header("location"), Some("/_fleet/failed"));
    assert_eq!(h.fleet.failed_jobs(None).await.unwrap().len(), 1);

    // Retry all, then a job that is gone.
    let back = post(&operator, "/_fleet/failed/retry", &[("search", "contacts")])
        .await
        .assert_status(303);
    assert_eq!(back.header("location"), Some("/_fleet/failed"));
    assert!(h.fleet.failed_jobs(None).await.unwrap().is_empty());
    // The message waits for the next page as a toast.
    let page = get(&operator, "/_fleet/failed").await.text();
    assert!(page.contains("back on its queue"), "{page}");
    post(&operator, &format!("/_fleet/failed/{}/retry", ids[0]), &[])
        .await
        .assert_status(404);
}

#[tokio::test]
async fn operators_pause_and_continue_from_the_pages() {
    let h = site().await;
    let operator = operator(&h).await;
    let back = post(
        &operator,
        "/_fleet/pause",
        &[("supervisor", "default"), ("next", "/_fleet/supervisors")],
    )
    .await
    .assert_status(303);
    assert_eq!(back.header("location"), Some("/_fleet/supervisors"));
    assert!(h.fleet.is_paused("default").await.unwrap());
    // The overview now offers to continue, and says what is paused.
    let page = get(&operator, "/_fleet").await.text();
    assert!(page.contains("action=\"/_fleet/continue\""), "{page}");
    assert!(page.contains("Continue all"), "{page}");
    assert!(page.contains("paused"), "{page}");
    post(&operator, "/_fleet/continue", &[("supervisor", "default")])
        .await
        .assert_status(303);
    assert!(!h.fleet.is_paused("default").await.unwrap());
    post(&operator, "/_fleet/pause", &[]).await.assert_status(303);
    assert_eq!(
        h.fleet.paused().await.unwrap().into_iter().collect::<Vec<_>>(),
        ["default"]
    );
    post(&operator, "/_fleet/pause", &[("supervisor", "nope")])
        .await
        .assert_status(404);
}

#[tokio::test]
async fn someone_who_may_only_look_cannot_write() {
    let h = site().await;
    fail_exports(&h, 1).await;
    let id = h.fleet.failed_jobs(None).await.unwrap().remove(0).id;
    let viewer = viewer(&h).await;
    // The pages open, with their forms.
    get(&viewer, "/_fleet").await.assert_status(200);
    for (path, form) in [
        (format!("/_fleet/failed/{id}/retry"), vec![]),
        (format!("/_fleet/failed/{id}/forget"), vec![]),
        ("/_fleet/failed/retry".to_owned(), vec![]),
        ("/_fleet/pause".to_owned(), vec![("supervisor", "default")]),
        ("/_fleet/continue".to_owned(), vec![]),
    ] {
        post(&viewer, &path, &form).await.assert_status(403);
    }
    assert_eq!(h.fleet.failed_jobs(None).await.unwrap().len(), 1);
    assert!(h.fleet.paused().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_json_api_is_the_same_for_a_session_and_a_token() {
    let h = site().await;
    let viewer = viewer(&h).await;

    // A signed-in browser reads it, and an error stays JSON even when the
    // client says it wants HTML.
    let overview = get(&viewer, "/_fleet/api/overview")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(overview["status"], json!("inactive"));
    let guest = get(&h.app.client(), "/_fleet/api/overview")
        .await
        .assert_status(401);
    assert!(
        !guest
            .header("content-type")
            .unwrap_or_default()
            .starts_with("text/html"),
        "the API answers errors in JSON"
    );

    // A bearer token from the API's own sign-in reads it too.
    let ada = h
        .app
        .get::<AuthService>()
        .await
        .create_user("Manpreet Kaur", "manpreet@example.in", PASSWORD)
        .await
        .unwrap();
    let access = h.app.get::<AccessService>().await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    access
        .grant_to_user(&uow, ada.id, "fleet.view")
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let tokens = h
        .app
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": "manpreet@example.in", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let token = tokens["access_token"].as_str().expect("an access token");
    h.app
        .client()
        .get("/_fleet/api/overview")
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("inactive"));
}
