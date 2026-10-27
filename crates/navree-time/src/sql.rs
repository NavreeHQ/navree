//! [`ZoneSql`]: local timestamps and local days in SQL, for reports that
//! aggregate by the tenant's days in the database.

use chrono::{DateTime, TimeDelta, Utc};
use sea_orm::DbBackend;
use sea_orm::sea_query::Expr;

use crate::Zone;

/// SQL expressions that read a UTC timestamp column on a zone's clock.
///
/// The zone is inlined as a quoted literal (`'Asia/Kolkata'`), which is
/// safe because a [`Zone`] only ever holds a checked IANA name, and lets
/// the same expression appear in `SELECT` and `GROUP BY` (bound parameters
/// would differ between the two).
///
/// | Backend | `local_timestamp(col)` | `local_date(col)` |
/// |---|---|---|
/// | PostgreSQL | `(col AT TIME ZONE 'zone')` | `CAST((col AT TIME ZONE 'zone') AS DATE)` |
/// | MySQL | `CONVERT_TZ(col, '+00:00', 'zone')` | `DATE(CONVERT_TZ(...))` |
/// | SQLite | `datetime(col, '+N seconds')` | `date(col, '+N seconds')` |
///
/// MySQL's named zones need the server's time zone tables
/// (`mysql_tzinfo_to_sql`). SQLite has no zones: it adds the offset in
/// force, so give it the window the query covers with
/// [`within`](ZoneSql::within) and it switches offsets at each DST change
/// inside the window (a `CASE` on `julianday(col)`); MySQL then uses fixed
/// offsets too and needs no zone tables. Without a window, SQLite uses the
/// offset in force now.
///
/// ```
/// use navree_time::Zone;
/// use sea_orm::DbBackend;
/// use sea_orm::sea_query::{Expr, PostgresQueryBuilder, Query};
///
/// let zone = Zone::parse("Asia/Kolkata").unwrap();
/// let day = zone.sql(DbBackend::Postgres).local_date(Expr::col("created_at"));
/// let sql = Query::select().expr(day).to_string(PostgresQueryBuilder);
/// assert_eq!(sql, r#"SELECT CAST(("created_at" AT TIME ZONE 'Asia/Kolkata') AS DATE)"#);
/// ```
#[derive(Clone, Debug)]
pub struct ZoneSql {
    zone: Zone,
    backend: DbBackend,
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

impl Zone {
    /// SQL helpers for this zone on `backend`.
    pub fn sql(&self, backend: DbBackend) -> ZoneSql {
        ZoneSql {
            zone: *self,
            backend,
            window: None,
        }
    }
}

impl ZoneSql {
    /// The span of instants the query covers (a [`DayRange`](crate::DayRange)'s
    /// `utc_bounds`): SQLite and MySQL use the offsets in force within it.
    pub fn within(mut self, from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        self.window = Some((from, to.max(from)));
        self
    }

    /// The zone as a quoted SQL literal: `'Asia/Kolkata'`.
    pub fn literal(&self) -> String {
        format!("'{}'", self.zone.name().replace('\'', "''"))
    }

    /// `expr` (a UTC timestamp) as a local timestamp.
    pub fn local_timestamp(&self, expr: impl Into<Expr>) -> Expr {
        self.convert(expr.into(), "datetime", |sql| sql, |sql| sql)
    }

    /// `expr` (a UTC timestamp) as a local calendar date.
    pub fn local_date(&self, expr: impl Into<Expr>) -> Expr {
        self.convert(
            expr.into(),
            "date",
            |sql| format!("CAST({sql} AS DATE)"),
            |sql| format!("DATE({sql})"),
        )
    }

    /// The statement that makes the zone the session's (inside a
    /// transaction on PostgreSQL), or `None` on SQLite.
    pub fn set_local(&self) -> Option<String> {
        match self.backend {
            DbBackend::Postgres => Some(format!("SET LOCAL TIME ZONE {}", self.literal())),
            DbBackend::MySql => Some(format!("SET time_zone = {}", self.literal())),
            _ => None,
        }
    }

    fn convert(
        &self,
        expr: Expr,
        sqlite_function: &str,
        postgres: impl Fn(String) -> String,
        mysql: impl Fn(String) -> String,
    ) -> Expr {
        let template = match self.backend {
            DbBackend::Postgres => postgres(format!("({EXPR} AT TIME ZONE {})", self.literal())),
            DbBackend::MySql => match self.window {
                None => mysql(format!("CONVERT_TZ({EXPR}, '+00:00', {})", self.literal())),
                Some(_) => self.by_offset(|seconds| {
                    mysql(format!(
                        "CONVERT_TZ({EXPR}, '+00:00', '{}')",
                        crate::zone::offset_label(seconds)
                    ))
                }),
            },
            _ => self.by_offset(|seconds| {
                format!("{sqlite_function}({EXPR}, '{seconds:+} seconds')")
            }),
        };
        self.custom(&template, expr)
    }

    /// `apply(offset)` with each offset in force within the window: one
    /// expression, or a `CASE` switching at each transition.
    fn by_offset(&self, apply: impl Fn(i32) -> String) -> String {
        let (from, to) = self.window.unwrap_or_else(|| {
            let now = Utc::now();
            (now, now)
        });
        let periods = offsets_between(&self.zone, from, to);
        if periods.len() == 1 {
            return apply(periods[0].1);
        }
        let mut sql = String::from("CASE");
        for pair in periods.windows(2) {
            let (_, offset) = pair[0];
            let (change, _) = pair[1];
            sql.push_str(&format!(
                " WHEN julianday({EXPR}) < julianday('{}') THEN {}",
                change.format("%Y-%m-%d %H:%M:%S"),
                apply(offset)
            ));
        }
        let (_, last) = periods[periods.len() - 1];
        sql.push_str(&format!(" ELSE {} END", apply(last)));
        sql
    }

    /// `template` with `expr` at every [`EXPR`] mark, in the backend's
    /// placeholder syntax: `$1` on PostgreSQL, one positional `?` per mark
    /// elsewhere.
    fn custom(&self, template: &str, expr: Expr) -> Expr {
        if self.backend == DbBackend::Postgres {
            return Expr::cust_with_expr(template.replace(EXPR, "$1"), expr);
        }
        let marks = template.matches(EXPR).count();
        Expr::cust_with_exprs(
            template.replace(EXPR, "?"),
            std::iter::repeat_n(expr, marks),
        )
    }
}

/// Where a template puts the converted expression.
const EXPR: &str = "\u{1}";

/// The offsets (seconds east of UTC) in force from `from` to `to`, each
/// with the instant it starts (the first starts at `from`).
pub(crate) fn offsets_between(
    zone: &Zone,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, i32)> {
    let offset = |at: DateTime<Utc>| zone.offset_at(at).local_minus_utc();
    let mut periods = vec![(from, offset(from))];
    let step = TimeDelta::hours(6);
    let mut probe = from;
    while probe < to {
        let next = (probe + step).min(to);
        let current = periods[periods.len() - 1].1;
        if offset(next) != current {
            // The first whole second with the new offset.
            let at = |seconds: i64| DateTime::from_timestamp(seconds, 0).unwrap_or(next);
            let (mut low, mut high) = (probe.timestamp(), next.timestamp());
            while high - low > 1 {
                let middle = low + (high - low) / 2;
                if offset(at(middle)) == current {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            periods.push((at(high), offset(at(high))));
        }
        probe = next;
    }
    periods
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    #[test]
    fn offsets_change_at_the_exact_transition() {
        let london = Zone::parse("Europe/London").unwrap();
        let periods = offsets_between(
            &london,
            utc("2026-03-01T00:00:00Z"),
            utc("2026-11-01T00:00:00Z"),
        );
        assert_eq!(
            periods,
            [
                (utc("2026-03-01T00:00:00Z"), 0),
                (utc("2026-03-29T01:00:00Z"), 3600),
                (utc("2026-10-25T01:00:00Z"), 0),
            ]
        );
        let kolkata = Zone::parse("Asia/Kolkata").unwrap();
        assert_eq!(
            offsets_between(&kolkata, utc("2026-01-01T00:00:00Z"), utc("2027-01-01T00:00:00Z")).len(),
            1
        );
    }
}
