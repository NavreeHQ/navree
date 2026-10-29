//! PostgreSQL (and TimescaleDB): binding values, reading columns, and the
//! options every pooled connection starts with.

use sqlx::postgres::{PgArguments, PgConnectOptions, PgPoolOptions, PgRow, PgSslMode, PgValueRef};
use sqlx::{Arguments, ConnectOptions, Decode, Postgres, Row as _, TypeInfo, ValueRef};

use crate::engine::{Backend, DbError, Kind, Value};
use crate::{DbOptions, PostgresOptions};

/// The values of a statement as PostgreSQL binds them: NULLs typed by
/// their kind, unsigned integers in the next wider signed type (PostgreSQL
/// has none), `i8` as `SMALLINT`.
pub(crate) fn arguments(values: &[Value]) -> Result<PgArguments, DbError> {
    let mut args = PgArguments::default();
    for value in values {
        bind(&mut args, value).map_err(|error| {
            DbError::invalid(format!(
                "cannot bind {} on PostgreSQL: {error}",
                value.kind()
            ))
        })?;
    }
    Ok(args)
}

fn bind(args: &mut PgArguments, value: &Value) -> Result<(), sqlx::error::BoxDynError> {
    match value {
        Value::Null(kind) => bind_null(args, *kind),
        Value::Bool(v) => args.add(*v),
        Value::I8(v) => args.add(i16::from(*v)),
        Value::I16(v) => args.add(*v),
        Value::I32(v) => args.add(*v),
        Value::I64(v) => args.add(*v),
        Value::U8(v) => args.add(i16::from(*v)),
        Value::U16(v) => args.add(i32::from(*v)),
        Value::U32(v) => args.add(i64::from(*v)),
        Value::U64(v) => match i64::try_from(*v) {
            Ok(v) => args.add(v),
            Err(_) => args.add(rust_decimal::Decimal::from(*v)),
        },
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
        Value::Timestamp(v) => args.add(*v),
        Value::TimestampTz(v) => args.add(*v),
        // The text; the statement casts it to its type.
        Value::Enum { value, .. } => args.add(value.as_str()),
        Value::Vector(v) => args.add(PgVector(Some(v))),
    }
}

/// A pgvector `vector` parameter (binary form: dimensions, an unused word,
/// then big-endian `f32`s); `None` binds a NULL `vector`.
struct PgVector<'a>(Option<&'a [f32]>);

impl sqlx::Type<Postgres> for PgVector<'_> {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("vector")
    }
}

impl sqlx::Encode<'_, Postgres> for PgVector<'_> {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        let Some(items) = self.0 else {
            return Ok(sqlx::encode::IsNull::Yes);
        };
        buf.extend_from_slice(&vector_binary(items)?);
        Ok(sqlx::encode::IsNull::No)
    }
}

/// pgvector's binary form of a `vector`: the dimensions (`u16`), an unused
/// `u16`, then each component as a big-endian `f32`.
fn vector_binary(items: &[f32]) -> Result<Vec<u8>, sqlx::error::BoxDynError> {
    let mut bytes = Vec::with_capacity(4 + items.len() * 4);
    bytes.extend_from_slice(&u16::try_from(items.len())?.to_be_bytes());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    for item in items {
        bytes.extend_from_slice(&item.to_be_bytes());
    }
    Ok(bytes)
}

/// The components of a `vector` in pgvector's binary form.
fn parse_vector(bytes: &[u8]) -> Result<Vec<f32>, sqlx::error::BoxDynError> {
    let dims = usize::from(u16::from_be_bytes(
        bytes.get(0..2).ok_or("a vector too short")?.try_into()?,
    ));
    let mut items = Vec::with_capacity(dims);
    for i in 0..dims {
        let at = 4 + i * 4;
        let float = f32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .ok_or("a vector too short")?
                .try_into()?,
        );
        items.push(float);
    }
    Ok(items)
}

fn bind_null(args: &mut PgArguments, kind: Kind) -> Result<(), sqlx::error::BoxDynError> {
    match kind {
        Kind::Bool => args.add(None::<bool>),
        Kind::I8 | Kind::I16 | Kind::U8 => args.add(None::<i16>),
        Kind::I32 | Kind::U16 => args.add(None::<i32>),
        Kind::I64 | Kind::U32 | Kind::U64 => args.add(None::<i64>),
        Kind::F32 => args.add(None::<f32>),
        Kind::F64 => args.add(None::<f64>),
        Kind::Decimal => args.add(None::<rust_decimal::Decimal>),
        Kind::String => args.add(None::<String>),
        Kind::Bytes => args.add(None::<Vec<u8>>),
        Kind::Json => args.add(None::<serde_json::Value>),
        Kind::Uuid => args.add(None::<uuid::Uuid>),
        Kind::Date => args.add(None::<chrono::NaiveDate>),
        Kind::Time => args.add(None::<chrono::NaiveTime>),
        Kind::DateTime => args.add(None::<chrono::NaiveDateTime>),
        Kind::Timestamp => args.add(None::<chrono::DateTime<chrono::Utc>>),
        Kind::TimestampTz => args.add(None::<chrono::DateTime<chrono::FixedOffset>>),
        Kind::Enum => args.add(None::<String>),
        Kind::Vector => args.add(PgVector(None)),
    }
}

/// Column `index` of `row` as a [`Value`], read by the column's type.
pub(crate) fn value(row: &PgRow, index: usize) -> Result<Value, DbError> {
    let raw = row
        .try_get_raw(index)
        .map_err(|error| DbError::from_sqlx(error, Some(Backend::Postgres)))?;
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

fn decode(raw: PgValueRef<'_>, type_name: &str) -> Result<Value, sqlx::error::BoxDynError> {
    fn d<'r, T: Decode<'r, Postgres>>(raw: PgValueRef<'r>) -> Result<T, sqlx::error::BoxDynError> {
        T::decode(raw)
    }
    fn json_array<'r, T>(raw: PgValueRef<'r>) -> Result<Value, sqlx::error::BoxDynError>
    where
        Vec<T>: Decode<'r, Postgres>,
        T: Into<Value>,
    {
        let items: Vec<T> = Vec::<T>::decode(raw)?;
        Ok(Value::Json(serde_json::Value::Array(
            items
                .into_iter()
                .map(|item| item.into().to_json())
                .collect(),
        )))
    }
    Ok(match type_name {
        "BOOL" => Value::Bool(d(raw)?),
        "\"CHAR\"" => Value::I8(d(raw)?),
        "INT2" => Value::I16(d(raw)?),
        "INT4" => Value::I32(d(raw)?),
        "INT8" => Value::I64(d(raw)?),
        "OID" => Value::U32(d::<sqlx::postgres::types::Oid>(raw)?.0),
        "FLOAT4" => Value::F32(d(raw)?),
        "FLOAT8" => Value::F64(d(raw)?),
        "NUMERIC" => Value::Decimal(d(raw)?),
        "TEXT" | "VARCHAR" | "CHAR" | "NAME" | "UNKNOWN" | "CITEXT" | "citext" => {
            Value::String(d(raw)?)
        }
        "BYTEA" => Value::Bytes(d(raw)?),
        "JSON" | "JSONB" => Value::Json(d(raw)?),
        "UUID" => Value::Uuid(d(raw)?),
        "DATE" => Value::Date(d(raw)?),
        "TIME" => Value::Time(d(raw)?),
        "TIMESTAMP" => Value::DateTime(d(raw)?),
        "TIMESTAMPTZ" => Value::Timestamp(d(raw)?),
        "VOID" => Value::Null(Kind::String),
        "BOOL[]" => json_array::<bool>(raw)?,
        "INT2[]" => json_array::<i16>(raw)?,
        "INT4[]" => json_array::<i32>(raw)?,
        "INT8[]" => json_array::<i64>(raw)?,
        "FLOAT4[]" => json_array::<f32>(raw)?,
        "FLOAT8[]" => json_array::<f64>(raw)?,
        "NUMERIC[]" => json_array::<rust_decimal::Decimal>(raw)?,
        "TEXT[]" | "VARCHAR[]" | "NAME[]" | "CHAR[]" => json_array::<String>(raw)?,
        "UUID[]" => json_array::<uuid::Uuid>(raw)?,
        "JSON[]" | "JSONB[]" => json_array::<serde_json::Value>(raw)?,
        "TIMESTAMPTZ[]" => json_array::<chrono::DateTime<chrono::Utc>>(raw)?,
        "vector" | "VECTOR" => vector(raw)?,
        // Enums, domains over text and other types whose binary form is
        // their text.
        _ => Value::String(
            d(raw).map_err(|_| format!("a column of type {type_name} cannot be read"))?,
        ),
    })
}

/// A pgvector `vector` (binary form: dimensions, an unused word, then
/// big-endian `f32`s).
fn vector(raw: PgValueRef<'_>) -> Result<Value, sqlx::error::BoxDynError> {
    Ok(Value::Vector(parse_vector(raw.as_bytes()?)?))
}

/// The kind of a column of PostgreSQL type `type_name` (for its NULLs).
fn kind_of(type_name: &str) -> Kind {
    match type_name {
        "BOOL" => Kind::Bool,
        "\"CHAR\"" => Kind::I8,
        "INT2" => Kind::I16,
        "INT4" => Kind::I32,
        "INT8" => Kind::I64,
        "OID" => Kind::U32,
        "FLOAT4" => Kind::F32,
        "FLOAT8" => Kind::F64,
        "NUMERIC" => Kind::Decimal,
        "BYTEA" => Kind::Bytes,
        "JSON" | "JSONB" => Kind::Json,
        "UUID" => Kind::Uuid,
        "DATE" => Kind::Date,
        "TIME" => Kind::Time,
        "TIMESTAMP" => Kind::DateTime,
        "TIMESTAMPTZ" => Kind::Timestamp,
        "vector" | "VECTOR" => Kind::Vector,
        name if name.ends_with("[]") => Kind::Json,
        _ => Kind::String,
    }
}

/// The connection options of `url` with `database.postgres`: session
/// settings, the application name and TLS.
pub(crate) fn connect_options(
    url: &str,
    postgres: &PostgresOptions,
) -> Result<PgConnectOptions, DbError> {
    let mut options: PgConnectOptions = url.parse().map_err(|error| {
        DbError::from_sqlx(error, Some(Backend::Postgres)).with_kind(crate::ErrorKind::Connect)
    })?;
    let settings = postgres.settings();
    if !settings.is_empty() {
        options = options.options(settings.iter().map(|(name, value)| (*name, value.as_str())));
    }
    if let Some(application) = &postgres.application_name {
        options = options.application_name(application);
    }
    if let Some(mode) = &postgres.ssl_mode {
        let mode = mode.parse::<PgSslMode>().map_err(|error| {
            DbError::new(
                crate::ErrorKind::Connect,
                format!("postgres.ssl_mode: {error}"),
            )
        })?;
        options = options.ssl_mode(mode);
    }
    Ok(options.disable_statement_logging())
}

/// The pool settings of PostgreSQL connections: the time zone is set once
/// connected (the driver starts every session with `TimeZone=UTC`, which
/// wins over the connection options).
pub(crate) fn pool_options(options: &DbOptions) -> PgPoolOptions {
    let pool = super::pool_options::<Postgres>(options);
    match &options.postgres.timezone {
        Some(timezone) => {
            let sql = format!("SET TIME ZONE '{timezone}'");
            pool.after_connect(move |conn, _| {
                let sql = sql.clone();
                Box::pin(async move {
                    sqlx::Executor::execute(conn, sqlx::AssertSqlSafe(sql))
                        .await
                        .map(drop)
                })
            })
        }
        None => pool,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_cross_in_pgvector_s_binary_form() {
        let items = [0.5_f32, -1.0, 2.25];
        let bytes = vector_binary(&items).unwrap();
        assert_eq!(&bytes[..4], &[0, 3, 0, 0]);
        assert_eq!(&bytes[4..8], &0.5_f32.to_be_bytes());
        assert_eq!(parse_vector(&bytes).unwrap(), items);
        assert_eq!(
            parse_vector(&vector_binary(&[]).unwrap()).unwrap(),
            [] as [f32; 0]
        );
        assert!(parse_vector(&bytes[..6]).is_err());
        assert!(vector_binary(&vec![0.0; 70_000]).is_err());
    }
}
