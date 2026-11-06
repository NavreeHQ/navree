//! Runs statements on a test database and reads rows back as normalized
//! JSON: the one place these tests touch a driver. Statements run through
//! navree-db's own engine on SQLx (its pools, transactions and statements),
//! and rows are read as navree-db `Value`s, so every case also exercises
//! the data layer the renderer's output will run on.
//!
//! Normalized values compare across databases: integers as integers,
//! decimals and floats as numbers (whole ones as integers, others rounded
//! to six places), booleans as 1 or 0, date-times as UTC RFC 3339
//! (`2026-10-27T08:30:00Z`; text that reads as a date-time too, since
//! SQLite keeps them as text), dates as `YYYY-MM-DD`, JSON as JSON (text
//! holding a JSON object or array too), bytes as text when they are UTF-8.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, SecondsFormat, Utc};
use navree_db::{Db, DbOptions, Executor, Value};
use navree_orm::{Decimal, Dialect, Statement, Syntax};
use serde_json::Value as Json;

/// One row: its columns in order, as `(name, value)`.
pub type Row = Vec<(String, Json)>;

/// A database the statements run on.
#[derive(Clone)]
pub struct Exec {
    db: Db,
    dialect: Dialect,
    syntax: Syntax,
}

impl Exec {
    /// Connects to `url`.
    pub async fn connect(url: &str, dialect: Dialect) -> Self {
        let db = Db::connect(&DbOptions {
            url: url.to_owned(),
            max_connections: 2,
            ..DbOptions::default()
        })
        .await
        .unwrap_or_else(|error| panic!("cannot connect to the {dialect} test database: {error}"));
        let mariadb = db.mariadb_version().await.unwrap_or_else(|error| {
            panic!("asks the {dialect} test database its version: {error}")
        });
        Exec {
            db,
            dialect,
            syntax: Syntax::of(dialect, mariadb),
        }
    }

    /// The dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// The server's syntax (MariaDB's, or the dialect's).
    pub fn syntax(&self) -> Syntax {
        self.syntax
    }

    /// The `utf8mb4` collation of new databases: MySQL 8's
    /// `utf8mb4_0900_ai_ci`, or on a MariaDB without it (10.6) the
    /// accent- and case-insensitive `utf8mb4_unicode_520_ci`.
    pub async fn utf8mb4_collation(&self) -> &'static str {
        let known: i64 = navree_db::ExecutorExt::fetch_scalar(
            &self.db,
            &navree_db::Statement::text(
                "SELECT COUNT(*) FROM information_schema.COLLATIONS WHERE COLLATION_NAME = 'utf8mb4_0900_ai_ci'",
            ),
        )
        .await
        .expect("reads the server's collations");
        if known > 0 {
            "utf8mb4_0900_ai_ci"
        } else {
            "utf8mb4_unicode_520_ci"
        }
    }

    /// Runs SQL written for this database (schema and fixtures), statement
    /// by statement.
    pub async fn batch(&self, sql: &str) -> Result<(), String> {
        for statement in sql.split(";\n").map(str::trim).filter(|s| !s.is_empty()) {
            self.db
                .execute_script(statement)
                .await
                .map_err(|error| format!("{statement}: {error}"))?;
        }
        Ok(())
    }

    /// The rows `statement` returns.
    pub async fn rows(&self, statement: &Statement) -> Result<Vec<Row>, String> {
        let rows = self
            .db
            .fetch_all(&engine_statement(statement)?)
            .await
            .map_err(|error| error.to_string())?;
        rows.iter().map(row).collect()
    }

    /// The rows `statement` returns, read inside a transaction of its own
    /// (row locks).
    pub async fn rows_in_transaction(&self, statement: &Statement) -> Result<Vec<Row>, String> {
        let tx = self.db.begin().await.map_err(|error| error.to_string())?;
        let rows = tx
            .fetch_all(&engine_statement(statement)?)
            .await
            .map_err(|error| error.to_string());
        tx.commit().await.map_err(|error| error.to_string())?;
        rows?.iter().map(row).collect()
    }

    /// Runs `statement`; the number of rows it changed.
    pub async fn execute(&self, statement: &Statement) -> Result<u64, String> {
        self.db
            .execute(&engine_statement(statement)?)
            .await
            .map(|done| done.rows_affected)
            .map_err(|error| error.to_string())
    }

    /// Every row of `table`, ordered by `order`.
    pub async fn dump(&self, table: &str, order: &str) -> Result<Vec<Row>, String> {
        let statement = Statement {
            sql: format!("SELECT * FROM {table} ORDER BY {order}"),
            values: Vec::new(),
            dialect: self.dialect,
        };
        self.rows(&statement).await
    }

    /// Closes the connection pool.
    pub async fn close(self) {
        let _ = self.db.close().await;
    }
}

/// The ORM's statement as navree-db runs it (its SQL is already in the
/// backend's placeholders).
fn engine_statement(statement: &Statement) -> Result<navree_db::Statement, String> {
    Ok(statement.to_db())
}

/// A row, normalized.
fn row(row: &navree_db::Row) -> Result<Row, String> {
    row.columns()
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let value = row.value(index).map_err(|error| error.to_string())?;
            Ok((name, normalize(value)))
        })
        .collect()
}

/// A value, normalized.
fn normalize(value: Value) -> Json {
    match value {
        Value::Null(_) => Json::Null,
        Value::Bool(b) => Json::from(i64::from(b)),
        Value::I8(n) => Json::from(n),
        Value::I16(n) => Json::from(n),
        Value::I32(n) => Json::from(n),
        Value::I64(n) => Json::from(n),
        Value::U8(n) => Json::from(n),
        Value::U16(n) => Json::from(n),
        Value::U32(n) => Json::from(n),
        Value::U64(n) => Json::from(n),
        Value::F32(n) => number(f64::from(n)),
        Value::F64(n) => number(n),
        Value::Decimal(d) => decimal(d),
        Value::String(s) => text(s),
        Value::Bytes(b) => bytes(b),
        Value::Json(j) => json(j),
        Value::Uuid(id) => Json::String(id.to_string()),
        Value::Date(d) => date(d),
        Value::Time(t) => time(t),
        Value::DateTime(at) => naive_date_time(at),
        Value::Timestamp(at) => date_time(at),
        Value::TimestampTz(at) => date_time(at.with_timezone(&Utc)),
        other => Json::String(format!("{other:?}")),
    }
}

/// A float as a comparable number: whole values as integers, others
/// rounded to six decimal places.
pub fn number(value: f64) -> Json {
    if !value.is_finite() {
        return Json::String(value.to_string());
    }
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        return Json::from(value as i64);
    }
    Json::from((value * 1e6).round() / 1e6)
}

/// A decimal as a comparable number.
pub fn decimal(value: Decimal) -> Json {
    let value = value.normalize();
    if value.scale() == 0
        && let Ok(int) = i64::try_from(value)
    {
        return Json::from(int);
    }
    match f64::try_from(value) {
        Ok(float) => number(float),
        Err(_) => Json::String(value.to_string()),
    }
}

/// A date-time in UTC as RFC 3339.
pub fn date_time(value: DateTime<Utc>) -> Json {
    Json::String(value.to_rfc3339_opts(SecondsFormat::AutoSi, true))
}

/// A date-time without zone, read as UTC.
pub fn naive_date_time(value: NaiveDateTime) -> Json {
    date_time(value.and_utc())
}

/// A date as `YYYY-MM-DD`.
pub fn date(value: NaiveDate) -> Json {
    Json::String(value.format("%Y-%m-%d").to_string())
}

/// A time of day as `HH:MM:SS[.f]`.
pub fn time(value: NaiveTime) -> Json {
    Json::String(value.format("%H:%M:%S%.f").to_string())
}

/// Text, read as a date-time or as JSON when it holds one.
pub fn text(value: String) -> Json {
    if let Some(at) = parse_date_time(&value) {
        return date_time(at);
    }
    let trimmed = value.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(json) = serde_json::from_str::<Json>(&value)
    {
        return json;
    }
    Json::String(value)
}

/// Bytes: their text when they are UTF-8, else hexadecimal.
pub fn bytes(value: Vec<u8>) -> Json {
    match String::from_utf8(value) {
        Ok(text) => self::text(text),
        Err(error) => Json::String(
            error
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        ),
    }
}

/// A JSON value: kept, numbers normalized.
pub fn json(value: Json) -> Json {
    match value {
        Json::Number(number) => match number.as_i64() {
            Some(int) => Json::from(int),
            None => self::number(number.as_f64().unwrap_or_default()),
        },
        Json::Array(items) => Json::Array(items.into_iter().map(json).collect()),
        Json::Object(map) => Json::Object(map.into_iter().map(|(k, v)| (k, json(v))).collect()),
        other => other,
    }
}

/// A date-time written as text (RFC 3339, or SQL's `YYYY-MM-DD
/// HH:MM:SS[.f][+zone]`), in UTC.
fn parse_date_time(text: &str) -> Option<DateTime<Utc>> {
    if text.len() < 16 || !text.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    if let Ok(at) = DateTime::parse_from_rfc3339(text) {
        return Some(at.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f%:z", "%Y-%m-%d %H:%M:%S%.f%#z"] {
        if let Ok(at) = DateTime::parse_from_str(text, format) {
            return Some(at.with_timezone(&Utc));
        }
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(at) = NaiveDateTime::parse_from_str(text, format) {
            return Some(at.and_utc());
        }
    }
    None
}

#[test]
fn values_normalize() {
    assert_eq!(number(150.0), Json::from(150));
    assert_eq!(number(10.1234567), Json::from(10.123457));
    assert_eq!(decimal("150.0000000000".parse().unwrap()), Json::from(150));
    assert_eq!(
        text("2026-10-27T08:30:00+00:00".to_owned()),
        Json::from("2026-10-27T08:30:00Z")
    );
    assert_eq!(
        text("2026-10-27 08:30:00".to_owned()),
        Json::from("2026-10-27T08:30:00Z")
    );
    assert_eq!(text("2026-10-27".to_owned()), Json::from("2026-10-27"));
    assert_eq!(text("[3, 4]".to_owned()), serde_json::json!([3, 4]));
    assert_eq!(text("Agra".to_owned()), Json::from("Agra"));
}
