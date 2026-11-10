//! SQLite: binding values, reading columns, and the pragmas every pooled
//! connection starts with.

use std::time::Duration;

use sqlx::sqlite::{
    SqliteArguments, SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow,
    SqliteSynchronous, SqliteValueRef,
};
use sqlx::{Arguments, ConnectOptions, Decode, Row as _, Sqlite, TypeInfo, ValueRef};

use crate::engine::{Backend, DbError, Kind, Value};
use crate::{DbOptions, SqliteOptions};

/// The text SQLite prepares for `sql` (the SQL as written) and its values
/// as SQLite binds them: decimals as text (SQLite has no exact decimal
/// type), UUIDs as 16 bytes, date-times as text.
///
/// SQLite types values, not parameters or columns: a prepared statement
/// binds each run's values as they are and reads each result cell by
/// what it holds, so one prepared statement serves values of any type
/// (unlike PostgreSQL and MySQL, see [`Keyed`](super::Keyed)).
pub(crate) fn statement(sql: &str, values: &[Value]) -> Result<(String, SqliteArguments), DbError> {
    let mut args = SqliteArguments::default();
    for value in values {
        bind(&mut args, value).map_err(|error| {
            DbError::invalid(format!("cannot bind {} on SQLite: {error}", value.kind()))
        })?;
    }
    Ok((sql.to_owned(), args))
}

fn bind(args: &mut SqliteArguments, value: &Value) -> Result<(), sqlx::error::BoxDynError> {
    match value {
        Value::Null(_) => args.add(None::<i64>),
        Value::Bool(v) => args.add(*v),
        Value::I8(v) => args.add(*v),
        Value::I16(v) => args.add(*v),
        Value::I32(v) => args.add(*v),
        Value::I64(v) => args.add(*v),
        Value::U8(v) => args.add(*v),
        Value::U16(v) => args.add(*v),
        Value::U32(v) => args.add(*v),
        Value::U64(v) => match i64::try_from(*v) {
            Ok(v) => args.add(v),
            Err(_) => Err(format!("{v} does not fit SQLite's 64-bit integers").into()),
        },
        Value::F32(v) => args.add(*v),
        Value::F64(v) => args.add(*v),
        Value::Decimal(v) => args.add(v.to_string()),
        Value::String(v) => args.add(v.clone()),
        Value::Bytes(v) => args.add(v.clone()),
        Value::Json(v) => args.add(v.clone()),
        Value::Uuid(v) => args.add(*v),
        Value::Date(v) => args.add(*v),
        Value::Time(v) => args.add(*v),
        Value::DateTime(v) => args.add(*v),
        Value::Timestamp(v) => args.add(*v),
        Value::TimestampTz(v) => args.add(*v),
        Value::Enum { value, .. } => args.add(value.clone()),
        Value::Vector(v) => args.add(super::vector_bytes(v)),
    }
}

/// Column `index` of `row` as a [`Value`], read by what the cell holds
/// (SQLite types values, not columns): integers, reals, text, bytes.
/// [`FromValue`](crate::FromValue) reads the typed values stored as text
/// (date-times, decimals, JSON) and as bytes (UUIDs).
pub(crate) fn value(row: &SqliteRow, index: usize) -> Result<Value, DbError> {
    let raw = row
        .try_get_raw(index)
        .map_err(|error| DbError::from_sqlx(error, Some(Backend::Sqlite)))?;
    if raw.is_null() {
        let declared = row
            .columns()
            .get(index)
            .map(|column| sqlx::Column::type_info(column).name().to_ascii_uppercase())
            .unwrap_or_default();
        return Ok(Value::Null(kind_of(&declared)));
    }
    let type_name = raw.type_info().name().to_owned();
    decode(raw, &type_name).map_err(|error| {
        let column = row.columns().get(index).map_or("?", sqlx::Column::name);
        DbError::new(
            crate::ErrorKind::Decode,
            format!("cannot read column `{column}` ({type_name}): {error}"),
        )
    })
}

fn decode(raw: SqliteValueRef<'_>, type_name: &str) -> Result<Value, sqlx::error::BoxDynError> {
    fn d<'r, T: Decode<'r, Sqlite>>(
        raw: SqliteValueRef<'r>,
    ) -> Result<T, sqlx::error::BoxDynError> {
        T::decode(raw)
    }
    Ok(match type_name {
        "INTEGER" | "BOOLEAN" => Value::I64(d(raw)?),
        "REAL" | "NUMERIC" => Value::F64(d(raw)?),
        "BLOB" => Value::Bytes(d(raw)?),
        _ => Value::String(d(raw)?),
    })
}

/// The kind of a column declared as `declared` (its affinity), for its
/// NULLs.
fn kind_of(declared: &str) -> Kind {
    if declared.contains("INT") {
        Kind::I64
    } else if declared.contains("BLOB") || declared.contains("BINARY") {
        Kind::Bytes
    } else if declared.contains("REAL") || declared.contains("FLOA") || declared.contains("DOUB") {
        Kind::F64
    } else if declared.contains("BOOL") {
        Kind::Bool
    } else {
        Kind::String
    }
}

/// The connection options of `url` with `database.sqlite`: journal mode,
/// lock wait, foreign keys, `synchronous` and further pragmas.
pub(crate) fn connect_options(
    url: &str,
    sqlite: &SqliteOptions,
) -> Result<SqliteConnectOptions, DbError> {
    let mut options: SqliteConnectOptions = url.parse().map_err(|error| {
        DbError::from_sqlx(error, Some(Backend::Sqlite)).with_kind(crate::ErrorKind::Connect)
    })?;
    let journal = sqlite
        .journal_mode
        .parse::<SqliteJournalMode>()
        .unwrap_or(SqliteJournalMode::Wal);
    options = options
        .journal_mode(journal)
        .busy_timeout(Duration::from_millis(sqlite.busy_timeout_ms))
        .foreign_keys(sqlite.foreign_keys);
    if let Some(synchronous) = sqlite
        .synchronous
        .as_deref()
        .and_then(|mode| mode.parse::<SqliteSynchronous>().ok())
    {
        options = options.synchronous(synchronous);
    }
    for (name, value) in &sqlite.pragmas {
        options = options.pragma(name.clone(), value.clone());
    }
    Ok(options.disable_statement_logging())
}

/// The pool settings of SQLite connections.
pub(crate) fn pool_options(options: &DbOptions) -> SqlitePoolOptions {
    super::pool_options::<Sqlite>(options)
}

/// Creates the directory of a SQLite database file, so
/// `sqlite://storage/app.db?mode=rwc` works on a fresh checkout.
pub(crate) async fn create_parent_dir(url: &str) -> Result<(), DbError> {
    let Ok(options) = url.parse::<SqliteConnectOptions>() else {
        return Ok(());
    };
    let file = options.get_filename();
    if file.as_os_str().is_empty() || file.as_os_str() == ":memory:" || url.contains("mode=memory")
    {
        return Ok(());
    }
    match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                DbError::new(
                    crate::ErrorKind::Connect,
                    format!(
                        "cannot create the database directory {}: {error}",
                        parent.display()
                    ),
                )
            })
        }
        _ => Ok(()),
    }
}
