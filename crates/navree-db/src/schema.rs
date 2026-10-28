//! navree-db's connections as navree-schema's [`SchemaConnection`]: a
//! [`Schema`](navree_schema::Schema) runs on a [`Pool`], a
//! [`Transaction`], a [`Uow`] or a [`Db`] (`Schema::new(&db)`).

use navree_kernel::BoxFuture;
use navree_schema::{Dialect, SchemaConnection, SchemaError, SchemaRow, SchemaValue};
use sea_orm::ConnectionTrait;

use crate::compat::SeaExecutor;
use crate::{Backend, Db, Executor, Kind, Pool, Row, Statement, Transaction, Uow, Value};

/// navree-schema's name for `backend`.
pub(crate) fn dialect(backend: Backend) -> Dialect {
    match backend {
        Backend::Postgres => Dialect::Postgres,
        Backend::MySql => Dialect::MySql,
        Backend::Sqlite => Dialect::Sqlite,
    }
}

/// A navree-db value as the schema builder reads it: integers, floats,
/// booleans, bytes and `NULL` as themselves, everything else as text
/// (exact decimals, dates, UUIDs; JSON as its text).
pub(crate) fn schema_value(value: Value) -> SchemaValue {
    match value {
        Value::Null(_) => SchemaValue::Null,
        Value::Bool(v) => SchemaValue::Bool(v),
        Value::I8(v) => SchemaValue::Int(i64::from(v)),
        Value::I16(v) => SchemaValue::Int(i64::from(v)),
        Value::I32(v) => SchemaValue::Int(i64::from(v)),
        Value::I64(v) => SchemaValue::Int(v),
        Value::U8(v) => SchemaValue::Int(i64::from(v)),
        Value::U16(v) => SchemaValue::Int(i64::from(v)),
        Value::U32(v) => SchemaValue::Int(i64::from(v)),
        Value::U64(v) => {
            i64::try_from(v).map_or_else(|_| SchemaValue::Text(v.to_string()), SchemaValue::Int)
        }
        Value::F32(v) => SchemaValue::Float(f64::from(v)),
        Value::F64(v) => SchemaValue::Float(v),
        Value::Decimal(v) => SchemaValue::Text(v.to_string()),
        Value::String(v) => SchemaValue::Text(v),
        Value::Bytes(v) => SchemaValue::Bytes(v),
        Value::Json(serde_json::Value::String(v)) => SchemaValue::Text(v),
        Value::Json(v) => SchemaValue::Text(v.to_string()),
        Value::Uuid(v) => SchemaValue::Text(v.to_string()),
        Value::Date(v) => SchemaValue::Text(v.to_string()),
        Value::Time(v) => SchemaValue::Text(v.to_string()),
        Value::DateTime(v) => SchemaValue::Text(v.to_string()),
        Value::Timestamp(v) => SchemaValue::Text(v.to_rfc3339()),
        Value::TimestampTz(v) => SchemaValue::Text(v.to_rfc3339()),
    }
}

/// A parameter of a catalog query, bound as navree-db binds its values.
pub(crate) fn bound_value(value: &SchemaValue) -> Value {
    match value {
        SchemaValue::Null => Value::Null(Kind::String),
        SchemaValue::Int(v) => Value::I64(*v),
        SchemaValue::Float(v) => Value::F64(*v),
        SchemaValue::Text(v) => Value::String(v.clone()),
        SchemaValue::Bytes(v) => Value::Bytes(v.clone()),
        SchemaValue::Bool(v) => Value::Bool(*v),
    }
}

/// `row` as a [`SchemaRow`].
pub(crate) fn schema_row(row: &Row, sql: &str) -> Result<SchemaRow, SchemaError> {
    let values = row
        .values()
        .map_err(|error| SchemaError::database(sql, error))?;
    Ok(SchemaRow::new(
        row.columns(),
        values.into_iter().map(schema_value).collect(),
    ))
}

/// Runs `sql` unprepared on `executor` (DDL; several statements allowed).
pub(crate) async fn execute_on(
    executor: &(impl Executor + ?Sized),
    sql: &str,
) -> Result<(), SchemaError> {
    executor
        .execute_script(sql)
        .await
        .map(drop)
        .map_err(|error| SchemaError::database(sql, error))
}

/// Runs a catalog query on `executor`, its placeholders in the backend's
/// own form.
pub(crate) async fn query_on(
    executor: &(impl Executor + ?Sized),
    sql: &str,
    params: &[SchemaValue],
) -> Result<Vec<SchemaRow>, SchemaError> {
    let statement = Statement::new(executor.backend(), sql, params.iter().map(bound_value));
    executor
        .fetch_all(&statement)
        .await
        .map_err(|error| SchemaError::database(sql, error))?
        .iter()
        .map(|row| schema_row(row, sql))
        .collect()
}

macro_rules! schema_connection {
    ($($ty:ty),*) => {
        $(
            /// A schema on it: `Schema::new(&executor)`. Statements run
            /// through [`Executor`], so they are timed and reported like
            /// every other statement.
            impl SchemaConnection for $ty {
                fn backend(&self) -> Dialect {
                    dialect(Executor::backend(self))
                }

                fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), SchemaError>> {
                    Box::pin(execute_on(self, sql))
                }

                fn query<'a>(
                    &'a self,
                    sql: &'a str,
                    params: &'a [SchemaValue],
                ) -> BoxFuture<'a, Result<Vec<SchemaRow>, SchemaError>> {
                    Box::pin(query_on(self, sql, params))
                }
            }
        )*
    };
}

schema_connection!(Pool, Transaction, Uow, Db);

/// A schema on a SeaORM connection or transaction, while code still holds
/// one: `Schema::new(&SeaExecutor::new(&*uow.tx().await?))`.
impl<C: ConnectionTrait + Sync + ?Sized> SchemaConnection for SeaExecutor<'_, C> {
    fn backend(&self) -> Dialect {
        dialect(Executor::backend(self))
    }

    fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), SchemaError>> {
        Box::pin(execute_on(self, sql))
    }

    fn query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [SchemaValue],
    ) -> BoxFuture<'a, Result<Vec<SchemaRow>, SchemaError>> {
        Box::pin(query_on(self, sql, params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_cross_into_the_schema_builder() {
        assert_eq!(schema_value(Value::I32(7)), SchemaValue::Int(7));
        assert_eq!(schema_value(Value::Null(Kind::I64)), SchemaValue::Null);
        assert_eq!(
            schema_value(Value::U64(u64::MAX)),
            SchemaValue::Text(u64::MAX.to_string())
        );
        assert_eq!(
            schema_value(Value::Json(serde_json::json!(["a", "b"]))),
            SchemaValue::Text(r#"["a","b"]"#.into())
        );
        assert_eq!(
            schema_value(Value::Json(serde_json::json!("x"))),
            SchemaValue::Text("x".into())
        );
        assert_eq!(
            schema_value(Value::Decimal(rust_decimal::Decimal::new(1250, 2))),
            SchemaValue::Text("12.50".into())
        );
        assert_eq!(
            bound_value(&SchemaValue::Text("t".into())),
            Value::from("t")
        );
        assert_eq!(dialect(Backend::MySql), Dialect::MySql);
    }
}
