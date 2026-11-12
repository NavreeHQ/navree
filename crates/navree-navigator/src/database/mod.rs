//! The app's database, seen read-only: `query_database` and
//! `describe_schema`.
//!
//! Navigator connects to `database.url` (or `NAVREE_MCP_DATABASE_URL`)
//! itself, one connection per call, closed afterwards, never through the
//! app. The database is what keeps it read-only; the statement guard
//! ([`check`]) is a second layer:
//!
//! - **SQLite** files are opened read-only (`SQLITE_OPEN_READONLY`), never
//!   created, with `PRAGMA query_only = ON`; relative paths are relative to
//!   the app. For `query_database`, SQLite itself prepares the text and
//!   refuses anything after the first statement, the statement must be
//!   read-only by SQLite's account and take no parameters, and an authorizer
//!   allows only reading (see `query::sqlite`).
//! - **PostgreSQL** sessions default to read-only transactions and pin
//!   `standard_conforming_strings = on`, checked after connecting, so the
//!   server reads a statement's strings as the guard did. `query_database`
//!   refuses a role that is, or can become, a superuser, or is a member of
//!   one of [`PRIVILEGED_ROLES`], unless
//!   `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1` is given: such a role
//!   reaches the server's files, programs and other sessions from a query,
//!   which no read-only transaction stops. It also refuses a role that may
//!   execute a `SECURITY DEFINER` function that is not an extension's, a
//!   function in an untrusted language, or a function of an extension that
//!   reaches files, programs or the network (dblink, postgres_fdw,
//!   file_fdw, adminpack, http, pg_net, ...). Each query runs inside
//!   `BEGIN READ ONLY` (which pins `standard_conforming_strings` again)
//!   with a statement timeout, is sent as one prepared statement (the
//!   extended protocol takes exactly one), reads each value as text cut on
//!   the server, then rolls back. What is left is what an ordinary role may
//!   still do: the functions of the extensions an administrator installed
//!   (PostGIS, pgcrypto, TimescaleDB), their C code and their own SECURITY
//!   DEFINER functions, run as written, and an extension Navigator doesn't
//!   know can add a function that reaches out under any name; that, and the
//!   checks above, are what "read-only" rests on.
//! - Unless `--allow-remote-database` is given, only SQLite files and
//!   servers on this machine (`localhost`, a loopback address or a Unix
//!   socket) are allowed ([`check_local`]).

mod guard;
mod query;
mod schema;

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgConnection};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Executor, Row};

pub use guard::{
    CheckedStatement, Dialect, MAX_EXPRESSION_DEPTH, MAX_SQL_BYTES, Refusal, StatementKind, check,
};
pub use query::{MASKED_COLUMN_PARTS, QueryResult, is_masked_column};
pub use schema::{AppliedMigration, Column, ForeignKey, Index, Schema, Table};

/// The most rows `query_database` returns.
pub const ROW_LIMIT: usize = 100;

/// How long one statement may run.
pub const STATEMENT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long connecting may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The settings every PostgreSQL connection starts with (see
/// `Target::postgres`): read-only transactions, and plain strings in which a
/// backslash is an ordinary character.
const POSTGRES_SESSION: [(&str, &str); 2] = [
    ("default_transaction_read_only", "on"),
    ("standard_conforming_strings", "on"),
];

/// The database drivers Navigator reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
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
#[non_exhaustive]
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
    /// The PostgreSQL session reads backslash escapes in plain strings
    /// although Navigator asked for `standard_conforming_strings = on`.
    #[error(
        "the server reports standard_conforming_strings = {setting} although Navigator asked for `on` when \
         connecting; a connection pooler that drops startup options (PgBouncer's ignore_startup_parameters) \
         does that. Navigator only queries PostgreSQL with it on: connect it to the server directly, or let \
         the pooler pass `options`."
    )]
    StringsNotStandard {
        /// What the server reported.
        setting: String,
    },
    /// `query_database` would run as a role that can act outside the
    /// read-only transaction, without `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`.
    #[error(
        "query_database does not run as the PostgreSQL role `{role}`: it has {}, and from a query such a role \
         can read or write the server's files, run programs or end other sessions, outside the read-only \
         transaction. Give Navigator a read-only role instead, for example:\n\n\
         CREATE ROLE navree_reader LOGIN PASSWORD 'choose-one';\n\
         GRANT CONNECT ON DATABASE your_db TO navree_reader;\n\
         GRANT USAGE ON SCHEMA public TO navree_reader;\n\
         GRANT SELECT ON ALL TABLES IN SCHEMA public TO navree_reader;\n\
         ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO navree_reader;\n\n\
         and start Navigator with `NAVREE_MCP_DATABASE_URL=postgres://navree_reader:...@localhost/your_db navree mcp` \
         (run the last line as the role that creates your tables). The role must be no superuser and \
         no member of one, and may execute no SECURITY DEFINER function of the database's own, no function in an untrusted \
         language and no function of an extension that reaches files, programs or the network \
         (Navigator checks). To run as `{role}` anyway, start it with \
         `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1 navree mcp`.",
        .privileges.join(", ")
    )]
    PrivilegedRole {
        /// The session's role.
        role: String,
        /// What makes it privileged: `superuser`, or the predefined roles it
        /// is a member of.
        privileges: Vec<String>,
    },
    /// `query_database` would run as a role that may execute functions that
    /// act with more than its own privileges, or outside the database,
    /// without `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`.
    #[error(
        "query_database does not run as the PostgreSQL role `{role}`: it may execute {}{}, which run with \
         their owner's privileges (SECURITY DEFINER), in an untrusted language as the server's operating \
         system user, or in an extension that reaches files, programs or the network, so a query could \
         act outside the read-only transaction. Revoke EXECUTE on them from PUBLIC and from the role \
         (REVOKE EXECUTE ON FUNCTION ... FROM PUBLIC, {role}), or start Navigator with \
         `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1 navree mcp` to run anyway.",
        .functions.join(", "),
        .more.then_some(" and more").unwrap_or_default()
    )]
    FunctionsReachOut {
        /// The session's role.
        role: String,
        /// The first of those functions, with their argument types.
        functions: Vec<String>,
        /// Whether there are more than listed.
        more: bool,
    },
}

/// The most functions [`DatabaseError::FunctionsReachOut`] names.
const LISTED_FUNCTIONS: usize = 10;

/// The functions outside `pg_catalog` and `information_schema` the session
/// may call (EXECUTE on the function, USAGE on its schema) that act beyond
/// the role: SECURITY DEFINER or untrusted-language functions of its own
/// database, and the functions of the extensions that reach outside the
/// database (dblink, postgres_fdw, file_fdw, adminpack, http, pg_net,
/// pg_background, pg_cron, pgagent and the untrusted languages) or of an
/// extension written in an untrusted procedural language. The C functions of other
/// extensions (pgcrypto, PostGIS, TimescaleDB, pgvector) are code the
/// server's administrator installed and are not counted. The first
/// [`LISTED_FUNCTIONS`] + 1, so the refusal knows there are more.
const REACHING_FUNCTIONS: &str = "SELECT p.oid::pg_catalog.regprocedure::text \
     FROM pg_catalog.pg_proc p \
     JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
     JOIN pg_catalog.pg_language l ON l.oid = p.prolang \
     LEFT JOIN pg_catalog.pg_depend d ON d.classid = 'pg_catalog.pg_proc'::pg_catalog.regclass \
       AND d.objid = p.oid AND d.deptype = 'e' \
     LEFT JOIN pg_catalog.pg_extension e ON e.oid = d.refobjid \
     WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') \
     AND p.prokind = 'f' \
     AND CASE WHEN e.oid IS NULL THEN p.prosecdef OR NOT l.lanpltrusted \
       ELSE e.extname IN ('adminpack', 'dblink', 'file_fdw', 'http', 'pg_background', 'pg_cron', \
         'pg_net', 'pgagent', 'plperlu', 'plpython3u', 'plpythonu', 'pltclu', 'postgres_fdw') \
         OR (NOT l.lanpltrusted AND l.lanname NOT IN ('c', 'internal')) END \
     AND pg_catalog.has_schema_privilege(n.oid, 'USAGE') \
     AND pg_catalog.has_function_privilege(p.oid, 'EXECUTE') \
     ORDER BY 1 LIMIT 11";

/// The predefined PostgreSQL roles whose members reach outside the database
/// from a query: server files, server programs, other sessions.
/// `pg_signal_autovacuum_worker` exists from PostgreSQL 18; an older server
/// has none to be a member of.
pub const PRIVILEGED_ROLES: &[&str] = &[
    "pg_read_server_files",
    "pg_write_server_files",
    "pg_execute_server_program",
    "pg_signal_backend",
    "pg_signal_autovacuum_worker",
];

/// The session's roles (`current_user`, then `session_user` when it
/// differs), whether either is a superuser or a member of a superuser role
/// (a member can `SET ROLE` to it), and the roles of [`PRIVILEGED_ROLES`]
/// either is a member of (comma-separated, or NULL). Every catalog name is
/// qualified, so nothing on the search path stands in for it. It always
/// answers one row.
const ROLE_PRIVILEGES: &str = "SELECT \
     CASE WHEN current_user = session_user THEN current_user::text \
          ELSE current_user::text || ' (session ' || session_user::text || ')' END, \
     EXISTS (SELECT 1 FROM pg_catalog.pg_roles s WHERE s.rolsuper \
       AND (pg_catalog.pg_has_role(current_user, s.oid, 'MEMBER') \
         OR pg_catalog.pg_has_role(session_user, s.oid, 'MEMBER'))), \
     (SELECT pg_catalog.string_agg(g.rolname::text, ',' ORDER BY g.rolname) \
      FROM pg_catalog.pg_roles g \
      WHERE g.rolname IN ('pg_read_server_files', 'pg_write_server_files', \
        'pg_execute_server_program', 'pg_signal_backend', 'pg_signal_autovacuum_worker') \
      AND (pg_catalog.pg_has_role(current_user, g.oid, 'MEMBER') \
        OR pg_catalog.pg_has_role(session_user, g.oid, 'MEMBER')))";

/// What makes a role privileged: `superuser`, then the predefined roles it
/// is a member of (`members`, comma-separated).
fn privileges_of(superuser: bool, members: Option<&str>) -> Vec<String> {
    let mut privileges = Vec::new();
    if superuser {
        privileges.push("superuser".to_owned());
    }
    privileges.extend(
        members
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|name| PRIVILEGED_ROLES.contains(name))
            .map(str::to_owned),
    );
    privileges
}

/// Fails unless `standard_conforming_strings` reads `on`.
fn standard_strings(setting: Option<&str>) -> Result<(), DatabaseError> {
    match setting {
        Some("on") => Ok(()),
        other => Err(DatabaseError::StringsNotStandard {
            setting: other.unwrap_or("unknown").to_owned(),
        }),
    }
}

/// The first column of the first row of `sql`, as text (simple protocol).
async fn first_text(
    conn: &mut PgConnection,
    sql: &'static str,
) -> Result<Option<String>, DatabaseError> {
    let rows = conn.fetch_all(sql).await?;
    Ok(rows.first().and_then(|row| {
        row.try_get_raw(0)
            .ok()
            .filter(|raw| !sqlx::ValueRef::is_null(raw))
            .and_then(|raw| raw.as_str().ok().map(str::to_owned))
    }))
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
    /// Whether `query_database` may run as a privileged PostgreSQL role
    /// (`NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`).
    allow_privileged: bool,
    /// Said after a failed connection (the dock's read-only role may not
    /// exist yet).
    hint: Option<&'static str>,
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
            allow_privileged: false,
            hint: None,
        })
    }

    /// Lets `query_database` run as a PostgreSQL role with superuser or
    /// server-file, server-program or signal privileges
    /// (`NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`). Off by default: such a role can
    /// act outside the read-only transaction, so a query is refused.
    pub fn allow_privileged_role(mut self, allow: bool) -> Target {
        self.allow_privileged = allow;
        self
    }

    /// The URL with its password masked, for messages.
    pub fn redacted_url(&self) -> String {
        redact_url(&self.url)
    }

    /// The SQLite database of the URL: `:memory:`, or a file that exists
    /// (relative paths start at the app; Navigator never creates one).
    fn sqlite_file(&self) -> Result<query::SqliteFile, DatabaseError> {
        let options =
            SqliteConnectOptions::from_str(&self.url).map_err(|e| self.connect_error(e))?;
        let file = options.get_filename().to_path_buf();
        if file.as_os_str() == ":memory:" || self.url.contains(":memory:") {
            return Ok(query::SqliteFile::Memory);
        }
        let path = if file.is_absolute() {
            file
        } else {
            self.root.join(file)
        };
        if !path.is_file() {
            return Err(DatabaseError::MissingFile { path });
        }
        Ok(query::SqliteFile::Path(path))
    }

    /// Opens a SQLite connection for `describe_schema`: read-only
    /// (`SQLITE_OPEN_READONLY`, which also holds for any database attached to
    /// it), never creates the file, `query_only` on. `query_database` runs
    /// through [`query::sqlite`] instead.
    async fn sqlite(&self) -> Result<SqliteConnection, DatabaseError> {
        let options =
            SqliteConnectOptions::from_str(&self.url).map_err(|e| self.connect_error(e))?;
        let options = match self.sqlite_file()? {
            query::SqliteFile::Memory => options,
            query::SqliteFile::Path(path) => options.filename(path),
        };
        let options = options
            .read_only(true)
            .create_if_missing(false)
            .pragma("query_only", "ON")
            .busy_timeout(Duration::from_secs(1))
            .disable_statement_logging();
        tokio::time::timeout(CONNECT_TIMEOUT, options.connect())
            .await
            .map_err(|_| self.connect_error("timed out"))?
            .map_err(|e| self.connect_error(e))
    }

    /// Opens a PostgreSQL connection whose transactions are read-only and
    /// whose plain strings read a backslash as a backslash.
    ///
    /// The guard parses statements as standard SQL, where `'\'` is a whole
    /// string. A server set to `standard_conforming_strings = off` (in its
    /// configuration, or by `ALTER DATABASE` / `ALTER ROLE`) reads the same
    /// text as an escaped quote, so the statement it runs could differ from
    /// the one checked. Options in the startup message win over those
    /// settings, so the connection pins the standard reading, then checks it
    /// held: a connection pooler can drop startup options.
    async fn postgres(&self) -> Result<PgConnection, DatabaseError> {
        let options = PgConnectOptions::from_str(&self.url)
            .map_err(|e| self.connect_error(e))?
            .application_name("navree-navigator")
            .options(POSTGRES_SESSION)
            .disable_statement_logging();
        let mut conn = tokio::time::timeout(CONNECT_TIMEOUT, options.connect())
            .await
            .map_err(|_| self.connect_error("timed out"))?
            .map_err(|e| self.connect_error(e))?;
        let setting = first_text(&mut conn, "SHOW standard_conforming_strings").await?;
        standard_strings(setting.as_deref())?;
        Ok(conn)
    }

    /// Fails with [`DatabaseError::PrivilegedRole`] when the session's role
    /// is a superuser or a member of a role that reaches outside the
    /// database, unless [`allow_privileged_role`](Self::allow_privileged_role).
    async fn check_role(&self, conn: &mut PgConnection) -> Result<(), DatabaseError> {
        if self.allow_privileged {
            return Ok(());
        }
        let rows = conn.fetch_all(ROLE_PRIVILEGES).await?;
        // The query always answers a row; without one, nothing is known, so
        // nothing runs.
        let Some(row) = rows.first() else {
            return Err(DatabaseError::PrivilegedRole {
                role: "unknown".to_owned(),
                privileges: vec!["privileges that could not be read".to_owned()],
            });
        };
        let text = |i: usize| -> Option<String> {
            row.try_get_raw(i)
                .ok()
                .filter(|raw| !sqlx::ValueRef::is_null(raw))
                .and_then(|raw| raw.as_str().ok().map(str::to_owned))
        };
        let role = text(0).unwrap_or_default();
        let privileges = privileges_of(text(1).as_deref() == Some("t"), text(2).as_deref());
        if !privileges.is_empty() {
            return Err(DatabaseError::PrivilegedRole { role, privileges });
        }
        let mut functions: Vec<String> = conn
            .fetch_all(REACHING_FUNCTIONS)
            .await?
            .iter()
            .filter_map(|row| {
                row.try_get_raw(0)
                    .ok()
                    .and_then(|raw| raw.as_str().ok().map(str::to_owned))
            })
            .collect();
        if functions.is_empty() {
            return Ok(());
        }
        let more = functions.len() > LISTED_FUNCTIONS;
        functions.truncate(LISTED_FUNCTIONS);
        Err(DatabaseError::FunctionsReachOut {
            role,
            functions,
            more,
        })
    }

    /// Adds `hint` to the message of a failed connection.
    pub(crate) fn connect_hint(mut self, hint: Option<&'static str>) -> Target {
        self.hint = hint;
        self
    }

    fn connect_error(&self, error: impl std::fmt::Display) -> DatabaseError {
        let message = match self.hint {
            Some(hint) => format!("{error}. {hint}"),
            None => error.to_string(),
        };
        DatabaseError::Connect {
            url: self.redacted_url(),
            message,
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
/// [`ROW_LIMIT`] rows. Values of columns with secret-looking names are
/// masked as a courtesy for logs and transcripts, not as a protection: an
/// alias (`password_hash AS h`) or `row_to_json` returns them. On
/// PostgreSQL a privileged role is refused first (see the module).
pub async fn query_database(target: &Target, sql: &str) -> Result<QueryResult, DatabaseError> {
    // The guard parses on a thread of its own (see `check`), waited for here
    // on a blocking thread, not on the async runtime's.
    let (owned, dialect) = (sql.to_owned(), target.driver.dialect());
    let checked = tokio::task::spawn_blocking(move || check(&owned, dialect))
        .await
        .map_err(|_| Refusal::Unchecked)??;
    match target.driver {
        Driver::Sqlite => query::sqlite(target.sqlite_file()?, &checked).await,
        Driver::Postgres => {
            let mut conn = target.postgres().await?;
            let result = match target.check_role(&mut conn).await {
                Ok(()) => query::postgres(&mut conn, &checked).await,
                Err(error) => Err(error),
            };
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
    fn only_standard_strings_are_accepted() {
        assert!(standard_strings(Some("on")).is_ok());
        for setting in [Some("off"), Some("ON "), None] {
            let error = standard_strings(setting).unwrap_err();
            assert!(
                matches!(error, DatabaseError::StringsNotStandard { .. }),
                "{setting:?}"
            );
            assert!(
                error.to_string().contains("ignore_startup_parameters"),
                "{error}"
            );
        }
    }

    #[test]
    fn privileged_roles_are_named_and_the_message_says_what_to_do() {
        assert!(privileges_of(false, None).is_empty());
        assert!(privileges_of(false, Some("pg_monitor")).is_empty());
        assert_eq!(
            privileges_of(true, Some("pg_read_server_files,pg_signal_backend")),
            ["superuser", "pg_read_server_files", "pg_signal_backend"]
        );
        let message = DatabaseError::PrivilegedRole {
            role: "postgres".to_owned(),
            privileges: privileges_of(true, None),
        }
        .to_string();
        for part in [
            "`postgres`",
            "superuser",
            "CREATE ROLE navree_reader LOGIN",
            "GRANT SELECT ON ALL TABLES IN SCHEMA public TO navree_reader",
            "ALTER DEFAULT PRIVILEGES",
            "NAVREE_MCP_DATABASE_URL",
            "NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1",
        ] {
            assert!(message.contains(part), "{part}: {message}");
        }
    }

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
