//! The bridge to SeaORM while crates move off it: values, backends and
//! errors converted both ways, and [`SeaExecutor`], which runs navree-db
//! statements on a SeaORM connection or transaction (a unit of work's
//! transaction, until units of work run on navree-db's own; see
//! docs/superpowers/specs/2026-10-28-navree-data-layer-on-sqlx.md). The
//! whole module goes when SeaORM does.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use navree_kernel::BoxFuture;
use sea_orm::{ConnectionTrait, DbBackend, DbErr, RuntimeErr};

use crate::engine::driver::{Executed, KeyedText, RowStream};
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

/// `value` as SeaORM binds it on PostgreSQL: a vector is pgvector's (with
/// the `postgres` feature, its text otherwise); see [`to_sea_value_on`]
/// for the other backends.
pub fn to_sea_value(value: &Value) -> sea_orm::Value {
    to_sea_value_on(Backend::Postgres, value)
}

/// `value` as SeaORM binds it on `backend`: a vector is pgvector's on
/// PostgreSQL, little-endian `f32` bytes elsewhere; an enum value is its
/// text.
pub fn to_sea_value_on(backend: Backend, value: &Value) -> sea_orm::Value {
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
            Kind::Enum => S::String(None),
            #[cfg(feature = "postgres")]
            Kind::Vector if backend == Backend::Postgres => S::Vector(None),
            Kind::Vector if backend == Backend::Postgres => S::String(None),
            Kind::Vector => S::Bytes(None),
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
        Value::Enum { value, .. } => S::String(Some(value.clone())),
        #[cfg(feature = "postgres")]
        Value::Vector(v) if backend == Backend::Postgres => {
            S::Vector(Some(sea_orm::prelude::PgVector::from(v.clone())))
        }
        Value::Vector(v) if backend == Backend::Postgres => {
            let items: Vec<String> = v.iter().map(f32::to_string).collect();
            S::String(Some(format!("[{}]", items.join(","))))
        }
        Value::Vector(v) => S::Bytes(Some(crate::engine::driver::vector_bytes(v))),
    }
}

/// A SeaORM value as navree-db's (an enum value keeps its type name; a vector
/// is navree-db's with the `postgres` feature). Arrays are an error.
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
        S::Enum(sea_orm::sea_query::OptionEnum::Some(value)) => Value::Enum {
            type_name: value.type_name.to_string(),
            value: value.value.into_owned(),
        },
        S::Enum(sea_orm::sea_query::OptionEnum::None(_)) => Value::Null(Kind::Enum),
        #[cfg(feature = "postgres")]
        S::Vector(v) => v.map_or(Value::Null(Kind::Vector), |v| Value::Vector(v.to_vec())),
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

    /// `statement` as SeaORM runs it on this connection.
    ///
    /// SeaORM's driver caches prepared statements by their text too, so on
    /// PostgreSQL and MySQL the text names the kind of each value
    /// (`/* params: i64, text */`): SeaORM binds each kind as one type, so
    /// values of other kinds make another statement (see
    /// [`Keyed`](crate::engine::driver::Keyed)). A mock connection
    /// (`migrate --pretend`, scripted tests) prepares nothing and keeps the
    /// SQL as written.
    pub(crate) fn statement(&self, statement: &Statement) -> Result<sea_orm::Statement, DbError> {
        let backend = Executor::backend(self);
        let statement = statement.for_backend(backend)?;
        let sql = match backend {
            _ if self.conn.is_mock_connection() => statement.sql().to_owned(),
            Backend::Sqlite => statement.sql().to_owned(),
            Backend::Postgres | Backend::MySql => {
                let mut text = KeyedText::new(statement.sql(), statement.values().len());
                for value in statement.values() {
                    text.name(value.kind().name());
                }
                text.finish()
            }
        };
        Ok(sea_orm::Statement::from_sql_and_values(
            db_backend(backend),
            sql,
            statement
                .values()
                .iter()
                .map(|value| to_sea_value_on(backend, value)),
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

/// The rows of `statement` on SeaORM's transaction `tx`, read as the
/// database sends them (SeaORM's stream, driven by the returned stream
/// itself, in the task that polls it). The stream holds the transaction's
/// connection until it ends or is dropped.
pub(crate) fn stream_on_transaction(
    tx: Arc<sea_orm::DatabaseTransaction>,
    statement: sea_orm::Statement,
) -> RowStream {
    use futures_util::StreamExt as _;
    use sea_orm::StreamTrait as _;

    let slot: Arc<Mutex<Option<Result<Row, DbError>>>> = Arc::new(Mutex::new(None));
    let handed = Arc::clone(&slot);
    let producer = async move {
        let rows = match tx.stream_raw(statement).await {
            Ok(rows) => rows,
            Err(error) => {
                hand_over(&handed, Err(error.into())).await;
                return;
            }
        };
        futures_util::pin_mut!(rows);
        while let Some(row) = rows.next().await {
            let item = row.map(Row::from).map_err(DbError::from);
            let failed = item.is_err();
            hand_over(&handed, item).await;
            if failed {
                return;
            }
        }
    };
    Box::pin(Generated {
        producer: Some(Box::pin(producer)),
        slot,
    })
}

/// Puts `item` in `slot` and waits until the stream handed it out: the
/// first poll gives control back to the stream, which takes the item in
/// the same `poll_next`; the next poll is the reader asking for more.
fn hand_over(
    slot: &Mutex<Option<Result<Row, DbError>>>,
    item: Result<Row, DbError>,
) -> impl Future<Output = ()> + Send + '_ {
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(item);
    let mut handed = false;
    std::future::poll_fn(move |_| {
        if handed {
            Poll::Ready(())
        } else {
            handed = true;
            Poll::Pending
        }
    })
}

/// A stream of what its producer hands over.
struct Generated {
    producer: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    slot: Arc<Mutex<Option<Result<Row, DbError>>>>,
}

impl futures_util::Stream for Generated {
    type Item = Result<Row, DbError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let Some(producer) = this.producer.as_mut() else {
            return Poll::Ready(None);
        };
        let finished = producer.as_mut().poll(cx).is_ready();
        if finished {
            this.producer = None;
        }
        match this
            .slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            Some(item) => Poll::Ready(Some(item)),
            None if finished => Poll::Ready(None),
            // Waiting for the database; its I/O wakes this task.
            None => Poll::Pending,
        }
    }
}

/// A row of SeaORM's scripted connection, by its values: each cell read as
/// the one SeaORM value kind it holds. (A scripted row reports any cell it
/// cannot read as the type asked for as `NULL`, so each kind is tried
/// without `Option`; a cell no kind reads is `NULL`.)
pub(crate) fn scripted_row(row: &sea_orm::QueryResult) -> Row {
    fn cell<T>(row: &sea_orm::QueryResult, index: usize) -> Option<Value>
    where
        T: sea_orm::TryGetable + Into<Value>,
    {
        row.try_get_by_index::<T>(index).ok().map(Into::into)
    }

    let columns = row.column_names();
    let values: Vec<Value> = (0..columns.len())
        .map(|i| {
            cell::<String>(row, i)
                .or_else(|| cell::<i64>(row, i))
                .or_else(|| cell::<i32>(row, i))
                .or_else(|| cell::<i16>(row, i))
                .or_else(|| cell::<i8>(row, i))
                .or_else(|| cell::<u64>(row, i))
                .or_else(|| cell::<u32>(row, i))
                .or_else(|| cell::<bool>(row, i))
                .or_else(|| cell::<f64>(row, i))
                .or_else(|| cell::<f32>(row, i))
                .or_else(|| cell::<rust_decimal::Decimal>(row, i))
                .or_else(|| cell::<chrono::DateTime<chrono::Utc>>(row, i))
                .or_else(|| cell::<chrono::DateTime<chrono::FixedOffset>>(row, i))
                .or_else(|| cell::<chrono::NaiveDateTime>(row, i))
                .or_else(|| cell::<chrono::NaiveDate>(row, i))
                .or_else(|| cell::<chrono::NaiveTime>(row, i))
                .or_else(|| cell::<uuid::Uuid>(row, i))
                .or_else(|| cell::<serde_json::Value>(row, i))
                .or_else(|| cell::<Vec<u8>>(row, i))
                .unwrap_or(Value::Null(Kind::String))
        })
        .collect();
    Row::from_values(columns.into_iter().zip(values))
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
