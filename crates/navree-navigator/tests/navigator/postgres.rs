//! `query_database` and `describe_schema` on PostgreSQL.
//!
//! Opt-in: set `NAVREE_TEST_POSTGRES_URL` to a superuser URL of a throwaway
//! local server, for example
//! `postgres://postgres:postgres@127.0.0.1:55432/postgres`. Each test
//! creates its own database and drops it at the end. Without the variable
//! the tests print `SKIPPED` and pass, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.

use serde_json::json;
use sqlx::{AssertSqlSafe, Connection, Executor, PgConnection};

use crate::support::{Fixture, call, refused};

/// A database of its own on the test server, dropped by [`Scratch::drop_db`].
struct Scratch {
    admin: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn new(test: &str) -> Option<Scratch> {
        let Ok(admin) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: NAVREE_TEST_REQUIRE_SERVICES=1 but NAVREE_TEST_POSTGRES_URL is not set"
            );
            eprintln!("SKIPPED {test}: set NAVREE_TEST_POSTGRES_URL");
            return None;
        };
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!("navree_nav_{}_{nanos}", std::process::id());
        let mut conn = PgConnection::connect(&admin).await.expect("connect");
        conn.execute(AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .await
            .unwrap();
        conn.close().await.unwrap();
        let mut url = url::Url::parse(&admin).expect("a URL");
        url.set_path(&format!("/{name}"));
        Some(Scratch {
            admin,
            name,
            url: url.to_string(),
        })
    }

    async fn setup(&self, statements: &[&str]) {
        let mut conn = PgConnection::connect(&self.url).await.unwrap();
        for statement in statements {
            conn.execute(AssertSqlSafe(statement.to_string()))
                .await
                .expect(statement);
        }
        conn.close().await.unwrap();
    }

    async fn drop_db(self) {
        let mut conn = PgConnection::connect(&self.admin).await.unwrap();
        conn.execute(AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        )))
        .await
        .unwrap();
        conn.close().await.unwrap();
    }

    fn fixture(&self) -> Fixture {
        Fixture::new(&format!("[database]\nurl = \"{}\"\n", self.url))
    }
}

const SCHEMA: &[&str] = &[
    "CREATE TABLE companies (id bigserial PRIMARY KEY, name text NOT NULL)",
    "CREATE TABLE deals (id bigserial PRIMARY KEY, title text NOT NULL, stage text NOT NULL DEFAULT 'lead', \
     amount_cents bigint, company_id bigint REFERENCES companies(id) ON DELETE CASCADE)",
    "CREATE INDEX deals_stage ON deals (stage)",
    "CREATE TABLE users (id bigserial PRIMARY KEY, email text NOT NULL, password_hash text)",
    "CREATE TABLE navree_migrations (module text NOT NULL, name text NOT NULL, batch bigint NOT NULL, \
     applied_at timestamptz NOT NULL, PRIMARY KEY (module, name))",
    "INSERT INTO navree_migrations VALUES ('crm', 'm1_create_deals', 1, now())",
    "INSERT INTO companies (name) VALUES ('Agra')",
    "INSERT INTO deals (title, stage, amount_cents, company_id) \
     SELECT 'Deal ' || i, (ARRAY['lead', 'negotiation', 'won'])[1 + i % 3], i * 1000, 1 FROM generate_series(1, 150) i",
    "INSERT INTO users (email, password_hash) VALUES ('anu@example.com', '$argon2id$x')",
];

#[tokio::test]
async fn postgres_queries_run_read_only_with_typed_values() {
    let Some(db) = Scratch::new("postgres_queries_run_read_only_with_typed_values").await else {
        return;
    };
    db.setup(SCHEMA).await;
    let fixture = db.fixture();
    let navigator = fixture.navigator();

    let sums = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT stage, count(*) AS deals, sum(amount_cents) AS total FROM deals GROUP BY stage ORDER BY stage"}),
    )
    .await;
    assert_eq!(sums["columns"], json!(["stage", "deals", "total"]));
    assert_eq!(sums["rows"][0][1], 50, "{sums}");
    assert!(
        sums["rows"][0][2].is_i64(),
        "numeric sums are numbers: {sums}"
    );

    let many = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT id FROM deals ORDER BY id"}),
    )
    .await;
    assert_eq!(many["rows"].as_array().unwrap().len(), 100);
    assert_eq!(many["truncated"], true);

    let users = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT email, password_hash FROM users"}),
    )
    .await;
    assert_eq!(users["rows"], json!([["anu@example.com", "[redacted]"]]));

    let read_only = call(
        &navigator,
        "query_database",
        json!({"sql": "SHOW transaction_read_only"}),
    )
    .await;
    assert_eq!(read_only["rows"], json!([["on"]]));

    let message = refused(
        &navigator,
        "query_database",
        json!({"sql": "WITH d AS (DELETE FROM deals RETURNING id) SELECT * FROM d"}),
    )
    .await;
    assert!(message.contains("changes data"), "{message}");

    let slow = refused(
        &navigator,
        "query_database",
        json!({"sql": "SELECT count(*) FROM generate_series(1, 100000000000)"}),
    )
    .await;
    assert!(slow.contains("longer than 5 seconds"), "{slow}");

    let count = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT count(*) AS n FROM deals"}),
    )
    .await;
    assert_eq!(count["rows"], json!([[150]]));
    drop(navigator);
    db.drop_db().await;
}

#[tokio::test]
async fn postgres_schema_has_columns_indexes_and_foreign_keys() {
    let Some(db) = Scratch::new("postgres_schema_has_columns_indexes_and_foreign_keys").await
    else {
        return;
    };
    db.setup(SCHEMA).await;
    let fixture = db.fixture();
    let answer = call(
        &fixture.navigator(),
        "describe_schema",
        json!({"table": "deals"}),
    )
    .await;
    assert_eq!(answer["driver"], "postgres");
    let deals = &answer["tables"][0];
    assert_eq!(deals["name"], "deals");
    assert_eq!(deals["columns"][0]["name"], "id");
    assert_eq!(deals["columns"][0]["primary_key"], true);
    assert_eq!(deals["columns"][2]["default"], "'lead'::text");
    let indexes = deals["indexes"].as_array().unwrap();
    assert!(
        indexes
            .iter()
            .any(|i| i["name"] == "deals_stage" && i["columns"] == json!(["stage"])),
        "{indexes:?}"
    );
    assert!(
        indexes
            .iter()
            .any(|i| i["name"] == "deals_pkey" && i["unique"] == true),
        "{indexes:?}"
    );
    assert_eq!(
        deals["foreign_keys"],
        json!([{
            "columns": ["company_id"],
            "references_table": "companies",
            "references_columns": ["id"],
            "on_delete": "CASCADE",
            "on_update": "NO ACTION",
        }])
    );
    assert_eq!(
        answer["migrations"]["applied"],
        json!(["crm/m1_create_deals"])
    );
    db.drop_db().await;
}

/// The table asked for is picked from the catalog's list and never written
/// into the SQL, and the catalog names that are written into it stay string
/// literals, even where a backslash escapes a quote in a plain string
/// (`standard_conforming_strings = off`).
#[tokio::test]
async fn postgres_schema_table_names_never_become_sql() {
    let Some(db) = Scratch::new("postgres_schema_table_names_never_become_sql").await else {
        return;
    };
    db.setup(SCHEMA).await;
    db.setup(&[r#"CREATE TABLE "it\'s" (id bigint PRIMARY KEY)"#])
        .await;
    // New connections to this database read backslashes in plain strings as
    // escapes. A server that no longer accepts the setting still runs the
    // rest of the test with standard strings.
    let mut conn = PgConnection::connect(&db.url).await.unwrap();
    let off = conn
        .execute(AssertSqlSafe(format!(
            "ALTER DATABASE {} SET standard_conforming_strings = off",
            db.name
        )))
        .await;
    if let Err(error) = off {
        eprintln!("standard_conforming_strings stays on: {error}");
    }
    conn.close().await.unwrap();
    let fixture = db.fixture();
    let navigator = fixture.navigator();

    // Written into the SQL with quotes doubled only, this would close the
    // string and list every table.
    let hostile = refused(
        &navigator,
        "describe_schema",
        json!({"table": r"\' OR 1=1 --"}),
    )
    .await;
    assert!(hostile.contains("no table named"), "{hostile}");

    let odd = call(&navigator, "describe_schema", json!({"table": r"it\'s"})).await;
    assert_eq!(odd["tables"].as_array().map(Vec::len), Some(1), "{odd}");
    assert_eq!(odd["tables"][0]["name"], r"it\'s");
    assert_eq!(odd["tables"][0]["columns"][0]["name"], "id");
    assert_eq!(odd["tables"][0]["columns"][0]["primary_key"], true);

    let all = call(&navigator, "describe_schema", json!({})).await;
    assert_eq!(all["tables"].as_array().map(Vec::len), Some(5), "{all}");
    drop(navigator);
    db.drop_db().await;
}

/// Lighthouse's tables as its migration creates them on PostgreSQL.
const LIGHTHOUSE: &[&str] = &[
    "CREATE TABLE lighthouse_issues (id uuid PRIMARY KEY, fingerprint varchar NOT NULL UNIQUE, kind varchar NOT NULL, \
     level varchar NOT NULL, status varchar NOT NULL, title varchar NOT NULL, culprit varchar NOT NULL, \
     message text NOT NULL, chain text NOT NULL, location varchar, error_type varchar, count bigint NOT NULL, \
     first_seen timestamptz NOT NULL, last_seen timestamptz NOT NULL, first_release varchar, last_release varchar, \
     environment varchar, resolved_at timestamptz, resolved_in_release varchar, ignored_until timestamptz, \
     ignore_remaining bigint, regressed_at timestamptz, comment text, created_at timestamptz NOT NULL, \
     updated_at timestamptz NOT NULL)",
    "CREATE TABLE lighthouse_occurrences (id uuid PRIMARY KEY, issue_id uuid NOT NULL REFERENCES lighthouse_issues(id), \
     at timestamptz NOT NULL, level varchar NOT NULL, message text NOT NULL, chain text NOT NULL, backtrace text, \
     location varchar, request_id varchar, job_id varchar, trace_id varchar, user_id varchar, workspace_id varchar, \
     release varchar, environment varchar, server varchar, process varchar, method varchar, route varchar, \
     path varchar, status integer, sample_id uuid, context text NOT NULL)",
    "CREATE TABLE lighthouse_aggregates (id uuid PRIMARY KEY, resolution integer NOT NULL, bucket timestamptz NOT NULL, \
     kind varchar NOT NULL, key_hash varchar NOT NULL, key text NOT NULL, process varchar NOT NULL, count bigint NOT NULL, \
     errors bigint NOT NULL, extra bigint NOT NULL, total_ms double precision NOT NULL, max_ms double precision NOT NULL, \
     histogram text NOT NULL, detail text)",
    "CREATE TABLE lighthouse_alerts (id uuid PRIMARY KEY, issue_id uuid, kind varchar NOT NULL, channel varchar NOT NULL, \
     subject_key varchar NOT NULL, status varchar NOT NULL, attempts integer NOT NULL DEFAULT 0, error text, \
     payload text NOT NULL, created_at timestamptz NOT NULL, delivered_at timestamptz)",
    "INSERT INTO lighthouse_issues (id, fingerprint, kind, level, status, title, culprit, message, chain, count, \
     first_seen, last_seen, created_at, updated_at) VALUES ('0192f3a4-b5c6-7d8e-9f00-112233445566', 'f1', 'exception', \
     'error', 'open', 'Database connection failed', 'GET /api/v1/deals', 'Database connection failed', \
     '[\"Database connection failed\"]', 3, now() - interval '2 days', now() - interval '1 hour', now(), now())",
    "INSERT INTO lighthouse_occurrences (id, issue_id, at, level, message, chain, request_id, status, context) VALUES \
     (gen_random_uuid(), '0192f3a4-b5c6-7d8e-9f00-112233445566', now() - interval '1 hour', 'error', 'x', '[]', 'req-1', \
     500, '{\"user_agent\":\"curl\"}')",
    "INSERT INTO lighthouse_aggregates (id, resolution, bucket, kind, key_hash, key, process, count, errors, extra, \
     total_ms, max_ms, histogram) VALUES \
     (gen_random_uuid(), 3600, date_trunc('hour', now()), 'request', 'a', 'GET /api/v1/deals', 'serve-1', 4, 1, 0, 40, 25, '10:2,14:2'), \
     (gen_random_uuid(), 3600, date_trunc('hour', now()) - interval '2 hours', 'request', 'a', 'GET /api/v1/deals', 'serve-2', 4, 0, 0, 40, 25, '10:2,14:2'), \
     (gen_random_uuid(), 3600, date_trunc('hour', now()), 'job', 'b', 'SendInvoice', 'worker', 3, 2, 1, 30, 12, '12:3')",
];

#[tokio::test]
async fn postgres_lighthouse_tools_read_its_tables() {
    let Some(db) = Scratch::new("postgres_lighthouse_tools_read_its_tables").await else {
        return;
    };
    db.setup(LIGHTHOUSE).await;
    let fixture = db.fixture();
    crate::support::write(
        &fixture.root().join("src/app.rs"),
        ".module(LighthouseModule::from_config())",
    );
    let navigator = fixture.navigator();

    let issues = call(&navigator, "lighthouse_list_issues", json!({})).await;
    assert_eq!(
        issues["issues"][0]["id"],
        "0192f3a4-b5c6-7d8e-9f00-112233445566"
    );
    assert_eq!(issues["issues"][0]["count"], 3);

    let issue = call(
        &navigator,
        "lighthouse_get_issue",
        json!({"id": "0192f3a4-b5c6-7d8e-9f00-112233445566"}),
    )
    .await;
    assert_eq!(
        issue["issue"]["chain"],
        json!(["Database connection failed"])
    );
    assert_eq!(issue["occurrences"][0]["request_id"], "req-1");
    assert_eq!(issue["occurrences"][0]["context"]["user_agent"], "curl");

    let routes = call(&navigator, "lighthouse_slow_routes", json!({})).await;
    assert_eq!(routes["routes"][0]["endpoint"], "GET /api/v1/deals");
    assert_eq!(routes["routes"][0]["requests"], 8);
    assert_eq!(routes["routes"][0]["avg_ms"], 10.0);

    let jobs = call(&navigator, "lighthouse_failed_jobs", json!({})).await;
    assert_eq!(jobs["jobs"][0]["failed_jobs"], 1);
    drop(navigator);
    db.drop_db().await;
}
