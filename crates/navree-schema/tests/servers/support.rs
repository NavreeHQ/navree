//! Throwaway databases on the test servers.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use navree_schema::SchemaRow;

#[path = "../common/conn.rs"]
mod conn;

#[allow(unused_imports)]
pub use conn::{TestConn, TestTx};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A database created for one test, on the server of an environment
/// variable; [`drop`](ScratchDb::drop) removes it.
pub struct ScratchDb {
    admin: String,
    pub name: String,
    pub url: String,
    mysql: bool,
}

impl ScratchDb {
    /// A fresh database on `NAVREE_TEST_POSTGRES_URL`, or `None` (with a
    /// notice) when it is unset.
    pub async fn postgres() -> Option<ScratchDb> {
        ScratchDb::create("NAVREE_TEST_POSTGRES_URL", false).await
    }

    /// A fresh database on `NAVREE_TEST_MYSQL_URL`, or `None`.
    pub async fn mysql() -> Option<ScratchDb> {
        ScratchDb::create("NAVREE_TEST_MYSQL_URL", true).await
    }

    async fn create(variable: &str, mysql: bool) -> Option<ScratchDb> {
        let Ok(admin) = std::env::var(variable) else {
            eprintln!("{variable} is not set: skipping this real-server test");
            return None;
        };
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or_default();
        let name = format!(
            "navree_schema_{}_{}_{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let conn = TestConn::server(&admin).await;
        conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
            .await
            .expect("the test user may create databases");
        conn.close().await.expect("closes");
        let url = with_database(&admin, &name);
        Some(ScratchDb {
            admin,
            name,
            url,
            mysql,
        })
    }

    /// A pool on the database (one connection: session settings stick).
    pub async fn connect(&self) -> TestConn {
        TestConn::server(&self.url).await
    }

    /// Drops the database.
    pub async fn drop(self) {
        let conn = TestConn::server(&self.admin).await;
        let sql = if self.mysql {
            format!("DROP DATABASE IF EXISTS {}", self.name)
        } else {
            format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)
        };
        conn.execute_unprepared(&sql).await.expect("drops");
        conn.close().await.expect("closes");
    }
}

/// `url` with its path (the database) replaced by `database`, keeping the
/// query string.
fn with_database(url: &str, database: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, format!("?{query}")),
        None => (url, String::new()),
    };
    let after_scheme = base.find("://").map_or(0, |at| at + 3);
    let host_end = base[after_scheme..]
        .find('/')
        .map_or(base.len(), |at| after_scheme + at);
    format!("{}/{database}{query}", &base[..host_end])
}

pub async fn exec(conn: &TestConn, sql: &str) {
    conn.execute_unprepared(sql)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

pub async fn query(conn: &TestConn, sql: &str) -> Vec<SchemaRow> {
    conn.rows(sql).await
}

/// The first column of every row, as text.
pub async fn column(conn: &TestConn, sql: &str) -> Vec<String> {
    query(conn, sql)
        .await
        .iter()
        .map(|row| {
            row.get_index(0)
                .and_then(navree_schema::SchemaValue::as_text)
                .unwrap_or_else(|| "NULL".to_owned())
        })
        .collect()
}

/// CHECK constraints on a real server: created with the table, added and
/// dropped later, each enforced while it exists.
pub async fn checks_are_enforced(conn: &TestConn) {
    let schema = navree_schema::Schema::new(conn);
    schema
        .create("templates", |t| {
            t.id();
            t.string("scope").length(16);
            t.integer("workspace_id").nullable();
            t.integer("position").default(0);
            t.check(
                "templates_owner_check",
                "(scope = 'global' AND workspace_id IS NULL) OR (scope = 'workspace' AND workspace_id IS NOT NULL)",
            );
        })
        .await
        .unwrap();
    exec(
        conn,
        "INSERT INTO templates (scope, workspace_id, position) VALUES ('global', NULL, 1), ('workspace', 7, 2)",
    )
    .await;
    assert!(
        conn.execute_unprepared("INSERT INTO templates (scope, workspace_id) VALUES ('global', 7)")
            .await
            .is_err(),
        "a global row with a workspace is refused"
    );
    schema
        .table("templates", |t| {
            t.check("templates_position_check", "position >= 0");
        })
        .await
        .unwrap();
    assert!(
        conn.execute_unprepared(
            "INSERT INTO templates (scope, workspace_id, position) VALUES ('global', NULL, -1)"
        )
        .await
        .is_err(),
        "the added check is enforced"
    );
    schema
        .table("templates", |t| t.drop_check("templates_owner_check"))
        .await
        .unwrap();
    exec(
        conn,
        "INSERT INTO templates (scope, workspace_id, position) VALUES ('global', 9, 0)",
    )
    .await;
    assert_eq!(column(conn, "SELECT COUNT(*) FROM templates").await, ["3"]);
    assert!(
        conn.execute_unprepared(
            "INSERT INTO templates (scope, workspace_id, position) VALUES ('global', NULL, -1)"
        )
        .await
        .is_err(),
        "the other check stays"
    );
}

#[test]
fn urls_point_at_the_scratch_database() {
    assert_eq!(
        with_database("postgres://u:p@h:5432/postgres", "x"),
        "postgres://u:p@h:5432/x"
    );
    assert_eq!(
        with_database("mysql://u:p@h:3306/test?ssl-mode=disabled", "x"),
        "mysql://u:p@h:3306/x?ssl-mode=disabled"
    );
    assert_eq!(with_database("postgres://h", "x"), "postgres://h/x");
}
