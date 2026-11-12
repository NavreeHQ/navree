//! Running a checked statement and turning its rows into JSON.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures_util::TryStreamExt;
use serde::Serialize;
use serde_json::Value;
use sqlx::postgres::{PgConnection, PgRow};
use sqlx::{AssertSqlSafe, Column, Executor, Row, TypeInfo, ValueRef};

use super::{
    CheckedStatement, DatabaseError, ROW_LIMIT, Refusal, STATEMENT_TIMEOUT, StatementKind,
};

/// Fragments (matched case-insensitively) that mark a column as secret: its
/// values come back as `"[redacted]"`.
pub const MASKED_COLUMN_PARTS: [&str; 5] =
    ["password", "secret", "token", "api_key", "private_key"];

/// What masked values are replaced with.
const MASKED: &str = "[redacted]";

/// Whether values of the column `name` are masked.
pub fn is_masked_column(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    MASKED_COLUMN_PARTS.iter().any(|part| name.contains(part))
}

/// The answer of `query_database`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct QueryResult {
    /// The column names, in order.
    pub columns: Vec<String>,
    /// At most [`ROW_LIMIT`] rows, one value per column.
    pub rows: Vec<Vec<Value>>,
    /// Whether the statement returned more rows than were kept.
    pub truncated: bool,
    /// How long the statement took.
    pub duration_ms: u64,
}

impl QueryResult {
    fn new(columns: Vec<String>, mut rows: Vec<Vec<Value>>, started: Instant) -> Self {
        let truncated = rows.len() > ROW_LIMIT;
        rows.truncate(ROW_LIMIT);
        for row in &mut rows {
            for (value, column) in row.iter_mut().zip(&columns) {
                if !value.is_null() && is_masked_column(column) {
                    *value = Value::from(MASKED);
                }
            }
        }
        QueryResult {
            columns,
            rows,
            truncated,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        }
    }
}

/// The SQLite database a query reads.
#[derive(Clone, Debug)]
pub(super) enum SqliteFile {
    /// `:memory:` (empty, and gone when the query ends).
    Memory,
    /// A file that exists.
    Path(PathBuf),
}

/// Runs `statement` on SQLite, with SQLite itself as the boundary (on a
/// blocking thread, through rusqlite):
///
/// - the file is opened read-only (`SQLITE_OPEN_READONLY`, which attached
///   databases share) with `PRAGMA query_only` on;
/// - an authorizer, installed before the statement is prepared, allows only
///   reading a table, `SELECT`, recursive CTEs and functions not in the
///   guard's deny list; PRAGMA (pragma table functions included), ATTACH,
///   DETACH, VACUUM, writes, DDL and transactions are refused while SQLite
///   prepares the text;
/// - SQLite prepares the text: anything but whitespace or comments after
///   the first statement, as SQLite's own tokenizer reads it, is refused;
/// - the statement must be read-only (`sqlite3_stmt_readonly`) and take no
///   parameters; then that one statement runs, interrupted after
///   [`STATEMENT_TIMEOUT`].
pub(super) async fn sqlite(
    file: SqliteFile,
    statement: &CheckedStatement,
) -> Result<QueryResult, DatabaseError> {
    let sql = statement.sql.clone();
    let work = tokio::task::spawn_blocking(move || run_sqlite(&file, &sql));
    with_timeout(async {
        work.await
            .map_err(|error| DatabaseError::Query(error.to_string()))?
    })
    .await
}

/// [`sqlite`] on this thread, for `sql` as given.
pub(super) fn run_sqlite(file: &SqliteFile, sql: &str) -> Result<QueryResult, DatabaseError> {
    use rusqlite::{Connection, OpenFlags};

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = match file {
        SqliteFile::Memory => Connection::open_in_memory_with_flags(flags),
        SqliteFile::Path(path) => Connection::open_with_flags(path, flags),
    }
    .map_err(sqlite_error)?;
    conn.busy_timeout(Duration::from_secs(1))
        .map_err(sqlite_error)?;
    conn.pragma_update(None, "query_only", "ON")
        .map_err(sqlite_error)?;
    let deadline = Instant::now() + STATEMENT_TIMEOUT;
    conn.progress_handler(1_000, Some(move || Instant::now() >= deadline))
        .map_err(sqlite_error)?;
    conn.authorizer(Some(authorize)).map_err(sqlite_error)?;
    let result = run_prepared(&conn, sql);
    let _ = conn
        .authorizer(None::<fn(rusqlite::hooks::AuthContext<'_>) -> rusqlite::hooks::Authorization>);
    result
}

/// What a guarded query may do on SQLite: read.
fn authorize(context: rusqlite::hooks::AuthContext<'_>) -> rusqlite::hooks::Authorization {
    use rusqlite::hooks::{AuthAction, Authorization};

    match context.action {
        AuthAction::Select | AuthAction::Read { .. } | AuthAction::Recursive => {
            Authorization::Allow
        }
        AuthAction::Function { function_name }
            if !super::guard::is_denied(&function_name.to_ascii_lowercase()) =>
        {
            Authorization::Allow
        }
        _ => Authorization::Deny,
    }
}

fn run_prepared(conn: &rusqlite::Connection, sql: &str) -> Result<QueryResult, DatabaseError> {
    let mut statement = conn.prepare(sql).map_err(sqlite_error)?;
    if !statement.readonly() {
        return Err(Refusal::Writes.into());
    }
    if statement.parameter_count() > 0 {
        let name = statement.parameter_name(1).unwrap_or("?").to_owned();
        return Err(Refusal::Parameter { name }.into());
    }
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let started = Instant::now();
    let mut rows = statement.query([]).map_err(sqlite_error)?;
    let mut values: Vec<Vec<Value>> = Vec::new();
    while let Some(row) = rows.next().map_err(sqlite_error)? {
        values.push(
            (0..columns.len())
                .map(|i| row.get_ref(i).map_or(Value::Null, sqlite_value))
                .collect(),
        );
        if values.len() > ROW_LIMIT {
            break;
        }
    }
    Ok(QueryResult::new(columns, values, started))
}

/// A SQLite value as JSON, by its storage class.
fn sqlite_value(value: rusqlite::types::ValueRef<'_>) -> Value {
    use rusqlite::types::ValueRef;

    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => Value::from(n),
        ValueRef::Real(f) => float(f),
        ValueRef::Text(text) => Value::from(String::from_utf8_lossy(text).into_owned()),
        ValueRef::Blob(bytes) => Value::from(hex(bytes)),
    }
}

/// A rusqlite error as Navigator says it.
fn sqlite_error(error: rusqlite::Error) -> DatabaseError {
    use rusqlite::ErrorCode;

    match &error {
        rusqlite::Error::MultipleStatement => Refusal::Trailing.into(),
        rusqlite::Error::SqliteFailure(failure, message) => match failure.code {
            ErrorCode::OperationInterrupted => DatabaseError::Timeout,
            ErrorCode::AuthorizationForStatementDenied => Refusal::NotAuthorized {
                reason: message
                    .clone()
                    .unwrap_or_else(|| "not authorized".to_owned()),
            }
            .into(),
            _ => DatabaseError::Query(message.clone().unwrap_or_else(|| error.to_string())),
        },
        _ => DatabaseError::Query(error.to_string()),
    }
}

/// Runs `statement` in a read-only PostgreSQL transaction with a statement
/// timeout, through a cursor for queries (only [`ROW_LIMIT`] + 1 rows are
/// fetched), and rolls back.
pub(super) async fn postgres(
    conn: &mut PgConnection,
    statement: &CheckedStatement,
) -> Result<QueryResult, DatabaseError> {
    conn.execute("BEGIN READ ONLY").await?;
    let result = postgres_in_transaction(conn, statement).await;
    let _ = conn.execute("ROLLBACK").await;
    result
}

async fn postgres_in_transaction(
    conn: &mut PgConnection,
    statement: &CheckedStatement,
) -> Result<QueryResult, DatabaseError> {
    conn.execute(AssertSqlSafe(format!(
        "SET LOCAL statement_timeout = {}",
        STATEMENT_TIMEOUT.as_millis()
    )))
    .await?;
    conn.execute("SET LOCAL lock_timeout = 1000").await?;
    let started = Instant::now();
    // The checked statement goes to the server as one prepared statement (the
    // extended protocol): a prepared statement holds exactly one command, so
    // however the server reads the text, it cannot run a second one. A query is
    // declared as a cursor that way, then read with a FETCH of this module's own
    // over the simple protocol, whose text results give every type a readable
    // form. EXPLAIN and SHOW answer in text-like types (text, json, xml), whose
    // binary form is their text.
    let rows = match statement.kind {
        StatementKind::Query => {
            let declare = format!(
                "DECLARE navigator_rows NO SCROLL CURSOR FOR {}",
                statement.sql
            );
            conn.execute(sqlx::query(AssertSqlSafe(declare)).persistent(false))
                .await?;
            let fetch = format!("FETCH FORWARD {} FROM navigator_rows", ROW_LIMIT + 1);
            with_timeout(collect(conn.fetch(AssertSqlSafe(fetch)))).await?
        }
        StatementKind::Explain | StatementKind::Show => {
            let prepared = sqlx::query(AssertSqlSafe(statement.sql.clone())).persistent(false);
            with_timeout(collect(conn.fetch(prepared))).await?
        }
    };
    let columns = rows
        .first()
        .map(|row| column_names(row.columns()))
        .unwrap_or_default();
    let values = rows.iter().map(postgres_values).collect();
    Ok(QueryResult::new(columns, values, started))
}

/// Every row of `stream`.
async fn collect(
    mut stream: impl futures_util::Stream<Item = Result<PgRow, sqlx::Error>> + Unpin,
) -> Result<Vec<PgRow>, DatabaseError> {
    let mut rows: Vec<PgRow> = Vec::new();
    while let Some(row) = stream.try_next().await? {
        rows.push(row);
    }
    Ok(rows)
}

/// A backstop above the database's own timeout.
async fn with_timeout<T>(
    work: impl Future<Output = Result<T, DatabaseError>>,
) -> Result<T, DatabaseError> {
    tokio::time::timeout(STATEMENT_TIMEOUT + Duration::from_secs(2), work)
        .await
        .map_err(|_| DatabaseError::Timeout)?
}

fn column_names<C: Column>(columns: &[C]) -> Vec<String> {
    columns.iter().map(|c| c.name().to_owned()).collect()
}

/// A PostgreSQL row (text format) as JSON, by each column's type.
fn postgres_values(row: &PgRow) -> Vec<Value> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(i, column)| {
            let Ok(raw) = row.try_get_raw(i) else {
                return Value::Null;
            };
            if raw.is_null() {
                return Value::Null;
            }
            let Ok(text) = raw.as_str() else {
                return Value::Null;
            };
            pg_text_value(column.type_info().name(), text)
        })
        .collect()
}

/// A PostgreSQL value in its text form, typed by `type_name`.
pub(super) fn pg_text_value(type_name: &str, text: &str) -> Value {
    match type_name {
        "INT2" | "INT4" | "INT8" | "OID" => text
            .parse::<i64>()
            .map_or_else(|_| Value::from(text), Value::from),
        "FLOAT4" | "FLOAT8" => text
            .parse::<f64>()
            .map_or_else(|_| Value::from(text), float),
        "NUMERIC" => match text.parse::<i64>() {
            Ok(int) => Value::from(int),
            Err(_) => text
                .parse::<f64>()
                .ok()
                .filter(|f| f.is_finite() && format_matches(text, *f))
                .map_or_else(|| Value::from(text), Value::from),
        },
        "BOOL" => match text {
            "t" => Value::Bool(true),
            "f" => Value::Bool(false),
            other => Value::from(other),
        },
        "JSON" | "JSONB" => serde_json::from_str(text).unwrap_or_else(|_| Value::from(text)),
        _ => Value::from(text),
    }
}

/// Whether `f` prints back as `text` (no precision lost), ignoring trailing zeros.
fn format_matches(text: &str, f: f64) -> bool {
    let trimmed = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    };
    f.to_string() == trimmed
}

/// A float as JSON; NaN and infinities become strings.
fn float(f: f64) -> Value {
    serde_json::Number::from_f64(f).map_or_else(|| Value::from(f.to_string()), Value::Number)
}

/// `\x0a0b...` like PostgreSQL's `bytea` output, at most 64 bytes shown.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::from("\\x");
    for byte in bytes.iter().take(64) {
        out.push_str(&format!("{byte:02x}"));
    }
    if bytes.len() > 64 {
        out.push_str(&format!("... ({} bytes)", bytes.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_looking_columns_are_masked() {
        for name in [
            "password",
            "password_hash",
            "two_factor_secret",
            "remember_token",
            "API_KEY",
            "private_key_pem",
        ] {
            assert!(is_masked_column(name), "{name}");
        }
        for name in ["email", "title", "keyword", "stage"] {
            assert!(!is_masked_column(name), "{name}");
        }
        let result = QueryResult::new(
            vec!["email".into(), "password_hash".into()],
            vec![
                vec![Value::from("anu@example.com"), Value::from("$argon2id$...")],
                vec![Value::from("bal@example.com"), Value::Null],
            ],
            Instant::now(),
        );
        assert_eq!(result.rows[0][1], "[redacted]");
        assert_eq!(result.rows[0][0], "anu@example.com");
        assert_eq!(result.rows[1][1], Value::Null, "nulls stay null");
    }

    #[test]
    fn rows_past_the_limit_are_dropped_and_flagged() {
        let rows = (0..=ROW_LIMIT as i64)
            .map(|i| vec![Value::from(i)])
            .collect();
        let result = QueryResult::new(vec!["n".into()], rows, Instant::now());
        assert_eq!(result.rows.len(), ROW_LIMIT);
        assert!(result.truncated);
        let result = QueryResult::new(vec!["n".into()], vec![vec![Value::from(1)]], Instant::now());
        assert!(!result.truncated);
    }

    #[test]
    fn postgres_text_values_get_json_types() {
        assert_eq!(pg_text_value("INT8", "42"), Value::from(42));
        assert_eq!(pg_text_value("NUMERIC", "1840000"), Value::from(1_840_000));
        assert_eq!(pg_text_value("NUMERIC", "12.50"), Value::from(12.5));
        assert_eq!(
            pg_text_value("NUMERIC", "123456789012345678901234567890"),
            Value::from("123456789012345678901234567890")
        );
        assert_eq!(pg_text_value("FLOAT8", "NaN"), Value::from("NaN"));
        assert_eq!(pg_text_value("BOOL", "t"), Value::Bool(true));
        assert_eq!(
            pg_text_value("JSONB", r#"{"a": 1}"#),
            serde_json::json!({"a": 1})
        );
        assert_eq!(
            pg_text_value("TIMESTAMPTZ", "2026-10-27 09:00:00+00"),
            Value::from("2026-10-27 09:00:00+00")
        );
    }

    #[test]
    fn blobs_are_shown_as_hex() {
        assert_eq!(hex(&[0x0a, 0xff]), "\\x0aff");
        assert!(hex(&[0; 100]).ends_with("... (100 bytes)"));
    }

    /// A database with one table, `users`, holding one row.
    fn users_db(dir: &std::path::Path) -> SqliteFile {
        let path = dir.join("app.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT); \
             INSERT INTO users (email) VALUES ('anu@example.com');",
        )
        .unwrap();
        drop(conn);
        SqliteFile::Path(path)
    }

    /// `char(..)` of `text`'s bytes: a string SQL without a quote in it.
    fn chars(text: &str) -> String {
        let codes: Vec<String> = text.bytes().map(|b| b.to_string()).collect();
        format!("char({})", codes.join(","))
    }

    /// Text SQLite reads as several statements while a SQL parser of
    /// another make reads one: each, sent to SQLite as it is (past the guard),
    /// is refused, and writes nothing, attaches nothing, creates no file.
    #[test]
    fn sqlite_itself_refuses_smuggled_statements() {
        let dir = tempfile::tempdir().unwrap();
        let db = users_db(dir.path());
        let copy = dir.path().join("copy.db");
        let attached = dir.path().join("attached.db");
        let mut inputs = Vec::new();
        for variable in ["$a", ":a", "@a", "#a", "$a::b", ":a::b"] {
            for tail in [
                format!("VACUUM INTO {}", chars(&copy.to_string_lossy())),
                format!(
                    "ATTACH DATABASE {} AS n; CREATE TABLE n.t (a)",
                    chars(&attached.to_string_lossy())
                ),
                "PRAGMA query_only = OFF; DELETE FROM users".to_owned(),
            ] {
                inputs.push(format!(
                    "SELECT {variable}::ENUM('x)'), '; PRAGMA query_only = OFF; {tail}; --'"
                ));
            }
        }
        inputs.extend([
            format!("VACUUM INTO {}", chars(&copy.to_string_lossy())),
            "PRAGMA query_only = OFF".to_owned(),
            "SELECT 1; DELETE FROM users".to_owned(),
            "DELETE FROM users".to_owned(),
            "BEGIN".to_owned(),
            format!(
                "ATTACH DATABASE {} AS n",
                chars(&attached.to_string_lossy())
            ),
            "SELECT * FROM pragma_table_info('users')".to_owned(),
            "SELECT load_extension('x')".to_owned(),
            "SELECT ?".to_owned(),
            "SELECT :name".to_owned(),
        ]);
        for sql in &inputs {
            assert!(run_sqlite(&db, sql).is_err(), "{sql}");
            assert!(!copy.exists(), "{sql}: VACUUM INTO wrote a copy");
            assert!(!attached.exists(), "{sql}: ATTACH created a file");
        }
        let users = run_sqlite(&db, "SELECT count(*) AS n FROM users").unwrap();
        assert_eq!(
            users.rows,
            vec![vec![Value::from(1)]],
            "nothing was deleted"
        );
        // Trailing whitespace and comments are not a second statement.
        let ok = run_sqlite(&db, "SELECT email FROM users -- the one user\n /* done */ ").unwrap();
        assert_eq!(ok.columns, ["email"]);
    }

    #[test]
    fn sqlite_refusals_say_why() {
        let dir = tempfile::tempdir().unwrap();
        let db = users_db(dir.path());
        assert!(matches!(
            run_sqlite(&db, "SELECT 1; SELECT 2"),
            Err(DatabaseError::Refused(Refusal::Trailing))
        ));
        assert!(matches!(
            run_sqlite(&db, "PRAGMA query_only = OFF"),
            Err(DatabaseError::Refused(Refusal::NotAuthorized { .. }))
        ));
        assert!(matches!(
            run_sqlite(&db, "SELECT :name"),
            Err(DatabaseError::Refused(Refusal::Parameter { .. }))
        ));
        let empty = run_sqlite(&db, "SELECT id, email FROM users WHERE id = 0").unwrap();
        assert_eq!(empty.columns, ["id", "email"], "columns without rows");
        assert!(empty.rows.is_empty());
    }
}
