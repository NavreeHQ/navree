//! A database of its own per test on every configured server: a SQLite
//! file (always), a schema on PostgreSQL (`NAVREE_TEST_POSTGRES_URL`, the
//! `postgres` feature) and a database on MySQL (`NAVREE_TEST_MYSQL_URL`,
//! the `mysql` feature), each dropped afterwards, also when the test
//! fails. The URLs come from the environment or from
//! `target/test-databases.env` (found from the current directory up).

#![allow(dead_code)]

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;

use futures_util::FutureExt;
use navree_db::{Backend, Db, DbOptions, Executor, ExecutorExt, Statement};
use uuid::Uuid;

/// The temporary-directory guard of navree-testing (its reaper retries a
/// folder Windows will not delete while SQLite has a file open, and its
/// sweep removes the folders of earlier runs). This crate cannot
/// depend on navree-testing, so the file is included.
#[allow(dead_code)]
#[path = "../../../navree-testing/src/temp.rs"]
mod temp;
pub use temp::TempDirGuard;

/// Where a test runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    Sqlite,
    Postgres,
    MySql,
}

impl Server {
    pub fn backend(self) -> Backend {
        match self {
            Server::Sqlite => Backend::Sqlite,
            Server::Postgres => Backend::Postgres,
            Server::MySql => Backend::MySql,
        }
    }

    fn variable(self) -> Option<&'static str> {
        match self {
            Server::Sqlite => None,
            Server::Postgres => Some("NAVREE_TEST_POSTGRES_URL"),
            Server::MySql => Some("NAVREE_TEST_MYSQL_URL"),
        }
    }

    fn compiled(self) -> bool {
        match self {
            Server::Sqlite => cfg!(feature = "sqlite"),
            Server::Postgres => cfg!(feature = "postgres"),
            Server::MySql => cfg!(feature = "mysql"),
        }
    }
}

/// A test setting: the environment's, else `target/test-databases.env`'s.
pub fn setting(name: &str) -> Option<String> {
    if let Some(value) = std::env::var(name).ok().filter(|v| !v.trim().is_empty()) {
        return Some(value);
    }
    let file = env_file()?;
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') {
            return None;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let (key, value) = line.split_once('=')?;
        if key.trim() != name {
            return None;
        }
        let value = value.trim();
        let unquoted = value
            .strip_prefix('\'')
            .and_then(|v| v.strip_suffix('\''))
            .or_else(|| value.strip_prefix('"').and_then(|v| v.strip_suffix('"')))
            .unwrap_or(value);
        Some(unquoted.to_owned()).filter(|v| !v.is_empty())
    })
}

fn env_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NAVREE_TEST_DATABASES_ENV") {
        return Some(PathBuf::from(path));
    }
    std::env::current_dir()
        .ok()?
        .ancestors()
        .map(|dir| dir.join("target").join("test-databases.env"))
        .find(|path| path.is_file())
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

/// A database of its own on `server`, as options to connect with (and
/// what drops it afterwards).
pub struct Scratch {
    pub server: Server,
    pub options: DbOptions,
    admin: Option<Db>,
    drop_sql: Option<String>,
    _dir: Option<TempDirGuard>,
}

impl Scratch {
    /// `None` (with a notice) when the server is not configured or its
    /// driver is not compiled in.
    pub async fn new(server: Server, test: &str) -> Option<Scratch> {
        if !server.compiled() {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test} on {server:?}: its driver feature is off and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            eprintln!("skipped {test} on {server:?}: its driver feature is off");
            return None;
        }
        let name = format!("engine_{}", Uuid::now_v7().simple());
        match server {
            Server::Sqlite => {
                let dir = TempDirGuard::new();
                let url = format!("sqlite://{}?mode=rwc", dir.path().join("test.db").display());
                Some(Scratch {
                    server,
                    options: DbOptions {
                        url,
                        ..DbOptions::default()
                    },
                    admin: None,
                    drop_sql: None,
                    _dir: Some(dir),
                })
            }
            Server::Postgres | Server::MySql => {
                let variable = server.variable().expect("a server variable");
                let Some(url) = setting(variable) else {
                    assert!(
                        std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                        "{test} on {server:?}: {variable} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
                    );
                    eprintln!("skipped {test} on {server:?}: set {variable}");
                    return None;
                };
                let admin = Db::connect(&DbOptions {
                    url: url.clone(),
                    max_connections: 2,
                    ..DbOptions::default()
                })
                .await
                .unwrap_or_else(|error| panic!("cannot connect to {variable}: {error}"));
                let mut options = DbOptions {
                    max_connections: 5,
                    ..DbOptions::default()
                };
                let drop_sql = match server {
                    Server::Postgres => {
                        admin
                            .execute_script(&format!("CREATE SCHEMA {name}"))
                            .await
                            .expect("creates the schema");
                        options.url = url;
                        options.postgres.search_path = vec![name.clone(), "public".to_owned()];
                        format!("DROP SCHEMA {name} CASCADE")
                    }
                    _ => {
                        // MySQL 8's collation, or on a MariaDB without it
                        // (10.6) the accent- and case-insensitive
                        // `utf8mb4_unicode_520_ci`.
                        let known: i64 = admin
                            .fetch_scalar(&Statement::text(
                                "SELECT COUNT(*) FROM information_schema.COLLATIONS WHERE COLLATION_NAME = 'utf8mb4_0900_ai_ci'",
                            ))
                            .await
                            .expect("reads the server's collations");
                        let collation = if known > 0 {
                            "utf8mb4_0900_ai_ci"
                        } else {
                            "utf8mb4_unicode_520_ci"
                        };
                        admin
                            .execute_script(&format!(
                                "CREATE DATABASE {name} CHARACTER SET utf8mb4 COLLATE {collation}"
                            ))
                            .await
                            .expect("creates the database");
                        options.url = with_database(&url, &name);
                        format!("DROP DATABASE {name}")
                    }
                };
                Some(Scratch {
                    server,
                    options,
                    admin: Some(admin),
                    drop_sql: Some(drop_sql),
                    _dir: None,
                })
            }
        }
    }

    /// A `Db` on the scratch database.
    pub async fn db(&self) -> Db {
        Db::connect(&self.options)
            .await
            .expect("connects to the scratch database")
    }

    async fn drop_database(&self) {
        if let (Some(admin), Some(sql)) = (&self.admin, &self.drop_sql) {
            if let Err(error) = admin.execute_script(sql).await {
                eprintln!("could not drop the scratch database: {error}");
            }
            let _ = admin.close().await;
        }
    }
}

/// Runs `test` on a database of its own on `server` (skipping with a
/// notice when it is not configured), dropping it afterwards.
pub async fn run<F, Fut>(server: Server, name: &str, test: F)
where
    F: FnOnce(Scratch) -> Fut,
    Fut: Future<Output = ()>,
{
    let Some(scratch) = Scratch::new(server, name).await else {
        return;
    };
    let options = scratch.options.clone();
    let admin = scratch.admin.clone();
    let drop_sql = scratch.drop_sql.clone();
    let outcome = AssertUnwindSafe(test(scratch)).catch_unwind().await;
    let cleanup = Scratch {
        server,
        options,
        admin,
        drop_sql,
        _dir: None,
    };
    cleanup.drop_database().await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// One `#[tokio::test]` per server for a scenario `fn(Scratch) -> Future`.
#[macro_export]
macro_rules! on_every_server {
    ($($scenario:ident),* $(,)?) => {
        $(
            mod $scenario {
                use super::support::{Server, run};

                #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                async fn sqlite() {
                    run(Server::Sqlite, concat!(stringify!($scenario), " on SQLite"), super::$scenario).await;
                }

                #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                async fn postgres() {
                    run(Server::Postgres, concat!(stringify!($scenario), " on PostgreSQL"), super::$scenario).await;
                }

                #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                async fn mysql() {
                    run(Server::MySql, concat!(stringify!($scenario), " on MySQL"), super::$scenario).await;
                }
            }
        )*
    };
}

/// Runs `sql` (each `;`-separated statement on its own).
pub async fn script(db: &impl Executor, sql: &str) {
    for statement in sql.split(";\n").map(str::trim).filter(|s| !s.is_empty()) {
        db.execute(&Statement::text(statement))
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// The type of an auto-increment key column on `backend`.
pub fn serial(backend: Backend) -> &'static str {
    match backend {
        Backend::Postgres => "BIGSERIAL PRIMARY KEY",
        Backend::MySql => "BIGINT AUTO_INCREMENT PRIMARY KEY",
        Backend::Sqlite => "INTEGER PRIMARY KEY AUTOINCREMENT",
    }
}
