//! The where family of builder methods, and [`WhereGroup`].
//!
//! The methods are stamped onto every builder ([`Builder`](crate::Builder),
//! [`TableBuilder`](crate::TableBuilder), [`WhereGroup`],
//! [`JoinClause`](crate::JoinClause)) by the `filters!` macro, so builders
//! need no trait imports. Each type gives the macro its clause list through
//! [`HasWheres`].

use std::borrow::Cow;

use crate::column::{ColumnOf, Filter, IntoExpr, IntoQuery};
use crate::ir::{
    Boolean, ColumnRef, DatePart, Expr, FullText, InValues, Operator, TableRef, Where, WhereClause,
};
use crate::value::{Value, ValueFor};

/// A builder with a list of where clauses (implemented by every builder).
pub trait HasWheres {
    /// The clause list.
    fn wheres_mut(&mut self) -> &mut Vec<WhereClause>;

    /// Appends a clause.
    fn push_where(&mut self, boolean: Boolean, negated: bool, filter: Where) {
        self.wheres_mut().push(WhereClause {
            boolean,
            negated,
            filter,
        });
    }
}

/// Stamps the where-family methods onto a builder implementing
/// [`HasWheres`].
macro_rules! filters {
    () => {
        /// Adds a condition (`AND`).
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// let big_wins = Deal::query()
        ///     .where_(Deal::STAGE.eq(Stage::Won))
        ///     .where_(Deal::AMOUNT_CENTS.gte(10_000));
        /// # let _ = big_wins;
        /// ```
        pub fn where_(mut self, filter: impl Into<$crate::Filter>) -> Self {
            $crate::query::filters::HasWheres::push_where(
                &mut self,
                $crate::ir::Boolean::And,
                false,
                filter.into().into_where(),
            );
            self
        }

        /// Adds a condition joined with `OR`.
        pub fn or_where(mut self, filter: impl Into<$crate::Filter>) -> Self {
            $crate::query::filters::HasWheres::push_where(
                &mut self,
                $crate::ir::Boolean::Or,
                false,
                filter.into().into_where(),
            );
            self
        }

        /// Adds `AND NOT (condition)`.
        pub fn where_not(mut self, filter: impl Into<$crate::Filter>) -> Self {
            $crate::query::filters::HasWheres::push_where(
                &mut self,
                $crate::ir::Boolean::And,
                true,
                filter.into().into_where(),
            );
            self
        }

        /// Adds `OR NOT (condition)`.
        pub fn or_where_not(mut self, filter: impl Into<$crate::Filter>) -> Self {
            $crate::query::filters::HasWheres::push_where(
                &mut self,
                $crate::ir::Boolean::Or,
                true,
                filter.into().into_where(),
            );
            self
        }

        /// Adds a parenthesised group built in `group` (`AND (...)`). An
        /// empty group adds nothing.
        pub fn where_group(
            self,
            group: impl FnOnce($crate::WhereGroup) -> $crate::WhereGroup,
        ) -> Self {
            self.push_group($crate::ir::Boolean::And, false, group)
        }

        /// Adds a parenthesised group joined with `OR`.
        pub fn or_where_group(
            self,
            group: impl FnOnce($crate::WhereGroup) -> $crate::WhereGroup,
        ) -> Self {
            self.push_group($crate::ir::Boolean::Or, false, group)
        }

        /// Adds `AND NOT (group)`.
        pub fn where_not_group(
            self,
            group: impl FnOnce($crate::WhereGroup) -> $crate::WhereGroup,
        ) -> Self {
            self.push_group($crate::ir::Boolean::And, true, group)
        }

        fn push_group(
            mut self,
            boolean: $crate::ir::Boolean,
            negated: bool,
            group: impl FnOnce($crate::WhereGroup) -> $crate::WhereGroup,
        ) -> Self {
            let clauses = group($crate::WhereGroup::new()).into_clauses();
            if !clauses.is_empty() {
                $crate::query::filters::HasWheres::push_where(
                    &mut self,
                    boolean,
                    negated,
                    $crate::ir::Where::Group(clauses),
                );
            }
            self
        }

        /// Adds `column IN (values)`; an empty list matches no row.
        pub fn where_in<T>(
            self,
            column: impl $crate::ColumnOf<T>,
            values: impl IntoIterator<Item = impl $crate::ValueFor<T>>,
        ) -> Self {
            let filter = $crate::query::filters::in_list(&column, values, false);
            self.where_(filter)
        }

        /// Adds `OR column IN (values)`.
        pub fn or_where_in<T>(
            self,
            column: impl $crate::ColumnOf<T>,
            values: impl IntoIterator<Item = impl $crate::ValueFor<T>>,
        ) -> Self {
            let filter = $crate::query::filters::in_list(&column, values, false);
            self.or_where(filter)
        }

        /// Adds `column NOT IN (values)`; an empty list matches every row.
        pub fn where_not_in<T>(
            self,
            column: impl $crate::ColumnOf<T>,
            values: impl IntoIterator<Item = impl $crate::ValueFor<T>>,
        ) -> Self {
            let filter = $crate::query::filters::in_list(&column, values, true);
            self.where_(filter)
        }

        /// Adds `OR column NOT IN (values)`.
        pub fn or_where_not_in<T>(
            self,
            column: impl $crate::ColumnOf<T>,
            values: impl IntoIterator<Item = impl $crate::ValueFor<T>>,
        ) -> Self {
            let filter = $crate::query::filters::in_list(&column, values, true);
            self.or_where(filter)
        }

        /// Adds `column IN (subquery)`.
        pub fn where_in_sub(
            self,
            column: impl $crate::IntoExpr,
            query: impl $crate::IntoQuery,
        ) -> Self {
            self.where_($crate::query::filters::in_sub(column, query, false))
        }

        /// Adds `column NOT IN (subquery)`.
        pub fn where_not_in_sub(
            self,
            column: impl $crate::IntoExpr,
            query: impl $crate::IntoQuery,
        ) -> Self {
            self.where_($crate::query::filters::in_sub(column, query, true))
        }

        /// Adds `column IS NULL`.
        pub fn where_null(self, column: impl $crate::IntoExpr) -> Self {
            self.where_($crate::query::filters::null(column, false))
        }

        /// Adds `OR column IS NULL`.
        pub fn or_where_null(self, column: impl $crate::IntoExpr) -> Self {
            self.or_where($crate::query::filters::null(column, false))
        }

        /// Adds `column IS NOT NULL`.
        pub fn where_not_null(self, column: impl $crate::IntoExpr) -> Self {
            self.where_($crate::query::filters::null(column, true))
        }

        /// Adds `OR column IS NOT NULL`.
        pub fn or_where_not_null(self, column: impl $crate::IntoExpr) -> Self {
            self.or_where($crate::query::filters::null(column, true))
        }

        /// Adds `column BETWEEN low AND high` (both included).
        pub fn where_between<T>(
            self,
            column: impl $crate::ColumnOf<T>,
            low: impl $crate::ValueFor<T>,
            high: impl $crate::ValueFor<T>,
        ) -> Self {
            let filter = $crate::query::filters::between(
                &column,
                $crate::ValueFor::into_value_for(low),
                $crate::ValueFor::into_value_for(high),
                false,
            );
            self.where_(filter)
        }

        /// Adds `OR column BETWEEN low AND high`.
        pub fn or_where_between<T>(
            self,
            column: impl $crate::ColumnOf<T>,
            low: impl $crate::ValueFor<T>,
            high: impl $crate::ValueFor<T>,
        ) -> Self {
            let filter = $crate::query::filters::between(
                &column,
                $crate::ValueFor::into_value_for(low),
                $crate::ValueFor::into_value_for(high),
                false,
            );
            self.or_where(filter)
        }

        /// Adds `column NOT BETWEEN low AND high`.
        pub fn where_not_between<T>(
            self,
            column: impl $crate::ColumnOf<T>,
            low: impl $crate::ValueFor<T>,
            high: impl $crate::ValueFor<T>,
        ) -> Self {
            let filter = $crate::query::filters::between(
                &column,
                $crate::ValueFor::into_value_for(low),
                $crate::ValueFor::into_value_for(high),
                true,
            );
            self.where_(filter)
        }

        /// Adds `OR column NOT BETWEEN low AND high`.
        pub fn or_where_not_between<T>(
            self,
            column: impl $crate::ColumnOf<T>,
            low: impl $crate::ValueFor<T>,
            high: impl $crate::ValueFor<T>,
        ) -> Self {
            let filter = $crate::query::filters::between(
                &column,
                $crate::ValueFor::into_value_for(low),
                $crate::ValueFor::into_value_for(high),
                true,
            );
            self.or_where(filter)
        }

        /// Adds `first op second` comparing two columns:
        /// `.where_column(Deal::UPDATED_AT, ">", Deal::CREATED_AT)`.
        pub fn where_column(
            self,
            first: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            second: impl $crate::IntoExpr,
        ) -> Self {
            self.where_($crate::query::filters::columns(first, op, second))
        }

        /// Adds `OR first op second` comparing two columns.
        pub fn or_where_column(
            self,
            first: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            second: impl $crate::IntoExpr,
        ) -> Self {
            self.or_where($crate::query::filters::columns(first, op, second))
        }

        /// Adds `column op (subquery)`: compares with the single value a
        /// subquery returns.
        pub fn where_sub(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            query: impl $crate::IntoQuery,
        ) -> Self {
            self.where_($crate::Filter::from($crate::ir::Where::Compare {
                left: $crate::IntoExpr::into_expr(column),
                op: op.into(),
                right: $crate::Expr::subquery(query),
            }))
        }

        /// Adds a comparison of the date part of `column` (`YYYY-MM-DD`)
        /// with `value` (a `NaiveDate`, or text).
        pub fn where_date(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.where_($crate::query::filters::date_part(
                $crate::ir::DatePart::Date,
                column,
                op,
                value,
            ))
        }

        /// Adds `OR` a comparison of the date part of `column`.
        pub fn or_where_date(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.or_where($crate::query::filters::date_part(
                $crate::ir::DatePart::Date,
                column,
                op,
                value,
            ))
        }

        /// Adds a comparison of the time of day of `column` (`HH:MM:SS`).
        pub fn where_time(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.where_($crate::query::filters::date_part(
                $crate::ir::DatePart::Time,
                column,
                op,
                value,
            ))
        }

        /// Adds `OR` a comparison of the time of day of `column`.
        pub fn or_where_time(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.or_where($crate::query::filters::date_part(
                $crate::ir::DatePart::Time,
                column,
                op,
                value,
            ))
        }

        /// Adds a comparison of the day of the month of `column`.
        pub fn where_day(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.where_($crate::query::filters::date_part(
                $crate::ir::DatePart::Day,
                column,
                op,
                value,
            ))
        }

        /// Adds `OR` a comparison of the day of the month of `column`.
        pub fn or_where_day(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.or_where($crate::query::filters::date_part(
                $crate::ir::DatePart::Day,
                column,
                op,
                value,
            ))
        }

        /// Adds a comparison of the month (1 to 12) of `column`.
        pub fn where_month(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.where_($crate::query::filters::date_part(
                $crate::ir::DatePart::Month,
                column,
                op,
                value,
            ))
        }

        /// Adds `OR` a comparison of the month of `column`.
        pub fn or_where_month(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.or_where($crate::query::filters::date_part(
                $crate::ir::DatePart::Month,
                column,
                op,
                value,
            ))
        }

        /// Adds a comparison of the year of `column`.
        pub fn where_year(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.where_($crate::query::filters::date_part(
                $crate::ir::DatePart::Year,
                column,
                op,
                value,
            ))
        }

        /// Adds `OR` a comparison of the year of `column`.
        pub fn or_where_year(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            value: impl Into<$crate::Value>,
        ) -> Self {
            self.or_where($crate::query::filters::date_part(
                $crate::ir::DatePart::Year,
                column,
                op,
                value,
            ))
        }

        /// Adds `column LIKE pattern`, ignoring case on every database
        /// (`%` and `_` are wildcards).
        pub fn where_like(self, column: impl $crate::IntoExpr, pattern: impl Into<String>) -> Self {
            self.where_($crate::query::filters::like(column, pattern, false, false))
        }

        /// Adds `OR column LIKE pattern`, ignoring case.
        pub fn or_where_like(
            self,
            column: impl $crate::IntoExpr,
            pattern: impl Into<String>,
        ) -> Self {
            self.or_where($crate::query::filters::like(column, pattern, false, false))
        }

        /// Adds `column NOT LIKE pattern`, ignoring case.
        pub fn where_not_like(
            self,
            column: impl $crate::IntoExpr,
            pattern: impl Into<String>,
        ) -> Self {
            self.where_($crate::query::filters::like(column, pattern, false, true))
        }

        /// Adds `OR column NOT LIKE pattern`, ignoring case.
        pub fn or_where_not_like(
            self,
            column: impl $crate::IntoExpr,
            pattern: impl Into<String>,
        ) -> Self {
            self.or_where($crate::query::filters::like(column, pattern, false, true))
        }

        /// Adds `column LIKE pattern`, matching case on every database.
        pub fn where_like_case_sensitive(
            self,
            column: impl $crate::IntoExpr,
            pattern: impl Into<String>,
        ) -> Self {
            self.where_($crate::query::filters::like(column, pattern, true, false))
        }

        /// Adds: the JSON column (or path, `"meta->tags"`) contains `value`
        /// (an element of an array; on PostgreSQL and MySQL also an
        /// object's keys and values).
        pub fn where_json_contains(
            self,
            column: impl $crate::IntoExpr,
            value: impl serde::Serialize,
        ) -> Self {
            self.where_($crate::query::filters::json_contains(column, value, false))
        }

        /// Adds `OR` the JSON column contains `value`.
        pub fn or_where_json_contains(
            self,
            column: impl $crate::IntoExpr,
            value: impl serde::Serialize,
        ) -> Self {
            self.or_where($crate::query::filters::json_contains(column, value, false))
        }

        /// Adds: the JSON column (or path) does not contain `value`.
        pub fn where_json_doesnt_contain(
            self,
            column: impl $crate::IntoExpr,
            value: impl serde::Serialize,
        ) -> Self {
            self.where_($crate::query::filters::json_contains(column, value, true))
        }

        /// Adds `OR` the JSON column (or path) does not contain `value`.
        pub fn or_where_json_doesnt_contain(
            self,
            column: impl $crate::IntoExpr,
            value: impl serde::Serialize,
        ) -> Self {
            self.or_where($crate::query::filters::json_contains(column, value, true))
        }

        /// Adds: the JSON array at the column (or path) shares an element
        /// with `values` (an array; a scalar stands for a one-element
        /// array). `json_overlaps` on MySQL; element containment on
        /// PostgreSQL; `json_each` on SQLite.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// let deals = Deal::query().where_json_overlaps("meta->labels", ["vip", "renewal"]);
        /// # let _ = deals;
        /// ```
        pub fn where_json_overlaps(
            self,
            column: impl $crate::IntoExpr,
            values: impl serde::Serialize,
        ) -> Self {
            self.where_($crate::query::filters::json_overlaps(column, values, false))
        }

        /// Adds `OR` the JSON array at the column shares an element with
        /// `values`.
        pub fn or_where_json_overlaps(
            self,
            column: impl $crate::IntoExpr,
            values: impl serde::Serialize,
        ) -> Self {
            self.or_where($crate::query::filters::json_overlaps(column, values, false))
        }

        /// Adds: the JSON array at the column shares no element with
        /// `values`.
        pub fn where_json_doesnt_overlap(
            self,
            column: impl $crate::IntoExpr,
            values: impl serde::Serialize,
        ) -> Self {
            self.where_($crate::query::filters::json_overlaps(column, values, true))
        }

        /// Adds `OR` the JSON array at the column shares no element with
        /// `values`.
        pub fn or_where_json_doesnt_overlap(
            self,
            column: impl $crate::IntoExpr,
            values: impl serde::Serialize,
        ) -> Self {
            self.or_where($crate::query::filters::json_overlaps(column, values, true))
        }

        /// Adds: the JSON column has the object key (or array index) its
        /// path ends with: `where_json_contains_key("meta->owner")`,
        /// `where_json_contains_key("meta->tags[1]")`.
        ///
        /// ```
        /// use navree_orm::{Dialect, table};
        ///
        /// let sql = table("deals").where_json_contains_key("meta->owner").to_sql(Dialect::Postgres).unwrap();
        /// assert_eq!(sql.sql, r#"SELECT * FROM "deals" WHERE COALESCE(("meta")::jsonb ? $1, false)"#);
        /// ```
        pub fn where_json_contains_key(self, column: impl $crate::IntoExpr) -> Self {
            self.where_($crate::query::filters::json_contains_key(column, false))
        }

        /// Adds `OR` the JSON column has the key its path ends with.
        pub fn or_where_json_contains_key(self, column: impl $crate::IntoExpr) -> Self {
            self.or_where($crate::query::filters::json_contains_key(column, false))
        }

        /// Adds: the JSON column does not have the key its path ends with.
        pub fn where_json_doesnt_contain_key(self, column: impl $crate::IntoExpr) -> Self {
            self.where_($crate::query::filters::json_contains_key(column, true))
        }

        /// Adds `OR` the JSON column does not have the key its path ends
        /// with.
        pub fn or_where_json_doesnt_contain_key(self, column: impl $crate::IntoExpr) -> Self {
            self.or_where($crate::query::filters::json_contains_key(column, true))
        }

        /// Adds: the vector column (an embedding) is within cosine distance
        /// `max_distance` of `vector` (pgvector on PostgreSQL, MariaDB's
        /// vector functions; unsupported on SQLite).
        pub fn where_vector_distance_less_than(
            self,
            column: impl $crate::IntoExpr,
            vector: &[f32],
            max_distance: f64,
        ) -> Self {
            self.where_($crate::query::filters::vector_within(column, vector, max_distance))
        }

        /// Adds `OR` the vector column is within cosine distance
        /// `max_distance` of `vector`.
        pub fn or_where_vector_distance_less_than(
            self,
            column: impl $crate::IntoExpr,
            vector: &[f32],
            max_distance: f64,
        ) -> Self {
            self.or_where($crate::query::filters::vector_within(column, vector, max_distance))
        }

        /// Adds a comparison of the length of the JSON array at the column
        /// (or path).
        pub fn where_json_length(
            self,
            column: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            length: i64,
        ) -> Self {
            self.where_($crate::query::filters::json_length(column, op, length))
        }

        /// Adds `EXISTS (query)`.
        pub fn where_exists(self, query: impl $crate::IntoQuery) -> Self {
            self.where_($crate::Filter::exists(query))
        }

        /// Adds `OR EXISTS (query)`.
        pub fn or_where_exists(self, query: impl $crate::IntoQuery) -> Self {
            self.or_where($crate::Filter::exists(query))
        }

        /// Adds `NOT EXISTS (query)`.
        pub fn where_not_exists(self, query: impl $crate::IntoQuery) -> Self {
            self.where_($crate::Filter::not_exists(query))
        }

        /// Adds `OR NOT EXISTS (query)`.
        pub fn or_where_not_exists(self, query: impl $crate::IntoQuery) -> Self {
            self.or_where($crate::Filter::not_exists(query))
        }

        /// Adds a full-text match of `query` against `columns`: on
        /// PostgreSQL `to_tsvector(...) @@ plainto_tsquery(...)` (English
        /// by default), on MySQL `MATCH (...) AGAINST (...)` over a
        /// `FULLTEXT` index. SQLite has no full-text conditions.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// let deals = Deal::query().where_full_text([Deal::TITLE.expr(), Deal::NOTES.expr()], "renewal pricing");
        /// # let _ = deals;
        /// ```
        pub fn where_full_text(
            self,
            columns: impl IntoIterator<Item = impl $crate::IntoExpr>,
            query: impl Into<String>,
        ) -> Self {
            self.where_($crate::query::filters::full_text(
                columns,
                query,
                $crate::ir::FullText::default(),
            ))
        }

        /// Adds a full-text match with options: the mode (`phrase`,
        /// `websearch`, `boolean`), PostgreSQL's text search language,
        /// MySQL's query expansion.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// let deals = Deal::query().where_full_text_with([Deal::TITLE], "\"net 30\" -draft", FullText::websearch());
        /// # let _ = deals;
        /// ```
        pub fn where_full_text_with(
            self,
            columns: impl IntoIterator<Item = impl $crate::IntoExpr>,
            query: impl Into<String>,
            options: $crate::ir::FullText,
        ) -> Self {
            self.where_($crate::query::filters::full_text(columns, query, options))
        }

        /// Adds `OR` a full-text match with options.
        pub fn or_where_full_text_with(
            self,
            columns: impl IntoIterator<Item = impl $crate::IntoExpr>,
            query: impl Into<String>,
            options: $crate::ir::FullText,
        ) -> Self {
            self.or_where($crate::query::filters::full_text(columns, query, options))
        }

        /// Adds `OR` a full-text match.
        pub fn or_where_full_text(
            self,
            columns: impl IntoIterator<Item = impl $crate::IntoExpr>,
            query: impl Into<String>,
        ) -> Self {
            self.or_where($crate::query::filters::full_text(
                columns,
                query,
                $crate::ir::FullText::default(),
            ))
        }

        /// Adds raw SQL with `?` placeholders for `values` (`??` for a
        /// literal `?`). SQL databases only.
        pub fn where_raw(
            self,
            sql: impl Into<std::borrow::Cow<'static, str>>,
            values: impl IntoIterator<Item = impl Into<$crate::Value>>,
        ) -> Self {
            self.where_($crate::Filter::raw(sql, values))
        }

        /// Adds `OR` raw SQL.
        pub fn or_where_raw(
            self,
            sql: impl Into<std::borrow::Cow<'static, str>>,
            values: impl IntoIterator<Item = impl Into<$crate::Value>>,
        ) -> Self {
            self.or_where($crate::Filter::raw(sql, values))
        }

        /// Applies `then` when `condition` holds.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// # let (only_mine, me) = (true, 7_i64);
        /// let deals = Deal::query().when(only_mine, |q| q.where_(Deal::OWNER_ID.eq(me)));
        /// # let _ = deals;
        /// ```
        pub fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
            if condition { then(self) } else { self }
        }

        /// Applies `then` when `condition` holds, `otherwise` when not.
        pub fn when_else(
            self,
            condition: bool,
            then: impl FnOnce(Self) -> Self,
            otherwise: impl FnOnce(Self) -> Self,
        ) -> Self {
            if condition {
                then(self)
            } else {
                otherwise(self)
            }
        }

        /// Applies `then` with the value when `value` is `Some`.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// # let stage: Option<Stage> = Some(Stage::Won);
        /// let deals = Deal::query().when_some(stage, |q, stage| q.where_(Deal::STAGE.eq(stage)));
        /// # let _ = deals;
        /// ```
        pub fn when_some<V>(self, value: Option<V>, then: impl FnOnce(Self, V) -> Self) -> Self {
            match value {
                Some(value) => then(self, value),
                None => self,
            }
        }

        /// Applies `then` unless `condition` holds.
        pub fn unless(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
            if condition { self } else { then(self) }
        }
    };
}

pub(crate) use filters;

/// A parenthesised group of conditions, built in a closure:
/// `.where_group(|g| g.where_(a).or_where(b))` is `AND (a OR b)`.
#[derive(Clone, Debug, Default)]
pub struct WhereGroup {
    pub(crate) clauses: Vec<WhereClause>,
}

impl WhereGroup {
    /// An empty group.
    pub fn new() -> Self {
        WhereGroup::default()
    }

    /// The group's clauses.
    pub fn into_clauses(self) -> Vec<WhereClause> {
        self.clauses
    }

    filters!();
}

impl HasWheres for WhereGroup {
    fn wheres_mut(&mut self) -> &mut Vec<WhereClause> {
        &mut self.clauses
    }
}

/// `column [NOT] IN (values)`, each value encoded as the column stores it.
pub(crate) fn in_list<T>(
    column: &impl ColumnOf<T>,
    values: impl IntoIterator<Item = impl ValueFor<T>>,
    not: bool,
) -> Filter {
    let encoded: Result<Vec<Expr>, String> = values
        .into_iter()
        .map(|value| {
            column
                .encode_value(&value.into_value_for())
                .map(Expr::Value)
        })
        .collect();
    match encoded {
        Ok(values) => Filter(Where::In {
            expr: Expr::Column(column.column_ref()),
            values: InValues::List(values),
            not,
        }),
        Err(error) => Filter(Where::Invalid(format!(
            "cannot use a value for column `{}`: {error}",
            column.column_ref()
        ))),
    }
}

pub(crate) fn in_sub(column: impl IntoExpr, query: impl IntoQuery, not: bool) -> Filter {
    Filter(Where::In {
        expr: column.into_expr(),
        values: InValues::Subquery(Box::new(query.into_query())),
        not,
    })
}

pub(crate) fn null(column: impl IntoExpr, not: bool) -> Filter {
    Filter(Where::Null {
        expr: column.into_expr(),
        not,
    })
}

pub(crate) fn between<T>(column: &impl ColumnOf<T>, low: T, high: T, not: bool) -> Filter {
    match (column.encode_value(&low), column.encode_value(&high)) {
        (Ok(low), Ok(high)) => Filter(Where::Between {
            expr: Expr::Column(column.column_ref()),
            low: Expr::Value(low),
            high: Expr::Value(high),
            not,
        }),
        (Err(error), _) | (_, Err(error)) => Filter(Where::Invalid(format!(
            "cannot use a value for column `{}`: {error}",
            column.column_ref()
        ))),
    }
}

pub(crate) fn columns(
    first: impl IntoExpr,
    op: impl Into<Operator>,
    second: impl IntoExpr,
) -> Filter {
    Filter(Where::Compare {
        left: first.into_expr(),
        op: op.into(),
        right: second.into_expr(),
    })
}

pub(crate) fn date_part(
    part: DatePart,
    column: impl IntoExpr,
    op: impl Into<Operator>,
    value: impl Into<Value>,
) -> Filter {
    Filter(Where::DatePart {
        part,
        expr: column.into_expr(),
        op: op.into(),
        value: Expr::Value(value.into()),
    })
}

pub(crate) fn like(
    column: impl IntoExpr,
    pattern: impl Into<String>,
    case_sensitive: bool,
    not: bool,
) -> Filter {
    Filter::like(column.into_expr(), pattern.into(), case_sensitive, not)
}

/// The column (with its JSON path) a JSON condition is about.
fn json_column(column: impl IntoExpr, what: &str) -> Result<ColumnRef, String> {
    match column.into_expr() {
        Expr::Column(column) => Ok(column),
        other => Err(format!(
            "{what} takes a column or a JSON path, not {other:?}"
        )),
    }
}

pub(crate) fn json_contains(
    column: impl IntoExpr,
    value: impl serde::Serialize,
    not: bool,
) -> Filter {
    let column = match json_column(column, "a JSON containment check") {
        Ok(column) => column,
        Err(message) => return Filter(Where::Invalid(message)),
    };
    match serde_json::to_value(value) {
        Ok(value) => Filter(Where::JsonContains { column, value, not }),
        Err(error) => Filter(Where::Invalid(format!(
            "the value for JSON column `{column}` cannot be encoded: {error}"
        ))),
    }
}

pub(crate) fn json_overlaps(
    column: impl IntoExpr,
    values: impl serde::Serialize,
    not: bool,
) -> Filter {
    let column = match json_column(column, "a JSON overlap check") {
        Ok(column) => column,
        Err(message) => return Filter(Where::Invalid(message)),
    };
    match serde_json::to_value(values) {
        Ok(value) => Filter(Where::JsonOverlaps { column, value, not }),
        Err(error) => Filter(Where::Invalid(format!(
            "the values for JSON column `{column}` cannot be encoded: {error}"
        ))),
    }
}

pub(crate) fn json_contains_key(column: impl IntoExpr, not: bool) -> Filter {
    match json_column(column, "a JSON key check") {
        Ok(column) if column.path.is_empty() => Filter(Where::Invalid(format!(
            "a JSON key check takes a path to the key (`{column}->key`), not the column alone"
        ))),
        Ok(column) => Filter(Where::JsonContainsKey { column, not }),
        Err(message) => Filter(Where::Invalid(message)),
    }
}

/// The vector column within cosine distance `max` of `vector`.
pub(crate) fn vector_within(column: impl IntoExpr, vector: &[f32], max: f64) -> Filter {
    Filter::compare(
        Expr::vector_distance(column.into_expr(), vector),
        Operator::Lte,
        Expr::Value(Value::from(max)),
    )
}

pub(crate) fn json_length(column: impl IntoExpr, op: impl Into<Operator>, length: i64) -> Filter {
    match json_column(column, "a JSON length check") {
        Ok(column) => Filter(Where::JsonLength {
            column,
            op: op.into(),
            length,
        }),
        Err(message) => Filter(Where::Invalid(message)),
    }
}

pub(crate) fn full_text(
    columns: impl IntoIterator<Item = impl IntoExpr>,
    query: impl Into<String>,
    options: FullText,
) -> Filter {
    let columns: Vec<Expr> = columns.into_iter().map(IntoExpr::into_expr).collect();
    if columns.is_empty() {
        return Filter(Where::Invalid(
            "a full-text condition needs at least one column".to_owned(),
        ));
    }
    Filter(Where::FullText {
        columns,
        query: query.into(),
        options,
    })
}

/// A table name with an optional alias: `"companies"`, `"companies as
/// c"`, `"companies c"`.
pub(crate) fn parse_table(table: &str) -> TableRef {
    let words: Vec<&str> = table.split_whitespace().collect();
    match words.as_slice() {
        [name] => TableRef::new(Cow::Owned((*name).to_owned())),
        [name, alias] => TableRef::aliased((*name).to_owned(), (*alias).to_owned()),
        [name, as_, alias] if as_.eq_ignore_ascii_case("as") => {
            TableRef::aliased((*name).to_owned(), (*alias).to_owned())
        }
        _ => TableRef::new(Cow::Owned(table.trim().to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_take_aliases_with_or_without_as() {
        for text in ["companies as c", "companies AS c", "companies c"] {
            let table = parse_table(text);
            assert_eq!(table.name, "companies", "{text}");
            assert_eq!(table.qualifier(), "c", "{text}");
        }
        assert_eq!(parse_table(" companies ").qualifier(), "companies");
    }
}
