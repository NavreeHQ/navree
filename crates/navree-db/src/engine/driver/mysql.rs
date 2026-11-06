//! MySQL and MariaDB: binding values, reading columns, and the session
//! every pooled connection starts with.

use std::sync::Arc;

use sqlx::mysql::{MySqlArguments, MySqlConnectOptions, MySqlPoolOptions, MySqlRow, MySqlValueRef};
use sqlx::{ConnectOptions, Decode, MySql, Row as _, TypeInfo, ValueRef};

use super::Keyed;

use crate::engine::{Backend, DbError, Kind, Value};
use crate::{DbOptions, MySqlOptions};

/// The text MySQL prepares for `sql` and its values as MySQL binds them:
/// UUIDs as 16 bytes (the `BINARY(16)` columns the schema builder makes),
/// instants as `DATETIME` values in UTC (see [`bind`]).
///
/// MySQL sends the values' types with every run, but a prepared statement
/// keeps the result types it derived from earlier runs' values (`SELECT
/// ?` read back text as a `DOUBLE` after a boolean ran), so the
/// text names the types the values bind, as on PostgreSQL
/// ([`Keyed`](super::Keyed)).
pub(crate) fn statement(sql: &str, values: &[Value]) -> Result<(String, MySqlArguments), DbError> {
    let mut keyed = Keyed::new(sql, values.len());
    for value in values {
        bind(&mut keyed, value).map_err(|error| {
            DbError::invalid(format!("cannot bind {} on MySQL: {error}", value.kind()))
        })?;
    }
    Ok(keyed.finish())
}

/// Binds `value`. An instant binds as a `DATETIME` holding its UTC time
/// (every session runs in UTC), not as a `TIMESTAMP`: MariaDB reads a
/// `TIMESTAMP` value through its own `TIMESTAMP` range and comparison, so
/// one after 2038 is stored as `NULL` in a `DATETIME` column, and
/// `timestamp_column IN (?, ?)` matches the first value only. MySQL stores
/// and compares both forms alike.
fn bind(
    args: &mut Keyed<'_, MySqlArguments>,
    value: &Value,
) -> Result<(), sqlx::error::BoxDynError> {
    match value {
        Value::Null(kind) => bind_null(args, *kind),
        Value::Bool(v) => args.add(*v),
        Value::I8(v) => args.add(*v),
        Value::I16(v) => args.add(*v),
        Value::I32(v) => args.add(*v),
        Value::I64(v) => args.add(*v),
        Value::U8(v) => args.add(*v),
        Value::U16(v) => args.add(*v),
        Value::U32(v) => args.add(*v),
        Value::U64(v) => args.add(*v),
        Value::F32(v) => args.add(*v),
        Value::F64(v) => args.add(*v),
        Value::Decimal(v) => args.add(*v),
        Value::String(v) => args.add(v.as_str()),
        Value::Bytes(v) => args.add(v.as_slice()),
        Value::Json(v) => args.add(v),
        Value::Uuid(v) => args.add(*v),
        Value::Date(v) => args.add(*v),
        Value::Time(v) => args.add(*v),
        Value::DateTime(v) => args.add(*v),
        Value::Timestamp(v) => args.add(v.naive_utc()),
        Value::TimestampTz(v) => args.add(v.naive_utc()),
        Value::Enum { value, .. } => args.add(value.as_str()),
        Value::Vector(v) => args.add(super::vector_bytes(v)),
    }
}

fn bind_null(
    args: &mut Keyed<'_, MySqlArguments>,
    kind: Kind,
) -> Result<(), sqlx::error::BoxDynError> {
    match kind {
        Kind::Bool => args.add(None::<bool>),
        Kind::I8 => args.add(None::<i8>),
        Kind::I16 => args.add(None::<i16>),
        Kind::I32 => args.add(None::<i32>),
        Kind::I64 => args.add(None::<i64>),
        Kind::U8 => args.add(None::<u8>),
        Kind::U16 => args.add(None::<u16>),
        Kind::U32 => args.add(None::<u32>),
        Kind::U64 => args.add(None::<u64>),
        Kind::F32 => args.add(None::<f32>),
        Kind::F64 => args.add(None::<f64>),
        Kind::Decimal => args.add(None::<rust_decimal::Decimal>),
        Kind::String => args.add(None::<String>),
        Kind::Bytes => args.add(None::<Vec<u8>>),
        Kind::Json => args.add(None::<serde_json::Value>),
        Kind::Uuid => args.add(None::<uuid::Uuid>),
        Kind::Date => args.add(None::<chrono::NaiveDate>),
        Kind::Time => args.add(None::<chrono::NaiveTime>),
        Kind::DateTime | Kind::Timestamp | Kind::TimestampTz => {
            args.add(None::<chrono::NaiveDateTime>)
        }
        Kind::Enum => args.add(None::<String>),
        Kind::Vector => args.add(None::<Vec<u8>>),
    }
}

/// Column `index` of `row` as a [`Value`], read by the column's type.
/// `TINYINT(1)` (`BOOLEAN`) reads as [`Value::I8`], since the column may
/// hold other small numbers; [`FromValue`](crate::FromValue) reads `0` and
/// `1` as booleans.
pub(crate) fn value(row: &MySqlRow, index: usize) -> Result<Value, DbError> {
    let raw = row
        .try_get_raw(index)
        .map_err(|error| DbError::from_sqlx(error, Some(Backend::MySql)))?;
    let type_name = raw.type_info().name().to_owned();
    if raw.is_null() {
        return Ok(Value::Null(kind_of(&type_name)));
    }
    decode(raw, &type_name).map_err(|error| {
        let column = row.columns().get(index).map_or("?", sqlx::Column::name);
        DbError::new(
            crate::ErrorKind::Decode,
            format!("cannot read column `{column}` ({type_name}): {error}"),
        )
    })
}

fn decode(raw: MySqlValueRef<'_>, type_name: &str) -> Result<Value, sqlx::error::BoxDynError> {
    fn d<'r, T: Decode<'r, MySql>>(raw: MySqlValueRef<'r>) -> Result<T, sqlx::error::BoxDynError> {
        T::decode(raw)
    }
    Ok(match type_name {
        "BOOLEAN" | "TINYINT" => Value::I8(d(raw)?),
        "TINYINT UNSIGNED" => Value::U8(d(raw)?),
        "SMALLINT" => Value::I16(d(raw)?),
        "SMALLINT UNSIGNED" | "YEAR" => Value::U16(d(raw)?),
        "INT" | "MEDIUMINT" => Value::I32(d(raw)?),
        "INT UNSIGNED" | "MEDIUMINT UNSIGNED" => Value::U32(d(raw)?),
        "BIGINT" => Value::I64(d(raw)?),
        "BIGINT UNSIGNED" | "BIT" => Value::U64(d(raw)?),
        "FLOAT" => Value::F32(d(raw)?),
        "DOUBLE" => Value::F64(d(raw)?),
        "DECIMAL" => Value::Decimal(d(raw)?),
        "DATE" => Value::Date(d(raw)?),
        "TIME" => match d::<chrono::NaiveTime>(raw.clone()) {
            Ok(time) => Value::Time(time),
            // A duration beyond a day, or a negative one.
            Err(_) => Value::String(format!("{:?}", d::<sqlx::mysql::types::MySqlTime>(raw)?)),
        },
        "DATETIME" => Value::DateTime(d(raw)?),
        "TIMESTAMP" => Value::Timestamp(d(raw)?),
        "JSON" => Value::Json(d(raw)?),
        "NULL" => Value::Null(Kind::String),
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" | "GEOMETRY" => {
            Value::Bytes(d(raw)?)
        }
        // Text, or what is not text (MySQL 9's binary `VECTOR`).
        _ => match d::<String>(raw.clone()) {
            Ok(text) => Value::String(text),
            Err(_) => Value::Bytes(d(raw)?),
        },
    })
}

/// The kind of a column of MySQL type `type_name` (for its NULLs).
fn kind_of(type_name: &str) -> Kind {
    match type_name {
        "BOOLEAN" | "TINYINT" => Kind::I8,
        "TINYINT UNSIGNED" => Kind::U8,
        "SMALLINT" => Kind::I16,
        "SMALLINT UNSIGNED" | "YEAR" => Kind::U16,
        "INT" | "MEDIUMINT" => Kind::I32,
        "INT UNSIGNED" | "MEDIUMINT UNSIGNED" => Kind::U32,
        "BIGINT" => Kind::I64,
        "BIGINT UNSIGNED" | "BIT" => Kind::U64,
        "FLOAT" => Kind::F32,
        "DOUBLE" => Kind::F64,
        "DECIMAL" => Kind::Decimal,
        "DATE" => Kind::Date,
        "TIME" => Kind::Time,
        "DATETIME" => Kind::DateTime,
        "TIMESTAMP" => Kind::Timestamp,
        "JSON" => Kind::Json,
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" | "GEOMETRY" => {
            Kind::Bytes
        }
        _ => Kind::String,
    }
}

/// The connection options of `url` (a `mariadb://` URL connects with
/// MySQL's protocol). SQLx starts every session in UTC with `utf8mb4`,
/// over TLS when the server offers it (`ssl-mode=PREFERRED`).
pub(crate) fn connect_options(url: &str) -> Result<MySqlConnectOptions, DbError> {
    let options: MySqlConnectOptions =
        crate::options::driver_url(url).parse().map_err(|error| {
            DbError::from_sqlx(error, Some(Backend::MySql)).with_kind(crate::ErrorKind::Connect)
        })?;
    Ok(options.disable_statement_logging())
}

/// The pool settings of MySQL connections: each new connection runs the
/// session statements of `database.mysql` (SQL mode, isolation level)
/// after SQLx's own.
pub(crate) fn pool_options(options: &DbOptions) -> MySqlPoolOptions {
    let statements: Arc<[String]> = session_statements(&options.mysql).into();
    super::pool_options::<MySql>(options).after_connect(move |conn, _meta| {
        let statements = Arc::clone(&statements);
        Box::pin(async move {
            for statement in statements.iter() {
                // Built from validated options (see `session_statements`).
                sqlx::raw_sql(sqlx::AssertSqlSafe(statement.clone()))
                    .execute(&mut *conn)
                    .await?;
            }
            Ok(())
        })
    })
}

fn session_statements(options: &MySqlOptions) -> Vec<String> {
    crate::mysql::session_statements(options)
}
