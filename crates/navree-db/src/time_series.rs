//! Time-series tables: deleting the rows older than a cutoff, by whole
//! chunks on TimescaleDB ([`prune_time_series`], [`TimePoint`]).

use chrono::{DateTime, Utc};
use navree_schema::{Schema, SchemaError, TimeDimension};
use sea_orm::sea_query::{
    Alias, Asterisk, DeleteStatement, Expr, ExprTrait, Query, SelectStatement, TableRef,
};
use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement, Value};

use crate::schema::OnExecutor;
use crate::{Backend, DbError, Executor, ExecutorExt};

/// A point on a time-series table's time axis: where
/// [`prune_time_series`] cuts.
///
/// Its type must match the time column: a moment for `TIMESTAMP WITH TIME
/// ZONE` columns (every Navree table that records when something
/// happened), an integer for integer time columns, in the column's unit
/// (Unix milliseconds in the queue's and Fleet's tables).
///
/// ```
/// use chrono::{TimeZone, Utc};
/// use navree_db::TimePoint;
///
/// let moment = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
/// assert_eq!(TimePoint::from(moment), TimePoint::At(moment));
/// assert_eq!(TimePoint::from(1_790_000_000_000_i64), TimePoint::Integer(1_790_000_000_000));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimePoint {
    /// A moment in time.
    At(DateTime<Utc>),
    /// A value of an integer time column.
    Integer(i64),
}

impl From<DateTime<Utc>> for TimePoint {
    fn from(moment: DateTime<Utc>) -> TimePoint {
        TimePoint::At(moment)
    }
}

impl From<i64> for TimePoint {
    fn from(value: i64) -> TimePoint {
        TimePoint::Integer(value)
    }
}

impl std::fmt::Display for TimePoint {
    /// RFC 3339 for a moment, the number for an integer.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimePoint::At(moment) => f.write_str(&moment.to_rfc3339()),
            TimePoint::Integer(value) => write!(f, "{value}"),
        }
    }
}

impl TimePoint {
    fn value(self) -> Value {
        match self {
            TimePoint::At(moment) => moment.into(),
            TimePoint::Integer(value) => value.into(),
        }
    }

    /// The cutoff as a bound value of navree-db's.
    fn bound(self) -> crate::Value {
        match self {
            TimePoint::At(moment) => moment.into(),
            TimePoint::Integer(value) => value.into(),
        }
    }

    /// The PostgreSQL type the bound value arrives as.
    fn postgres_type(self) -> &'static str {
        match self {
            TimePoint::At(_) => "timestamp with time zone",
            TimePoint::Integer(_) => "bigint",
        }
    }
}

/// Deletes the rows of `table` whose `time_column` is before `before`, and
/// returns how many there were.
///
/// On a TimescaleDB hypertable partitioned by `time_column`, it counts
/// those rows, drops every chunk that lies entirely before the cutoff
/// (`drop_chunks`: whole files go, with no row-by-row delete, no table
/// bloat to vacuum, and no empty chunks left behind), then deletes the rows
/// left in the chunk that straddles the cutoff. Anywhere else (a regular
/// table, PostgreSQL without TimescaleDB, MySQL, MariaDB, SQLite) it is
/// one `DELETE`.
///
/// `table` may be qualified (`analytics.events`). Run it inside a
/// transaction to have the count, the dropped chunks and the delete apply
/// together.
///
/// ```no_run
/// # async fn prune(db: &navree_db::Db) -> Result<(), sea_orm::DbErr> {
/// use chrono::{TimeDelta, Utc};
/// use navree_db::prune_time_series;
///
/// let deleted = prune_time_series(
///     db.conn(),
///     "events",
///     "occurred_at",
///     Utc::now() - TimeDelta::days(90),
/// )
/// .await?;
/// println!("deleted {deleted} events");
/// # Ok(()) }
/// ```
///
/// # Errors
///
/// The database's error: a missing table or column, or a cutoff whose
/// type does not fit the time column of a hypertable (an integer for a
/// timestamp column, a moment for an integer one).
pub async fn prune_time_series<C>(
    conn: &C,
    table: &str,
    time_column: &str,
    before: impl Into<TimePoint>,
) -> Result<u64, DbErr>
where
    C: ConnectionTrait + Send + Sync,
{
    let before = before.into();
    let delete = delete_before(table, time_column, before);
    if conn.get_database_backend() == DbBackend::Postgres
        && let Some(dimension) = time_dimension(conn, table).await?
        && dimension.column == time_column
    {
        // Built first: it checks the cutoff against the column's type
        // before any statement runs.
        let drop = drop_chunks(table, &dimension, before)?;
        let older = count_before(conn, table, time_column, before).await?;
        if older > 0 {
            conn.query_all_raw(drop).await?;
            conn.execute(&delete).await?;
        }
        return Ok(older);
    }
    Ok(conn.execute(&delete).await?.rows_affected())
}

/// [`prune_time_series`] on navree-db's executors (a pool, a transaction, a
/// unit of work, a [`Db`](crate::Db)), for code that no longer holds a
/// SeaORM connection: the same behaviour, the errors of the engine.
///
/// ```no_run
/// # async fn prune(db: &navree_db::Db) -> Result<(), navree_db::DbError> {
/// use chrono::{TimeDelta, Utc};
/// use navree_db::prune_time_series_on;
///
/// let deleted = prune_time_series_on(
///     db,
///     "events",
///     "occurred_at",
///     Utc::now() - TimeDelta::days(90),
/// )
/// .await?;
/// println!("deleted {deleted} events");
/// # Ok(()) }
/// ```
pub async fn prune_time_series_on<E>(
    executor: &E,
    table: &str,
    time_column: &str,
    before: impl Into<TimePoint>,
) -> Result<u64, DbError>
where
    E: Executor + ?Sized,
{
    let before = before.into();
    let backend = executor.backend();
    let delete = crate::Statement::portable(
        format!(
            "DELETE FROM {} WHERE {} < ?",
            backend.quote_identifier(table),
            backend.quote_identifier(time_column)
        ),
        [before.bound()],
    );
    if backend == Backend::Postgres
        && let Some(dimension) = time_dimension_on(executor, table).await?
        && dimension.column == time_column
    {
        // Built first: it checks the cutoff against the column's type
        // before any statement runs.
        let drop = drop_chunks_statement(table, &dimension, before)?;
        let count = crate::Statement::portable(
            format!(
                "SELECT COUNT(*) FROM {} WHERE {} < ?",
                backend.quote_identifier(table),
                backend.quote_identifier(time_column)
            ),
            [before.bound()],
        );
        let older: i64 = executor.fetch_scalar(&count).await?;
        if older > 0 {
            executor.fetch_all(&drop).await?;
            executor.execute(&delete).await?;
        }
        return Ok(u64::try_from(older).unwrap_or(0));
    }
    Ok(executor.execute(&delete).await?.rows_affected)
}

async fn time_dimension_on<E>(executor: &E, table: &str) -> Result<Option<TimeDimension>, DbError>
where
    E: Executor + ?Sized,
{
    Schema::new(&OnExecutor(executor))
        .time_dimension(table)
        .await
        .map_err(|error| match error {
            SchemaError::Database { source, .. } => match source.downcast::<DbError>() {
                Ok(error) => *error,
                Err(other) => DbError::unsupported(other.to_string()),
            },
            other => DbError::unsupported(format!(
                "cannot read the time dimension of `{table}`: {other}"
            )),
        })
}

/// `SELECT drop_chunks(..)` for chunks entirely before `before`, as
/// [`drop_chunks`] builds it, on navree-db's statements.
fn drop_chunks_statement(
    table: &str,
    dimension: &TimeDimension,
    before: TimePoint,
) -> Result<crate::Statement, DbError> {
    let column_type = dimension.column_type.as_str();
    check_cutoff(table, dimension, before).map_err(DbError::invalid)?;
    // Always cast: the function takes `"any"`, so a prepared statement is
    // typed by the first cutoff bound to it, and a pooled connection that
    // pruned a timestamp table must not reuse it for an integer one (the
    // statement text differs per column type).
    // The cast type is a catalog type name checked against the types pruning knows (above);
    // the cutoff itself is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    Ok(crate::Statement::portable(
        format!("SELECT drop_chunks(CAST(? AS regclass), older_than => CAST(? AS {column_type}))"),
        [table.into(), before.bound()],
    ))
}

/// The time dimension of `table` when it is a hypertable.
async fn time_dimension<C>(conn: &C, table: &str) -> Result<Option<TimeDimension>, DbErr>
where
    C: ConnectionTrait + Send + Sync,
{
    Schema::new(&crate::compat::SeaExecutor::new(conn))
        .time_dimension(table)
        .await
        .map_err(|error| match error {
            SchemaError::Database { source, .. } => match source.downcast::<crate::DbError>() {
                Ok(error) => DbErr::from(*error),
                Err(other) => DbErr::Custom(other.to_string()),
            },
            other => DbErr::Custom(format!(
                "cannot read the time dimension of `{table}`: {other}"
            )),
        })
}

/// `table` as a sea-query table reference (`schema.table` when qualified).
fn table_ref(table: &str) -> TableRef {
    match table.split_once('.') {
        Some((schema, name)) => (Alias::new(schema), Alias::new(name)).into(),
        None => Alias::new(table).into(),
    }
}

fn delete_before(table: &str, time_column: &str, before: TimePoint) -> DeleteStatement {
    Query::delete()
        .from_table(table_ref(table))
        .and_where(Expr::col(Alias::new(time_column)).lt(before.value()))
        .to_owned()
}

fn count_statement(table: &str, time_column: &str, before: TimePoint) -> SelectStatement {
    Query::select()
        .expr_as(Expr::col(Asterisk).count(), Alias::new("older"))
        .from(table_ref(table))
        .and_where(Expr::col(Alias::new(time_column)).lt(before.value()))
        .to_owned()
}

async fn count_before<C>(
    conn: &C,
    table: &str,
    time_column: &str,
    before: TimePoint,
) -> Result<u64, DbErr>
where
    C: ConnectionTrait + Send + Sync,
{
    let row = conn
        .query_one(&count_statement(table, time_column, before))
        .await?;
    let older: i64 = match row {
        Some(row) => row.try_get("", "older")?,
        None => 0,
    };
    Ok(u64::try_from(older).unwrap_or(0))
}

/// The types `drop_chunks` takes for each kind of time column.
const TIME_TYPES: [&str; 6] = [
    "timestamp with time zone",
    "timestamp without time zone",
    "date",
    "bigint",
    "integer",
    "smallint",
];

/// Why `before` cannot cut `table`'s time column, if it cannot.
fn check_cutoff(table: &str, dimension: &TimeDimension, before: TimePoint) -> Result<(), String> {
    let column_type = dimension.column_type.as_str();
    if !TIME_TYPES.contains(&column_type) {
        return Err(format!(
            "cannot drop the chunks of `{table}`: its time column `{}` has type `{column_type}`, which pruning does not know",
            dimension.column
        ));
    }
    if dimension.is_integer() != matches!(before, TimePoint::Integer(_)) {
        return Err(format!(
            "cannot prune `{table}` before {before}: its time column `{}` is `{column_type}`; pass {}",
            dimension.column,
            if dimension.is_integer() {
                "an integer (TimePoint::Integer)"
            } else {
                "a moment (TimePoint::At)"
            }
        ));
    }
    Ok(())
}

/// `SELECT drop_chunks(...)` for chunks entirely before `before`, which is
/// cast to the time column's own type (TimescaleDB requires it).
fn drop_chunks(
    table: &str,
    dimension: &TimeDimension,
    before: TimePoint,
) -> Result<Statement, DbErr> {
    let column_type = dimension.column_type.as_str();
    check_cutoff(table, dimension, before).map_err(DbErr::Custom)?;
    let cutoff = if column_type == before.postgres_type() {
        "$2".to_owned()
    } else {
        format!("CAST($2 AS {column_type})")
    };
    // The cast type is a catalog type name checked against the types pruning knows (above);
    // the cutoff itself is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    Ok(Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!("SELECT drop_chunks(CAST($1 AS regclass), older_than => {cutoff})"),
        [Value::from(table.to_owned()), before.value()],
    ))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn dimension(column_type: &str) -> TimeDimension {
        TimeDimension {
            column: "at".to_owned(),
            column_type: column_type.to_owned(),
            chunk_interval: "1 day".to_owned(),
        }
    }

    fn moment() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn statements_per_backend() {
        let before = TimePoint::At(moment());
        let delete = delete_before("events", "occurred_at", before);
        assert_eq!(
            DbBackend::Postgres.build(&delete).to_string(),
            r#"DELETE FROM "events" WHERE "occurred_at" < '2026-10-01 00:00:00.000000 +00:00'"#
        );
        let count = count_statement("analytics.events", "occurred_at", before);
        assert_eq!(
            DbBackend::Postgres.build(&count).to_string(),
            r#"SELECT COUNT(*) AS "older" FROM "analytics"."events" WHERE "occurred_at" < '2026-10-01 00:00:00.000000 +00:00'"#
        );
        assert_eq!(
            DbBackend::MySql
                .build(&delete_before("ticks", "at_ms", TimePoint::Integer(3000)))
                .to_string(),
            "DELETE FROM `ticks` WHERE `at_ms` < 3000"
        );
    }

    #[test]
    fn chunks_are_dropped_with_the_cutoff_cast_to_the_time_column() {
        let statement = drop_chunks(
            "events",
            &dimension("timestamp with time zone"),
            TimePoint::At(moment()),
        )
        .unwrap();
        assert_eq!(
            statement.sql,
            "SELECT drop_chunks(CAST($1 AS regclass), older_than => $2)"
        );
        let naive = drop_chunks(
            "events",
            &dimension("timestamp without time zone"),
            TimePoint::At(moment()),
        )
        .unwrap();
        assert_eq!(
            naive.sql,
            "SELECT drop_chunks(CAST($1 AS regclass), older_than => CAST($2 AS timestamp without time zone))"
        );
        let integer = drop_chunks("ticks", &dimension("integer"), TimePoint::Integer(5)).unwrap();
        assert_eq!(
            integer.sql,
            "SELECT drop_chunks(CAST($1 AS regclass), older_than => CAST($2 AS integer))"
        );
    }

    #[test]
    fn the_executor_statement_casts_the_cutoff_to_the_time_column() {
        let at = drop_chunks_statement(
            "events",
            &dimension("timestamp with time zone"),
            TimePoint::At(moment()),
        )
        .unwrap();
        assert_eq!(
            at.sql(),
            "SELECT drop_chunks(CAST(? AS regclass), older_than => CAST(? AS timestamp with time zone))"
        );
        // A different column type is a different statement text, so a
        // connection never reuses one prepared for another cutoff type.
        let integer =
            drop_chunks_statement("ticks", &dimension("bigint"), TimePoint::Integer(5)).unwrap();
        assert_ne!(integer.sql(), at.sql());
        assert_eq!(integer.values().len(), 2);
        let error = drop_chunks_statement("ticks", &dimension("bigint"), TimePoint::At(moment()))
            .unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InvalidStatement);
    }

    #[test]
    fn a_cutoff_of_the_wrong_kind_is_refused() {
        let error = drop_chunks("ticks", &dimension("bigint"), TimePoint::At(moment()))
            .unwrap_err()
            .to_string();
        assert!(
            error
                .contains("its time column `at` is `bigint`; pass an integer (TimePoint::Integer)"),
            "{error}"
        );
        let error = drop_chunks(
            "events",
            &dimension("timestamp with time zone"),
            TimePoint::Integer(5),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("pass a moment (TimePoint::At)"), "{error}");
        let error = drop_chunks("events", &dimension("tstzrange"), TimePoint::Integer(5))
            .unwrap_err()
            .to_string();
        assert!(error.contains("has type `tstzrange`"), "{error}");
    }
}
