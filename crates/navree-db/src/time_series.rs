//! Time-series tables: deleting the rows older than a cutoff, by whole
//! chunks on TimescaleDB ([`prune_time_series`], [`TimePoint`]).

use chrono::{DateTime, Utc};
use navree_schema::{Schema, SchemaError, TimeDimension};

use crate::schema::OnExecutor;
use crate::{Backend, DbError, Executor, ExecutorExt, Statement, Value};

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
    /// The cutoff as a bound value.
    fn bound(self) -> Value {
        match self {
            TimePoint::At(moment) => moment.into(),
            TimePoint::Integer(value) => value.into(),
        }
    }
}

/// Deletes the rows of `table` whose `time_column` is before `before`, and
/// returns how many there were. Runs on any of navree-db's executors: a
/// [`Db`](crate::Db), a [`Transaction`](crate::Transaction), a unit of
/// work ([`Uow`](crate::Uow)), a [`Pool`](crate::Pool).
///
/// On a TimescaleDB hypertable partitioned by `time_column`, it counts
/// those rows, drops every chunk that lies entirely before the cutoff
/// (`drop_chunks`: whole files go, with no row-by-row delete, no table
/// bloat to vacuum, and no empty chunks left behind), then deletes the rows
/// left in the chunk that straddles the cutoff. Anywhere else (a regular
/// table, PostgreSQL without TimescaleDB, MySQL, MariaDB, SQLite) it is
/// one `DELETE`.
///
/// `table` may be qualified (`analytics.events`). Run it on a transaction
/// (or a unit of work) to have the count, the dropped chunks and the delete
/// apply together.
///
/// ```no_run
/// # async fn prune(db: &navree_db::Db) -> Result<(), navree_db::DbError> {
/// use chrono::{TimeDelta, Utc};
/// use navree_db::prune_time_series;
///
/// let tx = db.begin().await?;
/// let deleted = prune_time_series(&tx, "events", "occurred_at", Utc::now() - TimeDelta::days(90)).await?;
/// tx.commit().await?;
/// println!("deleted {deleted} events");
/// # Ok(()) }
/// ```
///
/// # Errors
///
/// The database's error: a missing table or column, or a cutoff whose
/// type does not fit the time column of a hypertable (an integer for a
/// timestamp column, a moment for an integer one: `InvalidStatement`).
pub async fn prune_time_series<E>(
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
    // The table and column go through the backend's `quote_identifier`; the cutoff is bound.
    // nosemgrep: navree-sql-built-from-strings
    let delete = Statement::portable(
        format!(
            "DELETE FROM {} WHERE {} < ?",
            backend.quote_identifier(table),
            backend.quote_identifier(time_column)
        ),
        [before.bound()],
    );
    if backend == Backend::Postgres
        && let Some(dimension) = time_dimension(executor, table).await?
        && dimension.column == time_column
    {
        // Built first: it checks the cutoff against the column's type
        // before any statement runs.
        let drop = drop_chunks(table, &dimension, before)?;
        // The table and column go through the backend's `quote_identifier`; the cutoff is bound.
        // nosemgrep: navree-sql-built-from-strings
        let count = Statement::portable(
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

/// The time dimension of `table` when it is a hypertable.
async fn time_dimension<E>(executor: &E, table: &str) -> Result<Option<TimeDimension>, DbError>
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

/// `SELECT drop_chunks(..)` for the chunks entirely before `before`, which
/// is cast to the time column's own type (TimescaleDB requires it).
fn drop_chunks(
    table: &str,
    dimension: &TimeDimension,
    before: TimePoint,
) -> Result<Statement, DbError> {
    let column_type = dimension.column_type.as_str();
    check_cutoff(table, dimension, before).map_err(DbError::invalid)?;
    // Always cast: the function takes `"any"`, so a prepared statement is
    // typed by the first cutoff bound to it, and a pooled connection that
    // pruned a timestamp table must not reuse it for an integer one (the
    // statement text differs per column type).
    // The cast type is a catalog type name checked against the types pruning knows (above);
    // the cutoff itself is a bound parameter.
    // nosemgrep: navree-sql-built-from-strings
    Ok(Statement::portable(
        format!("SELECT drop_chunks(CAST(? AS regclass), older_than => CAST(? AS {column_type}))"),
        [table.into(), before.bound()],
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
    fn chunks_are_dropped_with_the_cutoff_cast_to_the_time_column() {
        let at = drop_chunks(
            "events",
            &dimension("timestamp with time zone"),
            TimePoint::At(moment()),
        )
        .unwrap();
        assert_eq!(
            at.sql(),
            "SELECT drop_chunks(CAST(? AS regclass), older_than => CAST(? AS timestamp with time zone))"
        );
        let naive = drop_chunks(
            "events",
            &dimension("timestamp without time zone"),
            TimePoint::At(moment()),
        )
        .unwrap();
        assert_eq!(
            naive.sql(),
            "SELECT drop_chunks(CAST(? AS regclass), older_than => CAST(? AS timestamp without time zone))"
        );
        // A different column type is a different statement text, so a
        // connection never reuses one prepared for another cutoff type.
        let integer = drop_chunks("ticks", &dimension("bigint"), TimePoint::Integer(5)).unwrap();
        assert_ne!(integer.sql(), at.sql());
        assert_eq!(integer.values().len(), 2);
    }

    #[test]
    fn a_cutoff_of_the_wrong_kind_is_refused() {
        let error =
            drop_chunks("ticks", &dimension("bigint"), TimePoint::At(moment())).unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InvalidStatement);
        let error = error.to_string();
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
