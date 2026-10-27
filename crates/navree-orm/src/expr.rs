//! Building [`Expr`]essions: columns, values, raw SQL, functions,
//! aggregates, arithmetic, subqueries and TimescaleDB's `time_bucket`,
//! `first` and `last`.
//!
//! ```
//! use navree_orm::{Dialect, Expr, TableBuilder, table};
//!
//! // Hourly temperature readings: bucket, first and last value per bucket.
//! let query = table("readings")
//!     .select_expr(Expr::time_bucket("1 hour", Expr::col("taken_at")), "bucket")
//!     .select_expr(Expr::first(Expr::col("celsius"), Expr::col("taken_at")), "opening")
//!     .select_expr(Expr::last(Expr::col("celsius"), Expr::col("taken_at")), "closing")
//!     .group_by("bucket")
//!     .order_by("bucket");
//! let sql = query.to_sql(Dialect::Postgres).unwrap();
//! assert_eq!(
//!     sql.sql,
//!     r#"SELECT time_bucket(CAST($1 AS INTERVAL), "taken_at") AS "bucket", first("celsius", "taken_at") AS "opening", last("celsius", "taken_at") AS "closing" FROM "readings" GROUP BY "bucket" ORDER BY "bucket" ASC"#
//! );
//! # let _: TableBuilder = query;
//! ```

use std::borrow::Cow;

use crate::column::{Filter, IntoQuery};
use crate::ir::{
    AggregateFunction, ArithmeticOp, ColumnRef, Expr, InValues, Operator, Order, Where,
};
use crate::value::Value;

impl<V: Into<Value>> From<V> for Expr {
    /// A value is a bound parameter.
    fn from(value: V) -> Self {
        Expr::Value(value.into())
    }
}

impl Expr {
    /// The column `name`: `"title"`, `"deals.title"`, or a JSON path
    /// `"meta->status"`.
    pub fn col(name: impl Into<Cow<'static, str>>) -> Expr {
        Expr::Column(ColumnRef::parse(name))
    }

    /// A bound value.
    pub fn val(value: impl Into<Value>) -> Expr {
        Expr::Value(value.into())
    }

    /// Raw SQL with `?` placeholders for `values` (`??` for a literal
    /// `?`). SQL databases only.
    pub fn raw(
        sql: impl Into<Cow<'static, str>>,
        values: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Expr {
        Expr::Raw {
            sql: sql.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// A call of the SQL function `name`.
    pub fn func(
        name: impl Into<Cow<'static, str>>,
        args: impl IntoIterator<Item = impl Into<Expr>>,
    ) -> Expr {
        Expr::Function {
            name: name.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// `lower(expr)`.
    pub fn lower(expr: impl Into<Expr>) -> Expr {
        Expr::func("lower", [expr.into()])
    }

    /// `upper(expr)`.
    pub fn upper(expr: impl Into<Expr>) -> Expr {
        Expr::func("upper", [expr.into()])
    }

    /// `coalesce(a, b, ...)`: the first non-NULL argument.
    pub fn coalesce(args: impl IntoIterator<Item = impl Into<Expr>>) -> Expr {
        Expr::func("coalesce", args)
    }

    fn aggregate(function: AggregateFunction, arg: Option<Expr>, distinct: bool) -> Expr {
        Expr::Aggregate {
            function,
            arg: arg.map(Box::new),
            distinct,
        }
    }

    /// `count(*)`.
    pub fn count_all() -> Expr {
        Expr::aggregate(AggregateFunction::Count, None, false)
    }

    /// `count(expr)`: the rows where `expr` is not NULL.
    pub fn count(expr: impl Into<Expr>) -> Expr {
        Expr::aggregate(AggregateFunction::Count, Some(expr.into()), false)
    }

    /// `count(DISTINCT expr)`.
    pub fn count_distinct(expr: impl Into<Expr>) -> Expr {
        Expr::aggregate(AggregateFunction::Count, Some(expr.into()), true)
    }

    /// `sum(expr)`.
    pub fn sum(expr: impl Into<Expr>) -> Expr {
        Expr::aggregate(AggregateFunction::Sum, Some(expr.into()), false)
    }

    /// `avg(expr)`.
    pub fn avg(expr: impl Into<Expr>) -> Expr {
        Expr::aggregate(AggregateFunction::Avg, Some(expr.into()), false)
    }

    /// `min(expr)`.
    pub fn min(expr: impl Into<Expr>) -> Expr {
        Expr::aggregate(AggregateFunction::Min, Some(expr.into()), false)
    }

    /// `max(expr)`.
    pub fn max(expr: impl Into<Expr>) -> Expr {
        Expr::aggregate(AggregateFunction::Max, Some(expr.into()), false)
    }

    /// A subquery returning one value.
    pub fn subquery(query: impl IntoQuery) -> Expr {
        Expr::Subquery(Box::new(query.into_query()))
    }

    /// TimescaleDB's `time_bucket(width, expr)`: the start of the bucket of
    /// width `width` (`"5 minutes"`, `"1 hour"`, `"1 day"`, `"1 week"`,
    /// `"1 month"`) that `expr` falls in.
    ///
    /// On SQLite and MySQL, fixed widths (microseconds to weeks) are
    /// computed with the same alignment as TimescaleDB (buckets start at
    /// 2000-01-03, a Monday, plus whole widths); month and year buckets
    /// need TimescaleDB.
    pub fn time_bucket(width: impl Into<String>, expr: impl Into<Expr>) -> Expr {
        Expr::TimeBucket {
            width: width.into(),
            expr: Box::new(expr.into()),
            origin: None,
        }
    }

    /// `time_bucket(width, expr, origin)`: buckets aligned to `origin`.
    pub fn time_bucket_from(
        width: impl Into<String>,
        expr: impl Into<Expr>,
        origin: impl Into<Expr>,
    ) -> Expr {
        Expr::TimeBucket {
            width: width.into(),
            expr: Box::new(expr.into()),
            origin: Some(Box::new(origin.into())),
        }
    }

    /// TimescaleDB's `first(value, time)`: in each group, `value` at the
    /// earliest `time`.
    pub fn first(value: impl Into<Expr>, time: impl Into<Expr>) -> Expr {
        Expr::First {
            value: Box::new(value.into()),
            time: Box::new(time.into()),
        }
    }

    /// TimescaleDB's `last(value, time)`: in each group, `value` at the
    /// latest `time`.
    pub fn last(value: impl Into<Expr>, time: impl Into<Expr>) -> Expr {
        Expr::Last {
            value: Box::new(value.into()),
            time: Box::new(time.into()),
        }
    }

    fn arithmetic(self, op: ArithmeticOp, other: impl Into<Expr>) -> Expr {
        Expr::Arithmetic {
            left: Box::new(self),
            op,
            right: Box::new(other.into()),
        }
    }

    /// `self + other`.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: impl Into<Expr>) -> Expr {
        self.arithmetic(ArithmeticOp::Add, other)
    }

    /// `self - other`.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, other: impl Into<Expr>) -> Expr {
        self.arithmetic(ArithmeticOp::Sub, other)
    }

    /// `self * other`.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, other: impl Into<Expr>) -> Expr {
        self.arithmetic(ArithmeticOp::Mul, other)
    }

    /// `self / other`.
    #[allow(clippy::should_implement_trait)]
    pub fn div(self, other: impl Into<Expr>) -> Expr {
        self.arithmetic(ArithmeticOp::Div, other)
    }

    /// `self = other` (`IS NULL` for a NULL value).
    pub fn eq(self, other: impl Into<Expr>) -> Filter {
        Filter::compare(self, Operator::Eq, other.into())
    }

    /// `self <> other` (`IS NOT NULL` for a NULL value).
    pub fn ne(self, other: impl Into<Expr>) -> Filter {
        Filter::compare(self, Operator::Ne, other.into())
    }

    /// `self > other`.
    pub fn gt(self, other: impl Into<Expr>) -> Filter {
        Filter::compare(self, Operator::Gt, other.into())
    }

    /// `self >= other`.
    pub fn gte(self, other: impl Into<Expr>) -> Filter {
        Filter::compare(self, Operator::Gte, other.into())
    }

    /// `self < other`.
    pub fn lt(self, other: impl Into<Expr>) -> Filter {
        Filter::compare(self, Operator::Lt, other.into())
    }

    /// `self <= other`.
    pub fn lte(self, other: impl Into<Expr>) -> Filter {
        Filter::compare(self, Operator::Lte, other.into())
    }

    /// `self <op> other`.
    pub fn op(self, op: impl Into<Operator>, other: impl Into<Expr>) -> Filter {
        Filter::compare(self, op.into(), other.into())
    }

    /// `self IN (values)`.
    pub fn is_in(self, values: impl IntoIterator<Item = impl Into<Expr>>) -> Filter {
        Filter(Where::In {
            expr: self,
            values: InValues::List(values.into_iter().map(Into::into).collect()),
            not: false,
        })
    }

    /// `self NOT IN (values)`.
    pub fn not_in(self, values: impl IntoIterator<Item = impl Into<Expr>>) -> Filter {
        Filter(Where::In {
            expr: self,
            values: InValues::List(values.into_iter().map(Into::into).collect()),
            not: true,
        })
    }

    /// `self IS NULL`.
    pub fn is_null(self) -> Filter {
        Filter(Where::Null {
            expr: self,
            not: false,
        })
    }

    /// `self IS NOT NULL`.
    pub fn is_not_null(self) -> Filter {
        Filter(Where::Null {
            expr: self,
            not: true,
        })
    }

    /// `self BETWEEN low AND high`.
    pub fn between(self, low: impl Into<Expr>, high: impl Into<Expr>) -> Filter {
        Filter(Where::Between {
            expr: self,
            low: low.into(),
            high: high.into(),
            not: false,
        })
    }

    /// `self NOT BETWEEN low AND high`.
    pub fn not_between(self, low: impl Into<Expr>, high: impl Into<Expr>) -> Filter {
        Filter(Where::Between {
            expr: self,
            low: low.into(),
            high: high.into(),
            not: true,
        })
    }

    /// `self LIKE pattern`, ignoring case on every database.
    pub fn like(self, pattern: impl Into<String>) -> Filter {
        Filter::like(self, pattern.into(), false, false)
    }

    /// `self NOT LIKE pattern`, ignoring case on every database.
    pub fn not_like(self, pattern: impl Into<String>) -> Filter {
        Filter::like(self, pattern.into(), false, true)
    }

    /// The expression in ascending order.
    pub fn asc(self) -> Order {
        Order::asc(self)
    }

    /// The expression in descending order.
    pub fn desc(self) -> Order {
        Order::desc(self)
    }
}
