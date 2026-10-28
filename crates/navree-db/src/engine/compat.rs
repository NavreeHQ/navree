//! The bridge to SeaORM while crates move off it: values, backends and
//! errors converted both ways, and [`SeaExecutor`], which runs navree-db
//! statements on a SeaORM connection or transaction (a unit of work's
//! transaction, until units of work run on navree-db's own; see
//! docs/design). The
//! whole module goes when SeaORM does.

use std::sync::Arc;

use navree_kernel::BoxFuture;
use sea_orm::{ConnectionTrait, DbBackend, DbErr, RuntimeErr};

use crate::engine::driver::{Executed, RowStream};
use crate::engine::{Backend, DbError, ErrorKind, Executor, Kind, Row, Statement, Value};

/// SeaORM's name for `backend`.
pub fn db_backend(backend: Backend) -> DbBackend {
    match backend {
        Backend::Postgres => DbBackend::Postgres,
        Backend::MySql => DbBackend::MySql,
        Backend::Sqlite => DbBackend::Sqlite,
    }
}

/// The backend SeaORM's `backend` names.
pub fn backend_of(backend: DbBackend) -> Backend {
    match backend {
        DbBackend::Postgres => Backend::Postgres,
        DbBackend::MySql => Backend::MySql,
        _ => Backend::Sqlite,
    }
}

/// `value` as SeaORM binds it.
pub fn to_sea_value(value: &Value) -> sea_orm::Value {
    use sea_orm::Value as S;
    match value {
        Value::Null(kind) => match kind {
            Kind::Bool => S::Bool(None),
            Kind::I8 => S::TinyInt(None),
            Kind::I16 => S::SmallInt(None),
            Kind::I32 => S::Int(None),
            Kind::I64 => S::BigInt(None),
            Kind::U8 => S::TinyUnsigned(None),
            Kind::U16 => S::SmallUnsigned(None),
            Kind::U32 => S::Unsigned(None),
            Kind::U64 => S::BigUnsigned(None),
            Kind::F32 => S::Float(None),
            Kind::F64 => S::Double(None),
            Kind::Decimal => S::Decimal(None),
            Kind::String => S::String(None),
            Kind::Bytes => S::Bytes(None),
            Kind::Json => S::Json(None),
            Kind::Uuid => S::Uuid(None),
            Kind::Date => S::ChronoDate(None),
            Kind::Time => S::ChronoTime(None),
            Kind::DateTime => S::ChronoDateTime(None),
            Kind::Timestamp => S::ChronoDateTimeUtc(None),
            Kind::TimestampTz => S::ChronoDateTimeWithTimeZone(None),
        },
        Value::Bool(v) => S::Bool(Some(*v)),
        Value::I8(v) => S::TinyInt(Some(*v)),
        Value::I16(v) => S::SmallInt(Some(*v)),
        Value::I32(v) => S::Int(Some(*v)),
        Value::I64(v) => S::BigInt(Some(*v)),
        Value::U8(v) => S::TinyUnsigned(Some(*v)),
        Value::U16(v) => S::SmallUnsigned(Some(*v)),
        Value::U32(v) => S::Unsigned(Some(*v)),
        Value::U64(v) => S::BigUnsigned(Some(*v)),
        Value::F32(v) => S::Float(Some(*v)),
        Value::F64(v) => S::Double(Some(*v)),
        Value::Decimal(v) => S::Decimal(Some(*v)),
        Value::String(v) => S::String(Some(v.clone())),
        Value::Bytes(v) => S::Bytes(Some(v.clone())),
        Value::Json(v) => S::Json(Some(Box::new(v.clone()))),
        Value::Uuid(v) => S::Uuid(Some(*v)),
        Value::Date(v) => S::ChronoDate(Some(*v)),
        Value::Time(v) => S::ChronoTime(Some(*v)),
        Value::DateTime(v) => S::ChronoDateTime(Some(*v)),
        Value::Timestamp(v) => S::ChronoDateTimeUtc(Some(*v)),
        Value::TimestampTz(v) => S::ChronoDateTimeWithTimeZone(Some(*v)),
    }
}

/// A SeaORM value as navree-db's. Kinds navree-db does not have (arrays,
/// vectors, enums of other crates) are an error.
pub fn from_sea_value(value: sea_orm::Value) -> Result<Value, DbError> {
    use sea_orm::Value as S;
    fn or_null<T: Into<Value>>(value: Option<T>, kind: Kind) -> Value {
        value.map_or(Value::Null(kind), Into::into)
    }
    #[allow(unreachable_patterns)]
    Ok(match value {
        S::Bool(v) => or_null(v, Kind::Bool),
        S::TinyInt(v) => or_null(v, Kind::I8),
        S::SmallInt(v) => or_null(v, Kind::I16),
        S::Int(v) => or_null(v, Kind::I32),
        S::BigInt(v) => or_null(v, Kind::I64),
        S::TinyUnsigned(v) => or_null(v, Kind::U8),
        S::SmallUnsigned(v) => or_null(v, Kind::U16),
        S::Unsigned(v) => or_null(v, Kind::U32),
        S::BigUnsigned(v) => or_null(v, Kind::U64),
        S::Float(v) => or_null(v, Kind::F32),
        S::Double(v) => or_null(v, Kind::F64),
        S::String(v) => or_null(v, Kind::String),
        S::Char(v) => v.map_or(Value::Null(Kind::String), |c| Value::String(c.to_string())),
        S::Bytes(v) => or_null(v, Kind::Bytes),
        S::Json(v) => v.map_or(Value::Null(Kind::Json), |json| Value::Json(*json)),
        S::ChronoDate(v) => or_null(v, Kind::Date),
        S::ChronoTime(v) => or_null(v, Kind::Time),
        S::ChronoDateTime(v) => or_null(v, Kind::DateTime),
        S::ChronoDateTimeUtc(v) => or_null(v, Kind::Timestamp),
        S::ChronoDateTimeLocal(v) => v.map_or(Value::Null(Kind::Timestamp), |at| {
            Value::Timestamp(at.with_timezone(&chrono::Utc))
        }),
        S::ChronoDateTimeWithTimeZone(v) => or_null(v, Kind::TimestampTz),
        S::Uuid(v) => or_null(v, Kind::Uuid),
        S::Decimal(v) => or_null(v, Kind::Decimal),
        S::Enum(sea_orm::sea_query::OptionEnum::Some(value)) => {
            Value::String(value.value.into_owned())
        }
        S::Enum(sea_orm::sea_query::OptionEnum::None(_)) => Value::Null(Kind::String),
        other => {
            return Err(DbError::unsupported(format!(
                "navree-db has no value for {other:?}"
            )));
        }
    })
}

impl From<DbError> for DbErr {
    /// The SeaORM error SeaORM itself would return for the same failure, so
    /// `DbErr::sql_err` and navree-db's [`map_db_error`](crate::map_db_error)
    /// classify it alike.
    fn from(error: DbError) -> DbErr {
        match (error.kind(), error.sqlx_error()) {
            (ErrorKind::NotFound, _) => DbErr::RecordNotFound(error.message().to_owned()),
            (ErrorKind::Connect | ErrorKind::PoolTimeout | ErrorKind::PoolClosed, Some(source)) => {
                DbErr::Conn(RuntimeErr::SqlxError(Arc::clone(source)))
            }
            (ErrorKind::Connect, None) => DbErr::Custom(error.message().to_owned()),
            (_, Some(source)) => DbErr::Query(RuntimeErr::SqlxError(Arc::clone(source))),
            (_, None) => DbErr::Custom(error.to_string()),
        }
    }
}

impl From<DbErr> for DbError {
    fn from(error: DbErr) -> DbError {
        match error {
            DbErr::Conn(RuntimeErr::SqlxError(source))
            | DbErr::Exec(RuntimeErr::SqlxError(source))
            | DbErr::Query(RuntimeErr::SqlxError(source)) => DbError::from_shared(source, None),
            DbErr::RecordNotFound(message) => DbError::not_found(message),
            DbErr::RecordNotUpdated => DbError::not_found("no record was updated"),
            other => DbError::new(ErrorKind::Other, other.to_string()),
        }
    }
}

/// navree-db statements on a SeaORM connection or transaction: values
/// converted to SeaORM's, rows read back through the driver rows SeaORM
/// holds. Streams read every row first.
pub struct SeaExecutor<'a, C: ConnectionTrait + ?Sized> {
    conn: &'a C,
}

impl<'a, C: ConnectionTrait + Sync + ?Sized> SeaExecutor<'a, C> {
    /// Statements on `conn`.
    pub fn new(conn: &'a C) -> Self {
        SeaExecutor { conn }
    }

    fn statement(&self, statement: &Statement) -> Result<sea_orm::Statement, DbError> {
        let backend = Executor::backend(self);
        let statement = statement.for_backend(backend)?;
        Ok(sea_orm::Statement::from_sql_and_values(
            db_backend(backend),
            statement.sql(),
            statement.values().iter().map(to_sea_value),
        ))
    }
}

impl<C: ConnectionTrait + Sync + ?Sized> Executor for SeaExecutor<'_, C> {
    fn backend(&self) -> Backend {
        backend_of(self.conn.get_database_backend())
    }

    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let statement = self.statement(statement)?;
            let backend = Executor::backend(self);
            let done = self.conn.execute_raw(statement).await?;
            Ok(Executed {
                rows_affected: done.rows_affected(),
                last_insert_id: (backend != Backend::Postgres).then(|| done.last_insert_id()),
            })
        })
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        Box::pin(async move {
            let statement = self.statement(statement)?;
            let rows = self.conn.query_all_raw(statement).await?;
            Ok(rows.into_iter().map(Row::from).collect())
        })
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        Box::pin(async move {
            let statement = self.statement(statement)?;
            Ok(self.conn.query_one_raw(statement).await?.map(Row::from))
        })
    }

    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
        Box::pin(async move {
            let rows = self.fetch_all(statement).await?;
            let stream: RowStream = Box::pin(futures_util::stream::iter(rows.into_iter().map(Ok)));
            Ok(stream)
        })
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let done = self.conn.execute_unprepared(sql).await?;
            Ok(Executed {
                rows_affected: done.rows_affected(),
                last_insert_id: None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_cross_both_ways() {
        let values = [
            Value::Null(Kind::I64),
            Value::Bool(true),
            Value::I32(7),
            Value::U64(u64::MAX),
            Value::String("a".into()),
            Value::Json(serde_json::json!({"a": [1]})),
            Value::Uuid(uuid::Uuid::nil()),
            Value::Decimal(rust_decimal::Decimal::new(1250, 2)),
            Value::Null(Kind::Timestamp),
        ];
        for value in values {
            assert_eq!(from_sea_value(to_sea_value(&value)).unwrap(), value);
        }
        assert_eq!(
            from_sea_value(sea_orm::Value::Char(Some('x'))).unwrap(),
            Value::String("x".into())
        );
    }

    #[test]
    fn errors_keep_their_classification() {
        let not_found: DbErr = DbError::not_found("deals 7").into();
        assert!(matches!(not_found, DbErr::RecordNotFound(ref m) if m == "deals 7"));
        let back: DbError = DbErr::RecordNotFound("x".into()).into();
        assert!(back.is_not_found());
        let custom: DbErr = DbError::invalid("bad").into();
        assert_eq!(custom.to_string(), "Custom Error: invalid statement: bad");
        assert_eq!(backend_of(db_backend(Backend::MySql)), Backend::MySql);
    }
}
