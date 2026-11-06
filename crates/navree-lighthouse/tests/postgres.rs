//! PostgreSQL, several processes: two apps record into the same database
//! at once (as a web process and a worker do), under bursts far larger
//! than their detail queues, and every count adds up exactly; a resolved
//! issue both of them hit again reopens once.
//!
//! Opt-in: build with `--features test-postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a superuser URL of a throwaway server, for
//! example `postgres://postgres:postgres@127.0.0.1:55432/postgres`. Each
//! test creates its own database and drops it at the end.
//!
//! The module's down migration runs under a writer that adds chunks to the
//! hypertables, as a flush does.
#![cfg(feature = "test-postgres")]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_db::{DatabaseModule, Db, DbOptions, Executor, Migrator, Pool, Statement};
use navree_lighthouse::{IssueFilter, IssueStatus, Lighthouse, LighthouseModule, Period, Report};
use navree_telemetry::{EventContext, RequestHandled, Telemetry, TelemetryEvent, TelemetryRecord};
use navree_testing::TestApp;
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
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "NAVREE_TEST_POSTGRES_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            eprintln!("NAVREE_TEST_POSTGRES_URL is not set: skipping the PostgreSQL test");
            return None;
        };
        let name = format!("navree_lighthouse_{}", Uuid::now_v7().simple());
        run_admin(&admin, &format!("CREATE DATABASE {name}")).await;
        let (base, _) = admin.rsplit_once('/').unwrap();
        let url = format!("{base}/{name}");
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        run_admin(
            &self.admin,
            &format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name),
        )
        .await;
    }
}

/// Runs `sql` on the server of `admin`.
async fn run_admin(admin: &str, sql: &str) {
    let pool = Pool::connect(&DbOptions {
        url: admin.to_owned(),
        max_connections: 1,
        ..DbOptions::default()
    })
    .await
    .unwrap();
    pool.execute_script(sql).await.unwrap();
    pool.close().await;
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

/// One flush as the recording thread makes it, in a transaction of its
/// own: an issue, an occurrence, a sample and an aggregate row, in that
/// order. The occurrence, the sample and the aggregate are `days` days
/// back, a day of their own in the hypertables: each adds a chunk.
async fn flush_a_day(db: &Db, n: u64, days: u64) -> Result<(), navree_db::DbError> {
    let tx = db.begin().await?;
    let statements = [
        format!(
            "INSERT INTO lighthouse_issues (id, fingerprint, kind, level, status, title, culprit, message, chain, count, first_seen, last_seen, created_at, updated_at) \
             VALUES (gen_random_uuid(), 'race-{n}', 'exception', 'error', 'open', 't', 'c', 'm', '[]', 1, now(), now(), now(), now())"
        ),
        format!(
            "INSERT INTO lighthouse_occurrences (id, issue_id, at, level, message, chain, context) \
             SELECT gen_random_uuid(), id, now() - interval '{days} days', 'error', 'm', '[]', '{{}}' FROM lighthouse_issues WHERE fingerprint = 'race-{n}'"
        ),
        format!(
            "INSERT INTO lighthouse_samples (id, kind, name, at, duration_ms, outcome, slow, spans, context) \
             VALUES (gen_random_uuid(), 'request', 'GET /', now() - interval '{days} days', 1.0, 'ok', false, '[]', '{{}}')"
        ),
        format!(
            "INSERT INTO lighthouse_aggregates (id, resolution, bucket, kind, key_hash, key, process, count, errors, extra, total_ms, max_ms, histogram) \
             VALUES (gen_random_uuid(), 60, now() - interval '{days} days', 'request', 'h', 'k', 'p', 1, 0, 0, 1.0, 1.0, '')"
        ),
    ];
    for sql in statements {
        tx.execute(&Statement::text(sql)).await?;
    }
    tx.commit().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_down_migration_survives_a_flush_adding_a_chunk() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = TestApp::builder()
        .config("database.url", scratch.url.clone())
        .config("database.migrate_on_boot", true)
        .config("lighthouse.enabled", false)
        .config("lighthouse.dashboard", false)
        .module(DatabaseModule::from_config())
        .module(LighthouseModule::from_config())
        .build()
        .await;
    let db = app.get::<Db>().await;
    for round in 0..8u64 {
        // A writer that flushes a new day every time (a chunk each), until
        // its tables are gone.
        let writer = {
            let db = db.clone();
            tokio::spawn(async move {
                let mut flushed = 0u64;
                for day in 0..10_000 {
                    if flush_a_day(&db, round * 10_000 + day, day % 300)
                        .await
                        .is_err()
                    {
                        break;
                    }
                    flushed += 1;
                }
                flushed
            })
        };
        // Let it get going, then roll the module back under it.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let reverted = Migrator::reset(app.ctx(), false)
            .await
            .unwrap_or_else(|error| panic!("round {round}: the rollback fails: {error}"));
        assert!(!reverted.migrations.is_empty());
        let flushed = writer.await.unwrap();
        assert!(flushed > 0, "round {round}: the writer was flushing");
        let tables = db
            .fetch_all(&Statement::text(
                "SELECT tablename FROM pg_tables WHERE tablename LIKE 'lighthouse_%'",
            ))
            .await
            .unwrap();
        assert!(tables.is_empty(), "round {round}: the tables are gone");
        Migrator::up(app.ctx()).await.unwrap();
    }
    app.shutdown().await.unwrap();
    scratch.drop().await;
}
