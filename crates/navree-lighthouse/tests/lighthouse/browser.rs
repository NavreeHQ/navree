//! The dashboard in a real browser (Chrome or Chromium, through
//! navree-browser): a guest signing in on the app's page and coming back,
//! an issue resolved with its form, and every page at a laptop's width and
//! at a phone's. Without a browser the tests print a notice and pass, unless
//! `NAVREE_TEST_REQUIRE_SERVICES=1` makes them fail.

use navree_browser::{Browser, Page, TestServer, Viewport};
use navree_lighthouse::IssueFilter;
use navree_session::SessionOptions;
use navree_testing::TestApp;
use serde_json::Value;

use crate::support::{PASSWORD, builder_with_session, flushed, user};

const EMAIL: &str = "gurpreet@example.in";

/// The dashboard on a real port, a browser, and the one person who uses it;
/// `None` (after a notice) without Chrome.
struct Site {
    app: TestApp,
    server: TestServer,
    browser: Browser,
}

impl Site {
    /// The site, with something to look at: two failing routes, and a person
    /// who has `permissions`.
    async fn start(test: &str, permissions: &[&str]) -> Option<Site> {
        if let Err(error) = Browser::find_chrome() {
            if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
                panic!("{test}: {error} (NAVREE_TEST_REQUIRE_SERVICES=1)");
            }
            eprintln!("skipped {test}: {error}");
            return None;
        }
        // The test server speaks plain HTTP: a session cookie without `Secure`.
        let app = builder_with_session(SessionOptions {
            secure: false,
            ..SessionOptions::default()
        })
        .build()
        .await;
        user(&app, EMAIL, Some("ops"), permissions).await;
        app.client()
            .get("/api/demo/boom/7")
            .send()
            .await
            .assert_status(500);
        app.client()
            .post("/api/demo/charge")
            .send()
            .await
            .assert_status(500);
        flushed(&app).await;
        let server = TestServer::start(app.ctx())
            .await
            .expect("the dashboard serves");
        let browser = Browser::launch().await.expect("the browser starts");
        Some(Site {
            app,
            server,
            browser,
        })
    }

    /// Signs the browser in on the app's sign-in page.
    async fn sign_in(&self) {
        let page = self
            .browser
            .visit(self.server.url("/sign-in?next=%2F_lighthouse"))
            .await
            .expect("the sign-in page opens");
        page.type_into("Email", EMAIL).await.unwrap();
        page.type_into("Password", PASSWORD).await.unwrap();
        page.press("Sign in").await.unwrap();
        page.assert_path_is("/_lighthouse").await.unwrap();
    }

    /// The page at `path`, at `viewport`, for a browser that is signed in.
    async fn open(&self, path: &str, viewport: Viewport) -> Page {
        let page = self
            .browser
            .visit_with(self.server.url(path), viewport)
            .await
            .expect("the page opens");
        page.assert_path_is(path.split('?').next().unwrap_or(path))
            .await
            .unwrap();
        page
    }
}

macro_rules! site_or_skip {
    ($name:expr, $permissions:expr) => {
        match Site::start($name, $permissions).await {
            Some(site) => site,
            None => return,
        }
    };
}

/// How wide the page is against the `width` of its window, and what sticks
/// out of the window or out of the card it sits in. (On a phone the browser
/// widens its window to the page, so the width is the one asked for.)
async fn overflow(page: &Page, width: u32) -> Value {
    page.script::<Value>(format!(
        "const w = {width};
         const out = [...document.querySelectorAll('body *')]
           .filter((e) => !e.closest('[data-slot=\"table-container\"]') && getComputedStyle(e).display !== 'none')
           .filter((e) => {{
             const right = e.getBoundingClientRect().right;
             const card = e.parentElement && e.parentElement.closest('[data-slot=\"card\"]');
             return right > w + 0.5 || (card && right > card.getBoundingClientRect().right + 0.5);
           }})
           .map((e) => e.tagName + '.' + (e.getAttribute('data-slot') || '') + ' right=' + Math.round(e.getBoundingClientRect().right));
         return {{ width: w, scroll: document.documentElement.scrollWidth, out: out.slice(0, 6) }};"
    ))
    .await
    .unwrap()
}

fn assert_fits(report: &Value, what: &str) {
    let width = report["width"].as_f64().unwrap();
    let scroll = report["scroll"].as_f64().unwrap();
    assert!(scroll <= width, "{what} scrolls sideways: {report}");
    assert!(
        report["out"].as_array().unwrap().is_empty(),
        "{what} sticks out of the window or its card: {report}"
    );
}

#[tokio::test]
async fn a_guest_signs_in_on_the_apps_page_and_comes_back_to_the_page_they_asked_for() {
    let site = site_or_skip!(
        "a_guest_signs_in_on_the_apps_page_and_comes_back_to_the_page_they_asked_for",
        &["lighthouse.view"]
    );
    let page = site
        .browser
        .visit(site.server.url("/_lighthouse/issues"))
        .await
        .unwrap();
    page.assert_see("Sign in").await.unwrap();
    page.assert_dont_see("invoice inv_7 not found").await.unwrap();
    page.press("Sign in").await.unwrap();
    page.assert_path_is("/sign-in").await.unwrap();
    page.assert_see("Acme CRM").await.unwrap();
    page.type_into("Email", EMAIL).await.unwrap();
    page.type_into("Password", PASSWORD).await.unwrap();
    page.press("Sign in").await.unwrap();
    page.assert_path_is("/_lighthouse/issues").await.unwrap();
    page.assert_see("invoice inv_7 not found").await.unwrap();
    page.assert_see("Lighthouse").await.unwrap();
    page.assert_see("Ada").await.unwrap();
    page.assert_no_console_errors().await.unwrap();
    site.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_issue_is_resolved_and_reopened_with_its_forms() {
    let site = site_or_skip!(
        "an_issue_is_resolved_and_reopened_with_its_forms",
        &["lighthouse.view", "lighthouse.resolve"]
    );
    site.sign_in().await;
    let store = flushed(&site.app).await;
    let issue = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .clone();
    let page = site
        .open(&format!("/_lighthouse/issues/{}", issue.id), Viewport::desktop())
        .await;
    page.assert_see("Resolve").await.unwrap();
    page.press("Resolve").await.unwrap();
    page.assert_see("Reopen").await.unwrap();
    assert_eq!(
        store.issue_detail(issue.id).await.unwrap().issue.status.as_str(),
        "resolved"
    );
    page.press("Reopen").await.unwrap();
    page.assert_see("Resolve").await.unwrap();
    assert_eq!(
        store.issue_detail(issue.id).await.unwrap().issue.status.as_str(),
        "open"
    );
    page.assert_no_console_errors().await.unwrap();
    site.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn every_page_fits_a_laptop_and_a_phone() {
    let site = site_or_skip!(
        "every_page_fits_a_laptop_and_a_phone",
        &[
            "lighthouse.view",
            "lighthouse.resolve",
            "lighthouse.configure"
        ]
    );
    site.sign_in().await;
    let store = flushed(&site.app).await;
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
    let paths = [
        "/_lighthouse".to_owned(),
        "/_lighthouse?period=7d".to_owned(),
        "/_lighthouse/issues".to_owned(),
        format!("/_lighthouse/issues/{}", issue.id),
        "/_lighthouse/requests".to_owned(),
        "/_lighthouse/requests/route?endpoint=GET%20%2Fapi%2Fdemo%2Fboom%2F%7Bn%7D&period=24h"
            .to_owned(),
        format!("/_lighthouse/traces/{sample}"),
        "/_lighthouse/queries".to_owned(),
        "/_lighthouse/jobs".to_owned(),
        "/_lighthouse/outgoing".to_owned(),
        "/_lighthouse/alerts".to_owned(),
        "/_lighthouse/releases".to_owned(),
        "/_lighthouse/settings".to_owned(),
    ];
    for (viewport, name) in [
        (Viewport::desktop(), "1440"),
        (Viewport::phone(), "390"),
        // The narrow phones: 360 pixels is a common Android width.
        (Viewport::new(360, 800), "360"),
    ] {
        let width = viewport.width();
        for path in &paths {
            let page = site.open(path, viewport.clone()).await;
            assert_fits(&overflow(&page, width).await, &format!("{path} at {name}"));
        }
    }
    site.app.shutdown().await.unwrap();
}
