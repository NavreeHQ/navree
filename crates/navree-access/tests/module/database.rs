//! Where the module tests run: a SQLite file by default, or a database of
//! its own on a server, with units of work on SeaORM's transactions (the
//! default) or on navree-db's own.
//!
//! - `NAVREE_TEST_DATABASE=postgres` (feature `postgres`,
//!   `NAVREE_TEST_POSTGRES_URL`: a TimescaleDB server) or `mysql` (feature
//!   `mysql`, `NAVREE_TEST_MYSQL_URL`): every app gets a database of its
//!   own on that server, dropped with the app;
//! - `NAVREE_TEST_NATIVE_UNITS_OF_WORK=1`: `database.native_units_of_work`.
//!
//! ```text
//! NAVREE_TEST_DATABASE=postgres NAVREE_TEST_NATIVE_UNITS_OF_WORK=1 \
//!   cargo test -p navree-access --features postgres,mysql --test module
//! ```

#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use navree_db::{Backend, Db, DbOptions, Executor, Pool};
use navree_testing::{TempDirGuard, TestAppBuilder};

/// A database server.
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
}

/// The server of `NAVREE_TEST_DATABASE` (SQLite when unset).
pub fn server() -> Server {
    match std::env::var("NAVREE_TEST_DATABASE").as_deref() {
        Err(_) | Ok("" | "sqlite") => Server::Sqlite,
        Ok("postgres") => Server::Postgres,
        Ok("mysql") => Server::MySql,
        Ok(other) => panic!("NAVREE_TEST_DATABASE: unknown database `{other}`"),
    }
}

/// Whether units of work run on navree-db's own transactions
/// (`NAVREE_TEST_NATIVE_UNITS_OF_WORK=1`).
pub fn native() -> bool {
    matches!(
        std::env::var("NAVREE_TEST_NATIVE_UNITS_OF_WORK").as_deref(),
        Ok("1" | "true")
    )
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A database of its own on a server, dropped with the last handle.
pub struct Scratch {
    server: Server,
    admin_url: String,
    name: String,
    pub url: String,
}

/// Runs `future` to completion on a runtime of its own, on another thread
/// (so it can be called from synchronous code inside a test's runtime).
fn block_on<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> T {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime")
            .block_on(future)
    })
    .join()
    .expect("the database helper thread")
}

async fn admin(url: String) -> Pool {
    Pool::connect(&DbOptions {
        url,
        max_connections: 1,
        ..DbOptions::default()
    })
    .await
    .expect("the admin URL connects")
}

impl Scratch {
    /// A new database on `server` (not SQLite).
    fn new(server: Server) -> Scratch {
        let (variable, compiled) = match server {
            Server::Sqlite => unreachable!("SQLite databases are files"),
            Server::Postgres => ("NAVREE_TEST_POSTGRES_URL", cfg!(feature = "postgres")),
            Server::MySql => ("NAVREE_TEST_MYSQL_URL", cfg!(feature = "mysql")),
        };
        assert!(
            compiled,
            "NAVREE_TEST_DATABASE={server:?} needs its feature"
        );
        let admin_url = std::env::var(variable)
            .ok()
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| panic!("NAVREE_TEST_DATABASE={server:?} needs {variable}"));
        let name = format!(
            "navree_access_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let (url, create) = (admin_url.clone(), format!("CREATE DATABASE {name}"));
        block_on(async move {
            let admin = admin(url).await;
            admin
                .execute_script(&create)
                .await
                .expect("the test user may create databases");
            admin.close().await;
        });
        let (base, query) = match admin_url.split_once('?') {
            Some((base, query)) => (base, format!("?{query}")),
            None => (admin_url.as_str(), String::new()),
        };
        let server_part = base.rsplit_once('/').map_or(base, |(server, _)| server);
        let url = format!("{server_part}/{name}{query}");
        Scratch {
            server,
            admin_url,
            name,
            url,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let force = if self.server == Server::Postgres {
            " WITH (FORCE)"
        } else {
            ""
        };
        let drop = format!("DROP DATABASE IF EXISTS {}{force}", self.name);
        let url = self.admin_url.clone();
        block_on(async move {
            let admin = admin(url).await;
            if let Err(error) = admin.execute_script(&drop).await {
                eprintln!("could not drop a test database: {error}");
            }
            admin.close().await;
        });
    }
}

/// What keeps a test database alive: the SQLite file's directory or the
/// server database.
pub enum Keep {
    File(TempDirGuard),
    Server(Scratch),
}

/// A database for one test on [`server`]: its URL and what keeps it.
pub fn scratch() -> (String, Keep) {
    match server() {
        Server::Sqlite => {
            let (url, guard) = navree_testing::temp_sqlite_url();
            (url, Keep::File(guard))
        }
        server => {
            let scratch = Scratch::new(server);
            (scratch.url.clone(), Keep::Server(scratch))
        }
    }
}

/// `builder` on a database of its own on [`server`], migrated on boot,
/// its units of work native when [`native`]. The database goes with the
/// app.
pub fn database(builder: TestAppBuilder) -> TestAppBuilder {
    let builder = builder
        .config("database.native_units_of_work", native())
        .config("database.max_connections", 8);
    match server() {
        Server::Sqlite => builder.temp_database(),
        server => {
            let scratch = Scratch::new(server);
            builder
                .config("database.url", scratch.url.as_str())
                .config("database.migrate_on_boot", true)
                .override_value::<Scratch>(Arc::new(scratch))
        }
    }
}

/// A connection to a database of its own on [`server`] (outside an app),
/// with what keeps the database.
pub async fn connect() -> (Db, Keep) {
    let (url, keep) = scratch();
    let db = Db::connect(&DbOptions {
        url,
        max_connections: 4,
        native_units_of_work: native(),
        ..DbOptions::default()
    })
    .await
    .expect("connects to the test database");
    assert_eq!(Executor::backend(&db), server().backend());
    (db, keep)
}
