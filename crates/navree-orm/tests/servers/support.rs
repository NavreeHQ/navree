//! A database of its own per test on a real server: a fresh schema on
//! PostgreSQL, a fresh database on MySQL, dropped afterwards (also when the
//! test fails). Other test runs share the servers, so nothing is shared.

use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures::FutureExt;
use navree_db::Executor as _;
use navree_db::{Db, DbOptions};
use navree_orm::Ulid;

use crate::models::{MYSQL_SCHEMA, POSTGRES_SCHEMA};

/// A real database server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    Postgres,
    MySql,
}

impl Server {
    fn variable(self) -> &'static str {
        match self {
            Server::Postgres => "NAVREE_TEST_POSTGRES_URL",
            Server::MySql => "NAVREE_TEST_MYSQL_URL",
        }
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

/// Runs `test` with a database of its own on `server`; skips (with a
/// notice) when the server's URL variable is unset.
pub async fn run<F, Fut>(server: Server, test: F)
where
    F: FnOnce(Db) -> Fut,
    Fut: Future<Output = ()>,
{
    let Some(url) = navree_orm::testing::database::test_setting(server.variable()) else {
        navree_orm::testing::database::skip(&format!(
            "set {} to run this test against a real server",
            server.variable()
        ));
        return;
    };
    let admin = Db::connect(&DbOptions {
        url: url.clone(),
        max_connections: 2,
        ..DbOptions::default()
    })
    .await
    .unwrap_or_else(|error| panic!("cannot connect to {}: {error}", server.variable()));
    let name = format!("orm_test_{}", Ulid::new().to_string().to_lowercase());
    let (url, schema) = match server {
        Server::Postgres => {
            admin
                .pool()
                .execute_script(&format!("CREATE SCHEMA {name}"))
                .await
                .expect("creates the schema");
            (
                with_query(&url, &format!("options=-c%20search_path%3D{name}%2Cpublic")),
                POSTGRES_SCHEMA,
            )
        }
        Server::MySql => {
            // MySQL 8's collation, or on a MariaDB without it (10.6) the
            // accent- and case-insensitive `utf8mb4_unicode_520_ci`.
            let known: i64 = navree_db::ExecutorExt::fetch_scalar(
                admin.pool(),
                &navree_db::Statement::text(
                    "SELECT COUNT(*) FROM information_schema.COLLATIONS WHERE COLLATION_NAME = 'utf8mb4_0900_ai_ci'",
                ),
            )
            .await
            .expect("reads the server's collations");
            let collation = if known > 0 {
                "utf8mb4_0900_ai_ci"
            } else {
                "utf8mb4_unicode_520_ci"
            };
            admin
                .pool()
                .execute_script(&format!(
                    "CREATE DATABASE {name} CHARACTER SET utf8mb4 COLLATE {collation}"
                ))
                .await
                .expect("creates the database");
            (with_database(&url, &name), MYSQL_SCHEMA)
        }
    };
    let db = Db::connect(&DbOptions {
        url,
        ..DbOptions::default()
    })
    .await
    .expect("connects to the test database");
    for statement in schema.split(";\n").map(str::trim).filter(|s| !s.is_empty()) {
        db.pool()
            .execute_script(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

    let outcome = AssertUnwindSafe(test(db.clone())).catch_unwind().await;
    let _ = db.close().await;
    let drop = match server {
        Server::Postgres => format!("DROP SCHEMA {name} CASCADE"),
        Server::MySql => format!("DROP DATABASE {name}"),
    };
    if let Err(error) = admin.pool().execute_script(&drop).await {
        eprintln!("could not drop {name}: {error}");
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn urls_are_rewritten() {
    assert_eq!(
        with_database("mysql://root:pw@127.0.0.1:53306/navree_test", "x"),
        "mysql://root:pw@127.0.0.1:53306/x"
    );
    assert_eq!(
        with_database("mysql://root@localhost:3306/a?ssl-mode=disabled", "b"),
        "mysql://root@localhost:3306/b?ssl-mode=disabled"
    );
    assert_eq!(with_query("postgres://h/db", "a=1"), "postgres://h/db?a=1");
    assert_eq!(
        with_query("postgres://h/db?x=2", "a=1"),
        "postgres://h/db?x=2&a=1"
    );
}
