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
    "INSERT INTO companies (name) VALUES ('Acme')",
    "INSERT INTO deals (title, stage, amount_cents, company_id) \
     SELECT 'Deal ' || i, (ARRAY['lead', 'negotiation', 'won'])[1 + i % 3], i * 1000, 1 FROM generate_series(1, 150) i",
    "INSERT INTO users (email, password_hash) VALUES ('ada@example.com', '$argon2id$x')",
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
    assert_eq!(users["rows"], json!([["ada@example.com", "[redacted]"]]));

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
