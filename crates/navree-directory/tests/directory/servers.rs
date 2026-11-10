//! The database the suite runs on, chosen by the environment:
//!
//! - `NAVREE_TEST_DATABASE`: `sqlite` (the default: a file of its own per
//!   app), `postgres` (a database of its own per app on the server of
//!   `NAVREE_TEST_POSTGRES_URL`, feature `postgres`) or `mysql` (on the
//!   server of `NAVREE_TEST_MYSQL_URL`, feature `mysql`). Server databases
//!   are dropped with the app.
//!
//! ```text
//! NAVREE_TEST_DATABASE=postgres \
//!   cargo test -p navree-directory --features test-postgres
//! ```

#![allow(dead_code)]

use std::any::Any;

use navree_db::{DbOptions, Executor, Pool};
use navree_testing::TestAppBuilder;
use uuid::Uuid;

/// A server the suite can run on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    Sqlite,
    Postgres,
    MySql,
}

impl Server {
    /// The server `NAVREE_TEST_DATABASE` names.
    ///
    /// # Panics
    ///
    /// For an unknown name, or a server whose driver feature is off or
    /// whose URL is not set.
    pub fn current() -> Server {
        let server = match std::env::var("NAVREE_TEST_DATABASE").as_deref() {
            Err(_) | Ok("" | "sqlite") => return Server::Sqlite,
            Ok("postgres") => Server::Postgres,
            Ok("mysql") => Server::MySql,
            Ok(other) => panic!("NAVREE_TEST_DATABASE: `{other}` is not sqlite, postgres or mysql"),
        };
        let compiled = match server {
            Server::Postgres => cfg!(feature = "test-postgres"),
            _ => cfg!(feature = "test-mysql"),
        };
        assert!(
            compiled,
            "NAVREE_TEST_DATABASE={server:?} needs the `{}` feature",
            server.feature()
        );
        server
    }

    fn feature(self) -> &'static str {
        match self {
            Server::Sqlite => "sqlite",
            Server::Postgres => "postgres",
            Server::MySql => "mysql",
        }
    }

    fn variable(self) -> &'static str {
        match self {
            Server::Postgres => "NAVREE_TEST_POSTGRES_URL",
            _ => "NAVREE_TEST_MYSQL_URL",
        }
    }
}

/// `builder` with a fresh, migrated database of its own on the current
/// server.
pub fn database(builder: TestAppBuilder) -> TestAppBuilder {
    let server = Server::current();
    if server == Server::Sqlite {
        return builder.temp_database();
    }
    let database = ServerDatabase::create(server);
    builder
        .config("database.url", database.url.clone())
        .config("database.migrate_on_boot", true)
        .config("database.max_connections", 4)
        .hold(database)
}

/// A fresh database on the current server for an app built by hand: its
/// URL and the guard that deletes it (a SQLite file's directory, or the
/// server database).
pub fn scratch() -> (String, Box<dyn Any + Send + Sync>) {
    match Server::current() {
        Server::Sqlite => {
            let (url, guard) = navree_testing::temp_sqlite_url();
            (url, Box::new(guard))
        }
        server => {
            let database = ServerDatabase::create(server);
            (database.url.clone(), Box::new(database))
        }
    }
}

/// A database of its own on a server, dropped with its guard.
pub struct ServerDatabase {
    server: Server,
    admin: String,
    name: String,
    /// The database's URL.
    pub url: String,
}

impl ServerDatabase {
    /// Creates a database on `server`.
    pub fn create(server: Server) -> ServerDatabase {
        let admin = std::env::var(server.variable())
            .ok()
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| panic!("NAVREE_TEST_DATABASE needs {}", server.variable()));
        let name = format!("navree_directory_{}", Uuid::now_v7().simple());
        let base = admin.split('?').next().unwrap_or(&admin);
        let (base, _) = base.rsplit_once('/').expect("a database URL");
        let url = format!("{base}/{name}");
        run_admin(&admin, format!("CREATE DATABASE {name}"))
            .unwrap_or_else(|error| panic!("{error}"));
        ServerDatabase {
            server,
            admin,
            name,
            url,
        }
    }
}

impl Drop for ServerDatabase {
    fn drop(&mut self) {
        let force = if self.server == Server::Postgres {
            " WITH (FORCE)"
        } else {
            ""
        };
        let sql = format!("DROP DATABASE IF EXISTS {}{force}", self.name);
        if let Err(error) = run_admin(&self.admin, sql) {
            eprintln!("could not drop a test database: {error}");
        }
    }
}

/// Runs `sql` on the server of `admin`, on a thread and runtime of its own
/// (so it works from synchronous code, `Drop` included, and from inside a
/// test's runtime alike).
fn run_admin(admin: &str, sql: String) -> Result<(), String> {
    let admin = admin.to_owned();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(async move {
            let pool = Pool::connect(&DbOptions {
                url: admin,
                max_connections: 1,
                ..DbOptions::default()
            })
            .await
            .map_err(|error| format!("cannot connect to the test server: {error}"))?;
            let result = pool.execute_script(&sql).await;
            pool.close().await;
            result.map(drop).map_err(|error| format!("{sql}: {error}"))
        })
    })
    .join()
    .map_err(|_| "the admin thread panicked".to_owned())?
}
