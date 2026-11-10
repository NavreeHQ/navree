//! TimescaleDB's `time_bucket`, `first` and `last`.
//!
//! On PostgreSQL they are the TimescaleDB functions (the extension must be
//! installed). On SQLite and MySQL, `time_bucket` with a fixed width
//! (microseconds to weeks) is computed in SQL with TimescaleDB's alignment;
//! month and year buckets, `first` and `last` need TimescaleDB.

use std::borrow::Cow;

use super::Lowerer;
use crate::error::{OrmError, Result};
use crate::ir::Expr;
use crate::render::ast::{self, SqlType};
use crate::sql::Dialect;
use crate::value::Value;

/// TimescaleDB's default bucket origin: 2000-01-03 00:00:00 UTC, a Monday,
/// as Unix seconds.
const ORIGIN_UNIX_SECONDS: i64 = 946_857_600;

/// TimescaleDB's default bucket origin as text (MySQL's emulation casts it
/// to `DATETIME(6)`, so buckets are date-times).
const ORIGIN_TEXT: &str = "2000-01-03 00:00:00";

impl Lowerer {
    pub(crate) fn time_bucket(
        &self,
        width: &str,
        expr: ast::Expr,
        origin: Option<ast::Expr>,
    ) -> Result<ast::Expr> {
        match self.dialect {
            Dialect::Postgres => {
                let mut args = vec![
                    ast::Expr::Value(Value::from(width.to_owned()))
                        .cast(SqlType::Custom(Cow::Borrowed("INTERVAL"))),
                    expr,
                ];
                args.extend(origin);
                Ok(ast::Expr::func("time_bucket", args))
            }
            Dialect::Sqlite => {
                let micros = fixed_width_micros(width).ok_or_else(|| self.calendar_width(width))?;
                if micros % 1_000_000 != 0 {
                    return Err(self.unsupported(format!(
                        "a time bucket of `{width}` (narrower than whole seconds)"
                    )));
                }
                let seconds = ast::Expr::int(micros / 1_000_000);
                let unix = |e: ast::Expr| {
                    ast::Expr::func("strftime", vec![ast::Expr::string("%s"), e])
                        .cast(SqlType::Integer)
                };
                let origin = match origin {
                    None => ast::Expr::int(ORIGIN_UNIX_SECONDS),
                    Some(origin) => unix(origin),
                };
                // origin + (t - origin) - (((t - origin) % w) + w) % w: the
                // floor, also for times before the origin.
                let since = unix(expr).sub(origin.clone());
                let rest = since
                    .clone()
                    .binary(ast::BinaryOp::Mod, seconds.clone())
                    .add(seconds.clone())
                    .binary(ast::BinaryOp::Mod, seconds);
                Ok(ast::Expr::func(
                    "datetime",
                    vec![origin.add(since).sub(rest), ast::Expr::string("unixepoch")],
                ))
            }
            Dialect::MySql => {
                let micros = fixed_width_micros(width).ok_or_else(|| self.calendar_width(width))?;
                let origin = origin.unwrap_or_else(|| {
                    ast::Expr::string(ORIGIN_TEXT)
                        .cast(SqlType::Custom(Cow::Borrowed("DATETIME(6)")))
                });
                // `MICROSECOND` is a unit keyword, not an expression.
                Ok(ast::Expr::template(
                    "TIMESTAMPADD(MICROSECOND, FLOOR(TIMESTAMPDIFF(MICROSECOND, {}, {}) / {}) * {}, {})",
                    vec![
                        origin.clone(),
                        expr,
                        ast::Expr::int(micros),
                        ast::Expr::int(micros),
                        origin,
                    ],
                ))
            }
        }
    }

    fn calendar_width(&self, width: &str) -> OrmError {
        self.unsupported(format!(
            "a time bucket of `{width}` (calendar or unreadable widths need TimescaleDB)"
        ))
    }

    /// TimescaleDB's `first(value, time)` / `last(value, time)`.
    pub(crate) fn first_last(
        &self,
        function: &'static str,
        value: &Expr,
        time: &Expr,
    ) -> Result<ast::Expr> {
        if self.dialect != Dialect::Postgres {
            return Err(
                self.unsupported(format!("{function}(value, time) (a TimescaleDB aggregate)"))
            );
        }
        let value = self.expr(value)?;
        let time = self.expr(time)?;
        Ok(ast::Expr::func(function, vec![value, time]))
    }
}

/// The width of a fixed interval in microseconds: `"15 minutes"`, `"1
/// hour 30 minutes"`, `"500ms"`, `"2 weeks"`. `None` for calendar units
/// (months, years) or text that is not an interval.
fn fixed_width_micros(width: &str) -> Option<i64> {
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
        let unit_end = rest
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(rest.len());
        let unit = rest[..unit_end]
            .trim()
            .trim_end_matches(',')
            .to_ascii_lowercase();
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
        assert_eq!(fixed_width_micros("1 hour 30 minutes"), Some(5_400_000_000));
        assert_eq!(fixed_width_micros("500ms"), Some(500_000));
        assert_eq!(fixed_width_micros("1 month"), None);
        assert_eq!(fixed_width_micros("0 days"), None);
    }

    #[test]
    fn the_default_origin_is_a_monday_at_midnight() {
        let origin = chrono::DateTime::from_timestamp(ORIGIN_UNIX_SECONDS, 0).unwrap();
        assert_eq!(origin.to_rfc3339(), "2000-01-03T00:00:00+00:00");
    }
}
