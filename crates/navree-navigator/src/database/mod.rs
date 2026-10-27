//! The app's database, seen read-only: `query_database` and
//! `describe_schema`.
//!
//! Navigator connects to `database.url` itself (one connection per call,
//! closed afterwards), never through the app:
//!
//! - **SQLite** files are opened without creating them and with `PRAGMA
//!   query_only = ON`; relative paths are relative to the app.
//! - **PostgreSQL** sessions default to read-only transactions, and each
//!   query runs inside `BEGIN READ ONLY` with a statement timeout, then
//!   rolls back.
//! - Unless `--allow-remote-database` is given, only SQLite files and
//!   servers on this machine (`localhost`, a loopback address or a Unix
//!   socket) are allowed ([`check_local`]).

mod guard;
mod query;
mod schema;

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::ConnectOptions;
use sqlx::postgres::{PgConnectOptions, PgConnection};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};

pub use guard::{CheckedStatement, Dialect, Refusal, StatementKind, check};
pub use query::{MASKED_COLUMN_PARTS, QueryResult, is_masked_column};
pub use schema::{AppliedMigration, Column, ForeignKey, Index, Schema, Table};

/// The most rows `query_database` returns.
pub const ROW_LIMIT: usize = 100;

/// How long one statement may run.
pub const STATEMENT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long connecting may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The database drivers Navigator reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driver {
    /// `sqlite:` URLs.
    Sqlite,
    /// `postgres:` and `postgresql:` URLs.
    Postgres,
}

impl Driver {
    /// The driver of `url`, by its scheme.
    pub fn of(url: &str) -> Result<Driver, DatabaseError> {
        let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
        match scheme.as_str() {
            "sqlite" => Ok(Driver::Sqlite),
            "postgres" | "postgresql" => Ok(Driver::Postgres),
            _ => Err(DatabaseError::Unsupported { scheme }),
        }
    }

    /// `sqlite` or `postgres`.
    pub fn name(self) -> &'static str {
        match self {
            Driver::Sqlite => "sqlite",
            Driver::Postgres => "postgres",
        }
    }

    /// The SQL dialect statements are checked with.
    pub fn dialect(self) -> Dialect {
        match self {
            Driver::Sqlite => Dialect::Sqlite,
            Driver::Postgres => Dialect::Postgres,
        }
    }
}

/// Why a database tool could not answer.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    /// No `database.url` in the configuration.
    #[error(
        "the app has no database configured: set `database.url` in config/app.toml or APP_DATABASE__URL in .env"
    )]
    NotConfigured,
    /// A scheme other than `sqlite`, `postgres` or `postgresql`.
    #[error("Navigator reads SQLite and PostgreSQL databases; `{scheme}:` URLs are not supported")]
    Unsupported {
        /// The URL scheme.
        scheme: String,
    },
    /// A server that isn't on this machine, without `--allow-remote-database`.
    #[error(
        "database.url points at {host}, which isn't on this machine. Navigator only connects to local databases \
         (SQLite files, localhost, loopback addresses and Unix sockets), so a staging or production URL in a \
         forgotten .env is never queried. Start Navigator with `navree mcp --allow-remote-database` if you \
         really mean to read that database."
    )]
    Remote {
        /// The host named in the URL.
        host: String,
    },
    /// The SQLite file does not exist (Navigator never creates one).
    #[error("the SQLite database {} doesn't exist yet; run `navree migrate` to create it", .path.display())]
    MissingFile {
        /// The file.
        path: PathBuf,
    },
    /// The statement was refused before it was sent.
    #[error("refused: {0}")]
    Refused(#[from] Refusal),
    /// Connecting failed.
    #[error("cannot connect to the database ({url}): {message}")]
    Connect {
        /// The URL with its password masked.
        url: String,
        /// The driver's message.
        message: String,
    },
    /// The database answered with an error.
    #[error("the database returned an error: {0}")]
    Query(String),
    /// The statement ran longer than [`STATEMENT_TIMEOUT`].
    #[error("the statement ran longer than {} seconds and was cancelled", STATEMENT_TIMEOUT.as_secs())]
    Timeout,
}

impl From<sqlx::Error> for DatabaseError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(db) if is_timeout(db.code().as_deref(), db.message()) => {
                DatabaseError::Timeout
            }
            sqlx::Error::Database(db) => DatabaseError::Query(db.message().to_owned()),
            _ => DatabaseError::Query(error.to_string()),
        }
    }
}

/// PostgreSQL's `query_canceled` (statement timeout) and SQLite's
/// `interrupted` (the progress handler's deadline).
fn is_timeout(code: Option<&str>, message: &str) -> bool {
    code == Some("57014") || code == Some("9") || message.contains("interrupted")
}

/// Where a database is, once its URL is checked.
#[derive(Clone, Debug)]
pub struct Target {
    /// The driver.
    pub driver: Driver,
    /// The URL as configured.
    url: String,
    /// The app's directory (relative SQLite paths start there).
    root: PathBuf,
}

impl Target {
    /// Checks `url` (the driver, and that it is local unless `allow_remote`)
    /// for the app at `root`.
    pub fn new(url: &str, root: &Path, allow_remote: bool) -> Result<Target, DatabaseError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(DatabaseError::NotConfigured);
        }
        let driver = Driver::of(url)?;
        if !allow_remote {
            check_local(url)?;
        }
        Ok(Target {
            driver,
            url: url.to_owned(),
            root: root.to_path_buf(),
        })
    }

    /// The URL with its password masked, for messages.
    pub fn redacted_url(&self) -> String {
        redact_url(&self.url)
    }

    /// Opens a SQLite connection: never creates the file, `query_only` on.
    async fn sqlite(&self) -> Result<SqliteConnection, DatabaseError> {
        let options =
            SqliteConnectOptions::from_str(&self.url).map_err(|e| self.connect_error(e))?;
        let file = options.get_filename().to_path_buf();
        let memory = file.as_os_str() == ":memory:" || self.url.contains(":memory:");
        let options = if memory {
            options
        } else {
            let path = if file.is_absolute() {
                file
            } else {
                self.root.join(file)
            };
            if !path.is_file() {
                return Err(DatabaseError::MissingFile { path });
            }
            options.filename(path)
        };
        let options = options
            .create_if_missing(false)
            .pragma("query_only", "ON")
            .busy_timeout(Duration::from_secs(1))
            .disable_statement_logging();
        tokio::time::timeout(CONNECT_TIMEOUT, options.connect())
            .await
            .map_err(|_| self.connect_error("timed out"))?
            .map_err(|e| self.connect_error(e))
    }

    /// Opens a PostgreSQL connection whose transactions are read-only.
    async fn postgres(&self) -> Result<PgConnection, DatabaseError> {
        let options = PgConnectOptions::from_str(&self.url)
            .map_err(|e| self.connect_error(e))?
            .application_name("navree-navigator")
            .options([("default_transaction_read_only", "on")])
            .disable_statement_logging();
        tokio::time::timeout(CONNECT_TIMEOUT, options.connect())
            .await
            .map_err(|_| self.connect_error("timed out"))?
            .map_err(|e| self.connect_error(e))
    }

    fn connect_error(&self, error: impl std::fmt::Display) -> DatabaseError {
        DatabaseError::Connect {
            url: self.redacted_url(),
            message: error.to_string(),
        }
    }
}

/// Fails with [`DatabaseError::Remote`] unless `url` is a SQLite URL or
/// names only local hosts: `localhost`, a loopback address (`127.0.0.1`,
/// `::1`), a Unix socket directory, or no host at all (the default socket).
pub fn check_local(url: &str) -> Result<(), DatabaseError> {
    if Driver::of(url)? == Driver::Sqlite {
        return Ok(());
    }
    for host in hosts(url) {
        if !is_local_host(&host) {
            return Err(DatabaseError::Remote { host });
        }
    }
    Ok(())
}

/// Every host a PostgreSQL URL names: the authority's hosts (comma
/// separated) and the `host` / `hostaddr` query parameters.
fn hosts(url: &str) -> Vec<String> {
    let rest = url.split_once("://").map_or("", |(_, rest)| rest);
    let (before_query, query) = rest.split_once('?').unwrap_or((rest, ""));
    let authority = before_query.split('/').next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
    let mut hosts: Vec<String> = Vec::new();
    for part in authority.split(',') {
        let host = if let Some(stripped) = part.strip_prefix('[') {
            stripped.split(']').next().unwrap_or("").to_owned()
        } else {
            part.split(':').next().unwrap_or("").to_owned()
        };
        let host = percent_decode(&host);
        if !host.is_empty() {
            hosts.push(host);
        }
    }
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=')
            && (key == "host" || key == "hostaddr")
        {
            hosts.extend(
                percent_decode(value)
                    .split(',')
                    .filter(|h| !h.is_empty())
                    .map(str::to_owned),
            );
        }
    }
    hosts
}

fn is_local_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.starts_with('/') {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `scheme://user:password@host/...` with the password replaced by `***`.
pub fn redact_url(url: &str) -> String {
    let Some(start) = url.find("://").map(|i| i + 3) else {
        return url.to_owned();
    };
    let rest = &url[start..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    match authority.rfind('@') {
        Some(at) => match authority[..at].find(':') {
            Some(colon) => format!(
                "{}{}:***{}",
                &url[..start],
                &authority[..colon],
                &rest[at..]
            ),
            None => url.to_owned(),
        },
        None => url.to_owned(),
    }
}

/// Runs one read-only statement (see [`check`]) and returns at most
/// [`ROW_LIMIT`] rows, secret-looking columns masked.
pub async fn query_database(target: &Target, sql: &str) -> Result<QueryResult, DatabaseError> {
    let checked = check(sql, target.driver.dialect())?;
    match target.driver {
        Driver::Sqlite => {
            let mut conn = target.sqlite().await?;
            let result = query::sqlite(&mut conn, &checked).await;
            close(sqlx::Connection::close(conn)).await;
            result
        }
        Driver::Postgres => {
            let mut conn = target.postgres().await?;
            let result = query::postgres(&mut conn, &checked).await;
            close(sqlx::Connection::close(conn)).await;
            result
        }
    }
}

/// The tables (or the one named `table`) with their columns, indexes and
/// foreign keys, plus the rows of `navree_migrations`.
pub async fn describe_schema(
    target: &Target,
    table: Option<&str>,
) -> Result<Schema, DatabaseError> {
    match target.driver {
        Driver::Sqlite => {
            let mut conn = target.sqlite().await?;
            let result = schema::sqlite(&mut conn, table).await;
            close(sqlx::Connection::close(conn)).await;
            result
        }
        Driver::Postgres => {
            let mut conn = target.postgres().await?;
            let result = schema::postgres(&mut conn, table).await;
            close(sqlx::Connection::close(conn)).await;
            result
        }
    }
}

/// Closes a connection, ignoring errors (the answer is already computed).
async fn close<F: std::future::Future<Output = Result<(), sqlx::Error>>>(closing: F) {
    let _ = tokio::time::timeout(Duration::from_secs(2), closing).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drivers_come_from_the_scheme() {
        assert_eq!(
            Driver::of("sqlite://storage/app.db?mode=rwc").unwrap(),
            Driver::Sqlite
        );
        assert_eq!(Driver::of("sqlite::memory:").unwrap(), Driver::Sqlite);
        assert_eq!(
            Driver::of("postgres://localhost/crm").unwrap(),
            Driver::Postgres
        );
        assert_eq!(
            Driver::of("postgresql://localhost/crm").unwrap(),
            Driver::Postgres
        );
        assert!(matches!(
            Driver::of("mysql://localhost/crm"),
            Err(DatabaseError::Unsupported { scheme }) if scheme == "mysql"
        ));
    }

    #[test]
    fn local_databases_are_allowed() {
        for url in [
            "sqlite://storage/app.db?mode=rwc",
            "postgres://navree:navree@127.0.0.1:5432/crm",
            "postgres://navree:navree@localhost/crm",
            "postgresql://navree@[::1]:5432/crm",
            "postgres:///crm",
            "postgres://navree@/crm?host=/var/run/postgresql",
            "postgres://navree@%2Fvar%2Frun%2Fpostgresql/crm",
            "postgres://u@127.0.0.2/crm",
        ] {
            assert!(check_local(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn remote_databases_are_refused() {
        for (url, host) in [
            (
                "postgres://app:secret@db.staging.example.com:5432/crm",
                "db.staging.example.com",
            ),
            ("postgres://app@10.0.0.5/crm", "10.0.0.5"),
            (
                "postgres://app@localhost,db.example.com/crm",
                "db.example.com",
            ),
            (
                "postgres://app@localhost/crm?host=db.example.com",
                "db.example.com",
            ),
            (
                "postgres://app@localhost/crm?hostaddr=192.168.1.4",
                "192.168.1.4",
            ),
        ] {
            match check_local(url) {
                Err(DatabaseError::Remote { host: found }) => assert_eq!(found, host, "{url}"),
                other => panic!("{url}: {other:?}"),
            }
        }
        let message = check_local("postgres://a@db.example.com/x")
            .unwrap_err()
            .to_string();
        assert!(message.contains("--allow-remote-database"), "{message}");
    }

    #[test]
    fn the_flag_allows_remote_databases() {
        let root = Path::new(".");
        assert!(Target::new("postgres://a@db.example.com/x", root, false).is_err());
        assert!(Target::new("postgres://a@db.example.com/x", root, true).is_ok());
        assert!(matches!(
            Target::new("  ", root, true),
            Err(DatabaseError::NotConfigured)
        ));
    }

    #[test]
    fn passwords_are_masked_in_urls() {
        assert_eq!(
            redact_url("postgres://navree:s3cret@127.0.0.1:5432/crm"),
            "postgres://navree:***@127.0.0.1:5432/crm"
        );
        assert_eq!(
            redact_url("postgres://localhost/crm"),
            "postgres://localhost/crm"
        );
        assert_eq!(
            redact_url("sqlite://storage/app.db"),
            "sqlite://storage/app.db"
        );
    }
}
