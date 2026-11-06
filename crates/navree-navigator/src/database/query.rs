//! Running a checked statement and turning its rows into JSON.

use std::time::{Duration, Instant};

use futures_util::TryStreamExt;
use serde::Serialize;
use serde_json::Value;
use sqlx::postgres::{PgConnection, PgRow};
use sqlx::sqlite::{SqliteConnection, SqliteRow};
use sqlx::{AssertSqlSafe, Column, Executor, Row, TypeInfo, ValueRef};

use super::{CheckedStatement, DatabaseError, ROW_LIMIT, STATEMENT_TIMEOUT, StatementKind};

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

/// Runs `statement` on a `query_only` SQLite connection; a progress handler
/// interrupts it after [`STATEMENT_TIMEOUT`].
pub(super) async fn sqlite(
    conn: &mut SqliteConnection,
    statement: &CheckedStatement,
) -> Result<QueryResult, DatabaseError> {
    let deadline = Instant::now() + STATEMENT_TIMEOUT;
    conn.lock_handle()
        .await?
        .set_progress_handler(1_000, move || Instant::now() < deadline);
    let started = Instant::now();
    let fetch = async {
        let mut stream = conn.fetch(AssertSqlSafe(statement.sql.clone()));
        let mut rows: Vec<SqliteRow> = Vec::new();
        while let Some(row) = stream.try_next().await? {
            rows.push(row);
            if rows.len() > ROW_LIMIT {
                break;
            }
        }
        Ok::<_, DatabaseError>(rows)
    };
    let rows = with_timeout(fetch).await?;
    let columns = match rows.first() {
        Some(row) => column_names(row.columns()),
        None => describe_columns_sqlite(conn, &statement.sql).await,
    };
    let values = rows.iter().map(sqlite_values).collect();
    Ok(QueryResult::new(columns, values, started))
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
    // Text results (the simple protocol): every type has a readable form.
    let sql = match statement.kind {
        StatementKind::Query => {
            conn.execute(AssertSqlSafe(format!(
                "DECLARE navigator_rows NO SCROLL CURSOR FOR {}",
                statement.sql
            )))
            .await?;
            format!("FETCH FORWARD {} FROM navigator_rows", ROW_LIMIT + 1)
        }
        StatementKind::Explain | StatementKind::Show => statement.sql.clone(),
    };
    let fetch = async {
        let mut stream = conn.fetch(AssertSqlSafe(sql));
        let mut rows: Vec<PgRow> = Vec::new();
        while let Some(row) = stream.try_next().await? {
            rows.push(row);
        }
        Ok::<_, DatabaseError>(rows)
    };
    let rows = with_timeout(fetch).await?;
    let columns = rows
        .first()
        .map(|row| column_names(row.columns()))
        .unwrap_or_default();
    let values = rows.iter().map(postgres_values).collect();
    Ok(QueryResult::new(columns, values, started))
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

/// The column names of a query that returned no rows.
async fn describe_columns_sqlite(conn: &mut SqliteConnection, sql: &str) -> Vec<String> {
    use sqlx::SqlSafeStr;
    match conn
        .prepare(AssertSqlSafe(sql.to_owned()).into_sql_str())
        .await
    {
        Ok(statement) => column_names(sqlx::Statement::columns(&statement)),
        Err(_) => Vec::new(),
    }
}

/// A SQLite row as JSON, by each value's storage class.
fn sqlite_values(row: &SqliteRow) -> Vec<Value> {
    (0..row.columns().len())
        .map(|i| {
            let Ok(raw) = row.try_get_raw(i) else {
                return Value::Null;
            };
            if raw.is_null() {
                return Value::Null;
            }
            let kind = raw.type_info().name().to_owned();
            match kind.as_str() {
                "INTEGER" | "BOOLEAN" => row
                    .try_get_unchecked::<i64, _>(i)
                    .map_or(Value::Null, Value::from),
                "REAL" | "NUMERIC" => row
                    .try_get_unchecked::<f64, _>(i)
                    .map_or(Value::Null, float),
                "BLOB" => row
                    .try_get_unchecked::<Vec<u8>, _>(i)
                    .map_or(Value::Null, |bytes| Value::from(hex(&bytes))),
                _ => row
                    .try_get_unchecked::<String, _>(i)
                    .map_or(Value::Null, Value::from),
            }
        })
        .collect()
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
                vec![Value::from("bob@example.com"), Value::Null],
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
}
