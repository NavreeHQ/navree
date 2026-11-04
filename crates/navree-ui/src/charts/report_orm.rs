//! A report's figures through navree-orm: one grouped query per measure, on the
//! app's scoped query, so workspace scoping and row-level security hold.
//!
//! - **TimescaleDB** on PostgreSQL, when it is installed and the report names its
//!   zone: `time_bucket` in that zone (hours from the window's first instant, so a
//!   clock change neither merges nor splits an hour).
//! - **Everywhere else on SQL** (PostgreSQL without it, MySQL, SQLite): each row's
//!   period by bound comparisons against the periods' ends, halving the list, so a
//!   row costs about log2(periods) comparisons and the periods are exactly the
//!   report's, in any zone.
//! - **MongoDB**: the ORM's time buckets of an hour (a quarter of an hour in a zone
//!   off UTC by a part of an hour), folded into the report's periods here.

use chrono::{DateTime, Utc};
use navree_orm::{
    Access, Builder, Dialect, Executor, Expr, FieldError, Model, Result, Row, Selectable, Value,
    col,
};

use super::report::Report;
use super::scale::Grain;

/// What a report adds up in each period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Metric {
    /// How many rows. A period with none is 0.
    Count,
    /// How many distinct values of a column (SQL databases).
    CountDistinct(&'static str),
    /// The sum of a column. A period with no rows is 0.
    Sum(&'static str),
    /// The mean of a column. A period with no rows has no value.
    Average(&'static str),
    /// The least value of a column.
    Min(&'static str),
    /// The greatest value of a column.
    Max(&'static str),
}

/// How the periods are found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Bucketing {
    /// TimescaleDB's `time_bucket` where it can be used, bound comparisons otherwise.
    #[default]
    Auto,
    /// Bound comparisons, on any SQL database.
    Exact,
    /// TimescaleDB's `time_bucket`; an error where it can't be used.
    TimeBucket,
}

/// How a column holds its instants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimeColumn {
    /// A timestamp column.
    Timestamp(&'static str),
    /// Seconds since 1970 in an integer column.
    UnixSeconds(&'static str),
    /// Milliseconds since 1970 in an integer column.
    UnixMillis(&'static str),
}

impl From<&'static str> for TimeColumn {
    fn from(name: &'static str) -> Self {
        Self::Timestamp(name)
    }
}

impl TimeColumn {
    fn name(self) -> &'static str {
        match self {
            Self::Timestamp(n) | Self::UnixSeconds(n) | Self::UnixMillis(n) => n,
        }
    }

    /// An instant as this column stores it.
    fn value(self, at: DateTime<Utc>) -> Value {
        match self {
            Self::Timestamp(_) => Value::Timestamp(at),
            Self::UnixSeconds(_) => Value::I64(at.timestamp()),
            Self::UnixMillis(_) => Value::I64(at.timestamp_millis()),
        }
    }
}

/// A value read as a number.
fn number(value: &Value) -> Option<f64> {
    let out = match value {
        Value::I8(n) => f64::from(*n),
        Value::I16(n) => f64::from(*n),
        Value::I32(n) => f64::from(*n),
        Value::I64(n) => *n as f64,
        Value::U8(n) => f64::from(*n),
        Value::U16(n) => f64::from(*n),
        Value::U32(n) => f64::from(*n),
        Value::U64(n) => *n as f64,
        Value::F32(n) => f64::from(*n),
        Value::F64(n) => *n,
        Value::Decimal(d) => d.to_string().parse().ok()?,
        Value::String(s) => s.trim().parse().ok()?,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Timestamp(at) => at.timestamp() as f64,
        _ => return None,
    };
    out.is_finite().then_some(out)
}

/// The aggregate, selected as a number.
struct Aggregate(Expr);

impl Selectable for Aggregate {
    type Output = Option<f64>;

    fn selected(&self) -> Expr {
        self.0.clone()
    }

    fn read(&self, row: &Row, column: &str) -> std::result::Result<Option<f64>, FieldError> {
        Ok(row.value(column).ok().as_ref().and_then(number))
    }
}

/// A row's period, as a number: an index, or the bucket's start in seconds.
struct Key(Expr);

impl Selectable for Key {
    type Output = i64;

    fn selected(&self) -> Expr {
        self.0.clone()
    }

    fn read(&self, row: &Row, column: &str) -> std::result::Result<i64, FieldError> {
        Ok(row
            .value(column)
            .ok()
            .as_ref()
            .and_then(number)
            .map_or(-1, |v| v.round() as i64))
    }
}

/// A key read as an instant: the ORM's time bucket on a document store.
struct InstantKey(Expr);

impl Selectable for InstantKey {
    type Output = i64;

    fn selected(&self) -> Expr {
        self.0.clone()
    }

    fn read(&self, row: &Row, column: &str) -> std::result::Result<i64, FieldError> {
        Ok(match row.value(column).ok() {
            Some(Value::Timestamp(at)) => at.timestamp(),
            Some(other) => number(&other).map_or(i64::MIN, |v| v as i64),
            None => i64::MIN,
        })
    }
}

/// Whether `name` is a plain column name (`at`, `events.at`).
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.split('.').count() <= 2
        && name.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// A column written for `dialect`, qualified by the model's table.
fn quoted<M: Model>(name: &str, dialect: Dialect) -> String {
    let full = if name.contains('.') {
        name.to_owned()
    } else {
        format!("{}.{name}", M::meta().table())
    };
    let q = if dialect == Dialect::MySql { '`' } else { '"' };
    full.split('.')
        .map(|part| format!("{q}{part}{q}"))
        .collect::<Vec<_>>()
        .join(".")
}

fn invalid(message: impl Into<String>) -> navree_orm::OrmError {
    navree_orm::OrmError::InvalidQuery(message.into())
}

/// `CASE WHEN c < ? THEN .. ELSE .. END`, halving the periods: the index of the period
/// each row is in. Rows are already inside the window.
fn halve(
    column: &str,
    ends: &[Value],
    lo: usize,
    hi: usize,
    sql: &mut String,
    bound: &mut Vec<Value>,
) {
    if lo >= hi {
        sql.push_str(&lo.to_string());
        return;
    }
    let mid = (lo + hi) / 2;
    sql.push_str("CASE WHEN ");
    sql.push_str(column);
    sql.push_str(" < ? THEN ");
    bound.push(ends[mid].clone());
    halve(column, ends, lo, mid, sql, bound);
    sql.push_str(" ELSE ");
    halve(column, ends, mid + 1, hi, sql, bound);
    sql.push_str(" END");
}

impl Metric {
    fn expr<M: Model>(self) -> Expr {
        let qualified = |c: &str| {
            if c.contains('.') {
                c.to_owned()
            } else {
                format!("{}.{c}", M::meta().table())
            }
        };
        match self {
            Self::Count => Expr::count_all(),
            Self::CountDistinct(c) => Expr::count_distinct(col(qualified(c))),
            Self::Sum(c) => Expr::sum(col(qualified(c))),
            Self::Average(c) => Expr::avg(col(qualified(c))),
            Self::Min(c) => Expr::min(col(qualified(c))),
            Self::Max(c) => Expr::max(col(qualified(c))),
        }
    }

    fn column(self) -> Option<&'static str> {
        match self {
            Self::Count => None,
            Self::CountDistinct(c)
            | Self::Sum(c)
            | Self::Average(c)
            | Self::Min(c)
            | Self::Max(c) => Some(c),
        }
    }

    /// Whether a period with no rows is 0 rather than no value.
    fn zero_when_empty(self) -> bool {
        matches!(self, Self::Count | Self::CountDistinct(_) | Self::Sum(_))
    }
}

/// Whether TimescaleDB is installed in the database `exec` reads.
async fn has_timescale<E: Executor + ?Sized>(exec: &E) -> bool {
    let probe = navree_orm::Raw::new(
        "SELECT extversion FROM pg_extension WHERE extname = 'timescaledb'",
        Vec::<Value>::new(),
    );
    matches!(navree_orm::select_one(exec, probe).await, Ok(Some(_)))
}

impl Report {
    /// `measure` in each period of the report, over `query` (the app's scoped query)
    /// with its rows' instants in `time`. A column of seconds or milliseconds since
    /// 1970 is named with [`TimeColumn::UnixSeconds`] or [`TimeColumn::UnixMillis`].
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db, report: navree_ui::charts::Report) -> navree_orm::Result<()> {
    /// use navree_ui::charts::{Chart, Metric, Series};
    ///
    /// let deals = report.measure(Deal::query(), "created_at", Metric::Count, &db).await?;
    /// let chart = report.chart(Chart::bar("Deals created")).series(Series::new("Deals", deals));
    /// # Ok(()) }
    /// ```
    pub async fn measure<M, E>(
        &self,
        query: Builder<M>,
        time: impl Into<TimeColumn>,
        measure: Metric,
        exec: &E,
    ) -> Result<Vec<Option<f64>>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        self.measure_with(query, time, measure, Bucketing::Auto, exec)
            .await
    }

    /// [`measure`](Self::measure), choosing how the periods are found.
    pub async fn measure_with<M, E>(
        &self,
        query: Builder<M>,
        time: impl Into<TimeColumn>,
        measure: Metric,
        bucketing: Bucketing,
        exec: &E,
    ) -> Result<Vec<Option<f64>>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let time = time.into();
        if !plain(time.name()) || measure.column().is_some_and(|c| !plain(c)) {
            return Err(invalid(
                "a report's columns are plain names: letters, digits, `_` and one `.`",
            ));
        }
        let count = self.buckets().len();
        let mut out = vec![None; count];
        if count == 0 {
            return Ok(out);
        }
        let (start, end) = self.bounds();
        let window = query
            .where_(col(time.name()).gte(time.value(start)))
            .where_(col(time.name()).lt(time.value(end)));
        let dialect = exec.target(Access::Read).await?.dialect();
        let Some(dialect) = dialect else {
            return self.measure_documents(window, time, measure, exec).await;
        };
        let timescale = match bucketing {
            Bucketing::Exact => false,
            Bucketing::Auto => {
                dialect == Dialect::Postgres
                    && self.timescale_fits(time)
                    && has_timescale(exec).await
            }
            Bucketing::TimeBucket => {
                if dialect != Dialect::Postgres || !self.timescale_fits(time) {
                    return Err(invalid(
                        "TimescaleDB buckets need PostgreSQL, a timestamp column, a named zone and weeks from Monday",
                    ));
                }
                true
            }
        };
        let column = quoted::<M>(time.name(), dialect);
        if timescale {
            let (sql, values) = self.time_bucket_sql(&column);
            let found = window
                .group_by_raw("2", Vec::<Value>::new())
                .pluck_keyed(
                    exec,
                    Aggregate(measure.expr::<M>()),
                    Key(Expr::raw(sql, values)),
                )
                .await?;
            for (seconds, value) in found {
                let Some(at) = DateTime::from_timestamp(seconds, 0) else {
                    continue;
                };
                if let Some(i) = self.bucket_of(at.max(start)) {
                    out[i] = value;
                }
            }
        } else {
            let ends: Vec<Value> = self.buckets().iter().map(|b| time.value(b.end)).collect();
            let mut sql = String::with_capacity(count * 40);
            let mut bound = Vec::with_capacity(count);
            halve(&column, &ends, 0, count - 1, &mut sql, &mut bound);
            let found = window
                .group_by_raw("2", Vec::<Value>::new())
                .pluck_keyed(
                    exec,
                    Aggregate(measure.expr::<M>()),
                    Key(Expr::raw(sql, bound)),
                )
                .await?;
            for (index, value) in found {
                if let Some(slot) = usize::try_from(index).ok().and_then(|i| out.get_mut(i)) {
                    *slot = value;
                }
            }
        }
        if measure.zero_when_empty() {
            for value in &mut out {
                value.get_or_insert(0.0);
            }
        }
        Ok(out)
    }

    /// `measure` per value of `group` in each period: one series per name in `groups`,
    /// in that order, and one more for every other value (Other). For a stacked chart
    /// of statuses over time.
    pub async fn measure_by<M, E>(
        &self,
        query: Builder<M>,
        time: impl Into<TimeColumn>,
        group: &'static str,
        groups: &[&str],
        measure: Metric,
        exec: &E,
    ) -> Result<Vec<Vec<Option<f64>>>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let time = time.into();
        if !plain(group) {
            return Err(invalid(
                "a report's columns are plain names: letters, digits, `_` and one `.`",
            ));
        }
        let mut out = Vec::with_capacity(groups.len() + 1);
        let column = if group.contains('.') {
            group.to_owned()
        } else {
            format!("{}.{group}", M::meta().table())
        };
        for name in groups {
            let scoped = query
                .clone()
                .where_(col(column.clone()).eq(Value::String((*name).to_owned())));
            out.push(self.measure(scoped, time, measure, exec).await?);
        }
        let names: Vec<Value> = groups
            .iter()
            .map(|n| Value::String((*n).to_owned()))
            .collect();
        let rest = if names.is_empty() {
            query
        } else {
            query.where_(
                col(column.clone())
                    .not_in(names)
                    .or(col(column.clone()).is_null()),
            )
        };
        out.push(self.measure(rest, time, measure, exec).await?);
        Ok(out)
    }

    /// Whether TimescaleDB's buckets give exactly this report's periods.
    fn timescale_fits(&self, time: TimeColumn) -> bool {
        matches!(time, TimeColumn::Timestamp(_))
            && (self.grain() == Grain::Hour
                || (self.zone().is_some()
                    && (self.grain() != Grain::Week || self.week_start() == chrono::Weekday::Mon)))
    }

    /// `EXTRACT(EPOCH FROM time_bucket(..))`: the start of each row's bucket.
    fn time_bucket_sql(&self, column: &str) -> (String, Vec<Value>) {
        let (start, _) = self.bounds();
        match self.grain() {
            Grain::Minute | Grain::Hour => (
                format!(
                    "EXTRACT(EPOCH FROM time_bucket(CAST(? AS INTERVAL), {column}, CAST(? AS TIMESTAMPTZ)))"
                ),
                vec![
                    Value::String(
                        if self.grain() == Grain::Hour {
                            "1 hour"
                        } else {
                            "1 minute"
                        }
                        .to_owned(),
                    ),
                    Value::Timestamp(start),
                ],
            ),
            grain => {
                let width = match grain {
                    Grain::Week => "1 week",
                    Grain::Month => "1 month",
                    Grain::Quarter => "3 months",
                    Grain::Year => "1 year",
                    _ => "1 day",
                };
                (
                    format!(
                        "EXTRACT(EPOCH FROM time_bucket(CAST(? AS INTERVAL), {column}, CAST(? AS TEXT)))"
                    ),
                    vec![
                        Value::String(width.to_owned()),
                        Value::String(self.zone().unwrap_or("UTC").to_owned()),
                    ],
                )
            }
        }
    }

    /// A document store: the ORM's hourly buckets, folded into the report's periods.
    async fn measure_documents<M, E>(
        &self,
        window: Builder<M>,
        time: TimeColumn,
        measure: Metric,
        exec: &E,
    ) -> Result<Vec<Option<f64>>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let count = self.buckets().len();
        if !matches!(time, TimeColumn::Timestamp(_)) {
            return Err(invalid("a report over MongoDB reads a date column"));
        }
        if matches!(measure, Metric::CountDistinct(_)) {
            return Err(invalid(
                "counting distinct values per period needs an SQL database",
            ));
        }
        // Whole hours fit every zone off UTC by whole hours; a quarter of an hour fits the rest.
        let fine = if self
            .buckets()
            .iter()
            .all(|b| b.start.timestamp().rem_euclid(3600) == 0)
        {
            "1 hour"
        } else {
            "15 minutes"
        };
        let qualified = format!("{}.{}", M::meta().table(), time.name());
        let fold = |found: Vec<(i64, Option<f64>)>, combine: fn(f64, f64) -> f64| {
            let mut out: Vec<Option<f64>> = vec![None; count];
            for (seconds, value) in found {
                let (Some(at), Some(value)) = (DateTime::from_timestamp(seconds, 0), value) else {
                    continue;
                };
                if let Some(i) = self.bucket_of(at) {
                    out[i] = Some(out[i].map_or(value, |v| combine(v, value)));
                }
            }
            out
        };
        let pick = |m: Metric| pick_documents(&window, exec, m, fine, &qualified);
        let mut out = match measure {
            Metric::Count | Metric::Sum(_) => fold(pick(measure).await?, |a, b| a + b),
            Metric::Min(_) => fold(pick(measure).await?, f64::min),
            Metric::Max(_) => fold(pick(measure).await?, f64::max),
            Metric::Average(c) => {
                let sums = fold(pick(Metric::Sum(c)).await?, |a, b| a + b);
                let counts = fold(pick(Metric::Count).await?, |a, b| a + b);
                sums.into_iter()
                    .zip(counts)
                    .map(|(s, n)| match (s, n) {
                        (Some(s), Some(n)) if n > 0.0 => Some(s / n),
                        _ => None,
                    })
                    .collect()
            }
            Metric::CountDistinct(_) => unreachable!("refused above"),
        };
        if measure.zero_when_empty() {
            for value in &mut out {
                value.get_or_insert(0.0);
            }
        }
        Ok(out)
    }
}

/// One grouped query on a document store: `m` per bucket of `fine`.
async fn pick_documents<M, E>(
    window: &Builder<M>,
    exec: &E,
    m: Metric,
    fine: &'static str,
    qualified: &str,
) -> Result<Vec<(i64, Option<f64>)>>
where
    M: Model,
    E: Executor + ?Sized,
{
    let found = window
        .clone()
        .group_by(["key"])
        .pluck_keyed(
            exec,
            Aggregate(m.expr::<M>()),
            InstantKey(Expr::time_bucket(fine, col(qualified.to_owned()))),
        )
        .await?;
    Ok(found.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_plain_names() {
        assert!(plain("at"));
        assert!(plain("events.created_at"));
        assert!(!plain("a.b.c"));
        assert!(!plain("at; DROP TABLE x"));
        assert!(!plain("\"at\""));
        assert!(!plain(""));
    }

    #[test]
    fn halving_finds_each_period_in_log_steps() {
        let ends: Vec<Value> = (1..=5).map(Value::I64).collect();
        let mut sql = String::new();
        let mut bound = Vec::new();
        halve("\"t\".\"at\"", &ends, 0, 4, &mut sql, &mut bound);
        assert_eq!(
            sql,
            "CASE WHEN \"t\".\"at\" < ? THEN CASE WHEN \"t\".\"at\" < ? THEN CASE WHEN \"t\".\"at\" < ? THEN 0 ELSE 1 END ELSE 2 END ELSE CASE WHEN \"t\".\"at\" < ? THEN 3 ELSE 4 END END"
        );
        assert_eq!(
            bound,
            [Value::I64(3), Value::I64(2), Value::I64(1), Value::I64(4)]
        );
        // One period is a constant.
        let mut one = String::new();
        halve("x", &ends, 0, 0, &mut one, &mut Vec::new());
        assert_eq!(one, "0");
    }
}
