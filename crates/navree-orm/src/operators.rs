//! Conditions beyond comparisons: pattern operators (`ILIKE`, regular
//! expressions, `SIMILAR TO`), JSON containment, overlap and key
//! existence on typed columns, null-safe inequality, hand-written
//! operators checked per database, and vector distances.
//!
//! | Method | PostgreSQL | MySQL | SQLite |
//! |---|---|---|---|
//! | [`Column::ilike`] | `ILIKE` | `LOWER(..) LIKE LOWER(..)` | `LIKE` (ASCII case) |
//! | [`Column::regex`] | `~` | `REGEXP_LIKE(.., 'c')` | `REGEXP` (a registered function) |
//! | [`Column::regex_case_insensitive`] | `~*` | `REGEXP_LIKE(.., 'i')` | `REGEXP` with `(?i)` |
//! | [`Column::similar_to`] | `SIMILAR TO` | unsupported | unsupported |
//! | [`Column::contains`] | `@>` | `json_contains` | `json_each` (scalars) |
//! | [`Column::contained_by`] | `<@` | `json_contains(value, column)` | unsupported |
//! | [`Column::overlaps`] | `@>` on each element | `json_overlaps` | `json_each` |
//! | [`Column::has_key`] | `?` | `json_contains_path` | `json_type(..) IS NOT NULL` |
//! | [`Column::is_distinct_from`] | `IS DISTINCT FROM` | `NOT (.. <=> ..)` | `IS NOT` |
//!
//! SQLite has no regular expressions of its own: `REGEXP` calls the
//! application's `regexp(pattern, text)` function (SQLx registers one with
//! `SqliteConnectOptions::with_regexp`); without it the statement fails.

use std::borrow::Cow;

use crate::column::{Column, Filter, IntoExpr};
use crate::ir::{ColumnRef, Expr, JsonSegment, Operator, VectorMetric, Where};
use crate::value::Value;

/// A JSON value for a condition, or the reason it cannot be one.
fn json_value(
    value: impl serde::Serialize,
    column: &ColumnRef,
) -> Result<serde_json::Value, Filter> {
    serde_json::to_value(value).map_err(|error| {
        Filter(Where::Invalid(format!(
            "the value for JSON column `{column}` cannot be encoded: {error}"
        )))
    })
}

impl<M, T> Column<M, T> {
    fn pattern(&self, op: Operator, pattern: impl Into<String>) -> Filter {
        Filter(Where::Compare {
            left: self.expr(),
            op,
            right: Expr::Value(Value::from(pattern.into())),
        })
    }

    /// `column ILIKE pattern`: a `LIKE` ignoring case on every database
    /// (`%` and `_` are wildcards).
    pub fn ilike(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), false, false)
    }

    /// `column NOT ILIKE pattern`.
    pub fn not_ilike(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), false, true)
    }

    /// The column matches regular expression `pattern`, matching case.
    ///
    /// ```
    /// use navree_orm::{Dialect, col};
    ///
    /// let sql = col("sku").regex("^[A-Z]{3}-[0-9]+$").to_sql(Dialect::MySql).unwrap();
    /// assert_eq!(sql.sql, "REGEXP_LIKE(`sku`, ?, 'c')");
    /// ```
    pub fn regex(&self, pattern: impl Into<String>) -> Filter {
        self.pattern(Operator::Regexp, pattern)
    }

    /// The column does not match regular expression `pattern`.
    pub fn not_regex(&self, pattern: impl Into<String>) -> Filter {
        self.pattern(Operator::NotRegexp, pattern)
    }

    /// The column matches regular expression `pattern`, ignoring case.
    pub fn regex_case_insensitive(&self, pattern: impl Into<String>) -> Filter {
        self.pattern(Operator::RegexpIgnoreCase, pattern)
    }

    /// The column does not match regular expression `pattern`, ignoring
    /// case.
    pub fn not_regex_case_insensitive(&self, pattern: impl Into<String>) -> Filter {
        self.pattern(Operator::NotRegexpIgnoreCase, pattern)
    }

    /// PostgreSQL's `column SIMILAR TO pattern` (SQL regular expressions:
    /// `%`, `_`, `|`, `*`, `+`, groups).
    pub fn similar_to(&self, pattern: impl Into<String>) -> Filter {
        self.pattern(Operator::Postgres("similar to"), pattern)
    }

    /// PostgreSQL's `column NOT SIMILAR TO pattern`.
    pub fn not_similar_to(&self, pattern: impl Into<String>) -> Filter {
        self.pattern(Operator::Postgres("not similar to"), pattern)
    }

    /// The JSON column (or path) contains `value`: an element of an array,
    /// or (PostgreSQL and MySQL) an object's keys and values. The same as
    /// `where_json_contains`.
    pub fn contains(&self, value: impl serde::Serialize) -> Filter {
        let column = self.column_ref();
        match json_value(value, &column) {
            Ok(value) => Filter(Where::JsonContains {
                column,
                value,
                not: false,
            }),
            Err(invalid) => invalid,
        }
    }

    /// The JSON column (or path) is contained in `value` (`<@`; PostgreSQL
    /// and MySQL).
    ///
    /// ```
    /// use navree_orm::{Dialect, col};
    ///
    /// let sql = col("roles").contained_by(["admin", "manager"]).to_sql(Dialect::Postgres).unwrap();
    /// assert_eq!(sql.sql, r#"("roles")::jsonb <@ $1"#);
    /// ```
    pub fn contained_by(&self, value: impl serde::Serialize) -> Filter {
        let column = self.column_ref();
        match json_value(value, &column) {
            Ok(value) => Filter(Where::JsonContainedBy {
                column,
                value,
                not: false,
            }),
            Err(invalid) => invalid,
        }
    }

    /// The JSON array column (or path) shares an element with `values` (an
    /// array; a scalar stands for a one-element array).
    pub fn overlaps(&self, values: impl serde::Serialize) -> Filter {
        let column = self.column_ref();
        match json_value(values, &column) {
            Ok(value) => Filter(Where::JsonOverlaps {
                column,
                value,
                not: false,
            }),
            Err(invalid) => invalid,
        }
    }

    /// The JSON object column (or path) has key `key`.
    ///
    /// ```
    /// use navree_orm::{Dialect, col};
    ///
    /// let sql = col("meta->owner").has_key("email").to_sql(Dialect::Sqlite).unwrap();
    /// assert_eq!(sql.sql, r#"json_type("meta", '$."owner"."email"') IS NOT NULL"#);
    /// ```
    pub fn has_key(&self, key: impl Into<String>) -> Filter {
        let mut column = self.column_ref();
        column.path.push(JsonSegment::Key(key.into()));
        Filter(Where::JsonContainsKey { column, not: false })
    }

    /// Null-safe inequality: true when the values differ, also when
    /// exactly one is NULL (`IS DISTINCT FROM`).
    pub fn is_distinct_from(&self, value: impl crate::value::ValueFor<T>) -> Filter {
        match self.encode(&value.into_value_for()) {
            Ok(value) => Filter(Where::Compare {
                left: self.expr(),
                op: Operator::NullSafeNe,
                right: Expr::Value(value),
            }),
            Err(error) => Filter(Where::Invalid(format!(
                "cannot use a value for column `{}`: {error}",
                self.name()
            ))),
        }
    }

    /// The distance between this vector column and `vector` (cosine; see
    /// [`Expr::vector_distance`]).
    pub fn vector_distance(&self, vector: &[f32]) -> Expr {
        Expr::vector_distance(self.expr(), vector)
    }
}

impl Filter {
    /// `left op right` with an operator written as text, checked against
    /// the database's list of condition operators when the query compiles
    /// (PostgreSQL's JSON, array, range and text search operators such as
    /// `@>`, `?|`, `&&`, `@@`; MySQL's `SOUNDS LIKE`; SQLite's `GLOB` and
    /// `MATCH`; and the comparisons). An operator the database does not
    /// take is an [`OrmError::InvalidQuery`](crate::OrmError::InvalidQuery)
    /// naming the ones it does.
    ///
    /// ```
    /// use navree_orm::{Dialect, Expr, Filter, col};
    ///
    /// let any_tag = Filter::op_raw(col("tags"), "?|", Expr::raw("ARRAY[?, ?]", ["vip", "gold"]));
    /// let sql = any_tag.to_sql(Dialect::Postgres).unwrap();
    /// assert_eq!(sql.sql, r#""tags" ?| ARRAY[$1, $2]"#);
    /// assert!(any_tag.to_sql(Dialect::MySql).unwrap_err().to_string().contains("is not an operator MySQL conditions take"));
    /// ```
    pub fn op_raw(left: impl IntoExpr, op: impl Into<String>, right: impl Into<Expr>) -> Filter {
        Filter(Where::Compare {
            left: left.into_expr(),
            op: Operator::Raw(op.into()),
            right: right.into(),
        })
    }
}

impl Expr {
    /// `self op other` with an operator written as text (`||`, `-`, `#-`,
    /// `->`, ...), checked against the database's list of value operators
    /// when the query compiles.
    ///
    /// ```
    /// use navree_orm::{Dialect, Expr, table};
    ///
    /// // The meta object without its `secret` key.
    /// let sql = table("deals")
    ///     .select_expr(Expr::col("meta").binary("-", Expr::val("secret")), "meta")
    ///     .to_sql(Dialect::Postgres)
    ///     .unwrap();
    /// assert_eq!(sql.sql, r#"SELECT ("meta" - $1) AS "meta" FROM "deals""#);
    /// ```
    pub fn binary(self, op: impl Into<Cow<'static, str>>, other: impl Into<Expr>) -> Expr {
        Expr::Binary {
            left: Box::new(self),
            op: op.into(),
            right: Box::new(other.into()),
        }
    }

    /// The cosine distance between the vector `expr` (an embedding column)
    /// and `vector`: 0 for the same direction, 1 for unrelated, 2 for
    /// opposite. pgvector's `<=>` on PostgreSQL, MariaDB's
    /// `VEC_DISTANCE_COSINE`.
    ///
    /// ```
    /// use navree_orm::{Dialect, Expr};
    ///
    /// let distance = Expr::vector_distance(Expr::col("embedding"), &[0.5, 1.0]);
    /// let sql = distance.lt(0.3).to_sql(Dialect::Postgres).unwrap();
    /// assert_eq!(sql.sql, r#"("embedding" <=> CAST($1 AS vector)) < $2"#);
    /// ```
    pub fn vector_distance(expr: impl Into<Expr>, vector: &[f32]) -> Expr {
        Expr::vector_distance_by(expr, vector, VectorMetric::Cosine)
    }

    /// The distance between the vector `expr` and `vector`, measured by
    /// `metric`.
    pub fn vector_distance_by(expr: impl Into<Expr>, vector: &[f32], metric: VectorMetric) -> Expr {
        Expr::VectorDistance {
            expr: Box::new(expr.into()),
            vector: vector.to_vec(),
            metric,
        }
    }
}
