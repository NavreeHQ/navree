//! A database of its own per test on every server the environment offers:
//! a SQLite file (always), a database on PostgreSQL
//! (`NAVREE_TEST_POSTGRES_URL`, feature `postgres`) and on MySQL
//! (`NAVREE_TEST_MYSQL_URL`, feature `mysql`), each dropped afterwards, also
//! when the test fails. A server that is not configured is skipped with a
//! notice, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.

#![allow(dead_code)]

use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use navree_db::{Backend, Db, DbOptions, Executor};
use uuid::Uuid;

/// The temporary-directory guard of navree-testing (its reaper retries a
/// folder Windows will not delete while SQLite has a file open, and its
/// sweep removes the folders of earlier runs). This crate cannot
/// depend on navree-testing, so the file is included.
#[allow(dead_code)]
#[path = "../../../navree-testing/src/temp.rs"]
mod temp;
use temp::TempDirGuard;

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
            Server::Sqlite => true,
            Server::Postgres => cfg!(feature = "postgres"),
            Server::MySql => cfg!(feature = "mysql"),
        }
    }
}

/// A database of its own on a server.
pub struct Scratch {
    pub server: Server,
    /// The URL of the database.
    pub url: String,
    _dir: Option<TempDirGuard>,
}

/// What drops the database afterwards.
struct Cleanup {
    admin: Db,
    sql: String,
}

impl Scratch {
    pub fn backend(&self) -> Backend {
        self.server.backend()
    }

    /// A connection to the database.
    pub async fn db(&self) -> Db {
        Db::connect(&DbOptions {
            url: self.url.clone(),
            max_connections: 5,
            ..DbOptions::default()
        })
        .await
        .expect("connects to the scratch database")
    }
}

async fn create(server: Server, test: &str, prefix: &str) -> Option<(Scratch, Option<Cleanup>)> {
    let required = std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1");
    if !server.compiled() {
        assert!(
            !required,
            "{test}: the {server:?} driver feature is off but NAVREE_TEST_REQUIRE_SERVICES=1"
        );
        println!("SKIPPED {test}: the {server:?} driver feature is off");
        return None;
    }
    let name = format!("{prefix}_{}", Uuid::now_v7().simple());
    let Some(variable) = server.variable() else {
        let dir = TempDirGuard::new();
        let url = format!("sqlite://{}?mode=rwc", dir.path().join("test.db").display());
        return Some((
            Scratch {
                server,
                url,
                _dir: Some(dir),
            },
            None,
        ));
    };
    let Some(admin_url) = std::env::var(variable).ok().filter(|v| !v.is_empty()) else {
        assert!(
            !required,
            "{test}: NAVREE_TEST_REQUIRE_SERVICES=1 but {variable} is not set"
        );
        println!("SKIPPED {test}: set {variable}");
        return None;
    };
    let admin = Db::connect(&DbOptions {
        url: admin_url.clone(),
        max_connections: 2,
        ..DbOptions::default()
    })
    .await
    .unwrap_or_else(|error| panic!("cannot connect to {variable}: {error}"));
    let (create, drop) = match server {
        Server::MySql => (
            format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4"),
            format!("DROP DATABASE IF EXISTS `{name}`"),
        ),
        _ => (
            format!("CREATE DATABASE {name}"),
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
        ),
    };
    admin
        .execute_script(&create)
        .await
        .expect("creates the database");
    let (base, rest) = admin_url.rsplit_once('/').expect("a database in the URL");
    let options = rest.find('?').map_or("", |at| &rest[at..]);
    Some((
        Scratch {
            server,
            url: format!("{base}/{name}{options}"),
            _dir: None,
        },
        Some(Cleanup { admin, sql: drop }),
    ))
}

/// Runs `test` on a database of its own on `server` (skipping it when the
/// server is not configured), dropping the database afterwards.
pub async fn run<F, Fut>(server: Server, name: &str, prefix: &str, test: F)
where
    F: FnOnce(Scratch) -> Fut,
    Fut: Future<Output = ()>,
{
    let Some((scratch, cleanup)) = create(server, name, prefix).await else {
        return;
    };
    let outcome = AssertUnwindSafe(test(scratch)).catch_unwind().await;
    if let Some(cleanup) = cleanup
        && let Err(error) = cleanup.admin.execute_script(&cleanup.sql).await
    {
        eprintln!("could not drop the scratch database: {error}");
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// One `#[tokio::test]` per server (`sqlite`, `postgres`, `mysql`) for each
/// scenario `async fn(Scratch)`, run on a database of its own.
#[macro_export]
macro_rules! on_every_server {
    ($prefix:literal; $($scenario:ident),* $(,)?) => {
        $(
            mod $scenario {
                use $crate::databases::{Server, run};

                #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                async fn sqlite() {
                    run(Server::Sqlite, concat!(stringify!($scenario), " on SQLite"), $prefix, super::$scenario).await;
                }

                #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                async fn postgres() {
                    run(Server::Postgres, concat!(stringify!($scenario), " on PostgreSQL"), $prefix, super::$scenario).await;
                }

                #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                async fn mysql() {
                    run(Server::MySql, concat!(stringify!($scenario), " on MySQL"), $prefix, super::$scenario).await;
                }
            }
        )*
    };
}
