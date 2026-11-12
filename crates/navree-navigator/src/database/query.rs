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

/// The longest value an answer carries: a longer one is cut to this many
/// bytes and ends with [`CUT_MARK`].
pub const MAX_VALUE_BYTES: usize = 4 * 1024;

/// What ends a value cut to [`MAX_VALUE_BYTES`].
pub const CUT_MARK: &str = "… [cut]";

/// About how much an answer holds: rows stop being collected once their
/// values pass it, and `truncated` says so.
pub const MAX_ANSWER_BYTES: usize = 1024 * 1024;

/// The longest string or blob SQLite may make during a query
/// (`SQLITE_LIMIT_LENGTH`): a statement that would make a longer one fails.
const SQLITE_MAX_VALUE: i32 = 1024 * 1024;

/// `text` as an answer's value: whole, or cut to [`MAX_VALUE_BYTES`] (at a
/// character boundary) with [`CUT_MARK`]; and whether it was cut.
fn cut(text: &str) -> (String, bool) {
    if text.len() <= MAX_VALUE_BYTES {
        return (text.to_owned(), false);
    }
    let mut end = MAX_VALUE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}{CUT_MARK}", &text[..end]), true)
}

/// About how many bytes `row` adds to an answer.
fn row_bytes(row: &[Value]) -> usize {
    row.iter()
        .map(|value| match value {
            Value::String(text) => text.len(),
            Value::Null => 4,
            other => other.to_string().len(),
        })
        .sum()
}

/// Rows kept for an answer: at most [`ROW_LIMIT`] + 1 (the extra one says
/// there are more), and none once [`MAX_ANSWER_BYTES`] is passed.
#[derive(Default)]
struct Kept {
    rows: Vec<Vec<Value>>,
    bytes: usize,
    over_budget: bool,
}

impl Kept {
    /// Keeps `row`, or says to stop: there are enough rows, or the answer is
    /// full (the first row is always kept).
    fn keep(&mut self, row: Vec<Value>) -> bool {
        let bytes = row_bytes(&row);
        if !self.rows.is_empty() && self.bytes + bytes > MAX_ANSWER_BYTES {
            self.over_budget = true;
            return false;
        }
        self.bytes += bytes;
        self.rows.push(row);
        self.rows.len() <= ROW_LIMIT
    }
}

impl QueryResult {
    fn new(columns: Vec<String>, kept: Kept, started: Instant) -> Self {
        let Kept {
            mut rows,
            over_budget,
            ..
        } = kept;
        let truncated = rows.len() > ROW_LIMIT || over_budget;
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
/// - no string or blob may grow past 1 MB (`SQLITE_LIMIT_LENGTH`), and the
///   text may not pass twice the guard's limit (`SQLITE_LIMIT_SQL_LENGTH`);
/// - an authorizer, installed before the statement is prepared, allows only
///   reading a table, `SELECT`, recursive CTEs, functions not in the guard's
///   deny list and reading `PRAGMA data_version` (FTS5 does); other PRAGMAs
///   (pragma table functions included), ATTACH, DETACH, writes, DDL and
///   transactions are refused while SQLite prepares the text. R*Tree tables
///   prepare a write when opened, so they cannot be read;
/// - SQLite prepares the text: anything but whitespace or comments after
///   the first statement, as SQLite's own tokenizer reads it, is refused;
/// - the statement must be read-only (`sqlite3_stmt_readonly`), which is
///   what refuses VACUUM (the authorizer has no action for it), and take no
///   parameters; then that one statement runs, interrupted after
///   [`STATEMENT_TIMEOUT`], each text value cut to [`MAX_VALUE_BYTES`] and
///   the rows to about [`MAX_ANSWER_BYTES`].
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
    // No value may grow past 1 MB (`printf('%.*c', 400000000, 'x')`, a
    // recursive `zeroblob(..)`), and no statement past the guard's limit.
    conn.set_limit(
        rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
        SQLITE_MAX_VALUE,
    )
    .map_err(sqlite_error)?;
    let sql_limit = i32::try_from(super::MAX_SQL_BYTES * 2).unwrap_or(i32::MAX);
    conn.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_SQL_LENGTH, sql_limit)
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
        // FTS5 reads `PRAGMA data_version` to see whether its index changed:
        // reading that one pragma, with no value, is all that is allowed.
        AuthAction::Pragma {
            pragma_name: "data_version",
            pragma_value: None,
        } => Authorization::Allow,
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
    let mut kept = Kept::default();
    while let Some(row) = rows.next().map_err(sqlite_error)? {
        let values = (0..columns.len())
            .map(|i| row.get_ref(i).map_or(Value::Null, sqlite_value))
            .collect();
        if !kept.keep(values) {
            break;
        }
    }
    Ok(QueryResult::new(columns, kept, started))
}

/// A SQLite value as JSON, by its storage class; text cut to
/// [`MAX_VALUE_BYTES`].
fn sqlite_value(value: rusqlite::types::ValueRef<'_>) -> Value {
    use rusqlite::types::ValueRef;

    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => Value::from(n),
        ValueRef::Real(f) => float(f),
        ValueRef::Text(text) => {
            let text = &text[..text.len().min(MAX_VALUE_BYTES + 4)];
            Value::from(cut(&String::from_utf8_lossy(text)).0)
        }
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
    // Pinned again inside the transaction: behind a pooler in transaction mode
    // another client's session-level SET could have reached this connection
    // since the check after connecting.
    conn.execute("SET LOCAL standard_conforming_strings = on")
        .await?;
    let started = Instant::now();
    // The checked statement goes to the server as one prepared statement (the
    // extended protocol): a prepared statement holds exactly one command, so
    // however the server reads the text, it cannot run a second one. A query is
    // declared as a cursor that way, then read with a FETCH of this module's own
    // over the simple protocol, whose text results give every type a readable
    // form. EXPLAIN and SHOW answer in text-like types (text, json, xml), whose
    // binary form is their text.
    let kept = match statement.kind {
        StatementKind::Query => {
            // The columns, from the statement as prepared (nothing runs). The
            // cursor selects each as text cut to MAX_VALUE_BYTES + 1 characters
            // on the server, so no value larger than that crosses the wire:
            // the server's own cost is bounded by the statement timeout.
            use sqlx::SqlSafeStr;
            let prepared = conn
                .prepare(AssertSqlSafe(statement.sql.clone()).into_sql_str())
                .await?;
            let described: Vec<(String, String)> = sqlx::Statement::columns(&prepared)
                .iter()
                .map(|column| {
                    (
                        column.name().to_owned(),
                        column.type_info().name().to_owned(),
                    )
                })
                .collect();
            let declare = format!(
                "DECLARE navigator_rows NO SCROLL CURSOR FOR {}",
                cut_columns(&statement.sql, described.len())
            );
            conn.execute(sqlx::query(AssertSqlSafe(declare)).persistent(false))
                .await?;
            let fetch = format!("FETCH FORWARD {} FROM navigator_rows", ROW_LIMIT + 1);
            let types: Vec<String> = described.iter().map(|(_, kind)| kind.clone()).collect();
            let mut kept = with_timeout(keep_rows(
                conn.fetch(AssertSqlSafe(fetch)),
                Some(types.as_slice()),
            ))
            .await?;
            kept.columns = described.into_iter().map(|(name, _)| name).collect();
            kept
        }
        StatementKind::Explain | StatementKind::Show => {
            let prepared = sqlx::query(AssertSqlSafe(statement.sql.clone())).persistent(false);
            with_timeout(keep_rows(conn.fetch(prepared), None)).await?
        }
    };
    let PgKept { columns, kept } = kept;
    Ok(QueryResult::new(columns, kept, started))
}

/// `sql` (a query with `columns` columns) as a query of the same rows whose
/// every column is its text cut to [`MAX_VALUE_BYTES`] + 1 characters,
/// named `c1`, `c2`, ... by position (so duplicate names don't matter).
fn cut_columns(sql: &str, columns: usize) -> String {
    if columns == 0 {
        return sql.to_owned();
    }
    let limit = MAX_VALUE_BYTES + 1;
    let picks: Vec<String> = (1..=columns)
        .map(|i| format!("pg_catalog.left(c{i}::text, {limit})"))
        .collect();
    let names: Vec<String> = (1..=columns).map(|i| format!("c{i}")).collect();
    format!(
        "SELECT {} FROM ({sql}) AS navigator_rows_source ({})",
        picks.join(", "),
        names.join(", ")
    )
}

/// Rows read from PostgreSQL, and the names of their columns.
struct PgKept {
    columns: Vec<String>,
    kept: Kept,
}

/// The rows of `stream` as an answer keeps them: each value cut, typed by
/// `types` (the columns' own types, when the rows are their text) or by the
/// rows' own column types, and no more once [`Kept::keep`] says to stop.
async fn keep_rows(
    mut stream: impl futures_util::Stream<Item = Result<PgRow, sqlx::Error>> + Unpin,
    types: Option<&[String]>,
) -> Result<PgKept, DatabaseError> {
    let mut kept = Kept::default();
    let mut columns = Vec::new();
    while let Some(row) = stream.try_next().await? {
        if columns.is_empty() {
            columns = column_names(row.columns());
        }
        if !kept.keep(postgres_values(&row, types)) {
            break;
        }
    }
    Ok(PgKept { columns, kept })
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
///
/// `types` names the columns' own types when the row holds their text (the
/// cursor's columns are all text); otherwise each column's type is used. A
/// value longer than [`MAX_VALUE_BYTES`] comes back cut, as text.
fn postgres_values(row: &PgRow, types: Option<&[String]>) -> Vec<Value> {
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
            let (text, was_cut) = cut(text);
            if was_cut {
                return Value::from(text);
            }
            let type_name = types
                .and_then(|types| types.get(i))
                .map_or_else(|| column.type_info().name(), String::as_str);
            pg_text_value(type_name, &text)
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
        // `t` / `f` as PostgreSQL writes it, `true` / `false` as `bool::text`.
        "BOOL" => match text {
            "t" | "true" => Value::Bool(true),
            "f" | "false" => Value::Bool(false),
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
            kept(vec![
                vec![Value::from("anu@example.com"), Value::from("$argon2id$...")],
                vec![Value::from("bal@example.com"), Value::Null],
            ]),
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
        let result = QueryResult::new(vec!["n".into()], kept(rows), Instant::now());
        assert_eq!(result.rows.len(), ROW_LIMIT);
        assert!(result.truncated);
        let result = QueryResult::new(
            vec!["n".into()],
            kept(vec![vec![Value::from(1)]]),
            Instant::now(),
        );
        assert!(!result.truncated);
    }

    /// `rows` kept as an answer keeps them.
    fn kept(rows: Vec<Vec<Value>>) -> Kept {
        let mut kept = Kept::default();
        for row in rows {
            if !kept.keep(row) {
                break;
            }
        }
        kept
    }

    #[test]
    fn long_values_are_cut_and_a_full_answer_stops() {
        let (short, was_cut) = cut("abc");
        assert_eq!((short.as_str(), was_cut), ("abc", false));
        let long = "é".repeat(MAX_VALUE_BYTES);
        let (text, was_cut) = cut(&long);
        assert!(was_cut);
        assert!(text.ends_with(CUT_MARK), "{text}");
        assert!(text.len() <= MAX_VALUE_BYTES + CUT_MARK.len());
        // 100 rows of 30 values of 4 KB each pass 1 MB long before the end.
        let row = vec![Value::from("x".repeat(MAX_VALUE_BYTES)); 30];
        let answer = QueryResult::new(vec!["c".into(); 30], kept(vec![row; 100]), Instant::now());
        assert!(answer.truncated);
        assert!(answer.rows.len() < 10, "{}", answer.rows.len());
        assert!(answer.rows.len() * 30 * MAX_VALUE_BYTES <= MAX_ANSWER_BYTES);
    }

    #[test]
    fn postgres_columns_are_read_as_cut_text_by_position() {
        assert_eq!(
            cut_columns("SELECT 1, 1", 2),
            format!(
                "SELECT pg_catalog.left(c1::text, {limit}), pg_catalog.left(c2::text, {limit}) \
                 FROM (SELECT 1, 1) AS navigator_rows_source (c1, c2)",
                limit = MAX_VALUE_BYTES + 1
            )
        );
        assert_eq!(cut_columns("SELECT", 0), "SELECT");
        assert_eq!(pg_text_value("BOOL", "true"), Value::Bool(true));
        assert_eq!(pg_text_value("BOOL", "f"), Value::Bool(false));
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

    /// A statement that would make a huge value fails in SQLite, instead of
    /// making Navigator hold it; a long value comes back cut.
    #[test]
    fn sqlite_values_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let db = users_db(dir.path());
        for sql in [
            "SELECT printf('%.*c', 400000000, 'x') FROM (SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3)",
            "WITH RECURSIVE b(n, z) AS (SELECT 1, zeroblob(300000000) UNION ALL SELECT n + 1, z FROM b WHERE n < 100) \
             SELECT n FROM b",
            "SELECT length(zeroblob(2000000))",
        ] {
            let failed = run_sqlite(&db, sql).unwrap_err();
            assert!(failed.to_string().contains("too big"), "{sql}: {failed}");
        }
        let long = run_sqlite(&db, "SELECT printf('%.*c', 900000, 'x') AS x").unwrap();
        let text = long.rows[0][0].as_str().unwrap();
        assert!(text.ends_with(CUT_MARK), "{}", text.len());
        assert!(text.len() <= MAX_VALUE_BYTES + CUT_MARK.len());
    }

    /// FTS5 reads `PRAGMA data_version`, which the authorizer allows, and
    /// nothing else of the kind.
    #[test]
    fn sqlite_fts5_tables_can_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docs.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE docs USING fts5(body); \
             INSERT INTO docs (body) VALUES ('ਸਤ ਸ੍ਰੀ ਅਕਾਲ'), ('the deal was won'), ('a lead');",
        )
        .unwrap();
        drop(conn);
        let db = SqliteFile::Path(path);
        let found = run_sqlite(&db, "SELECT body FROM docs WHERE docs MATCH 'deal'").unwrap();
        assert_eq!(found.rows, vec![vec![Value::from("the deal was won")]]);
        // Reading that counter is harmless; setting a pragma, or reading another, is not allowed.
        assert!(run_sqlite(&db, "PRAGMA data_version").is_ok());
        assert!(run_sqlite(&db, "PRAGMA data_version = 1").is_err());
        assert!(run_sqlite(&db, "PRAGMA user_version").is_err());
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
