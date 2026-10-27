//! TimescaleDB's `time_bucket`, `first` and `last`.
//!
//! On PostgreSQL they compile to the TimescaleDB functions (the extension
//! must be installed). On SQLite and MySQL, `time_bucket` with a fixed
//! width (microseconds to weeks) is computed in SQL with TimescaleDB's
//! alignment, so dashboards built on buckets run in tests and development
//! too; month and year buckets, `first` and `last` need TimescaleDB.

use super::{Compiler, Dialect};
use crate::error::OrmError;
use crate::ir::Expr;
use crate::value::Value;

/// TimescaleDB's default bucket origin: 2000-01-03 00:00:00 UTC, a Monday,
/// as Unix seconds.
const ORIGIN_UNIX_SECONDS: i64 = 946_857_600;

/// TimescaleDB's default bucket origin as text (MySQL emulation).
const ORIGIN_TEXT: &str = "2000-01-03 00:00:00";

impl Compiler {
    pub(crate) fn time_bucket(
        &self,
        width: &str,
        expr: sea_query::Expr,
        origin: Option<sea_query::Expr>,
    ) -> Result<sea_query::Expr, OrmError> {
        match self.dialect {
            Dialect::Postgres => {
                let width = sea_query::Expr::val(Value::from(width.to_owned()));
                Ok(match origin {
                    None => self.custom("time_bucket(CAST({} AS INTERVAL), {})", vec![width, expr]),
                    Some(origin) => self.custom(
                        "time_bucket(CAST({} AS INTERVAL), {}, {})",
                        vec![width, expr, origin],
                    ),
                })
            }
            Dialect::Sqlite => {
                let micros = fixed_width_micros(width).ok_or_else(|| self.calendar_width(width))?;
                if micros % 1_000_000 != 0 {
                    return Err(self.unsupported(format!(
                        "a time bucket of `{width}` (narrower than whole seconds)"
                    )));
                }
                let seconds = micros / 1_000_000;
                let s = |e: sea_query::Expr| self.custom("CAST(strftime('%s', {}) AS INTEGER)", vec![e]);
                let (o1, o2, o3) = match origin {
                    None => {
                        let o = || sea_query::Expr::cust(ORIGIN_UNIX_SECONDS.to_string());
                        (o(), o(), o())
                    }
                    Some(origin) => (s(origin.clone()), s(origin.clone()), s(origin)),
                };
                // origin + (t - origin) - (((t - origin) % w) + w) % w: the
                // floor, also for times before the origin.
                Ok(self.custom(
                    &format!(
                        "datetime({{}} + ({{}} - {{}}) - (((({{}} - {{}}) % {seconds}) + {seconds}) % {seconds}), 'unixepoch')"
                    ),
                    vec![o1, s(expr.clone()), o2.clone(), s(expr), o3],
                ))
            }
            Dialect::MySql => {
                let micros = fixed_width_micros(width).ok_or_else(|| self.calendar_width(width))?;
                Ok(match origin {
                    None => self.custom(
                        &format!(
                            "TIMESTAMPADD(MICROSECOND, FLOOR(TIMESTAMPDIFF(MICROSECOND, '{ORIGIN_TEXT}', {{}}) / {micros}) * {micros}, '{ORIGIN_TEXT}')"
                        ),
                        vec![expr],
                    ),
                    Some(origin) => self.custom(
                        &format!(
                            "TIMESTAMPADD(MICROSECOND, FLOOR(TIMESTAMPDIFF(MICROSECOND, {{}}, {{}}) / {micros}) * {micros}, {{}})"
                        ),
                        vec![origin.clone(), expr, origin],
                    ),
                })
            }
        }
    }

    fn calendar_width(&self, width: &str) -> OrmError {
        self.unsupported(format!(
            "a time bucket of `{width}` (calendar or unreadable widths need TimescaleDB)"
        ))
    }

    pub(crate) fn first_last(
        &self,
        function: &'static str,
        value: &Expr,
        time: &Expr,
    ) -> Result<sea_query::Expr, OrmError> {
        if self.dialect != Dialect::Postgres {
            return Err(self.unsupported(format!(
                "{function}(value, time) (a TimescaleDB aggregate)"
            )));
        }
        let value = self.expr(value)?;
        let time = self.expr(time)?;
        Ok(self.custom(&format!("{function}({{}}, {{}})"), vec![value, time]))
    }
}

/// The width of a fixed interval in microseconds: `"15 minutes"`, `"1
/// hour 30 minutes"`, `"500ms"`, `"2 weeks"`. `None` for calendar units
/// (months, years) or text that is not an interval.
pub(crate) fn fixed_width_micros(width: &str) -> Option<i64> {
    let mut total: i64 = 0;
    let mut rest = width.trim();
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return None;
        }
        let amount: i64 = rest[..digits].parse().ok()?;
        rest = rest[digits..].trim_start();
        let unit_end = rest.find(|c: char| c.is_ascii_digit()).unwrap_or(rest.len());
        let unit = rest[..unit_end].trim().trim_end_matches(',').to_ascii_lowercase();
        rest = rest[unit_end..].trim_start();
        let micros: i64 = match unit.as_str() {
            "us" | "usec" | "usecs" | "microsecond" | "microseconds" => 1,
            "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => 1_000,
            "s" | "sec" | "secs" | "second" | "seconds" => 1_000_000,
            "m" | "min" | "mins" | "minute" | "minutes" => 60_000_000,
            "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000_000,
            "d" | "day" | "days" => 86_400_000_000,
            "w" | "week" | "weeks" => 604_800_000_000,
            _ => return None,
        };
        total = total.checked_add(amount.checked_mul(micros)?)?;
    }
    (total > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_widths_parse() {
        assert_eq!(fixed_width_micros("1 hour"), Some(3_600_000_000));
        assert_eq!(fixed_width_micros("15 minutes"), Some(900_000_000));
        assert_eq!(fixed_width_micros("1 hour 30 minutes"), Some(5_400_000_000));
        assert_eq!(fixed_width_micros("500ms"), Some(500_000));
        assert_eq!(fixed_width_micros("2 WEEKS"), Some(1_209_600_000_000));
        assert_eq!(fixed_width_micros("1 month"), None);
        assert_eq!(fixed_width_micros("soon"), None);
        assert_eq!(fixed_width_micros("0 days"), None);
        assert_eq!(fixed_width_micros(""), None);
    }

    #[test]
    fn the_default_origin_is_a_monday_at_midnight() {
        let origin = chrono::DateTime::from_timestamp(ORIGIN_UNIX_SECONDS, 0).unwrap();
        assert_eq!(origin.to_rfc3339(), "2000-01-03T00:00:00+00:00");
        assert_eq!(chrono::Datelike::weekday(&origin), chrono::Weekday::Mon);
        assert_eq!(ORIGIN_TEXT, "2000-01-03 00:00:00");
    }
}
