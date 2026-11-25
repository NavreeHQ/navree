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
//!   socket) are allowed, whether the URL names the server or the
//!   environment SQLx reads does (`PGHOSTADDR`, `PGHOST`) ([`check_local`]).
//!   A SQLite file named as a network path (`\\server\share\...`) is on
//!   another machine too. A mounted network drive, or an NFS or SMB mount,
//!   cannot be told from a local folder, and a local port or socket may be
//!   forwarded elsewhere: the check judges the address, not where the data
//!   lives.

mod guard;
mod query;
mod schema;

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgConnection};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Executor, Row};
use url::Url;

pub use guard::{
    CheckedStatement, Dialect, MAX_EXPRESSION_DEPTH, MAX_SQL_BYTES, Refusal, StatementKind, check,
};
pub use query::{
    CUT_MARK, MASKED_COLUMN_PARTS, MAX_ANSWER_BYTES, MAX_COLUMNS, MAX_VALUE_BYTES, QueryResult,
    is_masked_column,
};
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
    /// The driver of `url`, by its scheme. What stands before the first `:`
    /// is named in the error only when it is written like a scheme: any
    /// other value (a libpq `key=value` string, say) may hold a password,
    /// and none of it is repeated.
    pub fn of(url: &str) -> Result<Driver, DatabaseError> {
        let Some((scheme, _)) = url.split_once(':') else {
            return Err(DatabaseError::NotAUrl);
        };
        let scheme = scheme.to_ascii_lowercase();
        match scheme.as_str() {
            "sqlite" => Ok(Driver::Sqlite),
            "postgres" | "postgresql" => Ok(Driver::Postgres),
            _ if is_scheme(&scheme) => Err(DatabaseError::Unsupported { scheme }),
            _ => Err(DatabaseError::NotAUrl),
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
    /// A value that is not written as a URL (a libpq `key=value` string, a
    /// bare name). None of it is shown: it may hold a password.
    #[error(
        "the database is not given as a URL: Navigator reads `sqlite:`, `postgres:` and `postgresql:` URLs"
    )]
    NotAUrl,
    /// A server that isn't on this machine, without `--allow-remote-database`.
    #[error(
        "database.url points at {host}, which isn't on this machine. Navigator only connects to local databases \
         (SQLite files, localhost, loopback addresses and Unix sockets), so a staging or production URL in a \
         forgotten .env is never queried. Start Navigator with `navree mcp --allow-remote-database` if you \
         really mean to read that database."
    )]
    Remote {
        /// The host named in the URL, or in the environment (then with the
        /// variable that names it).
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
        "query_database does not run as the PostgreSQL role `{role}`: it may reach {}{}. Such a function \
         runs with its owner's privileges (SECURITY DEFINER), in an untrusted language as the server's \
         operating system user, or in an extension that reaches files, programs, other servers or the \
         network, and a foreign table reads another server, so a query could act outside the read-only \
         transaction. Revoke EXECUTE on those functions from PUBLIC and from the role (for an aggregate's \
         support function, revoke it on the aggregate: PostgreSQL checks EXECUTE on those against the \
         aggregate's owner), revoke SELECT on the foreign tables and on the views that read them, or \
         start Navigator with \
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

/// What the session may reach beyond its own privileges, as "function
/// <signature>" and "foreign table <name>" lines, the first
/// [`LISTED_FUNCTIONS`] + 1 so the refusal knows there are more.
///
/// The functions are the user-defined ones (`oid >= 16384`, so not the
/// server's own) the role may execute (directly, through a role or through
/// PUBLIC), plus the support functions of every aggregate it may execute,
/// whatever its privilege on them (PostgreSQL checks those against the
/// aggregate's owner). The schema's USAGE is not required: a view, an
/// operator, a `BEGIN ATOMIC` body or a row-level policy calls a function
/// without it. Counted among them are a database's own SECURITY DEFINER or
/// untrusted-language (C included) functions, and the functions of an
/// extension that reaches files, programs, other servers or the network:
/// those named here, any extension that owns a foreign-data-wrapper
/// handler, and any written in an untrusted procedural language. The other
/// extensions' functions (PostGIS, pgcrypto, TimescaleDB, pgvector, their C
/// code and their own SECURITY DEFINER functions alike) are trusted as
/// their administrator installed them. Trigger functions can't be called,
/// and no trigger fires in a read-only transaction, so they don't count;
/// an aggregate itself only calls its support functions, which are counted
/// on their own. Foreign tables reach another server, so they count when
/// the role may SELECT from any of their columns, or from a view that
/// reads them, through any number of views (a view reads its tables with
/// its owner's privileges).
/// It runs with `search_path` set to `pg_catalog` alone (see `check_role`),
/// so no operator or cast of the database stands in for the server's.
const REACHING: &str = "WITH RECURSIVE risky_extension AS ( \
       SELECT e.oid FROM pg_catalog.pg_extension e \
       WHERE e.extname IN ('adminpack', 'aws_commons', 'aws_lambda', 'aws_s3', 'azure_ai', \
         'azure_storage', 'dblink', 'file_fdw', 'google_ml_integration', 'http', 'orafce', \
         'pg_background', 'pg_cron', 'pg_net', 'pgagent', 'pgml', 'plperlu', 'plproxy', \
         'plpython3u', 'plpythonu', 'pltclu', 'postgres_fdw') \
       OR EXISTS (SELECT 1 FROM pg_catalog.pg_depend d \
         JOIN pg_catalog.pg_proc h ON h.oid = d.objid \
         WHERE d.classid = 'pg_catalog.pg_proc'::pg_catalog.regclass \
         AND d.refclassid = 'pg_catalog.pg_extension'::pg_catalog.regclass \
         AND d.refobjid = e.oid AND d.deptype = 'e' \
         AND h.prorettype = 'pg_catalog.fdw_handler'::pg_catalog.regtype)), \
     callable AS ( \
       SELECT p.oid AS fn FROM pg_catalog.pg_proc p \
       WHERE pg_catalog.has_function_privilege(p.oid, 'EXECUTE') \
       UNION \
       SELECT s.fn::pg_catalog.oid FROM pg_catalog.pg_aggregate a, \
         LATERAL (VALUES (a.aggtransfn), (a.aggfinalfn), (a.aggcombinefn), (a.aggserialfn), \
           (a.aggdeserialfn), (a.aggmtransfn), (a.aggminvtransfn), (a.aggmfinalfn)) AS s(fn) \
       WHERE s.fn::pg_catalog.oid <> 0 \
       AND pg_catalog.has_function_privilege(a.aggfnoid, 'EXECUTE')), \
     readable (rel) AS ( \
       SELECT c.oid FROM pg_catalog.pg_class c \
       WHERE c.relkind IN ('v', 'f') AND pg_catalog.has_any_column_privilege(c.oid, 'SELECT') \
       UNION \
       SELECT d.refobjid FROM readable x \
       JOIN pg_catalog.pg_class v ON v.oid = x.rel AND v.relkind = 'v' \
       JOIN pg_catalog.pg_rewrite r ON r.ev_class = v.oid \
       JOIN pg_catalog.pg_depend d ON d.classid = 'pg_catalog.pg_rewrite'::pg_catalog.regclass \
         AND d.objid = r.oid AND d.refclassid = 'pg_catalog.pg_class'::pg_catalog.regclass \
       WHERE d.refobjid <> v.oid) \
     SELECT 'function ' || p.oid::pg_catalog.regprocedure::pg_catalog.text AS what \
     FROM callable c \
     JOIN pg_catalog.pg_proc p ON p.oid = c.fn \
     JOIN pg_catalog.pg_language l ON l.oid = p.prolang \
     LEFT JOIN pg_catalog.pg_depend d ON d.classid = 'pg_catalog.pg_proc'::pg_catalog.regclass \
       AND d.objid = p.oid AND d.deptype = 'e' \
     WHERE p.oid >= 16384 AND p.prokind <> 'a' \
     AND p.prorettype NOT IN ('pg_catalog.trigger'::pg_catalog.regtype, \
       'pg_catalog.event_trigger'::pg_catalog.regtype) \
     AND CASE WHEN d.refobjid IS NULL THEN p.prosecdef OR NOT l.lanpltrusted \
       ELSE d.refobjid IN (SELECT oid FROM risky_extension) \
         OR (NOT l.lanpltrusted AND l.lanname NOT IN ('c', 'internal')) END \
     UNION ALL \
     SELECT DISTINCT 'foreign table ' || c.oid::pg_catalog.regclass::pg_catalog.text \
     FROM readable x JOIN pg_catalog.pg_class c ON c.oid = x.rel \
     WHERE c.relkind = 'f' \
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
            sqlx::Error::Database(db) => DatabaseError::Query(short_message(db.message())),
            _ => DatabaseError::Query(short_message(&error.to_string())),
        }
    }
}

/// The longest error message a tool passes on: a database's error can quote
/// a value of the query at any size (`repeat('x', 300000000)::int`).
pub const MAX_MESSAGE_BYTES: usize = 2 * 1024;

/// `message` cut to [`MAX_MESSAGE_BYTES`] (at a character boundary), ending
/// with `… [cut]` when it was longer.
pub(crate) fn short_message(message: &str) -> String {
    if message.len() <= MAX_MESSAGE_BYTES {
        return message.to_owned();
    }
    let mut end = MAX_MESSAGE_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &message[..end], CUT_MARK)
}

/// Caps the heap SQLite may use in this whole process
/// (`PRAGMA hard_heap_limit`): past it, an allocation fails with "out of
/// memory" instead of growing. `navree mcp` sets 256 MB as it starts
/// ([`crate::run_stdio_with`]). The limit is process-wide, so code that
/// embeds Navigator in a process of its own decides whether and how to set
/// it: without it, an aggregate such as `json_group_array` can grow past
/// the per-value limit before that limit applies.
pub fn limit_sqlite_heap(bytes: u64) -> Result<(), DatabaseError> {
    let conn = rusqlite::Connection::open_in_memory()
        .map_err(|error| DatabaseError::Query(error.to_string()))?;
    let bytes = i64::try_from(bytes).unwrap_or(i64::MAX);
    conn.pragma_update(None, "hard_heap_limit", bytes)
        .map_err(|error| DatabaseError::Query(error.to_string()))
}

/// The heap `navree mcp` lets SQLite use ([`limit_sqlite_heap`]).
pub const SQLITE_HEAP_LIMIT: u64 = 256 * 1024 * 1024;

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
    /// Whether only a server on this machine may be connected to (no
    /// `--allow-remote-database`).
    local_only: bool,
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
        Self::new_in(url, root, allow_remote, |name| std::env::var(name).ok())
    }

    /// [`Target::new`], with the environment read through `var`.
    fn new_in(
        url: &str,
        root: &Path,
        allow_remote: bool,
        var: impl Fn(&str) -> Option<String>,
    ) -> Result<Target, DatabaseError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(DatabaseError::NotConfigured);
        }
        let driver = Driver::of(url)?;
        if !allow_remote {
            check_local_in(url, var)?;
        }
        Ok(Target {
            driver,
            url: url.to_owned(),
            root: root.to_path_buf(),
            local_only: !allow_remote,
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
        if self.local_only {
            check_sqlite_file(&file)?;
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
    ///
    /// The URL is parsed once. SQLx builds its options from the very URL the
    /// local-only check judges, in the environment as it is now, and the
    /// options are made to hold the host and the address the check found,
    /// so what is connected to is what was judged.
    async fn postgres(&self) -> Result<PgConnection, DatabaseError> {
        let url = Url::parse(&self.url).map_err(|e| self.connect_error(e))?;
        let mut options = PgConnectOptions::from_url(&url).map_err(|e| self.connect_error(e))?;
        if self.local_only {
            options = local_endpoint(&url, |name| std::env::var(name).ok())?.pin(options);
        }
        let options = options
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
        // In a read-only transaction of its own, with only the server's
        // catalog on the search path (no operator or cast of the database
        // stands in for the server's); rolled back whatever happens.
        conn.execute("BEGIN READ ONLY").await?;
        let read = async {
            conn.execute("SET LOCAL search_path = pg_catalog").await?;
            let role = conn.fetch_all(ROLE_PRIVILEGES).await?;
            let reached = conn.fetch_all(REACHING).await?;
            Ok::<_, sqlx::Error>((role, reached))
        }
        .await;
        let _ = conn.execute("ROLLBACK").await;
        let (rows, reached) = read?;
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
        // A row that can't be read still counts: nothing is known about it.
        let mut functions: Vec<String> = reached
            .iter()
            .map(|row| {
                row.try_get_raw(0)
                    .ok()
                    .and_then(|raw| raw.as_str().ok().map(str::to_owned))
                    .unwrap_or_else(|| "an object Navigator could not name".to_owned())
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
        let message = short_message(&error.to_string());
        let message = match self.hint {
            Some(hint) => format!("{message}. {hint}"),
            None => message,
        };
        DatabaseError::Connect {
            url: self.redacted_url(),
            message,
        }
    }
}

/// Fails with [`DatabaseError::Remote`] unless `url` is a SQLite URL whose
/// file is not named as a network path (`\\server\share\...`), or names
/// only local hosts: `localhost`, a loopback address (`127.0.0.1`,
/// `::1`), a Unix socket directory, or no host at all (the default socket).
///
/// A PostgreSQL URL is read as SQLx reads it, by the `url` crate and then
/// by its host and its `host` and `hostaddr` query parameters, so the server
/// judged is the one SQLx connects to. A URL that parser refuses is an error
/// here as it is there.
///
/// SQLx takes what the URL leaves out from the environment, so the
/// environment is held to the same rule: `PGHOSTADDR` is the address SQLx
/// connects to unless the URL has a `hostaddr` parameter (a host in the URL
/// does not replace it), and `PGHOST` is the host of a URL that names none.
pub fn check_local(url: &str) -> Result<(), DatabaseError> {
    check_local_in(url, |name| std::env::var(name).ok())
}

/// [`check_local`], with the environment read through `var`.
fn check_local_in(url: &str, var: impl Fn(&str) -> Option<String>) -> Result<(), DatabaseError> {
    if Driver::of(url)? == Driver::Sqlite {
        // A URL SQLx cannot read opens no file.
        return match SqliteConnectOptions::from_str(url) {
            Ok(options) => check_sqlite_file(options.get_filename()),
            Err(_) => Ok(()),
        };
    }
    local_endpoint(&parse_url(url)?, var).map(|_| ())
}

/// Fails with [`DatabaseError::Remote`] when the SQLite file `file` is on
/// another machine by its very name: a UNC path (`\\server\share\...`, or
/// `//server/share/...`, which Windows reads the same way), its long form
/// (`\\?\UNC\...`) or a device path (`\\.\...`). Opening one, or only asking
/// whether it exists, signs in to that server as the user. The rule is the
/// same on every system, so a URL is judged alike wherever it is read. The
/// long form of a path on a drive (`\\?\C:\...`) is local.
fn check_sqlite_file(file: &Path) -> Result<(), DatabaseError> {
    let name = file.to_string_lossy();
    let mut leading = name.chars();
    let two_separators = matches!(
        (leading.next(), leading.next()),
        (Some('\\' | '/'), Some('\\' | '/'))
    );
    if two_separators && !is_verbatim_disk(&name) {
        return Err(DatabaseError::Remote {
            host: format!("{name} (a network path)"),
        });
    }
    Ok(())
}

/// Whether `name` is the long form of a path on a drive: `\\?\C:\...`.
fn is_verbatim_disk(name: &str) -> bool {
    let bytes = name.as_bytes();
    name.starts_with(r"\\?\")
        && bytes.len() >= 7
        && bytes[4].is_ascii_alphabetic()
        && bytes[5] == b':'
        && bytes[6] == b'\\'
}

/// `url` as SQLx parses it.
fn parse_url(url: &str) -> Result<Url, DatabaseError> {
    Url::parse(url).map_err(|error| DatabaseError::Connect {
        url: redact_url(url),
        message: short_message(&error.to_string()),
    })
}

/// The host and the address SQLx connects to for a PostgreSQL URL, once
/// both are judged to be on this machine.
#[derive(Debug, PartialEq)]
struct PgEndpoint {
    /// The host: the URL's (the last one it names), else `PGHOST`. `None`
    /// leaves SQLx its own: a socket directory, or its default (a socket
    /// directory that exists, else `localhost`).
    host: Option<String>,
    /// The address connected to in place of the host: the URL's `hostaddr`,
    /// else `PGHOSTADDR`.
    addr: Option<IpAddr>,
}

impl PgEndpoint {
    /// `options` holding the host and the address that were judged,
    /// whatever SQLx read for itself.
    fn pin(self, mut options: PgConnectOptions) -> PgConnectOptions {
        if let Some(host) = &self.host {
            options = options.host(host);
        }
        if let Some(addr) = self.addr {
            options = options.hostaddr(addr);
        }
        options
    }
}

/// The endpoint SQLx connects to for the PostgreSQL URL `url`, in the
/// environment that `var` reads; fails with [`DatabaseError::Remote`] unless
/// it is on this machine. It follows sqlx-postgres 0.9
/// (`PgConnectOptions::parse_from_url`): the URL's host, then each `host`
/// and `hostaddr` query parameter, over `PGHOST` and `PGHOSTADDR`. Every
/// host and address named is judged, the ones a later parameter replaces
/// too.
fn local_endpoint(
    url: &Url,
    var: impl Fn(&str) -> Option<String>,
) -> Result<PgEndpoint, DatabaseError> {
    let remote = |host: &str| DatabaseError::Remote {
        host: host.to_owned(),
    };
    let mut endpoint = PgEndpoint {
        host: None,
        addr: None,
    };
    // Whether the URL names a host (a socket directory is one) and an
    // address.
    let mut names_host = false;
    let mut names_addr = false;
    if let Some(host) = url.host_str() {
        names_host = true;
        // SQLx takes a host whose first byte is `/` once percent-decoded for
        // a socket directory, and any other host as it is written.
        let socket = host
            .get(..3)
            .is_some_and(|start| start.eq_ignore_ascii_case("%2f"));
        if !socket {
            if !is_local_host(host) {
                return Err(remote(host));
            }
            endpoint.host = Some(host.to_owned());
        }
    }
    for (key, value) in url.query_pairs() {
        match &*key {
            "host" => {
                names_host = true;
                if !is_local_host(&value) {
                    return Err(remote(&value));
                }
                // A path is a socket directory, which SQLx keeps beside the
                // host.
                if !value.starts_with('/') {
                    endpoint.host = Some(value.into_owned());
                }
            }
            "hostaddr" => {
                names_addr = true;
                match value.parse::<IpAddr>() {
                    Ok(addr) if addr.is_loopback() => endpoint.addr = Some(addr),
                    _ => return Err(remote(&value)),
                }
            }
            _ => {}
        }
    }
    // SQLx reads `PGHOSTADDR` as an IP address and passes over anything
    // else. It is refused even where SQLx would not use it (a URL that names
    // a Unix socket): the message names the variable to unset.
    if !names_addr
        && let Some(named) = var("PGHOSTADDR")
        && let Ok(addr) = named.parse::<IpAddr>()
    {
        if !addr.is_loopback() {
            return Err(DatabaseError::Remote {
                host: format!("{named} (PGHOSTADDR in the environment)"),
            });
        }
        endpoint.addr = Some(addr);
    }
    if !names_host
        && let Some(named) = var("PGHOST")
        && !named.is_empty()
    {
        if !is_local_host(&named) {
            return Err(DatabaseError::Remote {
                host: format!("{named} (PGHOST in the environment)"),
            });
        }
        endpoint.host = Some(named);
    }
    Ok(endpoint)
}

/// Whether SQLx connects on this machine for `host`: `localhost` itself, a
/// loopback IP address (SQLx drops the brackets round one), or a path (a
/// Unix socket directory). No other name is taken for this machine, one
/// that ends in `.localhost` included: a name resolves wherever its
/// resolver says.
fn is_local_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") || host.starts_with('/') {
        return true;
    }
    host.trim_matches(&['[', ']'][..])
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// `url` with its password replaced by `***`, for messages.
///
/// The password is the one SQLx would read, found with the parser SQLx
/// uses: in the user part (`scheme://user:password@host/...`) or in a
/// `password` parameter, however the URL is spelled (the `url` crate reads
/// past tabs and line ends, and decodes the name of a parameter). Nothing
/// after a `#` is shown, and neither is the value of any parameter whose
/// name holds `pass`, nor a parameter SQLx does not know that follows one
/// (a password with an `&` written as it is reads as several).
///
/// A password with a `#`, `?` or `/` written as it is ends the user part
/// early for the parser, which then finds fewer `@` than the text holds. Of
/// such a URL, and of one that does not parse, only the scheme and what
/// follows the last `@` are shown, without the query; with an `@` after a
/// `?` or a `#`, only the scheme. A SQLite URL has no password and is
/// shown as it is.
pub fn redact_url(url: &str) -> String {
    if matches!(Driver::of(url), Ok(Driver::Sqlite)) {
        return url.to_owned();
    }
    let Ok(mut parsed) = Url::parse(url) else {
        return redact_unparsed(url);
    };
    let in_user_part = parsed.password().is_some();
    let user_part = in_user_part || !parsed.username().is_empty();
    if url.matches('@').count() > usize::from(user_part) {
        return redact_unparsed(url);
    }
    let in_query = parsed.query_pairs().any(|(key, _)| is_password_key(&key));
    if !in_user_part && !in_query && parsed.fragment().is_none() {
        return url.to_owned();
    }
    parsed.set_fragment(None);
    if in_user_part && parsed.set_password(Some("***")).is_err() {
        return redact_unparsed(url);
    }
    if in_query {
        let mut after_password = false;
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(key, value)| {
                if is_password_key(&key) {
                    after_password = true;
                    (key.into_owned(), "***".to_owned())
                } else if after_password && !is_connect_parameter(&key) {
                    ("***".to_owned(), String::new())
                } else {
                    (key.into_owned(), value.into_owned())
                }
            })
            .collect();
        parsed.query_pairs_mut().clear().extend_pairs(pairs);
    }
    parsed.into()
}

/// Whether a URL parameter called `key` holds a password: `password`, which
/// SQLx reads, and what was meant for it (`Password`, `passwd`).
fn is_password_key(key: &str) -> bool {
    key.to_ascii_lowercase().contains("pass")
}

/// Whether `key` is a parameter sqlx-postgres 0.9 reads from a URL, other
/// than `password`.
fn is_connect_parameter(key: &str) -> bool {
    matches!(
        key,
        "sslmode"
            | "ssl-mode"
            | "sslrootcert"
            | "ssl-root-cert"
            | "ssl-ca"
            | "sslcert"
            | "ssl-cert"
            | "sslkey"
            | "ssl-key"
            | "statement-cache-capacity"
            | "host"
            | "hostaddr"
            | "port"
            | "dbname"
            | "user"
            | "application_name"
            | "options"
    ) || key.starts_with("options[")
}

/// What may be shown of a URL whose password cannot be told from the rest:
/// its scheme and, when it has an `@` before its query, what follows the
/// last one. With an `@` after a `?` or a `#`, that mark may stand inside
/// the password, and only the scheme is shown.
fn redact_unparsed(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once(':') else {
        return "***".to_owned();
    };
    if !is_scheme(scheme) {
        return "***".to_owned();
    }
    let shown = before_query(rest);
    if rest[shown.len()..].contains('@') {
        return format!("{scheme}:***");
    }
    match shown.rfind('@') {
        Some(at) => format!("{scheme}:***{}", &shown[at..]),
        None => format!("{scheme}:***"),
    }
}

/// Whether `text` is written like the scheme of a URL: a letter, then
/// letters, digits, `+`, `-` and `.`, twenty characters at most.
fn is_scheme(text: &str) -> bool {
    text.len() <= 20
        && text.starts_with(|c: char| c.is_ascii_alphabetic())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}
/// `text` up to its first `?` or `#`.
fn before_query(text: &str) -> &str {
    &text[..text.find(['?', '#']).unwrap_or(text.len())]
}

/// Runs one read-only statement (see [`check`]) and returns at most
/// [`ROW_LIMIT`] rows. Values of columns with secret-looking names are
/// masked as a courtesy for logs and transcripts, not as a protection: an
/// alias (`password_hash AS h`) or `row_to_json` returns them. On
/// PostgreSQL a privileged role is refused first (see the module).
pub async fn query_database(target: &Target, sql: &str) -> Result<QueryResult, DatabaseError> {
    run_query(target, sql, query::AGENT_LIMITS).await
}

/// [`query_database`] for Navigator's own fixed statements (Lighthouse's):
/// the same guard and read-only path, with values read whole up to 8 MB and
/// an answer of up to 8 MB, so histograms and backtraces are not cut.
pub(crate) async fn query_own(target: &Target, sql: &str) -> Result<QueryResult, DatabaseError> {
    run_query(target, sql, query::OWN_LIMITS).await
}

async fn run_query(
    target: &Target,
    sql: &str,
    limits: query::Limits,
) -> Result<QueryResult, DatabaseError> {
    // The guard parses on a thread of its own (see `check`), waited for here
    // on a blocking thread, not on the async runtime's.
    let (owned, dialect) = (sql.to_owned(), target.driver.dialect());
    let checked = tokio::task::spawn_blocking(move || check(&owned, dialect))
        .await
        .map_err(|_| Refusal::Unchecked)??;
    match target.driver {
        Driver::Sqlite => query::sqlite(target.sqlite_file()?, &checked, limits).await,
        Driver::Postgres if checked.kind == StatementKind::Explain => {
            Err(Refusal::ExplainOnPostgres.into())
        }
        Driver::Postgres => {
            let mut conn = target.postgres().await?;
            let result = match target.check_role(&mut conn).await {
                Ok(()) => query::postgres(&mut conn, &checked, limits).await,
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

    /// A SQLite file on a share is on another machine, and only asking
    /// whether it exists signs in to that machine.
    #[test]
    fn a_sqlite_file_on_a_share_is_refused() {
        for url in [
            "sqlite:////dbserver/share/app.db",
            "sqlite://%5C%5Cdbserver%5Cshare%5Capp.db",
            r"sqlite://\\dbserver\share\app.db",
            "sqlite://%5C%5C%3F%5CUNC%5Cdbserver%5Cshare%5Capp.db",
            r"sqlite://\\.\pipe\app",
            r"sqlite:\/dbserver/share/app.db",
        ] {
            let host = refused(url, &[]);
            assert!(host.ends_with("(a network path)"), "{url}: {host}");
        }
        for url in [
            "sqlite://storage/app.db?mode=rwc",
            "sqlite:///var/lib/crm/app.db",
            r"sqlite://C:\data\app.db",
            "sqlite://%5C%5C%3F%5CC:%5Cdata%5Capp.db",
            "sqlite::memory:",
        ] {
            allowed(url, &[]);
        }
        let root = Path::new(".");
        let share = "sqlite:////dbserver/share/app.db";
        assert!(matches!(
            Target::new_in(share, root, false, env(&[])),
            Err(DatabaseError::Remote { .. })
        ));
        assert!(Target::new_in(share, root, true, env(&[])).is_ok());
    }

    /// A value that is no URL may hold a password, and none of it is
    /// repeated in the error.
    #[test]
    fn a_value_that_is_no_url_is_not_repeated() {
        for value in [
            "host=localhost user=app password=s3cret dbname=crm",
            "host=localhost user=app password=s3:cret dbname=crm",
            "s3cret",
            "an-uncommonly-long-scheme-s3cret://localhost/crm",
        ] {
            let error = Driver::of(value).unwrap_err();
            assert!(
                matches!(error, DatabaseError::NotAUrl),
                "{value}: {error:?}"
            );
            assert!(!error.to_string().contains("s3"), "{error}");
        }
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
            "postgres:///crm?host=/var/run/postgresql",
            "postgres://navree@localhost/crm?host=/var/run/postgresql",
            "postgres://navree@%2Fvar%2Frun%2Fpostgresql/crm",
            "postgres://u@127.0.0.2/crm",
        ] {
            assert!(check_local_in(url, env(&[])).is_ok(), "{url}");
        }
    }

    /// An environment that holds `pairs` and nothing else.
    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    /// The host that the refusal of `url` names, in an environment of `pairs`.
    fn refused(url: &str, pairs: &'static [(&'static str, &'static str)]) -> String {
        match check_local_in(url, env(pairs)) {
            Err(DatabaseError::Remote { host }) => host,
            other => panic!("{url} with {pairs:?}: {other:?}"),
        }
    }

    fn allowed(url: &str, pairs: &'static [(&'static str, &'static str)]) {
        let checked = check_local_in(url, env(pairs));
        assert!(checked.is_ok(), "{url} with {pairs:?}: {checked:?}");
    }

    /// SQLx connects to `PGHOSTADDR` whatever host the URL names, and to
    /// `PGHOST` when the URL names none, so both are held to the rule.
    #[test]
    fn a_remote_server_named_by_the_environment_is_refused() {
        assert_eq!(
            refused(
                "postgres://app@localhost/crm",
                &[("PGHOSTADDR", "10.0.0.5")]
            ),
            "10.0.0.5 (PGHOSTADDR in the environment)"
        );
        assert_eq!(
            refused("postgres:///crm", &[("PGHOSTADDR", "2001:db8::1")]),
            "2001:db8::1 (PGHOSTADDR in the environment)"
        );
        assert_eq!(
            refused("postgres:///crm", &[("PGHOST", "db.example.com")]),
            "db.example.com (PGHOST in the environment)"
        );
        // A local `hostaddr` in the URL still leaves the host to `PGHOST`.
        assert_eq!(
            refused(
                "postgres:///crm?hostaddr=127.0.0.1",
                &[("PGHOST", "db.example.com")]
            ),
            "db.example.com (PGHOST in the environment)"
        );
        // SQLx would not use the address on a socket; it is refused all the
        // same, and the message names the variable.
        assert_eq!(
            refused(
                "postgres:///crm?host=/var/run/postgresql",
                &[("PGHOSTADDR", "10.0.0.5")]
            ),
            "10.0.0.5 (PGHOSTADDR in the environment)"
        );
        let message = check_local_in(
            "postgres://app@localhost/crm",
            env(&[("PGHOSTADDR", "10.0.0.5")]),
        )
        .unwrap_err()
        .to_string();
        assert!(message.contains("PGHOSTADDR"), "{message}");
        assert!(message.contains("--allow-remote-database"), "{message}");
    }

    #[test]
    fn a_local_server_named_by_the_environment_is_allowed() {
        allowed(
            "postgres://app@localhost/crm",
            &[("PGHOSTADDR", "127.0.0.1")],
        );
        allowed("postgres://app@localhost/crm", &[("PGHOSTADDR", "::1")]);
        allowed("postgres:///crm", &[("PGHOST", "localhost")]);
        allowed("postgres:///crm", &[("PGHOST", "/var/run/postgresql")]);
        allowed(
            "postgres:///crm",
            &[("PGHOST", "127.0.0.1"), ("PGHOSTADDR", "127.0.0.1")],
        );
        // The URL's own `hostaddr` is the one SQLx uses.
        allowed(
            "postgres://app@localhost/crm?hostaddr=127.0.0.1",
            &[("PGHOSTADDR", "10.0.0.5")],
        );
        // A host in the URL takes the place of `PGHOST`.
        allowed(
            "postgres://app@localhost/crm",
            &[("PGHOST", "db.example.com")],
        );
        // SQLx passes over a `PGHOSTADDR` that is not an IP address, and an
        // empty `PGHOST` names no server.
        allowed(
            "postgres://app@localhost/crm",
            &[("PGHOSTADDR", "db.example.com")],
        );
        allowed("postgres:///crm", &[("PGHOST", "")]);
        // SQLite reads neither variable.
        allowed(
            "sqlite://storage/app.db",
            &[("PGHOSTADDR", "10.0.0.5"), ("PGHOST", "db.example.com")],
        );
    }
    /// A URL is read as SQLx reads it: with the key of a parameter decoded,
    /// with nothing after a `#`, and without the tabs and line ends that the
    /// `url` crate drops.
    #[test]
    fn a_url_is_judged_as_sqlx_parses_it() {
        assert_eq!(
            refused("postgres://app@localhost/crm?h%6Fst=db.example.com", &[]),
            "db.example.com"
        );
        assert_eq!(
            refused("postgres://app@localhost/crm?host%61ddr=10.0.0.5", &[]),
            "10.0.0.5"
        );
        assert_eq!(
            refused("postgres://db.example.com#@localhost/crm", &[]),
            "db.example.com"
        );
        assert_eq!(
            refused(
                "postgres:///crm#?host=localhost",
                &[("PGHOST", "db.example.com")]
            ),
            "db.example.com (PGHOST in the environment)"
        );
        assert_eq!(
            refused(
                "postgres:///crm#?hostaddr=127.0.0.1",
                &[("PGHOSTADDR", "10.0.0.5")]
            ),
            "10.0.0.5 (PGHOSTADDR in the environment)"
        );
        assert_eq!(
            refused("postgres:/\t/db.example.com/crm", &[]),
            "db.example.com"
        );
        assert_eq!(
            refused("postgres://db.exa\nmple.com/crm", &[]),
            "db.example.com"
        );
        assert_eq!(
            refused("postgres://app@local\rhost/crm?ho\tst=db.example.com", &[]),
            "db.example.com"
        );
        // The same forms, naming this machine, are still let through.
        allowed("postgres://app@localhost/crm?h%6Fst=127.0.0.1", &[]);
        allowed("postgres://localhost#@db.example.com/crm", &[]);
        allowed("postgres:/\t/local\nhost/crm", &[]);
    }

    #[test]
    fn only_localhost_itself_is_trusted_by_name() {
        assert_eq!(
            refused("postgres://app@db.localhost/crm", &[]),
            "db.localhost"
        );
        assert_eq!(
            refused("postgres://app@localhost.example.com/crm", &[]),
            "localhost.example.com"
        );
        assert_eq!(
            refused("postgres://app@localhost/crm?host=crm.localhost", &[]),
            "crm.localhost"
        );
        assert_eq!(
            refused("postgres:///crm", &[("PGHOST", "db.localhost")]),
            "db.localhost (PGHOST in the environment)"
        );
        allowed("postgres://app@LOCALHOST/crm", &[]);
    }

    /// The endpoint judged for `url`, in an environment of `pairs`.
    fn endpoint(url: &str, pairs: &'static [(&'static str, &'static str)]) -> PgEndpoint {
        local_endpoint(&Url::parse(url).unwrap(), env(pairs)).unwrap()
    }

    /// The host and the address that were judged are the ones the
    /// connection is given.
    #[test]
    fn the_connection_is_given_the_endpoint_that_was_judged() {
        let loopback = IpAddr::from([127, 0, 0, 1]);
        assert_eq!(
            endpoint("postgres://app@localhost/crm?hostaddr=127.0.0.1", &[]),
            PgEndpoint {
                host: Some("localhost".to_owned()),
                addr: Some(loopback),
            }
        );
        // The last host the URL names is the one SQLx keeps.
        assert_eq!(
            endpoint("postgres://app@localhost/crm?host=127.0.0.1", &[]).host,
            Some("127.0.0.1".to_owned())
        );
        // What the URL leaves out comes from the environment.
        assert_eq!(
            endpoint(
                "postgres:///crm",
                &[("PGHOST", "localhost"), ("PGHOSTADDR", "127.0.0.1")]
            ),
            PgEndpoint {
                host: Some("localhost".to_owned()),
                addr: Some(loopback),
            }
        );
        // A socket directory is SQLx's to keep, and so is its default.
        let none = PgEndpoint {
            host: None,
            addr: None,
        };
        assert_eq!(
            endpoint("postgres://%2Fvar%2Frun%2Fpostgresql/crm", &[]),
            none
        );
        assert_eq!(endpoint("postgres:///crm", &[]), none);

        // Options that read something else are made to hold the endpoint.
        let options = PgConnectOptions::new_without_pgpass()
            .username("app")
            .host("db.example.com")
            .hostaddr(IpAddr::from([10, 0, 0, 5]));
        let pinned = PgEndpoint {
            host: Some("localhost".to_owned()),
            addr: Some(loopback),
        }
        .pin(options);
        assert_eq!(pinned.get_host(), "localhost");
        let rebuilt = pinned.to_url_lossy();
        let addrs: Vec<String> = rebuilt
            .query_pairs()
            .filter(|(key, _)| key == "hostaddr")
            .map(|(_, value)| value.into_owned())
            .collect();
        assert_eq!(addrs, ["127.0.0.1"]);
    }
    /// SQLx cannot connect with a URL that its parser refuses, and the check
    /// does not let one through as local.
    #[test]
    fn a_url_that_sqlx_cannot_parse_is_an_error() {
        for url in [
            "postgres://navree@/crm?host=/var/run/postgresql",
            "postgres://localhost:port/crm",
        ] {
            assert!(
                matches!(
                    check_local_in(url, env(&[])),
                    Err(DatabaseError::Connect { .. })
                ),
                "{url}"
            );
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
                "localhost,db.example.com",
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
            match check_local_in(url, env(&[])) {
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
    fn the_flag_allows_what_the_environment_names() {
        let root = Path::new(".");
        let url = "postgres://app@localhost/crm";
        let remote: &'static [(&str, &str)] = &[("PGHOSTADDR", "10.0.0.5")];
        assert!(matches!(
            Target::new_in(url, root, false, env(remote)),
            Err(DatabaseError::Remote { .. })
        ));
        assert!(Target::new_in(url, root, true, env(remote)).is_ok());
        assert!(Target::new_in(url, root, false, env(&[])).is_ok());
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
        assert_eq!(
            redact_url("postgres://navree@localhost/crm"),
            "postgres://navree@localhost/crm"
        );
    }

    /// The password SQLx would read is masked whatever the spelling: past a
    /// tab or a line end, and in a `password` parameter.
    #[test]
    fn a_password_is_masked_however_the_url_is_spelled() {
        for (url, masked) in [
            (
                "postgres:/\t/navree:s3cret@db.example.com/crm",
                "postgres://navree:***@db.example.com/crm",
            ),
            (
                "postgres:\n//navree:s3cret@db.example.com/crm",
                "postgres://navree:***@db.example.com/crm",
            ),
            (
                "postgres:/\r/navree:s3\tcret@localhost:5432/crm?sslmode=require",
                "postgres://navree:***@localhost:5432/crm?sslmode=require",
            ),
            (
                "postgres://navree@localhost/crm?password=s3cret",
                "postgres://navree@localhost/crm?password=***",
            ),
            (
                "postgres://localhost/crm?pass%77ord=s3cret&sslmode=require",
                "postgres://localhost/crm?password=***&sslmode=require",
            ),
        ] {
            assert_eq!(redact_url(url), masked, "{url:?}");
        }
    }

    /// A `#`, `?` or `/` written as it is in a password ends the user part
    /// early for the parser. No part of such a password is shown, nor of a
    /// `password` parameter cut short by a `#` or an `&`.
    #[test]
    fn a_password_with_a_mark_written_as_it_is_is_not_shown() {
        for (url, masked) in [
            ("postgres://navree:#Abc1@localhost/crm", "postgres:***"),
            ("postgres://navree:?Abc1@localhost/crm", "postgres:***"),
            ("postgres://navree:2024#Spring@db/crm", "postgres:***"),
            ("postgres://us@er:12#s3cret@host/db", "postgres:***"),
            (
                "postgres://navree:pa/ss@localhost/crm",
                "postgres:***@localhost/crm",
            ),
            (
                "postgres://navree:12/ss@localhost/crm",
                "postgres:***@localhost/crm",
            ),
            (
                "postgres://navree@localhost/crm?password=se#cret",
                "postgres://navree@localhost/crm?password=***",
            ),
            (
                "postgres://navree@localhost/crm?password=se&cret",
                "postgres://navree@localhost/crm?password=***&***=",
            ),
            (
                "postgres://navree@localhost/crm?password=se&cr=et&sslmode=require",
                "postgres://navree@localhost/crm?password=***&***=&sslmode=require",
            ),
            (
                "postgres://localhost/crm?Password=s3cret",
                "postgres://localhost/crm?Password=***",
            ),
            (
                "postgres://localhost/crm#s3cret",
                "postgres://localhost/crm",
            ),
        ] {
            assert_eq!(redact_url(url), masked, "{url:?}");
        }
    }
    /// Of a URL that does not parse, or whose `@` names no user, nothing
    /// that could be a password is shown.
    #[test]
    fn a_url_that_cannot_be_read_shows_no_password() {
        for (url, masked) in [
            // No host after the user part: the `url` crate refuses it.
            (
                "postgres://navree:s3cret@/crm?password=s3cret",
                "postgres:***@/crm",
            ),
            // A password where the port belongs.
            ("postgres://navree:s3cret/crm", "postgres:***"),
            // No slashes: the parser reads a path, with no user in it.
            (
                "postgres:navree:s3cret@db.example.com/crm",
                "postgres:***@db.example.com/crm",
            ),
            (
                "postgres:/navree:s3cret@db.example.com/crm?password=s3cret",
                "postgres:***@db.example.com/crm",
            ),
            // No scheme to show.
            ("navree s3cret", "***"),
        ] {
            let shown = redact_url(url);
            assert_eq!(shown, masked, "{url:?}");
            assert!(!shown.contains("s3cret"), "{url:?}: {shown}");
        }
        let error = check_local_in("postgres://navree:s3cret@/crm", env(&[])).unwrap_err();
        assert!(!error.to_string().contains("s3cret"), "{error}");
    }
}
