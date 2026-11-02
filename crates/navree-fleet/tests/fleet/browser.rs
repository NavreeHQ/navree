//! The dashboard in a real browser (Chrome or Chromium, through
//! navree-browser): the overview's supervisors, squares, throughput chart
//! and failed jobs, the page titles, the no-access screen and the widths at
//! a laptop and a phone. Without a browser the tests print a notice and
//! pass, unless `NAVREE_TEST_REQUIRE_SERVICES=1` makes them fail.

use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use http::request::Parts;
use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree_browser::{Browser, Page, Target, TestServer, Viewport};
use navree_contracts::{AuthVia, Principal};
use navree_fleet::testing::FakeSpawner;
use navree_fleet::{Master, MetricKind, MetricSnapshot};
use navree_http::{AppError, Authenticator};
use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
use navree_queue::WorkOptions;
use navree_testing::TestAppBuilder;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::support::{Export, Harness, SendDigest, builder, eventually, fast};

const VIEWER: Uuid = Uuid::from_u128(1);
const OPERATOR: Uuid = Uuid::from_u128(2);
const NOBODY: Uuid = Uuid::from_u128(3);

/// The access tokens of the test: `viewer` may look, `operator` may also
/// operate, `nobody` has no ability.
struct TokenAuth;

impl Authenticator for TokenAuth {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let token = parts
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned);
        Box::pin(async move {
            let user_id = match token.as_deref() {
                Some("viewer") => VIEWER,
                Some("operator") => OPERATOR,
                Some("nobody") => NOBODY,
                _ => return Ok(None),
            };
            Ok(Some(Principal {
                user_id,
                via: AuthVia::Token,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

struct TokenModule;

impl Module for TokenModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global()
            .factory(Factory::<dyn Authenticator>::new(|_| async {
                Ok(Arc::new(TokenAuth) as Arc<dyn Authenticator>)
            }))
            .exports::<dyn Authenticator>();
    }
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    match ctx.user.as_ref().map(|user| user.id) {
        Some(id) if id == VIEWER => {
            rules.can("view", "Fleet");
        }
        Some(id) if id == OPERATOR => {
            rules.can("view", "Fleet");
            rules.can("operate", "Fleet");
        }
        _ => {}
    }
}

fn dashboard_builder() -> TestAppBuilder {
    builder()
        .config("fleet.dashboard", true)
        .config("http.per_ip_rate_limit", Value::Null)
        .module(TokenModule)
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
}

/// The supervisors of the docs' example: `default` balances three queues
/// over up to eight processes, `exports` runs two.
fn supervisors() -> Value {
    json!({
        "default": { "queues": ["default", "mail", "notifications"], "max_processes": 8 },
        "exports": { "queues": ["exports"], "balance": "off", "max_processes": 2 }
    })
}

/// The dashboard on a real port, and a browser; `None` (after a notice)
/// without Chrome.
struct Site {
    h: Harness,
    server: TestServer,
    browser: Browser,
}

impl Site {
    async fn start(test: &str, builder: TestAppBuilder) -> Option<Site> {
        if let Err(error) = Browser::find_chrome() {
            if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
                panic!("{test}: {error} (NAVREE_TEST_REQUIRE_SERVICES=1)");
            }
            eprintln!("skipped {test}: {error}");
            return None;
        }
        let h = Harness::new(builder).await;
        let server = TestServer::start(h.app.ctx())
            .await
            .expect("the dashboard serves");
        let browser = Browser::launch().await.expect("the browser starts");
        Some(Site { h, server, browser })
    }

    /// The dashboard's page `hash` ("#metrics"), signed in with `token`,
    /// at `viewport`.
    async fn open(&self, token: &str, hash: &str, viewport: Viewport) -> Page {
        let page = self
            .browser
            .visit_with(self.server.url("/_fleet"), viewport)
            .await
            .expect("the page opens");
        let session = json!({ "token": token, "name": token }).to_string();
        page.script::<Value>(format!(
            "sessionStorage.setItem('navree.fleet', {}); location.hash = {}; return null;",
            json!(session),
            json!(hash)
        ))
        .await
        .unwrap();
        page.refresh().await.unwrap();
        page
    }
}

macro_rules! site_or_skip {
    ($name:expr, $builder:expr) => {
        match Site::start($name, $builder).await {
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

async fn fail_exports(h: &Harness, count: usize) {
    for i in 0..count {
        h.queue
            .dispatch(Export {
                contact: format!("contact-{i}"),
                password: "hunter2".into(),
            })
            .await
            .unwrap();
    }
    h.queue
        .work(WorkOptions {
            queues: vec!["exports".into()],
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();
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
           .filter((e) => !e.closest('.table-wrap') && getComputedStyle(e).display !== 'none')
           .filter((e) => {{
             const right = e.getBoundingClientRect().right;
             const card = e.parentElement && e.parentElement.closest('.card');
             return right > w + 0.5 || (card && right > card.getBoundingClientRect().right + 0.5);
           }})
           .map((e) => e.tagName + '.' + (e.getAttribute('class') || '') + ' right=' + Math.round(e.getBoundingClientRect().right));
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
async fn the_overview_groups_queues_by_supervisor_with_a_square_per_process() {
    let site = site_or_skip!(
        "the_overview_groups_queues_by_supervisor_with_a_square_per_process",
        dashboard_builder()
            .config("fleet.supervisors", supervisors())
            .config("fleet.balance_cooldown_secs", 1)
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

    let page = site
        .open("operator", "#overview", Viewport::desktop())
        .await;
    page.wait_for(".workload").await.unwrap();
    page.assert_title("Overview \u{b7} Fleet").await.unwrap();
    let groups: Value = page
        .script(
            "return [...document.querySelectorAll('.workload tbody')].map((b) => ({
               group: b.querySelector('tr.group').innerText.replace(/\\s+/g, ' ').trim(),
               rows: [...b.querySelectorAll('tr:not(.group)')].map((r) => ({
                 queue: r.children[0].innerText,
                 on: r.querySelectorAll('.slot.on').length,
                 slots: r.querySelectorAll('.slot').length,
               })),
             }));",
        )
        .await
        .unwrap();
    assert_eq!(
        groups,
        json!([
            {
                "group": "default \u{b7} auto \u{b7} 1-8 processes",
                "rows": [
                    { "queue": "default", "on": 1, "slots": 8 },
                    { "queue": "mail", "on": 1, "slots": 8 },
                    { "queue": "notifications", "on": 1, "slots": 8 },
                ]
            },
            {
                "group": "exports \u{b7} off \u{b7} 2 processes",
                "rows": [{ "queue": "exports", "on": 2, "slots": 2 }]
            }
        ])
    );
    page.assert_see("1 master \u{b7} worker-1").await.unwrap();

    // A backlog on `mail` moves processes to it: its squares fill.
    for i in 0..50 {
        site.h
            .queue
            .dispatch(SendDigest {
                user: format!("u{i}"),
                api_token: "t".into(),
            })
            .await
            .unwrap();
    }
    eventually("six mail processes", || async {
        site.h
            .fleet
            .overview()
            .await
            .unwrap()
            .workload
            .iter()
            .any(|w| w.queue == "mail" && w.processes == 6)
    })
    .await;
    page.refresh().await.unwrap();
    page.wait_for(".workload").await.unwrap();
    let mail: u64 = page
        .script(
            "const row = [...document.querySelectorAll('.workload tr')].find((r) => r.children[0].innerText === 'mail');
             return row.querySelectorAll('.slot.on').length;",
        )
        .await
        .unwrap();
    assert_eq!(mail, 6);
    page.assert_no_console_errors().await.unwrap();
    master.stop().await;
}

#[tokio::test]
async fn the_overview_charts_jobs_per_minute_and_lists_failed_jobs() {
    let site = site_or_skip!(
        "the_overview_charts_jobs_per_minute_and_lists_failed_jobs",
        dashboard_builder().config("fleet.supervisors", supervisors())
    );
    snapshot(&site.h, MetricKind::Queue, "mail", 640, 3).await;
    snapshot(&site.h, MetricKind::Queue, "mail", 980, 2).await;
    snapshot(&site.h, MetricKind::Queue, "default", 12, 2).await;
    fail_exports(&site.h, 7).await;

    let page = site
        .open("operator", "#overview", Viewport::desktop())
        .await;
    page.wait_for(".chart-card .bars").await.unwrap();
    let chart: Value = page
        .script(
            "return {
               queue: document.querySelector('.chart-card select').value,
               bars: document.querySelectorAll('.chart-card rect.bar').length,
               filled: document.querySelectorAll('.chart-card rect.bar:not(.zero)').length,
               foot: document.querySelector('.chart-foot').innerText.replace(/\\s+/g, ' ').trim(),
             };",
        )
        .await
        .unwrap();
    assert_eq!(chart["queue"], "mail", "the busiest queue is shown first");
    assert_eq!(chart["bars"], 30);
    assert_eq!(chart["filled"], 2);
    assert!(
        chart["foot"]
            .as_str()
            .unwrap()
            .contains("1,620 jobs, peak 980/min"),
        "{chart}"
    );
    page.select(".chart-card select", "default").await.unwrap();
    page.wait_for(".chart-foot").await.unwrap();
    page.assert_see("12 jobs, peak 12/min").await.unwrap();

    // The five newest failed jobs, each with its attempts and its error.
    let failures: Value = page
        .script(
            "return [...document.querySelectorAll('.failure')].map((r) => r.innerText.replace(/\\s+/g, ' ').trim());",
        )
        .await
        .unwrap();
    let failures = failures.as_array().unwrap();
    assert_eq!(failures.len(), 5, "{failures:?}");
    assert!(
        failures[0]
            .as_str()
            .unwrap()
            .starts_with("contacts.export 1/1 the mail server is down Retry Forget"),
        "{failures:?}"
    );
    page.assert_see("View all 7 failed jobs").await.unwrap();
    assert_eq!(page.text("#count-failed").await.unwrap(), "7");

    // Retry puts one back on its queue; Forget deletes one.
    page.press(Target::from("Retry").nth(0)).await.unwrap();
    page.assert_see("The job is back on its queue.")
        .await
        .unwrap();
    eventually("one failed job is retried", || async {
        site.h.fleet.failed_jobs(None).await.unwrap().len() == 6
    })
    .await;
    page.press(Target::from("Forget").nth(0)).await.unwrap();
    eventually("one failed job is forgotten", || async {
        site.h.fleet.failed_jobs(None).await.unwrap().len() == 5
    })
    .await;
    page.assert_see("View all 5 failed jobs").await.ok();
    assert_eq!(
        page.script::<u64>("return document.querySelectorAll('[style]').length;")
            .await
            .unwrap(),
        0,
        "no style attributes: the page's policy allows none"
    );
    page.assert_no_console_errors().await.unwrap();
}

#[tokio::test]
async fn a_viewer_cannot_retry_and_an_empty_fleet_says_so() {
    let site = site_or_skip!(
        "a_viewer_cannot_retry_and_an_empty_fleet_says_so",
        dashboard_builder()
    );
    let page = site.open("viewer", "#overview", Viewport::desktop()).await;
    page.wait_for(".failed-card").await.unwrap();
    page.assert_see("No failed jobs.").await.unwrap();
    page.assert_see("No master running").await.unwrap();
    page.assert_see("No jobs finished").await.unwrap();
    page.assert_dont_see("Retry all").await.unwrap();

    fail_exports(&site.h, 2).await;
    page.refresh().await.unwrap();
    page.wait_for(".failure").await.unwrap();
    page.press(Target::from("Retry").nth(0)).await.unwrap();
    page.assert_see("This needs the fleet.operate permission.")
        .await
        .unwrap();
    assert_eq!(site.h.fleet.failed_jobs(None).await.unwrap().len(), 2);
}

#[tokio::test]
async fn every_view_is_titled_by_its_page() {
    let site = site_or_skip!(
        "every_view_is_titled_by_its_page",
        dashboard_builder().config("fleet.supervisors", supervisors())
    );
    let ids = {
        fail_exports(&site.h, 1).await;
        site.h.fleet.failed_jobs(None).await.unwrap()
    };
    let page = site
        .browser
        .visit(site.server.url("/_fleet"))
        .await
        .unwrap();
    page.assert_title("Sign in \u{b7} Fleet").await.unwrap();

    let page = site
        .open("operator", "#overview", Viewport::desktop())
        .await;
    for (hash, title) in [
        ("#overview", "Overview \u{b7} Fleet"),
        ("#supervisors", "Supervisors \u{b7} Fleet"),
        ("#jobs", "Jobs \u{b7} Fleet"),
        ("#failed", "Failed jobs \u{b7} Fleet"),
        ("#metrics", "Metrics \u{b7} Fleet"),
    ] {
        page.script::<Value>(format!("location.hash = '{hash}'; return null;"))
            .await
            .unwrap();
        page.assert_title(title).await.unwrap();
    }
    let id = &ids[0].id;
    page.script::<Value>(format!("location.hash = '#failed/{id}'; return null;"))
        .await
        .unwrap();
    page.assert_title("contacts.export \u{b7} Failed jobs \u{b7} Fleet")
        .await
        .unwrap();
}

#[tokio::test]
async fn an_account_without_access_sees_only_the_message() {
    let site = site_or_skip!(
        "an_account_without_access_sees_only_the_message",
        dashboard_builder()
    );
    for viewport in [Viewport::desktop(), Viewport::phone()] {
        let page = site.open("nobody", "#overview", viewport).await;
        page.wait_for(".message-card").await.unwrap();
        page.assert_title("No access \u{b7} Fleet").await.unwrap();
        page.assert_see("it needs the fleet.view permission")
            .await
            .unwrap();
        page.assert_dont_see("Loading").await.unwrap();
        let shown: Value = page
            .script(
                "const visible = (e) => e.getClientRects().length > 0;
                 return {
                   nav: [...document.querySelectorAll('.nav-item')].filter(visible).length,
                   status: visible(document.querySelector('#fleet-status')),
                   text: document.body.innerText.includes('Loading'),
                 };",
            )
            .await
            .unwrap();
        assert_eq!(shown, json!({ "nav": 0, "status": false, "text": false }));
    }
}

#[tokio::test]
async fn the_metrics_page_fits_its_window_whatever_a_job_type_is_called() {
    let site = site_or_skip!(
        "the_metrics_page_fits_its_window_whatever_a_job_type_is_called",
        dashboard_builder().config("fleet.supervisors", supervisors())
    );
    let long =
        "events.listener.workspace.member_added.navree_billing::seats::SyncSeatsAfterMemberAdded";
    for minutes_ago in [1, 2, 3, 4] {
        snapshot(&site.h, MetricKind::Job, long, 20, minutes_ago).await;
        snapshot(&site.h, MetricKind::Job, "mail.send", 5, minutes_ago).await;
        snapshot(&site.h, MetricKind::Queue, "mail", 25, minutes_ago).await;
    }
    for (viewport, name) in [(Viewport::desktop(), "1440"), (Viewport::phone(), "390")] {
        let width = viewport.width();
        let page = site.open("operator", "#metrics", viewport).await;
        page.wait_for(".charts").await.unwrap();
        page.assert_see("SyncSeatsAfterMemberAdded").await.ok();
        let report = overflow(&page, width).await;
        assert_fits(&report, &format!("Metrics at {name}"));
    }
}

#[tokio::test]
async fn the_overview_fits_a_laptop_and_a_phone_and_the_sidebar_becomes_a_rail() {
    let site = site_or_skip!(
        "the_overview_fits_a_laptop_and_a_phone_and_the_sidebar_becomes_a_rail",
        dashboard_builder()
            .config("fleet.supervisors", supervisors())
            .config("fleet.balance_cooldown_secs", 1)
    );
    let master = FakeMaster::start(&site.h).await;
    snapshot(&site.h, MetricKind::Queue, "mail", 30, 2).await;
    fail_exports(&site.h, 3).await;
    eventually("processes", || async {
        site.h.fleet.overview().await.unwrap().supervisors[0].processes > 0
    })
    .await;

    let collapsed = "return document.querySelector('#shell').classList.contains('collapsed');";
    for (viewport, name, rail) in [
        (Viewport::desktop(), "1440", false),
        (Viewport::new(900, 800), "900", true),
        (Viewport::phone(), "390", false),
    ] {
        let width = viewport.width();
        let page = site.open("operator", "#overview", viewport).await;
        page.wait_for(".failure").await.unwrap();
        assert_fits(
            &overflow(&page, width).await,
            &format!("Overview at {name}"),
        );
        assert_eq!(
            page.script::<bool>(collapsed).await.unwrap(),
            rail,
            "the icon rail at {name}"
        );
    }
    master.stop().await;
}
