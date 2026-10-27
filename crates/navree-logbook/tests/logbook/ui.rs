//! The UI at `/_logbook` and its API: pages, assets, the loopback guard and
//! `allow_remote`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use http::request::Parts;
use navree_contracts::AuthState;
use navree_http::{AppError, RouteAuthorizer, RouteMeta};
use navree_kernel::{BoxFuture, Module, ModuleBuilder};
use navree_logbook::{Logbook, LogbookModule, VIEW_PERMISSION};
use navree_telemetry::{EventContext, MailSent, TelemetryEvent, TelemetryRecord, TelemetrySink};
use navree_testing::principal;
use serde_json::json;
use uuid::Uuid;

use crate::app::{FixturesModule, app, builder};

/// The page's text with minijinja's HTML escapes undone (`/` is printed as
/// `&#x2f;`).
pub fn unescape(html: &str) -> String {
    html.replace("&#x2f;", "/")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

async fn with_requests() -> navree_testing::TestApp {
    let app = app().await;
    for n in 0..2 {
        app.client()
            .post("/echo")
            .json(&json!({ "n": n }))
            .send()
            .await
            .assert_status(200);
    }
    app
}

#[tokio::test]
async fn the_list_renders_with_a_strict_csp() {
    let app = with_requests().await;
    let page = app
        .client()
        .get("/_logbook")
        .send()
        .await
        .assert_status(200);
    let csp = page.header("content-security-policy").unwrap().to_owned();
    assert!(
        csp.contains("script-src 'self'") && !csp.contains("unsafe-inline"),
        "{csp}"
    );
    let html = unescape(&page.text());
    assert!(html.contains("POST /echo"), "{html}");
    assert!(html.contains("/_logbook/assets/logbook.js"));
    assert!(html.contains("Outgoing HTTP"), "the type buttons");
    assert!(!html.contains("<script>"), "no inline script");

    let filtered = unescape(
        &app.client()
            .get("/_logbook?q=status%3A5xx")
            .send()
            .await
            .assert_status(200)
            .text(),
    );
    assert!(!filtered.contains("POST /echo"));
    assert!(filtered.contains("for this filter"));

    let invalid = app
        .client()
        .get("/_logbook?q=status%3Anope")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(invalid.contains("status:nope"), "the error names the term");

    let typed = unescape(
        &app.client()
            .get("/_logbook?type=query")
            .send()
            .await
            .assert_status(200)
            .text(),
    );
    assert!(!typed.contains("POST /echo"));
}

#[tokio::test]
async fn new_rows_arrive_after_a_seq() {
    let app = with_requests().await;
    let logbook = app.get::<Logbook>().await;
    let latest = logbook.last_request().await.unwrap().head().unwrap().seq;
    let none = app
        .client()
        .get(&format!("/_logbook/rows?after={latest}"))
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        none.header("x-logbook-latest"),
        Some(latest.to_string().as_str())
    );
    assert!(!none.text().contains("<tr"));
    let all = app
        .client()
        .get("/_logbook/rows?after=0")
        .send()
        .await
        .assert_status(200)
        .text();
    assert_eq!(all.matches("<tr").count(), 2);
}

#[tokio::test]
async fn a_request_shows_its_detail_and_replay_form() {
    let app = with_requests().await;
    let logbook = app.get::<Logbook>().await;
    let head = logbook
        .last_request()
        .await
        .unwrap()
        .head()
        .unwrap()
        .clone();
    let html = unescape(
        &app.client()
            .get(&format!("/_logbook/entries/{}", head.id))
            .send()
            .await
            .assert_status(200)
            .text(),
    );
    for expected in [
        "Request id",
        "data-tab=\"request\"",
        "data-tab=\"response\"",
        "data-tab=\"timeline\"",
        "data-tab=\"replay\"",
        "/_logbook/api/requests/",
        "Copy as curl",
        "application/json",
    ] {
        assert!(html.contains(expected), "missing {expected}:\n{html}");
    }
    // The batch id finds the same page.
    let by_batch = app
        .client()
        .get(&format!("/_logbook/entries/{}", head.batch_id.unwrap()))
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(by_batch.contains("Request id"));
    app.client()
        .get("/_logbook/entries/nope")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn assets_are_served_with_their_types() {
    let app = app().await;
    let css = app
        .client()
        .get("/_logbook/assets/logbook.css")
        .send()
        .await
        .assert_status(200);
    assert_eq!(css.header("content-type"), Some("text/css; charset=utf-8"));
    let js = app
        .client()
        .get("/_logbook/assets/logbook.js")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        js.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert!(js.text().contains("data-replay"));
}

#[tokio::test]
async fn mail_opens_as_sandboxed_html() {
    let app = app().await;
    let logbook = app.get::<Logbook>().await;
    logbook.record(&TelemetryRecord {
        at: Utc::now(),
        context: EventContext::default(),
        event: TelemetryEvent::MailSent(MailSent {
            to: vec!["ada@example.com".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            from: None,
            subject: "Welcome".into(),
            text: Some("Hi".into()),
            html: Some("<h1>Hi Ada</h1>".into()),
            transport: "log".into(),
            duration: Duration::from_millis(1),
        }),
    });
    let mail = logbook
        .entries(&navree_logbook::ListQuery::new(
            navree_logbook::Filter::parse("type:mail").unwrap(),
            1,
        ))
        .await
        .unwrap()
        .remove(0);
    let page = app
        .client()
        .get(&format!("/_logbook/mail/{}", mail.id))
        .send()
        .await
        .assert_status(200);
    assert!(
        page.header("content-security-policy")
            .unwrap()
            .contains("sandbox")
    );
    assert_eq!(page.text(), "<h1>Hi Ada</h1>");
    let detail = app
        .client()
        .get(&format!("/_logbook/entries/{}", mail.id))
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(detail.contains("Open HTML"), "{detail}");
}

#[tokio::test]
async fn the_api_lists_shows_and_clears() {
    let app = with_requests().await;
    let list = app
        .client()
        .get("/_logbook/api/entries?type=request&limit=1")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(list["data"].as_array().unwrap().len(), 1);
    let next = list["next"].as_u64().expect("a next page");
    let older = app
        .client()
        .get(&format!("/_logbook/api/entries?type=request&before={next}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(older["data"].as_array().unwrap().len(), 1);
    assert!(older["next"].is_null());

    let id = list["data"][0]["id"].as_str().unwrap().to_owned();
    let batch_id = list["data"][0]["batch_id"].as_str().unwrap().to_owned();
    let entry = app
        .client()
        .get(&format!("/_logbook/api/entries/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(entry["content"]["type"], "request");
    let batch = app
        .client()
        .get(&format!("/_logbook/api/batches/{batch_id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(batch["id"], batch_id);
    assert!(batch["duplicate_queries"].as_array().unwrap().is_empty());

    app.client()
        .get("/_logbook/api/entries?q=status%3Anope")
        .send()
        .await
        .assert_status(400);
    let cleared = app
        .client()
        .delete("/_logbook/api/entries")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(cleared["deleted"].as_u64().unwrap() >= 2);
    assert_eq!(app.get::<Logbook>().await.count().await.unwrap(), 0);
}

#[tokio::test]
async fn only_loopback_clients_are_served() {
    let app = builder()
        .config("http.trust_proxy", true)
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    for path in [
        "/_logbook",
        "/_logbook/api/entries",
        "/_logbook/assets/logbook.js",
    ] {
        app.client()
            .get(path)
            .header("x-forwarded-for", "203.0.113.7")
            .send()
            .await
            .assert_status(404);
        app.client().get(path).send().await.assert_status(200);
    }
}

/// Lets a request through when its user was granted the permissions the
/// route needs.
#[derive(Default)]
struct GrantAuthorizer(Mutex<HashSet<(Uuid, String)>>);

impl RouteAuthorizer for GrantAuthorizer {
    fn authorize<'a>(
        &'a self,
        parts: &'a mut Parts,
        meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        let user = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id);
        let granted = self.0.lock().unwrap();
        let allowed = user.is_some_and(|user| {
            meta.access
                .permissions
                .iter()
                .all(|p| granted.contains(&(user, p.clone())))
        });
        Box::pin(async move {
            if allowed {
                Ok(())
            } else {
                Err(AppError::forbidden())
            }
        })
    }
}

struct Authorizing(Arc<GrantAuthorizer>);

impl Module for Authorizing {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc::<dyn RouteAuthorizer>(self.0.clone())
            .exports::<dyn RouteAuthorizer>();
    }
}

#[tokio::test]
async fn remote_access_needs_the_view_permission() {
    let authorizer = Arc::new(GrantAuthorizer::default());
    let viewer = Uuid::now_v7();
    authorizer
        .0
        .lock()
        .unwrap()
        .insert((viewer, VIEW_PERMISSION.to_owned()));
    let app = builder()
        .config("http.trust_proxy", true)
        .config("logbook.allow_remote", true)
        .module(Authorizing(authorizer))
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    app.client()
        .get("/_logbook")
        .send()
        .await
        .assert_status(401);
    app.acting_as(principal(Uuid::now_v7()))
        .get("/_logbook")
        .send()
        .await
        .assert_status(403);
    app.acting_as(principal(viewer))
        .get("/_logbook")
        .header("x-forwarded-for", "203.0.113.7")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn remote_access_without_an_authorizer_fails_the_boot() {
    let error = builder()
        .config("logbook.allow_remote", true)
        .module(LogbookModule::from_config())
        .try_build()
        .await
        .unwrap_err();
    assert!(error.to_string().contains("logbook"), "{error}");
}
