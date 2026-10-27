//! The MySQL server the suite runs against, and a scratch database per test.

#![allow(dead_code)]

use navree_db::{Db, DbOptions};
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
use uuid::Uuid;

/// A MySQL or MariaDB server URL whose user may create and drop databases,
/// e.g. `mysql://root:secret@127.0.0.1:3306/navree_test`.
pub const URL_VARIABLE: &str = "NAVREE_TEST_MYSQL_URL";

/// With `1`, a missing server fails the test instead of skipping it (CI).
pub const REQUIRE_VARIABLE: &str = "NAVREE_TEST_REQUIRE_SERVICES";

/// A fresh, empty database on the test server, dropped when the guard is
/// dropped (also when the test panics). Declare it before the pools that
/// use it, so they are dropped first.
pub struct Scratch {
    /// The server URL from the environment (used for `DROP DATABASE`).
    admin: String,
    /// The database name, `navree_db_<uuid>`.
    pub name: String,
    /// A `mysql://` URL of the scratch database.
    pub url: String,
}

impl Scratch {
    /// A scratch database, or `None` (after saying why) when
    /// `NAVREE_TEST_MYSQL_URL` is not set. Panics when the variable is
    /// missing and `NAVREE_TEST_REQUIRE_SERVICES=1`.
    pub async fn new(test: &str) -> Option<Scratch> {
        let Ok(admin) = std::env::var(URL_VARIABLE) else {
            assert!(
                std::env::var(REQUIRE_VARIABLE).as_deref() != Ok("1"),
                "{test}: {URL_VARIABLE} is not set and {REQUIRE_VARIABLE}=1"
            );
            eprintln!("SKIPPED {test}: set {URL_VARIABLE}");
            return None;
        };
        let name = format!("navree_db_{}", Uuid::now_v7().simple());
        let conn = Database::connect(admin.as_str())
            .await
            .unwrap_or_else(|error| panic!("{test}: cannot connect to {URL_VARIABLE}: {error}"));
        conn.execute_unprepared(&format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4"))
            .await
            .expect("creates the scratch database");
        conn.close().await.expect("closes the admin connection");
        let url = with_database(&admin, &name);
        Some(Scratch { admin, name, url })
    }

    /// Options for the scratch database.
    pub fn options(&self) -> DbOptions {
        DbOptions {
            url: self.url.clone(),
            ..DbOptions::default()
        }
    }

    /// The scratch database's URL with the `mariadb://` scheme.
    pub fn mariadb_url(&self) -> String {
        self.url.replacen("mysql://", "mariadb://", 1)
    }

    /// A pool on the scratch database.
    pub async fn db(&self) -> Db {
        Db::connect(&self.options()).await.expect("connects")
    }

    /// A pool on the scratch database with `options` changed by `f`.
    pub async fn db_with(&self, f: impl FnOnce(&mut DbOptions)) -> Db {
        let mut options = self.options();
        f(&mut options);
        Db::connect(&options).await.expect("connects")
    }

    /// A connection to the server as the admin of `NAVREE_TEST_MYSQL_URL`
    /// (to kill connections, or drop and create the scratch database).
    pub async fn admin(&self) -> sea_orm::DatabaseConnection {
        Database::connect(self.admin.as_str())
            .await
            .expect("connects as the admin")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let admin = self.admin.clone();
        let name = self.name.clone();
        // `Drop` cannot await and the test's runtime may be shutting down,
        // so the database is dropped from a thread with its own runtime.
        let dropped = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime");
            runtime.block_on(async move {
                let conn = Database::connect(admin.as_str()).await?;
                // An unfinished transaction of the test would block the drop.
                conn.execute_unprepared("SET SESSION lock_wait_timeout = 10")
                    .await?;
                conn.execute_unprepared(&format!("DROP DATABASE IF EXISTS `{name}`"))
                    .await?;
                conn.close().await
            })
        })
        .join();
        match dropped {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!(
                "the scratch database {} was not dropped: {error}",
                self.name
            ),
            Err(_) => eprintln!("dropping the scratch database {} panicked", self.name),
        }
    }
}

/// A server account that may use one scratch database, dropped with the
/// guard (also when the test panics). Accounts belong to the server, not to
/// a database, so a test that needs its own (a password never used to sign
/// in, which the server has not cached) creates one.
pub struct Account {
    admin: String,
    /// The account name, `nv_<16 hex digits>` (MySQL allows 32 characters).
    pub user: String,
    /// Its password.
    pub password: String,
    /// A `mysql://` URL of the scratch database as this account.
    pub url: String,
}

impl Account {
    /// Creates an account identified with `plugin` (e.g.
    /// `caching_sha2_password`) and grants it every privilege on
    /// `scratch`'s database.
    pub async fn create(scratch: &Scratch, plugin: &str) -> Account {
        let id = Uuid::now_v7().simple().to_string();
        let user = format!("nv_{}", &id[16..]);
        let password = format!("pw-{}", &id[..16]);
        let conn = Database::connect(scratch.admin.as_str())
            .await
            .expect("connects as the admin");
        for sql in [
            format!("CREATE USER '{user}'@'%' IDENTIFIED WITH {plugin} BY '{password}'"),
            format!("GRANT ALL ON `{}`.* TO '{user}'@'%'", scratch.name),
        ] {
            conn.execute_unprepared(&sql)
                .await
                .unwrap_or_else(|error| panic!("{sql}: {error}"));
        }
        conn.close().await.expect("closes the admin connection");
        let (scheme, rest) = scratch.url.split_once("://").expect("a URL");
        let (_, host_and_db) = rest.split_once('@').expect("a URL with a user");
        let url = format!("{scheme}://{user}:{password}@{host_and_db}");
        Account {
            admin: scratch.admin.clone(),
            user,
            password,
            url,
        }
    }
}

impl Drop for Account {
    fn drop(&mut self) {
        let admin = self.admin.clone();
        let user = self.user.clone();
        let dropped = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime");
            runtime.block_on(async move {
                let conn = Database::connect(admin.as_str()).await?;
                conn.execute_unprepared(&format!("DROP USER IF EXISTS '{user}'@'%'"))
                    .await?;
                conn.close().await
            })
        })
        .join();
        match dropped {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("the account {} was not dropped: {error}", self.user),
            Err(_) => eprintln!("dropping the account {} panicked", self.user),
        }
    }
}

/// Whether the server behind `conn` is MariaDB (`VERSION()` names it).
pub async fn is_mariadb(conn: &impl ConnectionTrait) -> bool {
    string(conn, "SELECT VERSION()").await.contains("MariaDB")
}

/// `url` with `key=value` added to its query.
pub fn with_param(url: &str, key: &str, value: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{key}={value}")
}

/// `url` with its database replaced by `database` (the query is kept).
pub fn with_database(url: &str, database: &str) -> String {
    let (scheme, rest) = url.split_once("://").expect("a URL with a scheme");
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let query = tail.find('?').map_or("", |at| &tail[at..]);
    format!("{scheme}://{authority}/{database}{query}")
}

/// Runs `sql` and returns the first column of every row as text.
pub async fn strings(conn: &impl ConnectionTrait, sql: &str) -> Vec<String> {
    conn.query_all_raw(Statement::from_string(DbBackend::MySql, sql))
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
        .iter()
        .map(|row| {
            row.try_get_by_index::<Option<String>>(0)
                .unwrap_or_else(|error| panic!("{sql}: {error}"))
                .unwrap_or_default()
        })
        .collect()
}

/// Runs `sql` and returns the first column of its one row as text.
pub async fn string(conn: &impl ConnectionTrait, sql: &str) -> String {
    let mut values = strings(conn, sql).await;
    assert_eq!(values.len(), 1, "{sql}: one row expected, got {values:?}");
    values.remove(0)
}

/// `SELECT COUNT(*) FROM table`.
pub async fn count(conn: &impl ConnectionTrait, table: &str) -> i64 {
    let row = conn
        .query_one_raw(Statement::from_string(
            DbBackend::MySql,
            format!("SELECT COUNT(*) AS n FROM `{table}`"),
        ))
        .await
        .expect("count query")
        .expect("count row");
    row.try_get::<i64>("", "n").expect("count")
}

/// The base tables of the current database, sorted.
pub async fn tables(conn: &impl ConnectionTrait) -> Vec<String> {
    strings(
        conn,
        "SELECT CAST(table_name AS CHAR) FROM information_schema.tables \
         WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' ORDER BY 1",
    )
    .await
}

#[test]
fn adds_query_parameters_to_a_url() {
    assert_eq!(
        with_param("mysql://root@db/app", "ssl-mode", "DISABLED"),
        "mysql://root@db/app?ssl-mode=DISABLED"
    );
    assert_eq!(
        with_param(
            "mysql://root@db/app?charset=utf8mb4",
            "ssl-mode",
            "DISABLED"
        ),
        "mysql://root@db/app?charset=utf8mb4&ssl-mode=DISABLED"
    );
}

#[test]
fn replaces_the_database_of_a_url() {
    assert_eq!(
        with_database("mysql://root:pw@127.0.0.1:3306/navree_test", "scratch"),
        "mysql://root:pw@127.0.0.1:3306/scratch"
    );
    assert_eq!(
        with_database("mysql://root@db?ssl-mode=DISABLED", "scratch"),
        "mysql://root@db/scratch?ssl-mode=DISABLED"
    );
    assert_eq!(
        with_database("mysql://root@db/old?charset=utf8mb4", "new"),
        "mysql://root@db/new?charset=utf8mb4"
    );
}
