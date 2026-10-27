//! Expressions ([`Expr`]) as MongoDB aggregation expressions, and
//! aggregates as `$group` accumulators.

use bson::{Bson, Document, doc};
use navree_orm::ir::{AggregateFunction, ArithmeticOp, Expr};
use navree_orm::OrmError;

use super::scope::{Cx, Scope};
use super::{Pending, subquery};
use crate::convert::to_bson;
use crate::error::{invalid, unsupported};

/// TimescaleDB's default bucket origin, 2000-01-03 00:00:00 UTC (a Monday),
/// in Unix milliseconds: fixed-width buckets start there plus whole widths,
/// on every database.
pub(crate) const BUCKET_ORIGIN_MS: i64 = 946_857_600_000;

/// `value` as a constant in an aggregation expression: wrapped in
/// `$literal` when MongoDB would otherwise read it as a field path (`"$x"`)
/// or an expression (a document or array).
pub(crate) fn literal(value: Bson) -> Bson {
    let needs_literal = match &value {
        Bson::String(text) => text.starts_with('$'),
        Bson::Document(_) | Bson::Array(_) => true,
        _ => false,
    };
    if needs_literal {
        Bson::Document(doc! { "$literal": value })
    } else {
        value
    }
}

/// `expr` as an aggregation expression over the documents of `scope`.
pub(crate) fn value(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    expr: &Expr,
) -> Result<Bson, OrmError> {
    Ok(match expr {
        Expr::Column(column) => scope.place(cx, column)?.expr(),
        Expr::Value(value) => literal(to_bson(value)?),
        Expr::Raw { .. } => {
            return Err(unsupported(
                "raw SQL expressions (write them with typed columns and functions)",
            ));
        }
        Expr::Function { name, args } => {
            let args = args
                .iter()
                .map(|arg| value(cx, scope, pending, arg))
                .collect::<Result<Vec<_>, _>>()?;
            function(name, args)?
        }
        Expr::Aggregate { function, .. } => {
            return Err(invalid(format!(
                "{}(..) is an aggregate: use it in a grouped select, `having`, or the builder's aggregate methods",
                function.sql_name().to_lowercase()
            )));
        }
        Expr::Arithmetic { left, op, right } => {
            let operator = match op {
                ArithmeticOp::Add => "$add",
                ArithmeticOp::Sub => "$subtract",
                ArithmeticOp::Mul => "$multiply",
                ArithmeticOp::Div => "$divide",
                other => return Err(unsupported(format!("the arithmetic operator {other:?}"))),
            };
            let left = value(cx, scope, pending, left)?;
            let right = value(cx, scope, pending, right)?;
            Bson::Document(doc! { operator: [left, right] })
        }
        Expr::Subquery(query) => subquery::scalar(cx, scope, pending, query)?,
        Expr::TimeBucket {
            width,
            expr,
            origin,
        } => {
            let time = value(cx, scope, pending, expr)?;
            let origin = origin
                .as_deref()
                .map(|origin| value(cx, scope, pending, origin))
                .transpose()?;
            time_bucket(width, time, origin)?
        }
        Expr::First { .. } | Expr::Last { .. } => {
            return Err(invalid(
                "first(value, time) and last(value, time) are aggregates: use them in a grouped select",
            ));
        }
        Expr::All { .. } => {
            return Err(invalid("`*` is a column list, not a value"));
        }
        other => return Err(unsupported(format!("the expression {other:?}"))),
    })
}

/// A call of the SQL function `name` as MongoDB's operator.
pub(crate) fn function(name: &str, args: Vec<Bson>) -> Result<Bson, OrmError> {
    let lower = name.to_ascii_lowercase();
    let one = |args: Vec<Bson>| -> Result<Bson, OrmError> {
        match <[Bson; 1]>::try_from(args) {
            Ok([arg]) => Ok(arg),
            Err(args) => Err(invalid(format!(
                "{name}() takes one argument, not {}",
                args.len()
            ))),
        }
    };
    Ok(match lower.as_str() {
        "lower" | "lcase" => Bson::Document(doc! { "$toLower": one(args)? }),
        "upper" | "ucase" => Bson::Document(doc! { "$toUpper": one(args)? }),
        "length" | "char_length" | "character_length" => {
            Bson::Document(doc! { "$strLenCP": one(args)? })
        }
        "trim" | "btrim" => Bson::Document(doc! { "$trim": { "input": one(args)? } }),
        "ltrim" => Bson::Document(doc! { "$ltrim": { "input": one(args)? } }),
        "rtrim" => Bson::Document(doc! { "$rtrim": { "input": one(args)? } }),
        "abs" => Bson::Document(doc! { "$abs": one(args)? }),
        "ceil" | "ceiling" => Bson::Document(doc! { "$ceil": one(args)? }),
        "floor" => Bson::Document(doc! { "$floor": one(args)? }),
        "sqrt" => Bson::Document(doc! { "$sqrt": one(args)? }),
        "round" => Bson::Document(doc! { "$round": args }),
        "power" | "pow" => Bson::Document(doc! { "$pow": args }),
        "mod" => Bson::Document(doc! { "$mod": args }),
        "concat" => Bson::Document(doc! { "$concat": args }),
        "coalesce" | "ifnull" => {
            if args.len() < 2 {
                return Err(invalid(format!("{name}() takes at least two arguments")));
            }
            Bson::Document(doc! { "$ifNull": args })
        }
        "nullif" => match <[Bson; 2]>::try_from(args) {
            Ok([a, b]) => Bson::Document(doc! {
                "$cond": [{ "$eq": [a.clone(), b] }, Bson::Null, a]
            }),
            Err(_) => return Err(invalid("nullif() takes two arguments")),
        },
        "greatest" => Bson::Document(doc! { "$max": args }),
        "least" => Bson::Document(doc! { "$min": args }),
        "replace" => match <[Bson; 3]>::try_from(args) {
            Ok([input, find, replacement]) => Bson::Document(doc! {
                "$replaceAll": { "input": input, "find": find, "replacement": replacement }
            }),
            Err(_) => return Err(invalid("replace() takes three arguments")),
        },
        "substr" | "substring" => {
            let mut args = args.into_iter();
            let (Some(text), Some(start)) = (args.next(), args.next()) else {
                return Err(invalid(format!("{name}() takes a text, a start and a length")));
            };
            // SQL counts from 1, MongoDB from 0.
            let start = Bson::Document(doc! { "$subtract": [start, 1] });
            let length = args.next().unwrap_or(Bson::Int64(i64::from(i32::MAX)));
            Bson::Document(doc! { "$substrCP": [text, start, length] })
        }
        "now" | "current_timestamp" | "localtimestamp" => Bson::String("$$NOW".into()),
        "date" => Bson::Document(doc! { "$dateTrunc": { "date": one(args)?, "unit": "day" } }),
        "year" => Bson::Document(doc! { "$year": one(args)? }),
        "month" => Bson::Document(doc! { "$month": one(args)? }),
        "day" | "dayofmonth" => Bson::Document(doc! { "$dayOfMonth": one(args)? }),
        "hour" => Bson::Document(doc! { "$hour": one(args)? }),
        "minute" => Bson::Document(doc! { "$minute": one(args)? }),
        _ => {
            return Err(unsupported(format!(
                "the SQL function `{name}` (MongoDB has lower, upper, length, trim, abs, ceil, floor, sqrt, round, power, mod, concat, coalesce, nullif, greatest, least, replace, substr, now, date, year, month, day, hour and minute)"
            )));
        }
    })
}

/// A bucket width: fixed milliseconds, or calendar months.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Width {
    Millis(i64),
    Months(i64),
}

/// Parses a bucket width: `"15 minutes"`, `"1 hour 30 minutes"`,
/// `"500ms"`, `"2 weeks"`, `"3 months"`, `"1 year"`.
fn width(text: &str) -> Option<Width> {
    let mut millis: i64 = 0;
    let mut months: i64 = 0;
    let mut rest = text.trim();
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
        let (ms, month_count): (i64, i64) = match unit.as_str() {
            "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => (1, 0),
            "s" | "sec" | "secs" | "second" | "seconds" => (1_000, 0),
            "m" | "min" | "mins" | "minute" | "minutes" => (60_000, 0),
            "h" | "hr" | "hrs" | "hour" | "hours" => (3_600_000, 0),
            "d" | "day" | "days" => (86_400_000, 0),
            "w" | "week" | "weeks" => (604_800_000, 0),
            "mon" | "mons" | "month" | "months" => (0, 1),
            "y" | "yr" | "yrs" | "year" | "years" => (0, 12),
            _ => return None,
        };
        millis = millis.checked_add(amount.checked_mul(ms)?)?;
        months = months.checked_add(amount.checked_mul(month_count)?)?;
    }
    match (millis, months) {
        (ms, 0) if ms > 0 => Some(Width::Millis(ms)),
        (0, months) if months > 0 => Some(Width::Months(months)),
        _ => None,
    }
}

/// TimescaleDB's `time_bucket(width, time[, origin])`: the start of the
/// bucket `time` falls in. Fixed widths are computed in milliseconds from
/// the origin (TimescaleDB's Monday 2000-01-03 by default), flooring for
/// times before it; month and year widths are `$dateTrunc` bins, which
/// start at 2000-01-01 like TimescaleDB's.
pub(crate) fn time_bucket(text: &str, time: Bson, origin: Option<Bson>) -> Result<Bson, OrmError> {
    match width(text) {
        Some(Width::Millis(width)) => {
            let origin = match origin {
                Some(origin) => Bson::Document(doc! { "$toLong": origin }),
                None => Bson::Int64(BUCKET_ORIGIN_MS),
            };
            let at = Bson::Document(doc! { "$toLong": time });
            let since = Bson::Document(doc! { "$subtract": [at.clone(), origin] });
            // ((t - origin) % w + w) % w: the offset into the bucket, also
            // before the origin.
            let offset = doc! {
                "$mod": [{ "$add": [{ "$mod": [since, width] }, width] }, width]
            };
            Ok(Bson::Document(doc! {
                "$toDate": { "$subtract": [at, offset] }
            }))
        }
        Some(Width::Months(months)) => {
            if origin.is_some() {
                return Err(unsupported(
                    "month and year time buckets from an origin (MongoDB's month bins start at 2000-01-01)",
                ));
            }
            let (unit, size) = if months % 12 == 0 {
                ("year", months / 12)
            } else {
                ("month", months)
            };
            Ok(Bson::Document(doc! {
                "$dateTrunc": { "date": time, "unit": unit, "binSize": size }
            }))
        }
        None => Err(invalid(format!(
            "`{text}` is not a time bucket width (write `15 minutes`, `1 hour`, `1 day`, `1 week`, `1 month`, `1 year`; at least a millisecond)"
        ))),
    }
}

/// A `$group` accumulator, and the expression that turns its field into
/// the aggregate's value afterwards (counting distinct values collects
/// them first).
#[derive(Debug)]
pub(crate) struct Accumulator {
    /// The accumulator: `{"$sum": 1}`, `{"$max": "$amount"}`, ...
    pub(crate) op: Bson,
    /// What the field becomes once grouped, if not itself.
    pub(crate) finish: Option<fn(&str) -> Bson>,
}

impl Accumulator {
    fn of(op: Document) -> Accumulator {
        Accumulator {
            op: Bson::Document(op),
            finish: None,
        }
    }

    /// The aggregate's value, from the group's field `field`.
    pub(crate) fn output(&self, field: &str) -> Bson {
        match self.finish {
            Some(finish) => finish(field),
            None => Bson::String(format!("${field}")),
        }
    }
}

/// `function(argument)` as an accumulator: `count(*)` counts documents,
/// `count(x)` those where `x` is not null, `distinct` collects the values
/// first (null left out, as SQL leaves NULL out).
pub(crate) fn accumulator(
    function: AggregateFunction,
    argument: Option<Bson>,
    distinct: bool,
) -> Result<Accumulator, OrmError> {
    let name = function.sql_name().to_lowercase();
    let Some(argument) = argument else {
        return match function {
            AggregateFunction::Count => Ok(Accumulator::of(doc! { "$sum": 1 })),
            _ => Err(invalid(format!("{name}(*) needs a column: {name}(column)"))),
        };
    };
    if distinct {
        let collected = doc! { "$addToSet": argument };
        let finish: fn(&str) -> Bson = match function {
            AggregateFunction::Count => |field| {
                Bson::Document(doc! { "$size": non_null(field) })
            },
            AggregateFunction::Sum => |field| Bson::Document(doc! { "$sum": non_null(field) }),
            AggregateFunction::Avg => |field| Bson::Document(doc! { "$avg": non_null(field) }),
            AggregateFunction::Min => |field| Bson::Document(doc! { "$min": non_null(field) }),
            AggregateFunction::Max => |field| Bson::Document(doc! { "$max": non_null(field) }),
            other => return Err(unsupported(format!("{other:?}(DISTINCT ..)"))),
        };
        return Ok(Accumulator {
            op: Bson::Document(collected),
            finish: Some(finish),
        });
    }
    Ok(Accumulator::of(match function {
        AggregateFunction::Count => doc! {
            "$sum": { "$cond": [{ "$eq": [{ "$ifNull": [argument, Bson::Null] }, Bson::Null] }, 0, 1] }
        },
        AggregateFunction::Sum => doc! { "$sum": argument },
        AggregateFunction::Avg => doc! { "$avg": argument },
        AggregateFunction::Min => doc! { "$min": argument },
        AggregateFunction::Max => doc! { "$max": argument },
        other => return Err(unsupported(format!("the aggregate {other:?}"))),
    }))
}

/// The values collected in `field`, without null.
fn non_null(field: &str) -> Bson {
    Bson::Document(doc! {
        "$filter": { "input": format!("${field}"), "cond": { "$ne": ["$$this", Bson::Null] } }
    })
}

/// TimescaleDB's `first(value, time)` / `last(value, time)` as `$top` /
/// `$bottom` over the group, sorted by `time` (a field).
pub(crate) fn first_last(first: bool, output: Bson, time_field: &str) -> Accumulator {
    let operator = if first { "$top" } else { "$bottom" };
    Accumulator::of(doc! {
        operator: { "sortBy": { time_field: 1 }, "output": output }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_parse_fixed_and_calendar_units() {
        assert_eq!(width("15 minutes"), Some(Width::Millis(900_000)));
        assert_eq!(width("1 hour 30 minutes"), Some(Width::Millis(5_400_000)));
        assert_eq!(width("500ms"), Some(Width::Millis(500)));
        assert_eq!(width("2 weeks"), Some(Width::Millis(1_209_600_000)));
        assert_eq!(width("3 months"), Some(Width::Months(3)));
        assert_eq!(width("1 year"), Some(Width::Months(12)));
        assert_eq!(width("1 month 2 days"), None, "calendar and fixed mixed");
        assert_eq!(width("0 days"), None);
        assert_eq!(width("soon"), None);
    }

    #[test]
    fn fixed_buckets_align_to_the_monday_origin() {
        let bucket = time_bucket("1 day", Bson::String("$at".into()), None).unwrap();
        assert_eq!(
            bucket,
            Bson::Document(doc! {
                "$toDate": { "$subtract": [
                    { "$toLong": "$at" },
                    { "$mod": [{ "$add": [{ "$mod": [{ "$subtract": [{ "$toLong": "$at" }, BUCKET_ORIGIN_MS] }, 86_400_000_i64] }, 86_400_000_i64] }, 86_400_000_i64] },
                ] }
            })
        );
        let monthly = time_bucket("3 months", Bson::String("$at".into()), None).unwrap();
        assert_eq!(
            monthly,
            Bson::Document(doc! { "$dateTrunc": { "date": "$at", "unit": "month", "binSize": 3_i64 } })
        );
        let yearly = time_bucket("2 years", Bson::String("$at".into()), None).unwrap();
        assert_eq!(
            yearly,
            Bson::Document(doc! { "$dateTrunc": { "date": "$at", "unit": "year", "binSize": 2_i64 } })
        );
        assert!(time_bucket("1 month", Bson::Null, Some(Bson::Null)).is_err());
        let error = time_bucket("fortnight", Bson::Null, None).unwrap_err();
        assert!(error.to_string().contains("not a time bucket width"), "{error}");
    }

    #[test]
    fn origin_is_monday_2000_01_03() {
        let origin = chrono::DateTime::from_timestamp_millis(BUCKET_ORIGIN_MS).unwrap();
        assert_eq!(origin.to_rfc3339(), "2000-01-03T00:00:00+00:00");
    }

    #[test]
    fn literals_protect_field_paths_and_documents() {
        assert_eq!(literal(Bson::Int32(3)), Bson::Int32(3));
        assert_eq!(literal(Bson::String("plain".into())), Bson::String("plain".into()));
        assert_eq!(
            literal(Bson::String("$price".into())),
            Bson::Document(doc! { "$literal": "$price" })
        );
        assert_eq!(
            literal(Bson::Document(doc! { "$gt": 1 })),
            Bson::Document(doc! { "$literal": { "$gt": 1 } })
        );
    }

    #[test]
    fn functions_map_to_operators() {
        let field = || Bson::String("$name".into());
        assert_eq!(function("LOWER", vec![field()]).unwrap(), Bson::Document(doc! { "$toLower": "$name" }));
        assert!(function("lower", vec![]).is_err());
        assert_eq!(
            function("substr", vec![field(), Bson::Int32(2), Bson::Int32(3)]).unwrap(),
            Bson::Document(doc! { "$substrCP": ["$name", { "$subtract": [2, 1] }, 3] })
        );
        let error = function("soundex", vec![field()]).unwrap_err();
        assert!(error.to_string().contains("`soundex`"), "{error}");
    }

    #[test]
    fn accumulators_follow_sql() {
        let count = accumulator(AggregateFunction::Count, None, false).unwrap();
        assert_eq!(count.op, Bson::Document(doc! { "$sum": 1 }));
        let distinct = accumulator(
            AggregateFunction::Count,
            Some(Bson::String("$owner".into())),
            true,
        )
        .unwrap();
        assert_eq!(distinct.op, Bson::Document(doc! { "$addToSet": "$owner" }));
        assert_eq!(
            distinct.output("n"),
            Bson::Document(doc! { "$size": { "$filter": { "input": "$n", "cond": { "$ne": ["$$this", Bson::Null] } } } })
        );
        assert!(accumulator(AggregateFunction::Sum, None, false).is_err());
    }
}
