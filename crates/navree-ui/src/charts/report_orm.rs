//! A report's figures through navree-orm: one grouped query per measure, on the
//! app's scoped query, so workspace scoping and row-level security hold.
//!
//! - **TimescaleDB** on PostgreSQL, when it is installed and the report's zone has an
//!   IANA name: `time_bucket` in that zone (hours from the window's first instant, so a
//!   clock change neither merges nor splits an hour).
//! - **Everywhere else on SQL** (PostgreSQL without it, MySQL, SQLite): each row's
//!   period by bound comparisons against the periods' ends, halving the list, so a
//!   row costs about log2(periods) comparisons and the periods are exactly the
//!   report's, in any zone.
//! - **MongoDB**: the ORM's time buckets of an hour (or of a quarter of an hour, a
//!   minute or a second, whichever every period starts and ends on), folded into the
//!   report's periods here.
//!
//! On every path a row outside the window counts nowhere, whatever the app's query
//! says, and the app's conditions are grouped before the window's are added.

use chrono::{DateTime, Utc};
use navree_orm::ir::{Boolean, Where, WhereClause};
use navree_orm::{
    Access, Builder, Dialect, Executor, Expr, FieldError, HasQuery, Model, Result, Row, Selectable,
    Value, col,
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

/// A row's period, as a number (an index, or the bucket's first second), times the
/// groups and plus its group; `None` for a row outside the window.
struct Key(Expr);

impl Selectable for Key {
    type Output = Option<i64>;

    fn selected(&self) -> Expr {
        self.0.clone()
    }

    fn read(&self, row: &Row, column: &str) -> std::result::Result<Option<i64>, FieldError> {
        Ok(row
            .value(column)
            .ok()
            .as_ref()
            .and_then(number)
            .map(|v| v.round() as i64))
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
/// each row is in, for a row inside the window.
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

/// The app's conditions as one group, so the window's are added to all of them: a
/// query `a OR b` is counted as `(a OR b) AND window`, never `a OR (b AND window)`.
fn grouped<M: Model>(mut query: Builder<M>) -> Builder<M> {
    let ir = query.query_mut();
    if ir
        .wheres
        .iter()
        .skip(1)
        .any(|clause| clause.boolean == Boolean::Or)
    {
        let all = std::mem::take(&mut ir.wheres);
        ir.wheres.push(WhereClause {
            boolean: Boolean::And,
            negated: false,
            filter: Where::Group(all),
        });
    }
    query
}

/// Two values of one period, from two groups of rows: added, or the least or greatest.
fn combine(metric: Metric, a: f64, b: f64) -> f64 {
    match metric {
        Metric::Min(_) => a.min(b),
        Metric::Max(_) => a.max(b),
        // A period is one group on every path; only counts and sums ever meet.
        _ => a + b,
    }
}

/// The most groups `measure_by` names: one colour each, and one more for Other.
const MOST_GROUPS: usize = crate::charts::MAX_SERIES - 1;

impl Report {
    /// `metric` in each period of the report, over `query` (the app's scoped query)
    /// with its rows' instants in `time`. A column of seconds or milliseconds since
    /// 1970 is named with [`TimeColumn::UnixSeconds`] or [`TimeColumn::UnixMillis`].
    ///
    /// Only rows inside the window count, whatever `query` says: its conditions are
    /// grouped before the window's are added, so an `or_where` can't reach outside.
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
        metric: Metric,
        exec: &E,
    ) -> Result<Vec<Option<f64>>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        self.measure_with(query, time, metric, Bucketing::Auto, exec)
            .await
    }

    /// [`measure`](Self::measure), choosing how the periods are found.
    pub async fn measure_with<M, E>(
        &self,
        query: Builder<M>,
        time: impl Into<TimeColumn>,
        metric: Metric,
        bucketing: Bucketing,
        exec: &E,
    ) -> Result<Vec<Option<f64>>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let mut series = self
            .measure_groups(query, time.into(), metric, None, bucketing, exec)
            .await?;
        Ok(series.pop().unwrap_or_default())
    }

    /// `metric` per value of `group` in each period: one series per name in `groups`, in
    /// that order, and one more for every other value, NULL included (Other). At most
    /// seven names are counted on their own, as a chart draws eight series: the rest are
    /// counted in Other. On an SQL database this is one grouped query.
    pub async fn measure_by<M, E>(
        &self,
        query: Builder<M>,
        time: impl Into<TimeColumn>,
        group: &'static str,
        groups: &[&str],
        metric: Metric,
        exec: &E,
    ) -> Result<Vec<Vec<Option<f64>>>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let names = &groups[..groups.len().min(MOST_GROUPS)];
        self.measure_groups(
            query,
            time.into(),
            metric,
            Some((group, names)),
            Bucketing::Auto,
            exec,
        )
        .await
    }

    /// The figures of each group (or of all rows, with no groups) in each period.
    async fn measure_groups<M, E>(
        &self,
        query: Builder<M>,
        time: TimeColumn,
        metric: Metric,
        groups: Option<(&'static str, &[&str])>,
        bucketing: Bucketing,
        exec: &E,
    ) -> Result<Vec<Vec<Option<f64>>>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let plain_names = "a report's columns are plain names: letters, digits, `_` and one `.`";
        if !plain(time.name())
            || metric.column().is_some_and(|c| !plain(c))
            || groups.is_some_and(|(g, _)| !plain(g))
        {
            return Err(invalid(plain_names));
        }
        let width = groups.map_or(1, |(_, names)| names.len() + 1);
        let count = self.buckets().len();
        let mut out = vec![vec![None; count]; width];
        if count == 0 {
            return Ok(out);
        }
        let (start, end) = self.bounds();
        let window = grouped(query)
            .where_(col(time.name()).gte(time.value(start)))
            .where_(col(time.name()).lt(time.value(end)));
        let dialect = exec.target(Access::Read).await?.dialect();
        let Some(dialect) = dialect else {
            // A document store: one query per group, each folded into the periods.
            let mut out = Vec::with_capacity(width);
            match groups {
                None => out.push(self.measure_documents(window, time, metric, exec).await?),
                Some((group, names)) => {
                    let column = qualified::<M>(group);
                    for name in names {
                        let only = window
                            .clone()
                            .where_(col(column.clone()).eq(Value::String((*name).to_owned())));
                        out.push(self.measure_documents(only, time, metric, exec).await?);
                    }
                    let values: Vec<Value> = names
                        .iter()
                        .map(|n| Value::String((*n).to_owned()))
                        .collect();
                    let rest = if values.is_empty() {
                        window
                    } else {
                        window.where_(
                            col(column.clone())
                                .not_in(values)
                                .or(col(column.clone()).is_null()),
                        )
                    };
                    out.push(self.measure_documents(rest, time, metric, exec).await?);
                }
            }
            return Ok(out);
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
                        "TimescaleDB buckets need PostgreSQL, a timestamp column, a zone with an IANA name and weeks from Monday",
                    ));
                }
                true
            }
        };
        let column = quoted::<M>(time.name(), dialect);
        // The period of each row: an index, or its bucket's first second with TimescaleDB;
        // NULL for a row outside the window, which is never counted.
        let mut values = vec![time.value(start), time.value(end)];
        let mut sql = String::with_capacity(count * 40 + 120);
        if timescale {
            let (bucket, bound) = self.time_bucket_sql(&column);
            sql.push_str(&format!(
                "CASE WHEN {column} >= ? AND {column} < ? THEN {bucket} END"
            ));
            values.extend(bound);
        } else {
            let ends: Vec<Value> = self.buckets().iter().map(|b| time.value(b.end)).collect();
            sql.push_str(&format!(
                "CASE WHEN {column} < ? OR {column} >= ? THEN NULL ELSE "
            ));
            halve(&column, &ends, 0, count - 1, &mut sql, &mut values);
            sql.push_str(" END");
        }
        // Each group is a number from 0; the key is the period times the groups, plus it.
        if let Some((group, names)) = groups {
            let group_column = quoted::<M>(group, dialect);
            let mut which = String::from("CASE");
            for (at, name) in names.iter().enumerate() {
                which.push_str(&format!(" WHEN {group_column} = ? THEN {at}"));
                values.push(Value::String((*name).to_owned()));
            }
            which.push_str(&format!(" ELSE {} END", names.len()));
            sql = format!("({sql}) * {width} + ({which})");
        }
        let found = window
            .group_by_raw("2", Vec::<Value>::new())
            .pluck_keyed(
                exec,
                Aggregate(metric.expr::<M>()),
                Key(Expr::raw(sql, values)),
            )
            .await?;
        let width = width as i64;
        for (key, value) in found {
            let (Some(key), Some(value)) = (key, value) else {
                continue;
            };
            let (period, group) = (key.div_euclid(width), key.rem_euclid(width) as usize);
            let index = if timescale {
                DateTime::from_timestamp(period, 0).and_then(|at| self.bucket_of(at.max(start)))
            } else {
                usize::try_from(period).ok().filter(|i| *i < count)
            };
            if let Some(i) = index {
                let slot = &mut out[group][i];
                *slot = Some(slot.map_or(value, |v| combine(metric, v, value)));
            }
        }
        if metric.zero_when_empty() {
            for series in &mut out {
                for value in series {
                    value.get_or_insert(0.0);
                }
            }
        }
        Ok(out)
    }

    /// Whether TimescaleDB's buckets give exactly this report's periods.
    fn timescale_fits(&self, time: TimeColumn) -> bool {
        matches!(time, TimeColumn::Timestamp(_))
            && (self.grain() == Grain::Hour
                || (self.zone().is_some()
                    && (self.grain() != Grain::Week || self.week_start() == chrono::Weekday::Mon)))
    }

    /// `EXTRACT(EPOCH FROM time_bucket(..))`: the first second of each row's bucket.
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

    /// The width of the ORM's buckets a document store counts in: the widest of an hour,
    /// a quarter of an hour, a minute and a second that every period starts and ends on,
    /// so each one falls whole inside a period, in any zone (Kolkata kept its own clock
    /// to the second until 1906).
    fn fine_width(&self) -> &'static str {
        let latest = super::report::latest_instant();
        let on = |unit: i64| {
            self.buckets().iter().all(|b| {
                [b.start, b.end].iter().all(|at| {
                    *at == latest
                        || (at.timestamp_subsec_nanos() == 0
                            && at.timestamp().rem_euclid(unit) == 0)
                })
            })
        };
        if on(3600) {
            "1 hour"
        } else if on(900) {
            "15 minutes"
        } else if on(60) {
            "1 minute"
        } else {
            "1 second"
        }
    }

    /// A document store: the ORM's buckets, folded into the report's periods.
    async fn measure_documents<M, E>(
        &self,
        window: Builder<M>,
        time: TimeColumn,
        metric: Metric,
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
        if matches!(metric, Metric::CountDistinct(_)) {
            return Err(invalid(
                "counting distinct values per period needs an SQL database",
            ));
        }
        let fine = self.fine_width();
        let at = qualified::<M>(time.name());
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
        let pick = |what: Expr| pick_documents(&window, exec, what, fine, &at);
        let mut out = match metric {
            Metric::Count | Metric::Sum(_) => fold(pick(metric.expr::<M>()).await?, |a, b| a + b),
            Metric::Min(_) => fold(pick(metric.expr::<M>()).await?, f64::min),
            Metric::Max(_) => fold(pick(metric.expr::<M>()).await?, f64::max),
            Metric::Average(c) => {
                // As SQL averages: over the rows whose value isn't NULL.
                let values = col(qualified::<M>(c));
                let sums = fold(pick(Expr::sum(values.clone())).await?, |a, b| a + b);
                let counts = fold(pick(Expr::count(values)).await?, |a, b| a + b);
                sums.into_iter()
                    .zip(counts)
                    .map(|(s, n)| match (s, n) {
                        (Some(s), Some(n)) if n > 0.0 => Some(s / n),
                        _ => None,
                    })
                    .collect()
            }
            Metric::CountDistinct(_) => Vec::new(),
        };
        if metric.zero_when_empty() {
            for value in &mut out {
                value.get_or_insert(0.0);
            }
        }
        Ok(out)
    }
}

/// `name` qualified by the model's table, unless it has a table already.
fn qualified<M: Model>(name: &str) -> String {
    if name.contains('.') {
        name.to_owned()
    } else {
        format!("{}.{name}", M::meta().table())
    }
}

/// One grouped query on a document store: `what` per bucket of `fine`.
async fn pick_documents<M, E>(
    window: &Builder<M>,
    exec: &E,
    what: Expr,
    fine: &'static str,
    at: &str,
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
            Aggregate(what),
            InstantKey(Expr::time_bucket(fine, col(at.to_owned()))),
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
