//! Lighthouse's five read tools against Lighthouse's tables in a SQLite
//! database, written the way Lighthouse writes them (UUIDs as blobs,
//! RFC 3339 timestamps).

use chrono::{DateTime, TimeDelta, Utc};
use navree_navigator::lighthouse::{Histogram, LIGHTHOUSE_TOOLS};
use serde_json::json;

use crate::support::{Fixture, call, refused, sqlite_db, write};

const APP_TOML: &str = "[database]\nurl = \"sqlite://storage/app.db?mode=rwc\"\n";

const APP_RS: &str = "pub fn app(sources: ConfigSources) -> AppBuilder {\n    Navree::app()\n        .module(DatabaseModule::from_config())\n        .module(LighthouseModule::from_config())\n}\n";

/// The tables as Lighthouse's migration creates them on SQLite.
const TABLES: &[&str] = &[
    "CREATE TABLE lighthouse_issues (id uuid_text NOT NULL PRIMARY KEY, fingerprint varchar NOT NULL UNIQUE, \
     kind varchar NOT NULL, level varchar NOT NULL, status varchar NOT NULL, title varchar NOT NULL, \
     culprit varchar NOT NULL, message text NOT NULL, chain text NOT NULL, location varchar NULL, \
     error_type varchar NULL, count bigint NOT NULL, first_seen timestamp_with_timezone_text NOT NULL, \
     last_seen timestamp_with_timezone_text NOT NULL, first_release varchar NULL, last_release varchar NULL, \
     environment varchar NULL, resolved_at timestamp_with_timezone_text NULL, resolved_in_release varchar NULL, \
     ignored_until timestamp_with_timezone_text NULL, ignore_remaining bigint NULL, \
     regressed_at timestamp_with_timezone_text NULL, comment text NULL, \
     created_at timestamp_with_timezone_text NOT NULL, updated_at timestamp_with_timezone_text NOT NULL)",
    "CREATE TABLE lighthouse_occurrences (id uuid_text NOT NULL PRIMARY KEY, issue_id uuid_text NOT NULL, \
     at timestamp_with_timezone_text NOT NULL, level varchar NOT NULL, message text NOT NULL, chain text NOT NULL, \
     backtrace text NULL, location varchar NULL, request_id varchar NULL, job_id varchar NULL, trace_id varchar NULL, \
     user_id varchar NULL, workspace_id varchar NULL, release varchar NULL, environment varchar NULL, \
     server varchar NULL, process varchar NULL, method varchar NULL, route varchar NULL, path varchar NULL, \
     status integer NULL, sample_id uuid_text NULL, context text NOT NULL)",
    "CREATE TABLE lighthouse_aggregates (id uuid_text NOT NULL PRIMARY KEY, resolution integer NOT NULL, \
     bucket timestamp_with_timezone_text NOT NULL, kind varchar NOT NULL, key_hash varchar NOT NULL, key text NOT NULL, \
     process varchar NOT NULL, count bigint NOT NULL, errors bigint NOT NULL, extra bigint NOT NULL, \
     total_ms real NOT NULL, max_ms real NOT NULL, histogram text NOT NULL, detail text NULL)",
    "CREATE TABLE lighthouse_alerts (id uuid_text NOT NULL PRIMARY KEY, issue_id uuid_text NULL, kind varchar NOT NULL, \
     channel varchar NOT NULL, subject_key varchar NOT NULL, status varchar NOT NULL, attempts integer NOT NULL DEFAULT 0, \
     error text NULL, payload text NOT NULL, created_at timestamp_with_timezone_text NOT NULL, \
     delivered_at timestamp_with_timezone_text NULL)",
];

const EXCEPTION: &str = "0192f3a4b5c67d8e9f00112233445566";
const SLOW_QUERY: &str = "0192f3a4b5c67d8e9f00112233445577";
const FAILED_JOB: &str = "0192f3a4b5c67d8e9f00112233445588";

/// A time as Lighthouse stores it (sqlx's RFC 3339 form).
fn ts(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

fn hours_ago(hours: i64) -> DateTime<Utc> {
    Utc::now() - TimeDelta::hours(hours)
}

/// The hour (or minute) `at` falls in, as Lighthouse buckets it.
fn bucket(at: DateTime<Utc>, seconds: i64) -> String {
    use chrono::DurationRound;
    ts(at.duration_trunc(TimeDelta::seconds(seconds)).unwrap())
}

fn issue(
    id: &str,
    kind: &str,
    status: &str,
    title: &str,
    culprit: &str,
    count: i64,
    last: i64,
) -> String {
    let first = ts(hours_ago(last + 48));
    let last = ts(hours_ago(last));
    format!(
        "INSERT INTO lighthouse_issues (id, fingerprint, kind, level, status, title, culprit, message, chain, location, \
         error_type, count, first_seen, last_seen, last_release, environment, created_at, updated_at) VALUES \
         (X'{id}', '{id}', '{kind}', 'error', '{status}', '{title}', '{culprit}', '{title}\nat line 2', \
         '[\"{title}\",\"connection refused\"]', 'src/deals.rs:42', 'DbErr', {count}, '{first}', '{last}', \
         '1.4.0', 'local', '{first}', '{last}')"
    )
}

/// One aggregate row: `durations` recorded into its histogram, with
/// `(errors, extra)`.
fn aggregate(
    resolution: i64,
    at: DateTime<Utc>,
    kind: &str,
    key: &str,
    process: &str,
    durations: &[f64],
    (errors, extra): (i64, i64),
) -> String {
    let mut histogram = Histogram::new();
    for ms in durations {
        histogram.record(*ms);
    }
    let total: f64 = durations.iter().sum();
    let max = durations.iter().cloned().fold(0.0, f64::max);
    let id = format!("{:032x}", rand_id());
    format!(
        "INSERT INTO lighthouse_aggregates (id, resolution, bucket, kind, key_hash, key, process, count, errors, extra, \
         total_ms, max_ms, histogram) VALUES (X'{id}', {resolution}, '{}', '{kind}', '{}', '{}', '{process}', {}, {errors}, \
         {extra}, {total}, {max}, '{}')",
        bucket(at, resolution),
        &format!(
            "{:016x}",
            key.len() * 7919 + key.bytes().map(usize::from).sum::<usize>()
        ),
        key.replace('\'', "''"),
        durations.len(),
        histogram.encode(),
    )
}

fn rand_id() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    u128::from(NEXT.fetch_add(1, Ordering::Relaxed)) << 64 | 0xabc
}

async fn lighthouse_db(fixture: &Fixture, file: &str) {
    write(&fixture.root().join("src/app.rs"), APP_RS);
    let mut statements: Vec<String> = TABLES.iter().map(|s| s.to_string()).collect();
    statements.push(issue(
        EXCEPTION,
        "exception",
        "open",
        "Database connection failed",
        "GET /api/v1/deals/{id}",
        12,
        1,
    ));
    statements.push(issue(
        SLOW_QUERY,
        "slow_query",
        "open",
        "Slow query: SELECT * FROM deals",
        "SELECT * FROM deals WHERE stage = ?",
        340,
        3,
    ));
    statements.push(issue(
        FAILED_JOB,
        "failed_job",
        "resolved",
        "SendInvoice failed",
        "SendInvoice",
        2,
        30,
    ));
    for (i, hours) in [(1, 1), (2, 5), (3, 2)] {
        statements.push(format!(
            "INSERT INTO lighthouse_occurrences (id, issue_id, at, level, message, chain, backtrace, request_id, method, \
             route, path, status, context) VALUES (X'{:032x}', X'{EXCEPTION}', '{}', 'error', 'Database connection failed', \
             '[\"Database connection failed\"]', 'deals.rs:42', 'req-{i}', 'GET', '/api/v1/deals/{{id}}', '/api/v1/deals/{i}', \
             500, '{{\"user_agent\":\"curl\"}}')",
            0x10 + i,
            ts(hours_ago(hours)),
        ));
    }
    statements.push(format!(
        "INSERT INTO lighthouse_alerts (id, issue_id, kind, channel, subject_key, status, attempts, payload, created_at, \
         delivered_at) VALUES (X'{:032x}', X'{EXCEPTION}', 'new_issue', 'slack', 'issue', 'delivered', 1, '{{}}', '{}', '{}')",
        0x99,
        ts(hours_ago(1)),
        ts(hours_ago(1)),
    ));
    // Requests: two endpoints, two processes, per-hour rows in the last day,
    // one row too old for 24h and per-minute rows for 1h.
    let now = Utc::now();
    let deals: Vec<f64> = (1..=100).map(f64::from).collect();
    statements.push(aggregate(
        3600,
        now,
        "request",
        "GET /api/v1/deals",
        "serve-1",
        &deals,
        (2, 0),
    ));
    statements.push(aggregate(
        3600,
        hours_ago(3),
        "request",
        "GET /api/v1/deals",
        "serve-2",
        &deals,
        (0, 0),
    ));
    statements.push(aggregate(
        3600,
        hours_ago(2),
        "request",
        "GET /api/v1/companies",
        "serve-1",
        &[5.0, 6.0, 7.0, 900.0],
        (0, 0),
    ));
    statements.push(aggregate(
        3600,
        hours_ago(30),
        "request",
        "GET /api/v1/old",
        "serve-1",
        &[5000.0],
        (0, 0),
    ));
    statements.push(aggregate(
        60,
        now,
        "request",
        "GET /api/v1/deals",
        "serve-1",
        &[10.0, 20.0],
        (0, 0),
    ));
    // Queries and jobs.
    statements.push(aggregate(
        3600,
        now,
        "query",
        "SELECT * FROM deals WHERE stage = ?",
        "serve-1",
        &[300.0, 400.0, 500.0],
        (0, 0),
    ));
    statements.push(aggregate(
        3600,
        now,
        "query",
        "SELECT 1",
        "serve-1",
        &[0.2; 50],
        (0, 0),
    ));
    statements.push(aggregate(
        3600,
        now,
        "job",
        "SendInvoice",
        "worker-1",
        &[100.0, 120.0, 140.0, 160.0],
        (3, 1),
    ));
    statements.push(aggregate(
        3600,
        now,
        "job",
        "SyncSearch",
        "worker-1",
        &[10.0, 12.0],
        (0, 0),
    ));
    let refs: Vec<&str> = statements.iter().map(String::as_str).collect();
    sqlite_db(&fixture.root().join(file), &refs).await;
}

fn hyphenated(simple: &str) -> String {
    format!(
        "{}-{}-{}-{}-{}",
        &simple[..8],
        &simple[8..12],
        &simple[12..16],
        &simple[16..20],
        &simple[20..]
    )
}

#[tokio::test]
async fn the_tools_are_listed_only_when_the_app_includes_lighthouse() {
    let fixture = Fixture::new(APP_TOML);
    let names = |fixture: &Fixture| -> Vec<&'static str> {
        fixture.navigator().tools().iter().map(|t| t.name).collect()
    };
    assert_eq!(names(&fixture).len(), 14);
    assert!(!names(&fixture).contains(&"lighthouse_list_issues"));
    let message = refused(&fixture.navigator(), "lighthouse_list_issues", json!({})).await;
    assert!(message.contains("LighthouseModule"), "{message}");

    write(&fixture.root().join("src/app.rs"), APP_RS);
    let names = names(&fixture);
    assert_eq!(names.len(), 19);
    assert_eq!(names[14..], LIGHTHOUSE_TOOLS);
}

#[tokio::test]
async fn issues_are_listed_most_recently_seen_first() {
    let fixture = Fixture::new(APP_TOML);
    lighthouse_db(&fixture, "storage/app.db").await;
    let navigator = fixture.navigator();

    let open = call(&navigator, "lighthouse_list_issues", json!({})).await;
    let issues = open["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 2, "open issues by default: {open}");
    assert_eq!(issues[0]["id"], hyphenated(EXCEPTION));
    assert_eq!(issues[0]["kind"], "exception");
    assert_eq!(issues[0]["title"], "Database connection failed");
    assert_eq!(issues[0]["culprit"], "GET /api/v1/deals/{id}");
    assert_eq!(issues[0]["count"], 12);
    assert!(issues[0]["last_seen"].as_str().unwrap().contains('T'));
    assert_eq!(issues[1]["id"], hyphenated(SLOW_QUERY));

    let all = call(
        &navigator,
        "lighthouse_list_issues",
        json!({"status": "all", "sort": "count"}),
    )
    .await;
    let counts: Vec<i64> = all["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["count"].as_i64().unwrap())
        .collect();
    assert_eq!(counts, [340, 12, 2]);

    let resolved = call(
        &navigator,
        "lighthouse_list_issues",
        json!({"status": "resolved"}),
    )
    .await;
    assert_eq!(resolved["issues"][0]["title"], "SendInvoice failed");

    let kind = call(
        &navigator,
        "lighthouse_list_issues",
        json!({"kind": "slow_query"}),
    )
    .await;
    assert_eq!(kind["issues"].as_array().unwrap().len(), 1);

    let search = call(
        &navigator,
        "lighthouse_list_issues",
        json!({"search": "CONNECTION", "limit": 1}),
    )
    .await;
    assert_eq!(search["issues"].as_array().unwrap().len(), 1);
    assert_eq!(search["issues"][0]["kind"], "exception");

    let quotes = call(
        &navigator,
        "lighthouse_list_issues",
        json!({"search": "it's 100%_"}),
    )
    .await;
    assert_eq!(
        quotes["issues"],
        json!([]),
        "quotes and wildcards are literal"
    );

    let bad = refused(
        &navigator,
        "lighthouse_list_issues",
        json!({"status": "closed"}),
    )
    .await;
    assert!(bad.contains("invalid arguments"), "{bad}");
}

#[tokio::test]
async fn one_issue_comes_with_its_latest_occurrences_and_alerts() {
    let fixture = Fixture::new(APP_TOML);
    lighthouse_db(&fixture, "storage/app.db").await;
    let navigator = fixture.navigator();

    for id in [hyphenated(EXCEPTION), EXCEPTION.to_uppercase()] {
        let answer = call(&navigator, "lighthouse_get_issue", json!({"id": id})).await;
        let issue = &answer["issue"];
        assert_eq!(issue["id"], hyphenated(EXCEPTION));
        assert_eq!(issue["message"], "Database connection failed\nat line 2");
        assert_eq!(
            issue["chain"],
            json!(["Database connection failed", "connection refused"])
        );
        assert_eq!(issue["location"], "src/deals.rs:42");
        assert_eq!(issue["error_type"], "DbErr");
        assert_eq!(issue["last_release"], "1.4.0");

        let occurrences = answer["occurrences"].as_array().unwrap();
        let requests: Vec<&str> = occurrences
            .iter()
            .map(|o| o["request_id"].as_str().unwrap())
            .collect();
        assert_eq!(requests, ["req-1", "req-3", "req-2"], "newest first");
        assert_eq!(occurrences[0]["status"], 500);
        assert_eq!(occurrences[0]["path"], "/api/v1/deals/1");
        assert_eq!(occurrences[0]["context"], json!({"user_agent": "curl"}));
        assert_eq!(occurrences[0]["backtrace"], "deals.rs:42");

        let alerts = answer["alerts"].as_array().unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0]["kind"], "new_issue");
        assert_eq!(alerts[0]["channel"], "slack");
        assert_eq!(alerts[0]["status"], "delivered");
    }

    let missing = refused(
        &navigator,
        "lighthouse_get_issue",
        json!({"id": hyphenated(&"1".repeat(32))}),
    )
    .await;
    assert!(missing.contains("no Lighthouse issue"), "{missing}");
    let invalid = refused(
        &navigator,
        "lighthouse_get_issue",
        json!({"id": "1' OR '1'='1"}),
    )
    .await;
    assert!(invalid.contains("not an issue id"), "{invalid}");
}

#[tokio::test]
async fn slow_routes_merge_processes_and_hours_slowest_first() {
    let fixture = Fixture::new(APP_TOML);
    lighthouse_db(&fixture, "storage/app.db").await;
    let navigator = fixture.navigator();

    let day = call(&navigator, "lighthouse_slow_routes", json!({})).await;
    assert_eq!(day["period"], "24h");
    let routes = day["routes"].as_array().unwrap();
    let endpoints: Vec<&str> = routes
        .iter()
        .map(|r| r["endpoint"].as_str().unwrap())
        .collect();
    assert_eq!(
        endpoints,
        ["GET /api/v1/companies", "GET /api/v1/deals"],
        "p95 first, the old row left out"
    );
    let deals = &routes[1];
    assert_eq!(deals["requests"], 200, "both processes and hours");
    assert_eq!(deals["errors"], 2);
    assert_eq!(deals["error_rate"], 0.01);
    assert_eq!(deals["avg_ms"], 50.5);
    assert_eq!(deals["max_ms"], 100.0);
    let p95 = deals["p95_ms"].as_f64().unwrap();
    assert!((95.0..=100.0).contains(&p95), "{p95}");
    let p50 = deals["p50_ms"].as_f64().unwrap();
    assert!((50.0..=62.5).contains(&p50), "{p50}");
    assert!(deals["p99_ms"].as_f64().unwrap() >= p95);

    let hour = call(
        &navigator,
        "lighthouse_slow_routes",
        json!({"period": "1h"}),
    )
    .await;
    let routes = hour["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 1, "per-minute rows for 1h: {hour}");
    assert_eq!(routes[0]["requests"], 2);

    let week = call(
        &navigator,
        "lighthouse_slow_routes",
        json!({"period": "7d", "limit": 1}),
    )
    .await;
    assert_eq!(week["routes"].as_array().unwrap().len(), 1);
    assert_eq!(week["routes"][0]["endpoint"], "GET /api/v1/old");

    let bad = refused(
        &navigator,
        "lighthouse_slow_routes",
        json!({"period": "2d"}),
    )
    .await;
    assert!(bad.contains("invalid arguments"), "{bad}");
}

#[tokio::test]
async fn slow_queries_by_total_time_and_failed_jobs() {
    let fixture = Fixture::new(APP_TOML);
    lighthouse_db(&fixture, "storage/app.db").await;
    let navigator = fixture.navigator();

    let queries = call(&navigator, "lighthouse_slow_queries", json!({})).await;
    let list = queries["queries"].as_array().unwrap();
    assert_eq!(list[0]["statement"], "SELECT * FROM deals WHERE stage = ?");
    assert_eq!(list[0]["count"], 3);
    assert_eq!(list[0]["total_ms"], 1200.0);
    assert_eq!(list[0]["avg_ms"], 400.0);
    assert_eq!(list[0]["max_ms"], 500.0);
    assert_eq!(list[1]["statement"], "SELECT 1");

    let jobs = call(&navigator, "lighthouse_failed_jobs", json!({})).await;
    let types = jobs["jobs"].as_array().unwrap();
    assert_eq!(types.len(), 1, "only job types with failures: {jobs}");
    assert_eq!(types[0]["job_type"], "SendInvoice");
    assert_eq!(types[0]["attempts"], 4);
    assert_eq!(types[0]["failed_attempts"], 3);
    assert_eq!(types[0]["failed_jobs"], 1);
    assert_eq!(
        jobs["issues"],
        json!([]),
        "the failed-job issue is resolved"
    );
}

#[tokio::test]
async fn lighthouse_database_url_is_read_when_set() {
    let fixture = Fixture::new(
        "[database]\nurl = \"sqlite://storage/app.db?mode=rwc\"\n[lighthouse]\ndatabase_url = \"sqlite://storage/lighthouse.db\"\n",
    );
    sqlite_db(
        &fixture.root().join("storage/app.db"),
        &["CREATE TABLE deals (id INTEGER PRIMARY KEY)"],
    )
    .await;
    lighthouse_db(&fixture, "storage/lighthouse.db").await;
    let answer = call(&fixture.navigator(), "lighthouse_list_issues", json!({})).await;
    assert_eq!(answer["issues"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn missing_tables_and_remote_databases_are_explained() {
    let fixture = Fixture::new(APP_TOML);
    write(&fixture.root().join("src/app.rs"), APP_RS);
    sqlite_db(
        &fixture.root().join("storage/app.db"),
        &["CREATE TABLE deals (id INTEGER PRIMARY KEY)"],
    )
    .await;
    let message = refused(&fixture.navigator(), "lighthouse_list_issues", json!({})).await;
    assert!(message.contains("navree migrate"), "{message}");

    let remote = Fixture::new(
        "[lighthouse]\ndatabase_url = \"postgres://monitor:pw@db.example.com/lighthouse\"\n",
    );
    write(&remote.root().join("src/app.rs"), APP_RS);
    let message = refused(&remote.navigator(), "lighthouse_slow_routes", json!({})).await;
    assert!(message.contains("--allow-remote-database"), "{message}");
}

/// Lighthouse's own statements are read whole, past the 4 KB an agent's
/// query gets per value: a histogram merged from thousands of rows, a long
/// backtrace and a long chain come back complete.
#[tokio::test]
async fn large_histograms_and_backtraces_are_read_whole() {
    let fixture = Fixture::new(APP_TOML);
    lighthouse_db(&fixture, "storage/app.db").await;
    const BIG: &str = "0192f3a4b5c67d8e9f00112233445599";
    let now = Utc::now();
    let mut statements: Vec<String> = Vec::new();
    // 1,500 fast rows, then 1,500 slow ones: cut after 4 KB, the merged
    // histogram would hold only fast ones.
    for ms in [10.0, 1000.0] {
        for _ in 0..1500 {
            statements.push(aggregate(
                3600,
                now,
                "request",
                "GET /api/v1/big",
                "serve-1",
                &[ms],
                (0, 0),
            ));
        }
    }
    let chain: Vec<String> = (0..300)
        .map(|i| format!("cause number {i:04} of many"))
        .collect();
    let chain = serde_json::to_string(&chain).unwrap();
    statements.push(format!(
        "INSERT INTO lighthouse_issues (id, fingerprint, kind, level, status, title, culprit, message, chain, \
         count, first_seen, last_seen, created_at, updated_at) VALUES (X'{BIG}', 'big', 'exception', 'error', \
         'open', 'Big', 'GET /big', 'Big', '{chain}', 1, '{now}', '{now}', '{now}', '{now}')",
        now = ts(now),
    ));
    let backtrace = "at deals::show (src/deals.rs:42)\n".repeat(400);
    statements.push(format!(
        "INSERT INTO lighthouse_occurrences (id, issue_id, at, level, message, chain, backtrace, request_id, \
         status, context) VALUES (X'{:032x}', X'{BIG}', '{}', 'error', 'Big', '[]', '{backtrace}', 'req-big', \
         500, '{{}}')",
        0x77,
        ts(now),
    ));
    let refs: Vec<&str> = statements.iter().map(String::as_str).collect();
    sqlite_db(&fixture.root().join("storage/app.db"), &refs).await;
    let navigator = fixture.navigator();

    let day = call(&navigator, "lighthouse_slow_routes", json!({"limit": 50})).await;
    let big = day["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| route["endpoint"] == "GET /api/v1/big")
        .unwrap_or_else(|| panic!("{day}"))
        .clone();
    assert_eq!(big["requests"], 3000);
    let p95 = big["p95_ms"].as_f64().unwrap();
    assert!(p95 >= 900.0, "the slow half is in the histogram: {big}");
    let p50 = big["p50_ms"].as_f64().unwrap();
    assert!(p50 <= 15.0, "{big}");

    let answer = call(&navigator, "lighthouse_get_issue", json!({"id": BIG})).await;
    assert_eq!(answer["issue"]["chain"].as_array().map(Vec::len), Some(300));
    assert_eq!(
        answer["occurrences"][0]["backtrace"].as_str().map(str::len),
        Some(backtrace.len())
    );
}
