//! Runs statements on a test database and reads rows back as normalized
//! JSON: the one place these tests touch a driver. Today it runs through
//! navree-db's connection (SeaORM's `ConnectionTrait`, the rows read as
//! sqlx rows); swapping it for navree-db's own executor changes this file
//! only.
//!
//! Normalized values compare across databases: integers as integers,
//! decimals and floats as numbers (whole ones as integers, others rounded
//! to six places), booleans as 1 or 0, date-times as UTC RFC 3339
//! (`2026-10-27T08:30:00Z`; text that reads as a date-time too, since
//! SQLite keeps them as text), dates as `YYYY-MM-DD`, JSON as JSON (text
//! holding a JSON object or array too), bytes as text when they are UTF-8.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, SecondsFormat, Utc};
use navree_db::sea_orm::{ConnectionTrait, QueryResult, TransactionTrait};
use navree_db::{Db, DbOptions};
use navree_orm::{Decimal, Dialect, Statement};
use serde_json::Value as Json;

/// One row: its columns in order, as `(name, value)`.
pub type Row = Vec<(String, Json)>;

/// A database the statements run on.
#[derive(Clone)]
pub struct Exec {
    db: Db,
    dialect: Dialect,
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
        Exec { db, dialect }
    }

    /// The dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Runs SQL written for this database (schema and fixtures), statement
    /// by statement.
    pub async fn batch(&self, sql: &str) -> Result<(), String> {
        for statement in sql.split(";\n").map(str::trim).filter(|s| !s.is_empty()) {
            self.db
                .conn()
                .execute_unprepared(statement)
                .await
                .map_err(|error| format!("{statement}: {error}"))?;
        }
        Ok(())
    }

    /// The rows `statement` returns.
    pub async fn rows(&self, statement: &Statement) -> Result<Vec<Row>, String> {
        let rows = self
            .db
            .conn()
            .query_all_raw(statement.to_sea_orm())
            .await
            .map_err(|error| error.to_string())?;
        rows.iter().map(|row| self.row(row)).collect()
    }

    /// The rows `statement` returns, read inside a transaction of its own
    /// (row locks).
    pub async fn rows_in_transaction(&self, statement: &Statement) -> Result<Vec<Row>, String> {
        let tx = self
            .db
            .conn()
            .begin()
            .await
            .map_err(|error| error.to_string())?;
        let rows = tx
            .query_all_raw(statement.to_sea_orm())
            .await
            .map_err(|error| error.to_string());
        tx.commit().await.map_err(|error| error.to_string())?;
        rows?.iter().map(|row| self.row(row)).collect()
    }

    /// Runs `statement`; the number of rows it changed.
    pub async fn execute(&self, statement: &Statement) -> Result<u64, String> {
        self.db
            .conn()
            .execute_raw(statement.to_sea_orm())
            .await
            .map(|result| result.rows_affected())
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
        let _ = self.db.conn().clone().close().await;
    }

    fn row(&self, row: &QueryResult) -> Result<Row, String> {
        #[cfg(feature = "postgres")]
        if let Some(row) = row.try_as_pg_row() {
            return postgres::row(row);
        }
        #[cfg(feature = "mysql")]
        if let Some(row) = row.try_as_mysql_row() {
            return mysql::row(row);
        }
        if let Some(row) = row.try_as_sqlite_row() {
            return sqlite::row(row);
        }
        Err("a row of an unexpected driver".to_owned())
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
// Read by the PostgreSQL and MySQL decoders.
#[cfg_attr(not(any(feature = "postgres", feature = "mysql")), allow(dead_code))]
pub fn naive_date_time(value: NaiveDateTime) -> Json {
    date_time(value.and_utc())
}

/// A date as `YYYY-MM-DD`.
// Read by the PostgreSQL and MySQL decoders.
#[cfg_attr(not(any(feature = "postgres", feature = "mysql")), allow(dead_code))]
pub fn date(value: NaiveDate) -> Json {
    Json::String(value.format("%Y-%m-%d").to_string())
}

/// A time of day as `HH:MM:SS[.f]`.
// Read by the PostgreSQL and MySQL decoders.
#[cfg_attr(not(any(feature = "postgres", feature = "mysql")), allow(dead_code))]
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
// Read by the PostgreSQL and MySQL decoders.
#[cfg_attr(not(any(feature = "postgres", feature = "mysql")), allow(dead_code))]
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

#[cfg(feature = "postgres")]
mod postgres {
    use navree_db::sea_orm::sqlx::postgres::PgRow;
    use navree_db::sea_orm::sqlx::{Column, Row as _, TypeInfo, ValueRef};

    use super::*;

    pub fn row(row: &PgRow) -> Result<Row, String> {
        let mut out = Vec::with_capacity(row.columns().len());
        for (i, column) in row.columns().iter().enumerate() {
            out.push((column.name().to_owned(), value(row, i)?));
        }
        Ok(out)
    }

    fn value(row: &PgRow, i: usize) -> Result<Json, String> {
        let raw = row.try_get_raw(i).map_err(|error| error.to_string())?;
        if raw.is_null() {
            return Ok(Json::Null);
        }
        let ty = raw.type_info().name().to_owned();
        let error = |error: navree_db::sea_orm::sqlx::Error| format!("{ty}: {error}");
        Ok(match ty.as_str() {
            "INT2" => Json::from(row.try_get_unchecked::<i16, _>(i).map_err(error)?),
            "INT4" => Json::from(row.try_get_unchecked::<i32, _>(i).map_err(error)?),
            "INT8" => Json::from(row.try_get_unchecked::<i64, _>(i).map_err(error)?),
            "FLOAT4" => number(f64::from(
                row.try_get_unchecked::<f32, _>(i).map_err(error)?,
            )),
            "FLOAT8" => number(row.try_get_unchecked::<f64, _>(i).map_err(error)?),
            "NUMERIC" => decimal(row.try_get_unchecked::<Decimal, _>(i).map_err(error)?),
            "BOOL" => Json::from(i64::from(
                row.try_get_unchecked::<bool, _>(i).map_err(error)?,
            )),
            "TIMESTAMPTZ" => date_time(
                row.try_get_unchecked::<DateTime<Utc>, _>(i)
                    .map_err(error)?,
            ),
            "TIMESTAMP" => naive_date_time(
                row.try_get_unchecked::<NaiveDateTime, _>(i)
                    .map_err(error)?,
            ),
            "DATE" => date(row.try_get_unchecked::<NaiveDate, _>(i).map_err(error)?),
            "TIME" => time(row.try_get_unchecked::<NaiveTime, _>(i).map_err(error)?),
            "JSON" | "JSONB" => json(row.try_get_unchecked::<Json, _>(i).map_err(error)?),
            "UUID" => Json::String(
                row.try_get_unchecked::<uuid::Uuid, _>(i)
                    .map_err(error)?
                    .to_string(),
            ),
            "BYTEA" => bytes(row.try_get_unchecked::<Vec<u8>, _>(i).map_err(error)?),
            _ => text(row.try_get_unchecked::<String, _>(i).map_err(error)?),
        })
    }
}

#[cfg(feature = "mysql")]
mod mysql {
    use navree_db::sea_orm::sqlx::mysql::MySqlRow;
    use navree_db::sea_orm::sqlx::{Column, Row as _, TypeInfo, ValueRef};

    use super::*;

    pub fn row(row: &MySqlRow) -> Result<Row, String> {
        let mut out = Vec::with_capacity(row.columns().len());
        for (i, column) in row.columns().iter().enumerate() {
            out.push((column.name().to_owned(), value(row, i)?));
        }
        Ok(out)
    }

    fn value(row: &MySqlRow, i: usize) -> Result<Json, String> {
        let raw = row.try_get_raw(i).map_err(|error| error.to_string())?;
        if raw.is_null() {
            return Ok(Json::Null);
        }
        let ty = raw.type_info().name().to_owned();
        let error = |error: navree_db::sea_orm::sqlx::Error| format!("{ty}: {error}");
        Ok(match ty.as_str() {
            "BOOLEAN" => Json::from(i64::from(
                row.try_get_unchecked::<bool, _>(i).map_err(error)?,
            )),
            name if name.ends_with("UNSIGNED") => {
                Json::from(row.try_get_unchecked::<u64, _>(i).map_err(error)?)
            }
            "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" | "YEAR" => {
                Json::from(row.try_get_unchecked::<i64, _>(i).map_err(error)?)
            }
            "FLOAT" => number(f64::from(
                row.try_get_unchecked::<f32, _>(i).map_err(error)?,
            )),
            "DOUBLE" => number(row.try_get_unchecked::<f64, _>(i).map_err(error)?),
            "DECIMAL" => decimal(row.try_get_unchecked::<Decimal, _>(i).map_err(error)?),
            "DATETIME" | "TIMESTAMP" => naive_date_time(
                row.try_get_unchecked::<NaiveDateTime, _>(i)
                    .map_err(error)?,
            ),
            "DATE" => date(row.try_get_unchecked::<NaiveDate, _>(i).map_err(error)?),
            "TIME" => time(row.try_get_unchecked::<NaiveTime, _>(i).map_err(error)?),
            "JSON" => json(row.try_get_unchecked::<Json, _>(i).map_err(error)?),
            "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BIT" => {
                bytes(row.try_get_unchecked::<Vec<u8>, _>(i).map_err(error)?)
            }
            _ => text(row.try_get_unchecked::<String, _>(i).map_err(error)?),
        })
    }
}

mod sqlite {
    use navree_db::sea_orm::sqlx::sqlite::SqliteRow;
    use navree_db::sea_orm::sqlx::{Column, Row as _, TypeInfo, ValueRef};

    use super::*;

    pub fn row(row: &SqliteRow) -> Result<Row, String> {
        let mut out = Vec::with_capacity(row.columns().len());
        for (i, column) in row.columns().iter().enumerate() {
            out.push((column.name().to_owned(), value(row, i)?));
        }
        Ok(out)
    }

    /// SQLite's values carry their own storage class, whatever the
    /// column's declared type.
    fn value(row: &SqliteRow, i: usize) -> Result<Json, String> {
        let raw = row.try_get_raw(i).map_err(|error| error.to_string())?;
        if raw.is_null() {
            return Ok(Json::Null);
        }
        let ty = raw.type_info().name().to_owned();
        let error = |error: navree_db::sea_orm::sqlx::Error| format!("{ty}: {error}");
        Ok(match ty.as_str() {
            "INTEGER" | "BOOLEAN" | "INT8" => {
                Json::from(row.try_get_unchecked::<i64, _>(i).map_err(error)?)
            }
            "REAL" => number(row.try_get_unchecked::<f64, _>(i).map_err(error)?),
            "BLOB" => bytes(row.try_get_unchecked::<Vec<u8>, _>(i).map_err(error)?),
            _ => text(row.try_get_unchecked::<String, _>(i).map_err(error)?),
        })
    }
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
    assert_eq!(text("Acme".to_owned()), Json::from("Acme"));
}
