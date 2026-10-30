//! A database of its own per test: a fresh SQLite file, a fresh schema on
//! PostgreSQL, a fresh database on MySQL (dropped afterwards, also when the
//! test fails), and the fixture tables every case reads and writes.

use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures::FutureExt;
use navree_orm::{Dialect, Ulid};

use crate::exec::Exec;

/// The URL variable of a real server.
fn variable(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::Postgres => "NAVREE_TEST_POSTGRES_URL",
        Dialect::MySql => "NAVREE_TEST_MYSQL_URL",
        Dialect::Sqlite => "",
    }
}

/// `url` with `pair` added to its query string.
fn with_query(url: &str, pair: &str) -> String {
    if url.contains('?') {
        format!("{url}&{pair}")
    } else {
        format!("{url}?{pair}")
    }
}

/// `url` pointing at database `name` instead.
fn with_database(url: &str, name: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let authority_start = base.find("://").map_or(0, |at| at + 3);
    let server = match base[authority_start..].find('/') {
        Some(slash) => &base[..authority_start + slash],
        None => base,
    };
    match query {
        Some(query) => format!("{server}/{name}?{query}"),
        None => format!("{server}/{name}"),
    }
}

/// Runs `test` on a database of its own for `dialect`, with the fixture
/// tables created; skips (with a notice) a server whose URL is unset.
pub async fn run<F, Fut>(dialect: Dialect, test: F)
where
    F: FnOnce(Exec) -> Fut,
    Fut: Future<Output = ()>,
{
    if dialect == Dialect::Sqlite {
        let dir = navree_testing::TempDirGuard::new();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("differential.db").display()
        );
        let exec = Exec::connect(&url, dialect).await;
        exec.batch(&schema(dialect))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        test(exec.clone()).await;
        exec.close().await;
        return;
    }
    let Some(url) = navree_orm::testing::database::test_setting(variable(dialect)) else {
        eprintln!(
            "skipped: set {} to run the differential tests on {dialect}",
            variable(dialect)
        );
        return;
    };
    let admin = Exec::connect(&url, dialect).await;
    let name = format!("render_test_{}", Ulid::new().to_string().to_lowercase());
    let url = match dialect {
        Dialect::Postgres => {
            admin
                .batch(&format!("CREATE SCHEMA {name}"))
                .await
                .expect("creates the schema");
            with_query(&url, &format!("options=-c%20search_path%3D{name}%2Cpublic"))
        }
        _ => {
            admin
                .batch(&format!(
                    "CREATE DATABASE {name} CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci"
                ))
                .await
                .expect("creates the database");
            with_database(&url, &name)
        }
    };
    let exec = Exec::connect(&url, dialect).await;
    let outcome = AssertUnwindSafe(async {
        exec.batch(&schema(dialect))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        test(exec.clone()).await;
    })
    .catch_unwind()
    .await;
    exec.close().await;
    let drop = match dialect {
        Dialect::Postgres => format!("DROP SCHEMA {name} CASCADE"),
        _ => format!("DROP DATABASE {name}"),
    };
    if let Err(error) = admin.batch(&drop).await {
        eprintln!("could not drop {name}: {error}");
    }
    admin.close().await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// The fixture tables, `(name, order)`: every write case compares their
/// contents afterwards.
pub const TABLES: [(&str, &str); 9] = [
    ("companies", "id"),
    ("users", "id"),
    ("deals", "id"),
    ("tags", "id"),
    ("readings", "id"),
    ("archived_deals", "id"),
    ("regions", "id"),
    ("counters", "id"),
    ("scratch", "id"),
];

/// Empties the fixture tables, restarts their identities and puts their
/// rows back, so every run starts alike.
pub async fn reset(exec: &Exec) {
    let dialect = exec.dialect();
    let mut sql = String::new();
    match dialect {
        Dialect::Postgres => {
            let tables: Vec<&str> = TABLES.iter().map(|(table, _)| *table).collect();
            sql.push_str(&format!(
                "TRUNCATE TABLE {} RESTART IDENTITY;\n",
                tables.join(", ")
            ));
        }
        Dialect::MySql => {
            for (table, _) in TABLES {
                sql.push_str(&format!(
                    "DELETE FROM {table};\nALTER TABLE {table} AUTO_INCREMENT = 1;\n"
                ));
            }
        }
        Dialect::Sqlite => {
            for (table, _) in TABLES {
                sql.push_str(&format!("DELETE FROM {table};\n"));
            }
            sql.push_str("DELETE FROM sqlite_sequence;\n");
        }
    }
    sql.push_str(&seed(dialect));
    exec.batch(&sql)
        .await
        .unwrap_or_else(|error| panic!("resets the fixture: {error}"));
}

/// The fixture: its tables (dropped first) and rows, written for
/// `dialect`.
pub fn schema(dialect: Dialect) -> String {
    let mut sql = tables(dialect);
    sql.push_str(&seed(dialect));
    sql
}

/// The fixture's tables, dropped first.
fn tables(dialect: Dialect) -> String {
    let mut sql = String::new();
    for (table, _) in TABLES.iter().rev() {
        sql.push_str(&format!("DROP TABLE IF EXISTS {table};\n"));
    }
    let (id, text, key_text, int, boolean, json, at, real) = match dialect {
        Dialect::Postgres => (
            "BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY",
            "TEXT",
            "TEXT",
            "BIGINT",
            "BOOLEAN",
            "JSONB",
            "TIMESTAMPTZ",
            "DOUBLE PRECISION",
        ),
        Dialect::MySql => (
            "BIGINT AUTO_INCREMENT PRIMARY KEY",
            "TEXT",
            "VARCHAR(191)",
            "BIGINT",
            "BOOLEAN",
            "JSON",
            "DATETIME(6)",
            "DOUBLE",
        ),
        Dialect::Sqlite => (
            "INTEGER PRIMARY KEY AUTOINCREMENT",
            "TEXT",
            "TEXT",
            "INTEGER",
            "INTEGER",
            "TEXT",
            "TEXT",
            "REAL",
        ),
    };
    let fulltext = if dialect == Dialect::MySql {
        ", FULLTEXT INDEX deals_title (title), FULLTEXT INDEX deals_title_stage (title, stage)"
    } else {
        ""
    };
    sql.push_str(&format!(
        "CREATE TABLE companies (id {id}, name {key_text} NOT NULL, country {key_text} NULL, active {boolean} NOT NULL, region_id {int} NULL);\n\
         CREATE TABLE users (id {id}, name {key_text} NOT NULL, email {key_text} NULL, active {boolean} NOT NULL);\n\
         CREATE TABLE deals (id {id}, company_id {int} NULL, owner_id {int} NULL, title {key_text} NOT NULL, amount_cents {int} NOT NULL, stage {key_text} NOT NULL, tags {json} NULL, meta {json} NULL, closed_at {at} NULL, created_at {at} NOT NULL, updated_at {at} NOT NULL, deleted_at {at} NULL{fulltext});\n\
         CREATE TABLE tags (id {id}, name {key_text} NOT NULL UNIQUE, color {key_text} NULL, hits {int} NOT NULL, note {text} NULL, locked {boolean} NOT NULL);\n\
         CREATE TABLE readings (id {id}, taken_at {at} NOT NULL, celsius {real} NOT NULL);\n\
         CREATE TABLE archived_deals (id {int} PRIMARY KEY, title {key_text} NOT NULL);\n\
         CREATE TABLE regions (id {int} PRIMARY KEY, code {key_text} NOT NULL);\n\
         CREATE TABLE counters (id {id}, hits {int} NOT NULL DEFAULT 0);\n\
         CREATE TABLE scratch (id {id}, name {key_text} NOT NULL);\n"
    ));
    sql
}

/// The fixture's rows.
fn seed(dialect: Dialect) -> String {
    let mut sql = String::new();
    let b = |value: bool| match (dialect, value) {
        (Dialect::Sqlite, true) => "1",
        (Dialect::Sqlite, false) => "0",
        (_, true) => "TRUE",
        (_, false) => "FALSE",
    };
    let t = |text: &str| {
        let text = text.replace('\'', "''");
        // MySQL's strings read backslash escapes.
        if dialect == Dialect::MySql {
            format!("'{}'", text.replace('\\', "\\\\"))
        } else {
            format!("'{text}'")
        }
    };
    let at = |at: Option<&str>| match at {
        None => "NULL".to_owned(),
        Some(at) => match dialect {
            Dialect::Postgres => format!("'{at}+00'"),
            Dialect::MySql => format!("'{at}'"),
            Dialect::Sqlite => format!("'{}+00:00'", at.replace(' ', "T")),
        },
    };
    let j = |json: Option<&str>| match json {
        None => "NULL".to_owned(),
        Some(json) => t(json),
    };
    sql.push_str(&format!(
        "INSERT INTO companies (name, country, active, region_id) VALUES \
         ('Acme', 'US', {}, 1), ('Globex', 'DE', {}, 2), ('Initech', NULL, {}, NULL), ({}, 'FR', {}, 1);\n",
        b(true),
        b(true),
        b(false),
        t("Équipe Lyon"),
        b(true)
    ));
    sql.push_str(&format!(
        "INSERT INTO users (name, email, active) VALUES \
         ('Ada', 'ada@example.com', {}), ('Grace', NULL, {}), ('Linus', 'linus@example.com', {});\n",
        b(true),
        b(true),
        b(false)
    ));
    type Deal<'a> = (
        Option<i64>,
        Option<i64>,
        &'a str,
        i64,
        &'a str,
        Option<&'a str>,
        Option<&'a str>,
        Option<&'a str>,
        &'a str,
        Option<&'a str>,
    );
    let deals: [Deal<'_>; 8] = [
        (
            Some(1),
            Some(1),
            "Renewal Q1",
            120_000,
            "won",
            Some(r#"["vip","q1"]"#),
            Some(
                r#"{"owner":{"name":"ada","email":"ada@example.com"},"items":[3,4],"status":"open"}"#,
            ),
            Some("2026-10-20 09:15:00"),
            "2026-10-01 08:00:00",
            None,
        ),
        (
            Some(1),
            Some(2),
            "Upsell 50% off",
            4_500,
            "lead",
            Some(r#"["q2"]"#),
            Some(r#"{"owner":{"name":"grace"},"labels":["spam"]}"#),
            None,
            "2026-10-05 10:30:00",
            None,
        ),
        (
            Some(2),
            Some(1),
            "renewal q2",
            98_000,
            "lost",
            Some("[]"),
            Some(r#"{"items":[],"status":"closed"}"#),
            Some("2026-09-30 17:45:00"),
            "2026-09-01 12:00:00",
            None,
        ),
        (
            Some(2),
            None,
            "Draft deal",
            0,
            "lead",
            None,
            None,
            None,
            "2026-10-10 00:00:00",
            None,
        ),
        (
            Some(3),
            Some(3),
            "Équipe expansion",
            250_000,
            "won",
            Some(r#"["vip"]"#),
            Some(r#"{"owner":{"name":"linus"},"items":[1]}"#),
            Some("2026-10-27 08:30:00"),
            "2026-10-15 15:20:00",
            None,
        ),
        (
            None,
            Some(2),
            "Orphan_deal",
            1_000,
            "lost",
            Some(r#"["spam","q1"]"#),
            Some(r#"{"labels":["vip"]}"#),
            Some("2025-12-31 23:59:59"),
            "2025-12-01 11:11:11",
            None,
        ),
        (
            Some(4),
            Some(1),
            "Trashed deal",
            7_700,
            "won",
            Some(r#"["vip"]"#),
            Some("{}"),
            None,
            "2026-08-08 08:08:08",
            Some("2026-10-21 10:00:00"),
        ),
        (
            Some(1),
            Some(3),
            "Big renewal",
            990_000,
            "lead",
            Some(r#"["q1","q2","vip"]"#),
            Some(r#"{"owner":{"name":"ada"},"items":[3],"we\"ird":"x"}"#),
            None,
            "2026-10-26 18:00:00",
            None,
        ),
    ];
    let rows: Vec<String> = deals
        .iter()
        .map(
            |(company, owner, title, amount, stage, tags, meta, closed, created, deleted)| {
                let opt = |v: &Option<i64>| v.map_or("NULL".to_owned(), |v| v.to_string());
                format!(
                    "({}, {}, {}, {amount}, {}, {}, {}, {}, {}, {}, {})",
                    opt(company),
                    opt(owner),
                    t(title),
                    t(stage),
                    j(*tags),
                    j(*meta),
                    at(*closed),
                    at(Some(created)),
                    at(Some(closed.unwrap_or(created))),
                    at(*deleted)
                )
            },
        )
        .collect();
    sql.push_str(&format!(
        "INSERT INTO deals (company_id, owner_id, title, amount_cents, stage, tags, meta, closed_at, created_at, updated_at, deleted_at) VALUES {};\n",
        rows.join(", ")
    ));
    sql.push_str(&format!(
        "INSERT INTO tags (name, color, hits, note, locked) VALUES ('vip', 'gold', 3, NULL, {}), ('spam', 'grey', 0, 'muted', {});\n",
        b(false),
        b(true)
    ));
    let readings = [
        ("2026-10-27 08:05:00", "10.5"),
        ("2026-10-27 08:20:00", "11.0"),
        ("2026-10-27 08:50:00", "12.5"),
        ("2026-10-27 09:10:00", "13.0"),
        ("2026-10-26 23:55:00", "9.0"),
        ("2026-10-28 00:15:00", "8.0"),
    ];
    let rows: Vec<String> = readings
        .iter()
        .map(|(taken, celsius)| format!("({}, {celsius})", at(Some(taken))))
        .collect();
    sql.push_str(&format!(
        "INSERT INTO readings (taken_at, celsius) VALUES {};\n",
        rows.join(", ")
    ));
    sql.push_str("INSERT INTO archived_deals (id, title) VALUES (3, 'renewal q2');\n");
    sql.push_str("INSERT INTO regions (id, code) VALUES (1, 'NA'), (2, 'EU');\n");
    sql.push_str("INSERT INTO scratch (name) VALUES ('a'), ('b'), ('c');\n");
    sql
}

#[test]
fn urls_are_rewritten() {
    assert_eq!(
        with_database("mysql://root:pw@127.0.0.1:53306/navree_test", "x"),
        "mysql://root:pw@127.0.0.1:53306/x"
    );
    assert_eq!(with_query("postgres://h/db", "a=1"), "postgres://h/db?a=1");
}
