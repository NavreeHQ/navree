//! `query_database` and `describe_schema` on a SQLite database, and every
//! refusal.

use serde_json::json;

use crate::support::{Fixture, call, refused, sqlite_db};

const APP_TOML: &str = "[database]\nurl = \"sqlite://storage/app.db?mode=rwc\"\n";

async fn crm_db(fixture: &Fixture) {
    let mut statements = vec![
        "CREATE TABLE companies (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE deals (id INTEGER PRIMARY KEY, title TEXT NOT NULL, stage TEXT NOT NULL DEFAULT 'lead', \
         amount_cents INTEGER, company_id INTEGER REFERENCES companies(id) ON DELETE CASCADE)",
        "CREATE INDEX deals_stage ON deals (stage)",
        "CREATE UNIQUE INDEX deals_title ON deals (title)",
        "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL, password_hash TEXT, remember_token TEXT)",
        "CREATE TABLE navree_migrations (module TEXT NOT NULL, name TEXT NOT NULL, batch INTEGER NOT NULL, \
         applied_at TEXT NOT NULL, PRIMARY KEY (module, name))",
        "INSERT INTO navree_migrations VALUES ('crm', 'm20261026_000001_create_deals', 1, '2026-10-26T10:00:00Z')",
        "INSERT INTO companies (id, name) VALUES (1, 'Agra')",
        "INSERT INTO users (email, password_hash, remember_token) VALUES ('anu@example.com', '$argon2id$v=19$x', 'tok')",
    ];
    let deals: Vec<String> = (1..=150)
        .map(|i| {
            let stage = ["lead", "negotiation", "won"][i % 3];
            format!(
                "INSERT INTO deals (title, stage, amount_cents, company_id) VALUES ('Deal {i}', '{stage}', {}, 1)",
                i * 1000
            )
        })
        .collect();
    statements.extend(deals.iter().map(String::as_str));
    sqlite_db(&fixture.root().join("storage/app.db"), &statements).await;
}

#[tokio::test]
async fn a_select_returns_columns_and_rows() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    let navigator = fixture.navigator();
    let answer = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT stage, count(*) AS deals, sum(amount_cents) AS total FROM deals GROUP BY stage ORDER BY stage"}),
    )
    .await;
    assert_eq!(answer["columns"], json!(["stage", "deals", "total"]));
    assert_eq!(answer["rows"][0], json!(["lead", 50, 3_825_000]));
    assert_eq!(answer["rows"].as_array().unwrap().len(), 3);
    assert_eq!(answer["truncated"], false);
    assert!(answer["duration_ms"].is_u64());
}

#[tokio::test]
async fn at_most_100_rows_come_back() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    let answer = call(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT id FROM deals"}),
    )
    .await;
    assert_eq!(answer["rows"].as_array().unwrap().len(), 100);
    assert_eq!(answer["truncated"], true);

    let none = call(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT id, title FROM deals WHERE id < 0"}),
    )
    .await;
    assert_eq!(none["rows"], json!([]));
    assert_eq!(
        none["columns"],
        json!(["id", "title"]),
        "columns of an empty result"
    );
}

#[tokio::test]
async fn secret_looking_columns_are_masked() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    let answer = call(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT * FROM users"}),
    )
    .await;
    assert_eq!(
        answer["columns"],
        json!(["id", "email", "password_hash", "remember_token"])
    );
    assert_eq!(
        answer["rows"][0],
        json!([1, "anu@example.com", "[redacted]", "[redacted]"])
    );
}

#[tokio::test]
async fn the_connection_itself_is_read_only() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    let answer = call(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT query_only FROM pragma_query_only"}),
    )
    .await;
    assert_eq!(answer["rows"], json!([[1]]));
}

#[tokio::test]
async fn writes_and_tricks_are_refused_before_they_are_sent() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    let navigator = fixture.navigator();
    for (sql, expected) in [
        ("DELETE FROM deals", "DELETE FROM is not read-only"),
        ("UPDATE deals SET stage = 'won'", "UPDATE is not read-only"),
        (
            "INSERT INTO deals (title) VALUES ('x')",
            "INSERT INTO is not read-only",
        ),
        ("DROP TABLE deals", "DROP TABLE is not read-only"),
        ("PRAGMA query_only = 0", "PRAGMA is not read-only"),
        ("PRAGMA query_only = OFF", "could not be parsed"),
        ("ATTACH DATABASE 'x.db' AS x", "is not read-only"),
        ("SELECT 1; DELETE FROM deals", "one statement at a time"),
        (
            "SELECT load_extension('evil')",
            "load_extension() is not allowed",
        ),
        ("   ", "no SQL statement"),
    ] {
        let message = refused(&navigator, "query_database", json!({ "sql": sql })).await;
        assert!(message.starts_with("refused: "), "{sql}: {message}");
        assert!(message.contains(expected), "{sql}: {message}");
    }
    let count = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT count(*) FROM deals"}),
    )
    .await;
    assert_eq!(count["rows"], json!([[150]]), "nothing changed");
}

/// Statements whose literals or types the SQL renderer writes back
/// differently from how they were read: sent as rendered, each would be
/// several statements, the later ones turning `query_only` off and writing.
#[tokio::test]
async fn statements_that_render_back_differently_are_refused() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    let navigator = fixture.navigator();
    for sql in [
        "SELECT X'00''; PRAGMA query_only = OFF; DROP TABLE users; --'",
        "SELECT CAST(1 AS x('1)); PRAGMA query_only=OFF; DROP TABLE users; SELECT CAST(1 AS x(1'))",
        "SELECT X'00''; ATTACH DATABASE ''storage/new.db'' AS n; CREATE TABLE n.t (a); --'",
    ] {
        let message = refused(&navigator, "query_database", json!({ "sql": sql })).await;
        assert!(message.starts_with("refused: "), "{sql}: {message}");
    }
    let users = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT count(*) FROM users"}),
    )
    .await;
    assert_eq!(users["rows"], json!([[1]]), "the table is still there");
    assert!(!fixture.root().join("storage/new.db").exists());
}

#[tokio::test]
async fn a_long_statement_is_cancelled_after_5_seconds() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    let started = std::time::Instant::now();
    let message = refused(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT max(x) FROM c"}),
    )
    .await;
    assert!(message.contains("longer than 5 seconds"), "{message}");
    assert!(started.elapsed() < std::time::Duration::from_secs(9));
}

#[tokio::test]
async fn remote_databases_are_refused_without_the_flag() {
    let fixture =
        Fixture::new("[database]\nurl = \"postgres://app:hunter2@db.staging.example.com/crm\"\n");
    let message = refused(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT 1"}),
    )
    .await;
    assert!(message.contains("db.staging.example.com"), "{message}");
    assert!(message.contains("--allow-remote-database"), "{message}");
    assert!(!message.contains("hunter2"), "{message}");
    let message = refused(&fixture.navigator(), "describe_schema", json!({})).await;
    assert!(message.contains("isn't on this machine"), "{message}");
}

#[tokio::test]
async fn a_missing_sqlite_file_is_never_created() {
    let fixture = Fixture::new(APP_TOML);
    let message = refused(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT 1"}),
    )
    .await;
    assert!(message.contains("doesn't exist yet"), "{message}");
    assert!(!fixture.root().join("storage/app.db").exists());
}

#[tokio::test]
async fn no_database_configured_says_how_to_configure_one() {
    let fixture = Fixture::new("");
    let message = refused(&fixture.navigator(), "describe_schema", json!({})).await;
    assert!(message.contains("database.url"), "{message}");
}

#[tokio::test]
async fn the_schema_has_tables_columns_indexes_and_foreign_keys() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    fixture.built();
    fixture.answers(
        &["migrate:status", "--json"],
        json!([
            {"module": "crm", "name": "m20261026_000001_create_deals", "applied": true, "batch": 1},
            {"module": "crm", "name": "m20261026_000004_add_expected_close_date", "applied": false, "batch": null}
        ]),
    );
    let answer = call(&fixture.navigator(), "describe_schema", json!({})).await;
    assert_eq!(answer["driver"], "sqlite");
    let names: Vec<&str> = answer["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["companies", "deals", "navree_migrations", "users"]);
    assert_eq!(
        answer["migrations"],
        json!({
            "applied": ["crm/m20261026_000001_create_deals"],
            "pending": ["crm/m20261026_000004_add_expected_close_date"],
        })
    );

    let deals = call(
        &fixture.navigator(),
        "describe_schema",
        json!({"table": "deals"}),
    )
    .await;
    let table = &deals["tables"][0];
    assert_eq!(deals["tables"].as_array().unwrap().len(), 1);
    assert_eq!(
        table["columns"][0],
        json!({"name": "id", "type": "INTEGER", "nullable": false, "default": null, "primary_key": true})
    );
    assert_eq!(
        table["columns"][2],
        json!({"name": "stage", "type": "TEXT", "nullable": false, "default": "'lead'", "primary_key": false})
    );
    assert_eq!(table["columns"][3]["nullable"], true);
    let indexes = table["indexes"].as_array().unwrap();
    assert!(
        indexes.contains(&json!({"name": "deals_stage", "columns": ["stage"], "unique": false}))
    );
    assert!(
        indexes.contains(&json!({"name": "deals_title", "columns": ["title"], "unique": true}))
    );
    assert_eq!(
        table["foreign_keys"],
        json!([{
            "columns": ["company_id"],
            "references_table": "companies",
            "references_columns": ["id"],
            "on_delete": "CASCADE",
            "on_update": "NO ACTION",
        }])
    );

    let message = refused(
        &fixture.navigator(),
        "describe_schema",
        json!({"table": "nope"}),
    )
    .await;
    assert!(message.contains("no table named nope"), "{message}");
}

#[tokio::test]
async fn pending_migrations_are_unknown_when_the_app_cannot_answer() {
    let fixture = Fixture::new(APP_TOML);
    crm_db(&fixture).await;
    let answer = call(&fixture.navigator(), "describe_schema", json!({})).await;
    assert_eq!(
        answer["migrations"]["applied"],
        json!(["crm/m20261026_000001_create_deals"])
    );
    assert!(
        answer["migrations"]["pending"]["unknown"].is_string(),
        "{}",
        answer["migrations"]
    );
}
