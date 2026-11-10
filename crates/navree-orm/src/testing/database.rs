//! Test databases (feature `testing`): a database of its own for every
//! test.
//!
//! - **Templates** ([`Template`]): the schema is built once per test binary
//!   (migrations, tables, reference rows), then every test gets a copy: a
//!   copied file on SQLite, `CREATE DATABASE … TEMPLATE` on PostgreSQL, a
//!   new database with the template's tables and rows on MySQL.
//! - **Fresh databases** ([`TestDb::sqlite`], [`TestDb::postgres`],
//!   [`TestDb::mysql`]) and **in-memory SQLite** ([`TestDb::memory`];
//!   `TestAppBuilder::memory_database` for apps).
//! - **Truncation** ([`TestDb::truncate`]): empty every table but the ones
//!   named, for suites that share one database.
//! - **A transaction per test** ([`TestDb::transaction`]): the test's unit
//!   of work rolls back afterwards, its after-commit hooks still running.
//! - **Lazy databases** ([`LazyTestDb`]): created when a test first uses
//!   one, so tests that never touch the database never pay for it.
//! - **Real servers** ([`TestServer`]): PostgreSQL, MySQL and MongoDB from
//!   `NAVREE_TEST_POSTGRES_URL`, `NAVREE_TEST_MYSQL_URL` and
//!   `NAVREE_TEST_MONGODB_URL`, or from `target/test-databases.env` (written
//!   by `scripts/dev-databases.ps1 start`) when the variables are unset;
//!   without either, the test skips with a notice.
//!
//! Server databases are dropped when their [`TestDb`] is dropped (or
//! [`close`](TestDb::close)d), also when the test panics; SQLite files are
//! deleted with their directory.
//!
//! Each database needs its driver in the test build: navree-db's `sqlite`
//! feature for the SQLite ones (the default), `postgres` or `mysql` for the
//! servers. An app turns on the ones its tests use in `[dev-dependencies]`
//! (`navree = { .., features = ["sqlite"] }`).
//!
//! ```
//! use navree_db::Executor;
//! use navree_orm::testing::database::Template;
//!
//! /// The schema and its reference rows, built once per test binary.
//! static DATABASE: Template = Template::new("notes", |template| {
//!     Box::pin(async move {
//!         let db = template.db();
//!         db.execute_script("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)")
//!             .await?;
//!         db.execute_script("INSERT INTO notes (body) VALUES ('welcome')").await?;
//!         Ok(())
//!     })
//! });
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() {
//! let first = DATABASE.sqlite().await.unwrap();
//! first.db().execute_script("DELETE FROM notes").await.unwrap();
//! // Every copy starts from the template.
//! let second = DATABASE.sqlite().await.unwrap();
//! let rows = navree_orm::table("notes").count(second.db()).await.unwrap();
//! assert_eq!(rows, 1);
//! # }
//! ```
//!
//! On a server (navree-db's driver for it compiled in), the same template
//! gives copies with `DATABASE.postgres()` or `DATABASE.mysql()`: `None`,
//! after a notice, when the server is not configured.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use navree_db::{Backend, Db, DbOptions, Executor, ExecutorExt, Statement};
use navree_kernel::{BoxError, BoxFuture};
use navree_testing::TempDirGuard;
use tokio::sync::OnceCell;

use crate::ulid::Ulid;

/// How old a leftover template or test database must be before a new
/// template build drops it (a crashed run's; no test run lasts this long).
const STALE_AFTER: Duration = Duration::from_secs(6 * 60 * 60);

/// The prefix of every test database's name on a server.
const TEST_PREFIX: &str = "navree_test_";

/// The prefix of every template database's name on a server.
const TEMPLATE_PREFIX: &str = "navree_tpl_";

/// A real database server tests can run against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestServer {
    /// PostgreSQL (with TimescaleDB): `NAVREE_TEST_POSTGRES_URL`.
    Postgres,
    /// MySQL: `NAVREE_TEST_MYSQL_URL`.
    MySql,
    /// MongoDB: `NAVREE_TEST_MONGODB_URL`.
    MongoDb,
}

impl TestServer {
    /// The variable holding the server's URL.
    pub const fn variable(self) -> &'static str {
        match self {
            TestServer::Postgres => "NAVREE_TEST_POSTGRES_URL",
            TestServer::MySql => "NAVREE_TEST_MYSQL_URL",
            TestServer::MongoDb => "NAVREE_TEST_MONGODB_URL",
        }
    }

    /// The server's URL ([`test_setting`] of its variable); `None`, after a
    /// notice on standard error, when it is not configured: the test then
    /// skips.
    pub fn url(self) -> Option<String> {
        let url = test_setting(self.variable());
        if url.is_none() {
            skip(&format!(
                "set {} (or start the test servers: scripts/ci-services.sh start, or pwsh -File scripts/dev-databases.ps1 start) to run this test",
                self.variable()
            ));
        }
        url
    }
}

/// Whether `NAVREE_TEST_REQUIRE_SERVICES=1`: a test that needs a server (or
/// Chrome, Node.js, FFmpeg) fails when it is missing instead of skipping.
pub fn services_required() -> bool {
    requires(
        std::env::var("NAVREE_TEST_REQUIRE_SERVICES")
            .ok()
            .as_deref(),
    )
}

/// Whether the value of `NAVREE_TEST_REQUIRE_SERVICES` asks for the servers:
/// exactly `1`.
fn requires(value: Option<&str>) -> bool {
    value == Some("1")
}

/// What a test calls where it would skip because a server is missing: prints
/// `skipped: <reason>` and returns, or panics when
/// [`services_required`], so a run that must have its servers cannot pass by
/// skipping.
///
/// # Panics
///
/// When `NAVREE_TEST_REQUIRE_SERVICES=1`.
pub fn skip(reason: &str) {
    skip_unless(services_required(), reason);
}

fn skip_unless(required: bool, reason: &str) {
    assert!(!required, "{reason} (NAVREE_TEST_REQUIRE_SERVICES=1)");
    eprintln!("skipped: {reason}");
}

/// Setting `name` for tests: the environment variable, else its line in
/// the test-databases file (`export NAME='value'`), which is
/// `NAVREE_TEST_DATABASES_ENV` when set, else the first
/// `target/test-databases.env` found from the current directory upwards.
pub fn test_setting(name: &str) -> Option<String> {
    if let Some(value) = std::env::var(name).ok().filter(|v| !v.trim().is_empty()) {
        return Some(value);
    }
    let file = env_file()?;
    let text = std::fs::read_to_string(file).ok()?;
    env_file_value(&text, name)
}

/// The test-databases file, if there is one.
fn env_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NAVREE_TEST_DATABASES_ENV") {
        return Some(PathBuf::from(path));
    }
    let start = std::env::current_dir().ok()?;
    start
        .ancestors()
        .map(|dir| dir.join("target").join("test-databases.env"))
        .find(|path| path.is_file())
}

/// `name`'s value in an env file of `NAME=value` lines (`export`, quotes
/// and comments allowed).
fn env_file_value(text: &str, name: &str) -> Option<String> {
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

/// The database system a [`TestDb`] runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DatabaseKind {
    /// SQLite (a file, or memory).
    Sqlite,
    /// PostgreSQL.
    Postgres,
    /// MySQL or MariaDB.
    MySql,
}

impl DatabaseKind {
    fn of(backend: Backend) -> Self {
        match backend {
            Backend::Postgres => DatabaseKind::Postgres,
            Backend::MySql => DatabaseKind::MySql,
            Backend::Sqlite => DatabaseKind::Sqlite,
        }
    }

    fn quote(self, name: &str) -> String {
        match self {
            DatabaseKind::MySql => format!("`{}`", name.replace('`', "``")),
            _ => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    }
}

/// What dropping a [`TestDb`] removes.
enum Cleanup {
    /// Nothing (a template's own handle, an in-memory database).
    Nothing,
    /// The directory holding the SQLite file (deleted when dropped).
    Dir { _dir: TempDirGuard },
    /// A server database, dropped through `admin_url`.
    Database {
        admin_url: String,
        name: String,
        kind: DatabaseKind,
    },
}

/// A database of its own for one test: [`db`](TestDb::db) to run
/// queries, [`url`](TestDb::url) to boot an app on it
/// (`TestApp::builder().config("database.url", test.url())`).
pub struct TestDb {
    db: Db,
    url: String,
    kind: DatabaseKind,
    cleanup: Cleanup,
}

impl fmt::Debug for TestDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestDb")
            .field("kind", &self.kind)
            .field("url", &redact(&self.url))
            .finish_non_exhaustive()
    }
}

impl TestDb {
    /// An in-memory SQLite database. Its pool has one connection (each
    /// connection would see a database of its own), so end a unit of work
    /// before querying through the pool; it is gone when the pool closes.
    pub async fn memory() -> Result<TestDb, BoxError> {
        let url = "sqlite::memory:".to_owned();
        let db = Db::connect(&DbOptions {
            url: url.clone(),
            max_connections: 1,
            min_connections: 1,
            ..DbOptions::default()
        })
        .await?;
        Ok(TestDb {
            db,
            url,
            kind: DatabaseKind::Sqlite,
            cleanup: Cleanup::Nothing,
        })
    }

    /// An empty SQLite database in a file of its own, deleted (with its
    /// directory) when the `TestDb` is dropped.
    pub async fn sqlite() -> Result<TestDb, BoxError> {
        let dir = TempDirGuard::new();
        let file = dir.path().join("database.sqlite");
        let url = sqlite_url(&file);
        let db = connect(&url).await?;
        Ok(TestDb {
            db,
            url,
            kind: DatabaseKind::Sqlite,
            cleanup: Cleanup::Dir { _dir: dir },
        })
    }

    /// An empty database of its own on the PostgreSQL test server; `None`
    /// (after a notice) when no server is configured, or this binary has
    /// no PostgreSQL driver.
    pub async fn postgres() -> Result<Option<TestDb>, BoxError> {
        let Some(server) = usable_server(TestServer::Postgres).await? else {
            return Ok(None);
        };
        server_database(&server, DatabaseKind::Postgres, None)
            .await
            .map(Some)
    }

    /// An empty database of its own on the MySQL test server; `None`
    /// (after a notice) when no server is configured, or this binary has
    /// no MySQL driver.
    pub async fn mysql() -> Result<Option<TestDb>, BoxError> {
        let Some(server) = usable_server(TestServer::MySql).await? else {
            return Ok(None);
        };
        server_database(&server, DatabaseKind::MySql, None)
            .await
            .map(Some)
    }

    /// The database's pool.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// The database's URL, to boot an app on it.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The database system.
    pub fn kind(&self) -> DatabaseKind {
        self.kind
    }

    /// Empties every table but `except` (`&["navree_migrations"]` keeps the
    /// migration history), resetting auto-increment counters: the
    /// truncation strategy, for tests that share a database. Foreign keys
    /// do not get in the way (they are checked after, or switched off
    /// while emptying).
    pub async fn truncate(&self, except: &[&str]) -> Result<(), BoxError> {
        truncate_tables(&self.db, except).await
    }

    /// Runs `test` with a unit of work on this database, then rolls it
    /// back: the transaction-per-test strategy, for tests sharing a
    /// database. Nothing `test` writes through the unit of work stays; its
    /// change observers and after-commit hooks still run, as on a commit
    /// ([`Uow::rollback_as_committed`](navree_db::Uow::rollback_as_committed)).
    /// Drop every transaction handle `test` took from the unit of work
    /// before it returns. Requests a test sends commit units of work of
    /// their own: give HTTP tests a copy ([`Template`]) or truncate.
    ///
    /// ```
    /// use navree_orm::testing::database::TestDb;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let test = TestDb::memory().await.unwrap();
    /// let ran = test
    ///     .transaction(|uow| async move {
    ///         uow.after_commit(|| Box::pin(async { println!("as on a commit") }));
    ///         "done"
    ///     })
    ///     .await
    ///     .unwrap();
    /// assert_eq!(ran, "done");
    /// # }
    /// ```
    pub async fn transaction<F, Fut, T>(&self, test: F) -> Result<T, BoxError>
    where
        F: FnOnce(navree_db::Uow) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let uow = self.db.uow(None, None).await;
        let output = test(uow.clone()).await;
        uow.rollback_as_committed().await?;
        Ok(output)
    }

    /// Closes the pool and removes the database now (dropping the
    /// `TestDb` does the same, blocking briefly for a server database).
    pub async fn close(mut self) -> Result<(), BoxError> {
        let cleanup = std::mem::replace(&mut self.cleanup, Cleanup::Nothing);
        close_pool(&self.db).await;
        match cleanup {
            Cleanup::Nothing | Cleanup::Dir { .. } => Ok(()),
            Cleanup::Database {
                admin_url,
                name,
                kind,
            } => drop_database(&admin_url, &name, kind).await,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let Cleanup::Database {
            admin_url,
            name,
            kind,
        } = std::mem::replace(&mut self.cleanup, Cleanup::Nothing)
        else {
            return;
        };
        // Async work in `drop`: on a thread of its own with its own runtime
        // (the test's runtime may be the one dropping this value).
        let outcome = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime
                .block_on(drop_database(&admin_url, &name, kind))
                .map_err(|error| format!("cannot drop test database {name}: {error}"))
        })
        .join();
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("{error}"),
            Err(_) => eprintln!("dropping a test database panicked"),
        }
    }
}

/// Builds a template's schema on its database (see [`Template::new`]).
pub type BuildTemplate = for<'a> fn(&'a TestDb) -> BoxFuture<'a, Result<(), BoxError>>;

/// A database schema built once per test binary and copied for every test
/// (see the [module documentation](self)). Declare it as a `static`; the
/// first test to ask for a copy on a database system builds the template
/// there, with `build`.
pub struct Template {
    name: &'static str,
    build: BuildTemplate,
    sqlite: OnceCell<PathBuf>,
    postgres: OnceCell<String>,
    mysql: OnceCell<String>,
}

impl fmt::Debug for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Template")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Template {
    /// Template `name` (letters, digits and `_` are kept for the database
    /// names), whose schema `build` creates on the template's database:
    /// run migrations, create tables, insert reference rows. `build` may
    /// boot an app on [`TestDb::url`] with `database.migrate_on_boot`.
    pub const fn new(name: &'static str, build: BuildTemplate) -> Self {
        Template {
            name,
            build,
            sqlite: OnceCell::const_new(),
            postgres: OnceCell::const_new(),
            mysql: OnceCell::const_new(),
        }
    }

    /// A copy of the template on SQLite: the template's file, copied.
    pub async fn sqlite(&self) -> Result<TestDb, BoxError> {
        let template = self
            .sqlite
            .get_or_try_init(|| build_sqlite_template(self))
            .await?;
        let dir = TempDirGuard::new();
        let file = dir.path().join("database.sqlite");
        tokio::fs::copy(template, &file).await.map_err(|error| {
            format!(
                "cannot copy the `{}` template ({}): {error}",
                self.name,
                template.display()
            )
        })?;
        let url = sqlite_url(&file);
        let db = connect(&url).await?;
        Ok(TestDb {
            db,
            url,
            kind: DatabaseKind::Sqlite,
            cleanup: Cleanup::Dir { _dir: dir },
        })
    }

    /// A copy of the template on the PostgreSQL test server (`CREATE
    /// DATABASE … TEMPLATE`); `None` (after a notice) without a server or
    /// its driver.
    pub async fn postgres(&self) -> Result<Option<TestDb>, BoxError> {
        let Some(server) = usable_server(TestServer::Postgres).await? else {
            return Ok(None);
        };
        let template = self
            .postgres
            .get_or_try_init(|| build_server_template(self, &server, DatabaseKind::Postgres))
            .await?;
        server_database(&server, DatabaseKind::Postgres, Some(template))
            .await
            .map(Some)
    }

    /// A copy of the template on the MySQL test server (a new database
    /// with the template's tables and rows); `None` (after a notice)
    /// without a server or its driver.
    pub async fn mysql(&self) -> Result<Option<TestDb>, BoxError> {
        let Some(server) = usable_server(TestServer::MySql).await? else {
            return Ok(None);
        };
        let template = self
            .mysql
            .get_or_try_init(|| build_server_template(self, &server, DatabaseKind::MySql))
            .await?;
        server_database(&server, DatabaseKind::MySql, Some(template))
            .await
            .map(Some)
    }

    /// The template's name as database names use it.
    fn slug(&self) -> String {
        let slug: String = self
            .name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .map(|c| c.to_ascii_lowercase())
            .take(20)
            .collect();
        if slug.is_empty() {
            "db".to_owned()
        } else {
            slug
        }
    }

    async fn run_build(&self, template: &TestDb) -> Result<(), BoxError> {
        (self.build)(template).await.map_err(|error| {
            format!("building the `{}` template failed: {error}", self.name).into()
        })
    }
}

/// A [`TestDb`] created on first use: tests that never ask for it never
/// create one.
///
/// ```
/// use navree_orm::testing::database::{LazyTestDb, TestDb};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let database = LazyTestDb::new(|| Box::pin(TestDb::sqlite()));
/// assert!(!database.is_created());
/// let test = database.get().await.unwrap();
/// assert!(database.is_created());
/// # let _ = test;
/// # }
/// ```
pub struct LazyTestDb {
    make: Box<dyn Fn() -> BoxFuture<'static, Result<TestDb, BoxError>> + Send + Sync>,
    cell: OnceCell<TestDb>,
}

impl fmt::Debug for LazyTestDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyTestDb")
            .field("database", &self.cell.get())
            .finish_non_exhaustive()
    }
}

impl LazyTestDb {
    /// A database `make` creates when it is first asked for
    /// (`|| Box::pin(TEMPLATE.sqlite())`).
    pub fn new(
        make: impl Fn() -> BoxFuture<'static, Result<TestDb, BoxError>> + Send + Sync + 'static,
    ) -> Self {
        LazyTestDb {
            make: Box::new(make),
            cell: OnceCell::new(),
        }
    }

    /// The database, created now if this is the first call.
    pub async fn get(&self) -> Result<&TestDb, BoxError> {
        self.cell.get_or_try_init(|| (self.make)()).await
    }

    /// Whether the database was created.
    pub fn is_created(&self) -> bool {
        self.cell.initialized()
    }
}

/// Empties every table of `db` but `except` (see [`TestDb::truncate`]).
pub async fn truncate_tables(db: &Db, except: &[&str]) -> Result<(), BoxError> {
    let kind = DatabaseKind::of(Executor::backend(db));
    let list = match kind {
        DatabaseKind::Sqlite => {
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        }
        DatabaseKind::Postgres => {
            "SELECT tablename AS name FROM pg_tables WHERE schemaname = current_schema() ORDER BY tablename"
        }
        DatabaseKind::MySql => {
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME"
        }
    };
    let tables: Vec<String> = strings(db, list)
        .await?
        .into_iter()
        .filter(|table| !except.contains(&table.as_str()))
        .collect();
    if tables.is_empty() {
        return Ok(());
    }
    // One connection throughout: the settings below are per session.
    let tx = db.begin().await?;
    match kind {
        DatabaseKind::Sqlite => {
            tx.execute_script("PRAGMA defer_foreign_keys = ON").await?;
            for table in &tables {
                tx.execute_script(&format!("DELETE FROM {}", kind.quote(table)))
                    .await?;
            }
            let sequences = strings(
                &tx,
                "SELECT name FROM sqlite_master WHERE name = 'sqlite_sequence'",
            )
            .await?;
            if !sequences.is_empty() {
                let names: Vec<String> = tables
                    .iter()
                    .map(|table| format!("'{}'", table.replace('\'', "''")))
                    .collect();
                tx.execute_script(&format!(
                    "DELETE FROM sqlite_sequence WHERE name IN ({})",
                    names.join(", ")
                ))
                .await?;
            }
        }
        DatabaseKind::Postgres => {
            let names: Vec<String> = tables.iter().map(|table| kind.quote(table)).collect();
            tx.execute_script(&format!(
                "TRUNCATE TABLE {} RESTART IDENTITY CASCADE",
                names.join(", ")
            ))
            .await?;
        }
        DatabaseKind::MySql => {
            tx.execute_script("SET FOREIGN_KEY_CHECKS = 0").await?;
            let mut outcome = Ok(());
            for table in &tables {
                // TRUNCATE commits implicitly; the connection stays this one.
                if let Err(error) = tx
                    .execute_script(&format!("TRUNCATE TABLE {}", kind.quote(table)))
                    .await
                {
                    outcome = Err(error);
                    break;
                }
            }
            tx.execute_script("SET FOREIGN_KEY_CHECKS = 1").await?;
            outcome?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// `sqlite://<file>?mode=rwc`.
fn sqlite_url(file: &Path) -> String {
    format!(
        "sqlite://{}?mode=rwc",
        file.display().to_string().replace('\\', "/")
    )
}

async fn connect(url: &str) -> Result<Db, BoxError> {
    Db::connect(&DbOptions {
        url: url.to_owned(),
        ..DbOptions::default()
    })
    .await
    .map_err(|error| {
        format!(
            "cannot connect to the test database {}: {error}",
            redact(url)
        )
        .into()
    })
}

/// Closes `db`'s pools (every clone of it).
async fn close_pool(db: &Db) {
    let _ = db.close().await;
}

/// The first column of `sql`'s rows, as text.
async fn strings(exec: &impl Executor, sql: &str) -> Result<Vec<String>, BoxError> {
    Ok(exec.fetch_column::<String>(&Statement::text(sql)).await?)
}

/// Seconds since the Unix epoch.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A new unique name: `<prefix><seconds>_<ulid>`.
fn unique_name(prefix: &str) -> String {
    format!(
        "{prefix}{}_{}",
        now_secs(),
        Ulid::new().to_string().to_lowercase()
    )
}

/// Whether `name` (`<prefix><seconds>_...`) was made more than
/// [`STALE_AFTER`] ago.
fn is_stale(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    let Some(seconds) = rest
        .split(['_', '-', '.'])
        .next()
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return false;
    };
    now_secs().saturating_sub(seconds) > STALE_AFTER.as_secs()
}

/// `url` without its password.
fn redact(url: &str) -> String {
    let Some(start) = url.find("://").map(|at| at + 3) else {
        return url.to_owned();
    };
    let rest = &url[start..];
    let Some(at) = rest.find('@') else {
        return url.to_owned();
    };
    match rest[..at].find(':') {
        Some(colon) => format!("{}{}:***{}", &url[..start], &rest[..colon], &rest[at..]),
        None => url.to_owned(),
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

/// The directory of SQLite template files.
fn sqlite_templates_dir() -> PathBuf {
    std::env::temp_dir().join("navree-test-templates")
}

/// Builds `template`'s SQLite file: a new file, built, checkpointed and
/// closed. Stale files of earlier runs are removed first.
async fn build_sqlite_template(template: &Template) -> Result<PathBuf, BoxError> {
    let dir = sqlite_templates_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let slug = template.slug();
    let prefix = format!("{slug}-");
    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_stale(&name, &prefix) {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }
    let file = dir.join(format!(
        "{prefix}{}-{}.sqlite",
        now_secs(),
        Ulid::new().to_string().to_lowercase()
    ));
    let url = sqlite_url(&file);
    let built = TestDb {
        db: connect(&url).await?,
        url,
        kind: DatabaseKind::Sqlite,
        cleanup: Cleanup::Nothing,
    };
    template.run_build(&built).await?;
    // Every committed page into the main file, which the copies copy.
    built
        .db
        .execute_script("PRAGMA wal_checkpoint(TRUNCATE)")
        .await?;
    close_pool(&built.db).await;
    Ok(file)
}

/// Builds `template` as a database of its own on a server; returns its
/// name. Stale templates and test databases of earlier runs are dropped
/// first.
async fn build_server_template(
    template: &Template,
    server: &str,
    kind: DatabaseKind,
) -> Result<String, BoxError> {
    let admin = admin_db(server).await?;
    drop_stale(
        &admin,
        kind,
        &format!("{TEMPLATE_PREFIX}{}_", template.slug()),
    )
    .await;
    drop_stale(&admin, kind, TEST_PREFIX).await;
    let name = unique_name(&format!("{TEMPLATE_PREFIX}{}_", template.slug()));
    create_database(&admin, kind, &name, None).await?;
    let url = with_database(server, &name);
    let built = TestDb {
        db: connect(&url).await?,
        url,
        kind,
        cleanup: Cleanup::Nothing,
    };
    let outcome = template.run_build(&built).await;
    close_pool(&built.db).await;
    if let Err(error) = outcome {
        let _ = drop_database(server, &name, kind).await;
        return Err(error);
    }
    if kind == DatabaseKind::Postgres {
        // A copy needs the template without connections (an app the build
        // booted may still be closing its pool).
        admin
            .fetch_all(&Statement::new(
                Backend::Postgres,
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
                [name.clone().into()],
            ))
            .await?;
    }
    close_pool(&admin).await;
    Ok(name)
}

/// A new database on `server` (a copy of `template`, if given), as a
/// [`TestDb`] that drops it.
async fn server_database(
    server: &str,
    kind: DatabaseKind,
    template: Option<&str>,
) -> Result<TestDb, BoxError> {
    let admin = admin_db(server).await?;
    let name = unique_name(TEST_PREFIX);
    let created = create_database(&admin, kind, &name, template).await;
    close_pool(&admin).await;
    created?;
    let cleanup = Cleanup::Database {
        admin_url: server.to_owned(),
        name: name.clone(),
        kind,
    };
    let url = with_database(server, &name);
    let db = match connect(&url).await {
        Ok(db) => db,
        Err(error) => {
            let _ = drop_database(server, &name, kind).await;
            return Err(error);
        }
    };
    // From here on, dropping `test` drops the database.
    let test = TestDb {
        db,
        url,
        kind,
        cleanup,
    };
    if let (DatabaseKind::MySql, Some(template)) = (kind, template) {
        copy_mysql_tables(&test, template).await?;
    }
    Ok(test)
}

/// `server`'s URL when it is configured and this binary can talk to it
/// (navree-db's driver for it compiled in); `None`, after a notice, when
/// either is missing.
async fn usable_server(server: TestServer) -> Result<Option<String>, BoxError> {
    let Some(url) = server.url() else {
        return Ok(None);
    };
    match Db::connect(&DbOptions {
        url: url.clone(),
        max_connections: 1,
        ..DbOptions::default()
    })
    .await
    {
        Ok(db) => {
            close_pool(&db).await;
            Ok(Some(url))
        }
        Err(error) if error.to_string().contains("no supporting driver") => {
            skip(&format!(
                "{} is set, but this test binary has no driver for it: {error}",
                server.variable()
            ));
            Ok(None)
        }
        Err(error) => Err(format!(
            "cannot connect to the test server {} ({}): {error}",
            redact(&url),
            server.variable()
        )
        .into()),
    }
}

/// A pool on the server's own database, for creating and dropping others.
async fn admin_db(server: &str) -> Result<Db, BoxError> {
    Db::connect(&DbOptions {
        url: server.to_owned(),
        max_connections: 2,
        ..DbOptions::default()
    })
    .await
    .map_err(|error| {
        format!(
            "cannot connect to the test server {}: {error}",
            redact(server)
        )
        .into()
    })
}

/// `CREATE DATABASE name` (from `template` on PostgreSQL).
async fn create_database(
    admin: &Db,
    kind: DatabaseKind,
    name: &str,
    template: Option<&str>,
) -> Result<(), BoxError> {
    let sql = match (kind, template) {
        (DatabaseKind::Postgres, Some(template)) => format!(
            "CREATE DATABASE {} TEMPLATE {}",
            kind.quote(name),
            kind.quote(template)
        ),
        (DatabaseKind::MySql, _) => {
            format!("CREATE DATABASE {} CHARACTER SET utf8mb4", kind.quote(name))
        }
        _ => format!("CREATE DATABASE {}", kind.quote(name)),
    };
    // A template another session still touches is retried briefly.
    let mut attempts = 0;
    loop {
        match admin.execute_script(&sql).await {
            Ok(_) => return Ok(()),
            Err(error)
                if attempts < 20 && error.to_string().contains("being accessed by other users") =>
            {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => return Err(format!("{sql}: {error}").into()),
        }
    }
}

/// Drops database `name` on `server`.
async fn drop_database(server: &str, name: &str, kind: DatabaseKind) -> Result<(), BoxError> {
    let admin = admin_db(server).await?;
    let sql = match kind {
        DatabaseKind::Postgres => {
            format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", kind.quote(name))
        }
        _ => format!("DROP DATABASE IF EXISTS {}", kind.quote(name)),
    };
    let outcome = admin.execute_script(&sql).await;
    close_pool(&admin).await;
    outcome?;
    Ok(())
}

/// Drops the databases named `<prefix><seconds>_...` made more than
/// [`STALE_AFTER`] ago (left by crashed runs). Best effort.
async fn drop_stale(admin: &Db, kind: DatabaseKind, prefix: &str) {
    let list = match kind {
        DatabaseKind::Postgres => "SELECT datname AS name FROM pg_database",
        _ => "SELECT SCHEMA_NAME AS name FROM information_schema.SCHEMATA",
    };
    let Ok(names) = strings(&admin, list).await else {
        return;
    };
    for name in names.iter().filter(|name| is_stale(name, prefix)) {
        let sql = match kind {
            DatabaseKind::Postgres => {
                format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", kind.quote(name))
            }
            _ => format!("DROP DATABASE IF EXISTS {}", kind.quote(name)),
        };
        let _ = admin.execute_script(&sql).await;
    }
}

/// Copies every table of MySQL database `template` (structure, foreign
/// keys and rows) into `test`'s database.
async fn copy_mysql_tables(test: &TestDb, template: &str) -> Result<(), BoxError> {
    let kind = DatabaseKind::MySql;
    let tables: Vec<String> = test
        .db
        .fetch_column(&Statement::new(
            Backend::MySql,
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME",
            [template.into()],
        ))
        .await?;
    // One connection: foreign key checks are off for the session.
    let tx = test.db.begin().await?;
    tx.execute_script("SET FOREIGN_KEY_CHECKS = 0").await?;
    for table in &tables {
        let source = format!("{}.{}", kind.quote(template), kind.quote(table));
        let create = tx
            .fetch_optional(&Statement::text(format!("SHOW CREATE TABLE {source}")))
            .await?
            .map(|row| row.get::<String>("Create Table"))
            .transpose()?
            .ok_or_else(|| format!("SHOW CREATE TABLE {source} returned nothing"))?;
        tx.execute_script(&create).await?;
        tx.execute_script(&format!(
            "INSERT INTO {} SELECT * FROM {source}",
            kind.quote(table)
        ))
        .await?;
    }
    tx.execute_script("SET FOREIGN_KEY_CHECKS = 1").await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_files_are_read_like_a_shell_would() {
        let text = "# comment\nexport NAVREE_TEST_POSTGRES_URL='postgres://u:p@h:1/db'\nNAVREE_TEST_MYSQL_URL=\"mysql://h/db\"\nEMPTY=''\n";
        assert_eq!(
            env_file_value(text, "NAVREE_TEST_POSTGRES_URL").as_deref(),
            Some("postgres://u:p@h:1/db")
        );
        assert_eq!(
            env_file_value(text, "NAVREE_TEST_MYSQL_URL").as_deref(),
            Some("mysql://h/db")
        );
        assert_eq!(env_file_value(text, "EMPTY"), None);
        assert_eq!(env_file_value(text, "NAVREE_TEST_MONGODB_URL"), None);
    }

    #[test]
    fn only_a_one_requires_the_servers() {
        assert!(requires(Some("1")));
        for other in [None, Some(""), Some("0"), Some("true"), Some("yes")] {
            assert!(!requires(other), "{other:?}");
        }
    }

    #[test]
    #[should_panic(
        expected = "NAVREE_TEST_EXAMPLE_URL is not set (NAVREE_TEST_REQUIRE_SERVICES=1)"
    )]
    fn a_test_that_would_skip_fails_when_the_servers_are_required() {
        skip_unless(true, "NAVREE_TEST_EXAMPLE_URL is not set");
    }

    #[test]
    fn a_test_that_would_skip_skips_when_they_are_not() {
        // A variable no suite reads, so the notice this prints is never a real
        // server's (scripts/services/skips.allow lists it).
        skip_unless(false, "NAVREE_TEST_EXAMPLE_URL is not set");
    }

    #[test]
    fn names_are_unique_and_age() {
        let name = unique_name(TEST_PREFIX);
        assert!(name.starts_with(TEST_PREFIX) && name.len() <= 63, "{name}");
        assert!(!is_stale(&name, TEST_PREFIX));
        let old = format!("{TEST_PREFIX}{}_x", now_secs() - STALE_AFTER.as_secs() - 60);
        assert!(is_stale(&old, TEST_PREFIX));
        assert!(
            !is_stale("navree_test", TEST_PREFIX),
            "the shared database stays"
        );
        assert!(!is_stale("other_123_x", TEST_PREFIX));
    }

    #[test]
    fn urls_are_rewritten_and_redacted() {
        assert_eq!(
            with_database("mysql://root:pw@127.0.0.1:53306/navree_test", "x"),
            "mysql://root:pw@127.0.0.1:53306/x"
        );
        assert_eq!(
            with_database("postgres://h:5432/a?sslmode=disable", "b"),
            "postgres://h:5432/b?sslmode=disable"
        );
        assert_eq!(
            redact("postgres://postgres:navree@127.0.0.1:55432/x"),
            "postgres://postgres:***@127.0.0.1:55432/x"
        );
        assert_eq!(redact("sqlite::memory:"), "sqlite::memory:");
    }

    #[test]
    fn template_names_keep_identifier_characters() {
        let template = Template::new("CRM schema-v2!", |_| Box::pin(async { Ok(()) }));
        assert_eq!(template.slug(), "crmschemav2");
        let blank = Template::new("--", |_| Box::pin(async { Ok(()) }));
        assert_eq!(blank.slug(), "db");
    }
}
