//! The dashboard in a real browser (Chrome or Chromium, through
//! navree-browser): a guest signing in and coming back, the overview's
//! supervisors, queues and failed jobs, Retry pressed on a page, a viewer who
//! may not operate, and every page at a laptop's width and at a phone's.
//! Without a browser the tests print a notice and pass, unless
//! `NAVREE_TEST_REQUIRE_SERVICES=1` makes them fail.

use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use navree_access::{AbilityCtx, AccessModule, AccessOptions, AccessService, Rules};
use navree_auth::{AuthModule, AuthPages, AuthService};
use navree_browser::{Browser, Page, Target, TestServer, Viewport};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::Db;
use navree_fleet::testing::FakeSpawner;
use navree_fleet::{Master, MetricKind, MetricSnapshot};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::TestAppBuilder;
use navree_ui::UiModule;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::support::{Harness, SendDigest, builder, eventually, fail_exports, fast};

const PASSWORD: &str = "correct horse battery";
const EMAIL: &str = "harjinder@example.in";

/// Only the permissions stored for a user count.
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

/// The app of a team: sessions (without `Secure`, as the test server speaks
/// plain HTTP), sign-in pages, the toolkit and the dashboard.
fn dashboard_builder() -> TestAppBuilder {
    builder()
        .config("fleet.dashboard", true)
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule)
        .module(SessionModule::for_root(SessionOptions {
            secure: false,
            ..SessionOptions::default()
        }))
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(UiModule)
        .module(AuthPages::new().app_name("Agra Operations"))
}

/// The supervisors of the docs' example: `default` balances three queues
/// over up to eight processes, `exports` runs two.
fn supervisors() -> Value {
    json!({
        "default": { "queues": ["default", "mail", "notifications"], "max_processes": 8 },
        "exports": { "queues": ["exports"], "balance": "off", "max_processes": 2 }
    })
}

/// The dashboard on a real port, a browser, and the one person who uses it;
/// `None` (after a notice) without Chrome.
struct Site {
    h: Harness,
    server: TestServer,
    browser: Browser,
}

impl Site {
    /// The site, with a person who has `permissions`.
    async fn start(test: &str, builder: TestAppBuilder, permissions: &[&str]) -> Option<Site> {
        if let Err(error) = Browser::find_chrome() {
            if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
                panic!("{test}: {error} (NAVREE_TEST_REQUIRE_SERVICES=1)");
            }
            eprintln!("skipped {test}: {error}");
            return None;
        }
        let h = Harness::new(builder).await;
        let user = h
            .app
            .get::<AuthService>()
            .await
            .create_user("Harjinder Singh", EMAIL, PASSWORD)
            .await
            .expect("a user");
        let access = h.app.get::<AccessService>().await;
        let db = h.app.get::<Db>().await;
        let uow = db.uow(None, None).await;
        for permission in permissions {
            access
                .grant_to_user(&uow, user.id, permission)
                .await
                .expect("a permission");
        }
        uow.commit().await.expect("the permissions are stored");
        let server = TestServer::start(h.app.ctx())
            .await
            .expect("the dashboard serves");
        let browser = Browser::launch().await.expect("the browser starts");
        Some(Site { h, server, browser })
    }

    /// The page at `path`, at `viewport`: a browser that is not signed in is
    /// sent to the sign-in page, signs in, and comes back to it.
    async fn open(&self, path: &str, viewport: Viewport) -> Page {
        let next = path
            .replace('/', "%2F")
            .replace('?', "%3F")
            .replace('=', "%3D");
        let page = self
            .browser
            .visit_with(self.server.url(&format!("/sign-in?next={next}")), viewport)
            .await
            .expect("the page opens");
        // A browser that is signed in already is sent on to `next` at once.
        if page.url().await.expect("an address").contains("/sign-in") {
            page.type_into("Email", EMAIL).await.unwrap();
            page.type_into("Password", PASSWORD).await.unwrap();
            page.press("Sign in").await.unwrap();
        }
        page.assert_path_is(path.split('?').next().unwrap_or(path))
            .await
            .unwrap();
        page
    }
}

macro_rules! site_or_skip {
    ($name:expr, $builder:expr, $permissions:expr) => {
        match Site::start($name, $builder, $permissions).await {
            Some(site) => site,
            None => return,
        }
    };
}

/// A master with fake workers, running until stopped.
struct FakeMaster {
    stop: CancellationToken,
    task: tokio::task::JoinHandle<Result<navree_fleet::MasterSummary, navree_fleet::FleetError>>,
}

impl FakeMaster {
    async fn start(h: &Harness) -> FakeMaster {
        let master = Master::from_app(h.app.ctx())
            .await
            .expect("a master")
            .with_spawner(Arc::new(FakeSpawner::new()))
            .with_settings(fast());
        let stop = CancellationToken::new();
        let task = tokio::spawn(master.run(stop.clone()));
        FakeMaster { stop, task }
    }

    async fn stop(self) {
        self.stop.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.task).await;
    }
}

/// A snapshot of `queue` or a job type, `minutes_ago`.
async fn snapshot(h: &Harness, kind: MetricKind, name: &str, processed: u64, minutes_ago: i64) {
    h.fleet
        .record_snapshots(
            Some("m1"),
            &[MetricSnapshot {
                at: h.fleet.now() - TimeDelta::minutes(minutes_ago),
                kind,
                name: name.into(),
                processed,
                failed: 0,
                runtime_avg_ms: 20,
                runtime_p95_ms: 45,
                pending: None,
                wait_ms: None,
            }],
        )
        .await
        .unwrap();
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
async fn a_guest_signs_in_and_comes_back_to_the_page_they_asked_for() {
    let site = site_or_skip!(
        "a_guest_signs_in_and_comes_back_to_the_page_they_asked_for",
        dashboard_builder(),
        &["fleet.view"]
    );
    let page = site
        .browser
        .visit(site.server.url("/_fleet/failed"))
        .await
        .unwrap();
    page.assert_see("Sign in").await.unwrap();
    page.assert_dont_see("Failed jobs").await.unwrap();
    page.press("Sign in").await.unwrap();
    page.assert_path_is("/sign-in").await.unwrap();
    page.assert_see("Agra Operations").await.unwrap();
    page.type_into("Email", EMAIL).await.unwrap();
    page.type_into("Password", PASSWORD).await.unwrap();
    page.press("Sign in").await.unwrap();
    page.assert_path_is("/_fleet/failed").await.unwrap();
    page.assert_see("No failed jobs.").await.unwrap();
    page.assert_see("Harjinder Singh").await.unwrap();
    page.assert_no_console_errors().await.unwrap();
}

#[tokio::test]
async fn the_overview_shows_the_workload_and_retry_puts_a_job_back() {
    let site = site_or_skip!(
        "the_overview_shows_the_workload_and_retry_puts_a_job_back",
        dashboard_builder()
            .config("fleet.supervisors", supervisors())
            .config("fleet.balance_cooldown_secs", 1),
        &["fleet.view", "fleet.operate"]
    );
    let master = FakeMaster::start(&site.h).await;
    eventually("every queue has its process", || async {
        site.h
            .fleet
            .overview()
            .await
            .unwrap()
            .supervisors
            .iter()
            .map(|s| s.processes)
            .sum::<usize>()
            == 5
    })
    .await;
    snapshot(&site.h, MetricKind::Queue, "mail", 640, 3).await;
    snapshot(&site.h, MetricKind::Queue, "mail", 980, 2).await;
    for i in 0..3 {
        site.h
            .queue
            .dispatch(SendDigest {
                user: format!("u{i}"),
                api_token: "t".into(),
            })
            .await
            .unwrap();
    }
    fail_exports(&site.h, 3).await;

    let page = site.open("/_fleet", Viewport::desktop()).await;
    page.assert_see("1 master").await.unwrap();
    page.assert_see("worker-1").await.unwrap();
    // Each supervisor is a card of its queues, with the processes running.
    for text in [
        "default",
        "exports",
        "mail",
        "notifications",
        "of 8 processes",
    ] {
        page.assert_see(text).await.unwrap();
    }
    // The busiest queue is charted, and the chart carries its numbers.
    page.assert_see("Jobs per minute").await.unwrap();
    page.assert_see("mail: 1,620 jobs").await.unwrap();
    assert_eq!(
        page.script::<u64>(
            "return document.querySelectorAll('figure[data-slot=\"chart\"]').length;"
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        page.script::<u64>("return document.querySelectorAll('[style]').length;")
            .await
            .unwrap(),
        0,
        "no style attributes: the page's policy allows none"
    );

    // The newest failed jobs, with the count in the sidebar.
    page.assert_see("contacts.export").await.unwrap();
    page.assert_see("the mail server is down").await.unwrap();

    // Retry puts one back on its queue, and the page says so.
    page.press(Target::from("Retry").nth(0)).await.unwrap();
    page.assert_see("The job is back on its queue.")
        .await
        .unwrap();
    eventually("one failed job is retried", || async {
        site.h.fleet.failed_jobs(None).await.unwrap().len() == 2
    })
    .await;
    // Forget deletes one.
    page.press(Target::from("Forget").nth(0)).await.unwrap();
    eventually("one failed job is forgotten", || async {
        site.h.fleet.failed_jobs(None).await.unwrap().len() == 1
    })
    .await;

    // Pausing a supervisor from its card.
    page.press(Target::from("Pause").nth(0)).await.unwrap();
    eventually("the first supervisor is paused", || async {
        !site.h.fleet.paused().await.unwrap().is_empty()
    })
    .await;
    page.assert_see("Continue all").await.unwrap();
    page.assert_no_console_errors().await.unwrap();
    master.stop().await;
}

#[tokio::test]
async fn someone_who_may_only_look_cannot_retry() {
    let site = site_or_skip!(
        "someone_who_may_only_look_cannot_retry",
        dashboard_builder(),
        &["fleet.view"]
    );
    fail_exports(&site.h, 2).await;
    let page = site.open("/_fleet/failed", Viewport::desktop()).await;
    page.assert_see("contacts.export").await.unwrap();
    page.press(Target::from("Retry").nth(0)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(site.h.fleet.failed_jobs(None).await.unwrap().len(), 2);
    assert_eq!(site.h.queue.pending_count("exports").await.unwrap(), 0);
}

#[tokio::test]
async fn every_page_fits_a_laptop_and_a_phone_whatever_a_job_type_is_called() {
    let site = site_or_skip!(
        "every_page_fits_a_laptop_and_a_phone_whatever_a_job_type_is_called",
        dashboard_builder()
            .config("fleet.supervisors", supervisors())
            .config("fleet.balance_cooldown_secs", 1),
        &["fleet.view", "fleet.operate"]
    );
    let master = FakeMaster::start(&site.h).await;
    let long =
        "events.listener.workspace.member_added.navree_billing::seats::SyncSeatsAfterMemberAdded";
    for minutes_ago in [1, 2, 3, 4] {
        snapshot(&site.h, MetricKind::Job, long, 20, minutes_ago).await;
        snapshot(&site.h, MetricKind::Job, "mail.send", 5, minutes_ago).await;
        snapshot(&site.h, MetricKind::Queue, "mail", 25, minutes_ago).await;
    }
    fail_exports(&site.h, 3).await;
    eventually("processes", || async {
        site.h.fleet.overview().await.unwrap().supervisors[0].processes > 0
    })
    .await;
    let failed = site.h.fleet.failed_jobs(None).await.unwrap().remove(0).id;
    let pending = site
        .h
        .queue
        .dispatch(SendDigest {
            user: "u".into(),
            api_token: "t".into(),
        })
        .await
        .unwrap();

    let paths = [
        "/_fleet".to_owned(),
        "/_fleet/supervisors".to_owned(),
        "/_fleet/jobs".to_owned(),
        "/_fleet/jobs?tab=completed".to_owned(),
        format!("/_fleet/jobs/{pending}"),
        "/_fleet/failed".to_owned(),
        format!("/_fleet/failed/{failed}"),
        "/_fleet/metrics".to_owned(),
        "/_fleet/metrics?period=day".to_owned(),
    ];
    for (viewport, name) in [
        (Viewport::desktop(), "1440"),
        (Viewport::new(900, 800), "900"),
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
    master.stop().await;
}
