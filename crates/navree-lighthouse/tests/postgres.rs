//! PostgreSQL, several processes: two apps record into the same database
//! at once (as a web process and a worker do), under bursts far larger
//! than their detail queues, and every count adds up exactly; a resolved
//! issue both of them hit again reopens once.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a superuser URL of a throwaway server, for
//! example `postgres://postgres:postgres@127.0.0.1:55432/postgres`. Each
//! test creates its own database and drops it at the end.
#![cfg(feature = "postgres")]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_db::DatabaseModule;
use navree_lighthouse::{IssueFilter, IssueStatus, Lighthouse, LighthouseModule, Period, Report};
use navree_telemetry::{EventContext, RequestHandled, Telemetry, TelemetryEvent, TelemetryRecord};
use navree_testing::TestApp;
use sea_orm::{ConnectionTrait, Database};
use uuid::Uuid;

/// A database of its own on the server of `NAVREE_TEST_POSTGRES_URL`.
struct Scratch {
    admin: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn new() -> Option<Self> {
        let Ok(admin) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
            eprintln!("NAVREE_TEST_POSTGRES_URL is not set: skipping the PostgreSQL test");
            return None;
        };
        let name = format!("navree_lighthouse_{}", Uuid::now_v7().simple());
        let conn = Database::connect(&admin).await.unwrap();
        conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        conn.close().await.unwrap();
        let (base, _) = admin.rsplit_once('/').unwrap();
        let url = format!("{base}/{name}");
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        let conn = Database::connect(&self.admin).await.unwrap();
        conn.execute_unprepared(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .await
        .unwrap();
    }
}

/// One process: the database and Lighthouse with a small detail queue.
async fn process(scratch: &Scratch, server: &str) -> TestApp {
    TestApp::builder()
        .config("database.url", scratch.url.clone())
        .config("database.migrate_on_boot", true)
        .config("lighthouse.server", server)
        .config("lighthouse.release", "r1")
        .config("lighthouse.buffer", 100)
        .config("lighthouse.dashboard", false)
        .config("lighthouse.sample.requests", 1.0)
        .module(DatabaseModule::from_config())
        .module(LighthouseModule::from_config())
        .build()
        .await
}

fn handled(id: &str, status: u16) -> TelemetryRecord {
    TelemetryRecord {
        at: Utc::now(),
        context: EventContext {
            request_id: Some(id.to_owned()),
            ..EventContext::default()
        },
        event: TelemetryEvent::RequestHandled(RequestHandled {
            method: "GET".into(),
            route: Some("/api/v1/deals/{id}".into()),
            path: format!("/api/v1/deals/{id}"),
            status,
            duration: Duration::from_millis(3),
            user_id: None,
            workspace_id: None,
            request_id: id.into(),
            client_ip: None,
        }),
    }
}

/// `n` requests from `threads` tasks at once, with a write every so often
/// (as the writer task does).
async fn burst(app: &TestApp, prefix: &str, threads: usize, n: usize) {
    let telemetry = Arc::new(Telemetry::from_app(app.ctx()).await);
    let lighthouse = app.get::<Lighthouse>().await;
    let mut tasks = Vec::new();
    for t in 0..threads {
        let telemetry = telemetry.clone();
        let prefix = prefix.to_owned();
        tasks.push(tokio::spawn(async move {
            for i in 0..n {
                let status = if i % 100 == 0 { 500 } else { 200 };
                telemetry.record(handled(&format!("{prefix}-{t}-{i}"), status));
                if i % 1000 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        }));
    }
    let writer = tokio::spawn(async move {
        for _ in 0..5 {
            lighthouse.flush().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    for task in tasks {
        task.await.unwrap();
    }
    writer.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn two_processes_count_every_request_and_reopen_a_resolved_issue_once() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let web = process(&scratch, "web-1").await;
    let worker = process(&scratch, "web-2").await;

    tokio::join!(burst(&web, "a", 8, 5_000), burst(&worker, "b", 8, 5_000));
    let (a, b) = (
        web.get::<Lighthouse>().await,
        worker.get::<Lighthouse>().await,
    );
    tokio::join!(a.flush(), b.flush());
    let store = a.store().unwrap().clone();
    let routes = store.routes(Period::Hour).await.unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].count, 80_000, "every request of both processes");
    assert_eq!(routes[0].server_errors, 800);
    let overview = store.overview(Period::Hour).await.unwrap();
    assert_eq!(overview.requests, 80_000);
    assert_eq!(
        overview.sampled_out,
        (a.sampled_out() + b.sampled_out()) as i64,
        "both processes' sampled-out details add up"
    );
    assert!(overview.sampled_out > 0);

    // One issue, hit by both, resolved, then hit by both at once.
    let report = || Report::message("ledger out of balance").fingerprint(["ledger"]);
    a.capture(report());
    b.capture(report());
    tokio::join!(a.flush(), b.flush());
    let issues = store.issues(&IssueFilter::default()).await.unwrap().data;
    assert_eq!(issues.len(), 1);
    let id = issues[0].issue.id;
    assert_eq!(issues[0].issue.count, 2);
    store.resolve(id, false, None).await.unwrap();
    for _ in 0..20 {
        a.capture(report());
        b.capture(report());
    }
    tokio::join!(a.flush(), b.flush());
    let issue = store.issue(id).await.unwrap();
    assert_eq!(issue.status, IssueStatus::Open);
    assert_eq!(issue.count, 42, "every hit of both processes");

    web.shutdown().await.unwrap();
    worker.shutdown().await.unwrap();
    scratch.drop().await;
}
