//! The select, join, group, having, order, limit, union and lock methods,
//! stamped onto every builder ([`Builder`](crate::Builder),
//! [`TableBuilder`](crate::TableBuilder)) by the `clauses!` macro, and
//! [`IntoSelect`].

use std::borrow::Cow;

use crate::column::{Column, IntoExpr, IntoQuery};
use crate::ir::{ColumnRef, Expr, QueryIr, SelectItem};

/// A builder with a query (implemented by every builder).
pub trait HasQuery {
    /// The query.
    fn query_mut(&mut self) -> &mut QueryIr;

    /// What an empty select list means for the builder: the model's
    /// columns for a model builder, nothing for a table builder. Adding a
    /// select item to an empty list keeps these first.
    fn default_select(&self) -> Vec<SelectItem>;

    /// Appends select items, keeping the default ones when the list was
    /// empty.
    fn push_select(&mut self, items: Vec<SelectItem>) {
        if self.query_mut().columns.is_empty() {
            let defaults = self.default_select();
            self.query_mut().columns = defaults;
        }
        self.query_mut().columns.extend(items);
    }
}

/// An item of a select list: a typed column, a column name (`"title"`,
/// `"deals.*"`, `"title as name"`), an [`Expr`], or an `(item, alias)`
/// pair.
pub trait IntoSelect {
    /// The select item.
    fn into_select(self) -> SelectItem;
}

impl<M, T> IntoSelect for Column<M, T> {
    fn into_select(self) -> SelectItem {
        SelectItem {
            expr: self.expr(),
            alias: None,
        }
    }
}

impl<M, T> IntoSelect for &Column<M, T> {
    fn into_select(self) -> SelectItem {
        SelectItem {
            expr: self.expr(),
            alias: None,
        }
    }
}

/// `"title"`, `"deals.title"`, `"deals.*"`, `"meta->status"`, and any of
/// them followed by ` as alias`.
impl IntoSelect for &str {
    fn into_select(self) -> SelectItem {
        let lower = self.to_ascii_lowercase();
        match lower.rfind(" as ") {
            Some(at) => SelectItem {
                expr: self[..at].trim().into_expr(),
                alias: Some(Cow::Owned(self[at + 4..].trim().to_owned())),
            },
            None => SelectItem {
                expr: self.into_expr(),
                alias: None,
            },
        }
    }
}

impl IntoSelect for String {
    fn into_select(self) -> SelectItem {
        self.as_str().into_select()
    }
}

impl IntoSelect for Expr {
    fn into_select(self) -> SelectItem {
        SelectItem {
            expr: self,
            alias: None,
        }
    }
}

impl IntoSelect for ColumnRef {
    fn into_select(self) -> SelectItem {
        SelectItem {
            expr: Expr::Column(self),
            alias: None,
        }
    }
}

impl IntoSelect for SelectItem {
    fn into_select(self) -> SelectItem {
        self
    }
}

/// `(item, alias)`: the item named `alias`.
impl<E: IntoExpr, A: Into<Cow<'static, str>>> IntoSelect for (E, A) {
    fn into_select(self) -> SelectItem {
        SelectItem {
            expr: self.0.into_expr(),
            alias: Some(self.1.into()),
        }
    }
}

/// `expr AS alias`.
pub(crate) fn aliased(expr: Expr, alias: impl Into<Cow<'static, str>>) -> SelectItem {
    SelectItem {
        expr,
        alias: Some(alias.into()),
    }
}

/// Splits a raw table function, `name(arguments) [AS] alias`, into its
/// name, its argument list and its alias (the name when there is none).
pub(crate) fn parse_table_function(sql: &str) -> Result<(String, String, String), String> {
    let invalid = || {
        format!(
            "from_raw takes a table function, `name(arguments) [AS] alias`, not `{sql}` (use from_sub for a subquery)"
        )
    };
    let text = sql.trim();
    let open = text.find('(').ok_or_else(invalid)?;
    let name = text[..open].trim();
    if !crate::sql::is_identifier(name) {
        return Err(invalid());
    }
    // The matching `)`, outside quoted text.
    let mut depth = 0_usize;
    let mut quote: Option<char> = None;
    let mut close = None;
    for (at, c) in text[open..].char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' | '`' => quote = Some(c),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + at);
                        break;
                    }
                }
                _ => {}
            },
        }
    }
    let close = close.ok_or_else(invalid)?;
    let arguments = text[open + 1..close].to_owned();
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    let alias = match rest.as_slice() {
        [] => name,
        [alias] => alias,
        [as_, alias] if as_.eq_ignore_ascii_case("as") => alias,
        _ => return Err(invalid()),
    };
    if !crate::sql::is_identifier(alias) || alias.contains('.') {
        return Err(invalid());
    }
    Ok((name.to_owned(), arguments, alias.to_owned()))
}

/// A subquery as a select item.
pub(crate) fn subquery_item(
    query: impl IntoQuery,
    alias: impl Into<Cow<'static, str>>,
) -> SelectItem {
    aliased(Expr::Subquery(Box::new(query.into_query())), alias)
}

/// Stamps the select, join, group, having, order, limit, union and lock
/// methods onto a builder implementing [`HasQuery`].
macro_rules! clauses {
    () => {
        /// Selects `items`, replacing the select list.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// let deals = Deal::query().select([Deal::ID.expr(), Deal::TITLE.expr()]);
        /// let rows = table("deals").select(["id", "title as name"]);
        /// # let _ = (deals, rows);
        /// ```
        pub fn select<S: $crate::IntoSelect>(mut self, items: impl IntoIterator<Item = S>) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self).columns = items
                .into_iter()
                .map($crate::IntoSelect::into_select)
                .collect();
            self
        }

        /// Adds `items` to the select list (a model builder keeps the
        /// model's columns).
        pub fn add_select<S: $crate::IntoSelect>(
            mut self,
            items: impl IntoIterator<Item = S>,
        ) -> Self {
            let items = items
                .into_iter()
                .map($crate::IntoSelect::into_select)
                .collect();
            $crate::query::clauses::HasQuery::push_select(&mut self, items);
            self
        }

        /// Adds `expr AS alias` to the select list.
        pub fn select_expr(
            mut self,
            expr: impl Into<$crate::Expr>,
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            let item = $crate::query::clauses::aliased(expr.into(), alias);
            $crate::query::clauses::HasQuery::push_select(&mut self, vec![item]);
            self
        }

        /// Adds raw SQL to the select list, with `?` placeholders for
        /// `values` (name the result with `AS`).
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// let deals = Deal::query().select_raw("amount_cents * ? AS with_tax", [1.2]);
        /// # let _ = deals;
        /// ```
        pub fn select_raw(
            mut self,
            sql: impl Into<std::borrow::Cow<'static, str>>,
            values: impl IntoIterator<Item = impl Into<$crate::Value>>,
        ) -> Self {
            let item = $crate::ir::SelectItem {
                expr: $crate::Expr::raw(sql, values),
                alias: None,
            };
            $crate::query::clauses::HasQuery::push_select(&mut self, vec![item]);
            self
        }

        /// Adds `(subquery) AS alias` to the select list.
        pub fn select_sub(
            mut self,
            query: impl $crate::IntoQuery,
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            let item = $crate::query::clauses::subquery_item(query, alias);
            $crate::query::clauses::HasQuery::push_select(&mut self, vec![item]);
            self
        }

        /// Adds TimescaleDB's `time_bucket(width, column) AS alias`: the
        /// start of the `width` bucket (`"15 minutes"`, `"1 day"`) each
        /// row's time falls in. Fixed widths also run on SQLite and MySQL.
        pub fn select_time_bucket(
            self,
            width: impl Into<String>,
            column: impl $crate::IntoExpr,
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            self.select_expr(
                $crate::Expr::time_bucket(width, $crate::IntoExpr::into_expr(column)),
                alias,
            )
        }

        /// Adds TimescaleDB's `first(value, time) AS alias`: per group, the
        /// value at the earliest time.
        pub fn select_first(
            self,
            value: impl $crate::IntoExpr,
            time: impl $crate::IntoExpr,
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            self.select_expr(
                $crate::Expr::first(
                    $crate::IntoExpr::into_expr(value),
                    $crate::IntoExpr::into_expr(time),
                ),
                alias,
            )
        }

        /// Adds TimescaleDB's `last(value, time) AS alias`: per group, the
        /// value at the latest time.
        pub fn select_last(
            self,
            value: impl $crate::IntoExpr,
            time: impl $crate::IntoExpr,
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            self.select_expr(
                $crate::Expr::last(
                    $crate::IntoExpr::into_expr(value),
                    $crate::IntoExpr::into_expr(time),
                ),
                alias,
            )
        }

        /// `SELECT DISTINCT`.
        pub fn distinct(mut self) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self).distinct =
                $crate::ir::Distinct::Yes;
            self
        }

        /// PostgreSQL's `SELECT DISTINCT ON (columns)`: the first row of
        /// each distinct combination (by the query's order).
        pub fn distinct_on<C: $crate::IntoExpr>(
            mut self,
            columns: impl IntoIterator<Item = C>,
        ) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self).distinct =
                $crate::ir::Distinct::On(
                    columns
                        .into_iter()
                        .map($crate::IntoExpr::into_expr)
                        .collect(),
                );
            self
        }

        /// `INNER JOIN table ON first op second`. `table` may carry an
        /// alias: `"companies as c"`.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// let deals = Deal::query().join("companies", Deal::COMPANY_ID, "=", Company::ID);
        /// # let _ = deals;
        /// ```
        pub fn join(
            self,
            table: &str,
            first: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            second: impl $crate::IntoExpr,
        ) -> Self {
            self.push_join(
                $crate::ir::JoinKind::Inner,
                $crate::query::filters::parse_table(table),
                |join| join.on(first, op, second),
            )
        }

        /// `INNER JOIN table ON ...` with the conditions built in `on`.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// let deals = Deal::query().join_on("companies", |j| {
        ///     j.on(Deal::COMPANY_ID, "=", Company::ID).where_(Company::ACTIVE.eq(true))
        /// });
        /// # let _ = deals;
        /// ```
        pub fn join_on(
            self,
            table: &str,
            on: impl FnOnce($crate::JoinClause) -> $crate::JoinClause,
        ) -> Self {
            self.push_join(
                $crate::ir::JoinKind::Inner,
                $crate::query::filters::parse_table(table),
                on,
            )
        }

        /// `LEFT JOIN table ON first op second`.
        pub fn left_join(
            self,
            table: &str,
            first: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            second: impl $crate::IntoExpr,
        ) -> Self {
            self.push_join(
                $crate::ir::JoinKind::Left,
                $crate::query::filters::parse_table(table),
                |join| join.on(first, op, second),
            )
        }

        /// `LEFT JOIN table ON ...` with the conditions built in `on`.
        pub fn left_join_on(
            self,
            table: &str,
            on: impl FnOnce($crate::JoinClause) -> $crate::JoinClause,
        ) -> Self {
            self.push_join(
                $crate::ir::JoinKind::Left,
                $crate::query::filters::parse_table(table),
                on,
            )
        }

        /// `RIGHT JOIN table ON first op second`.
        pub fn right_join(
            self,
            table: &str,
            first: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            second: impl $crate::IntoExpr,
        ) -> Self {
            self.push_join(
                $crate::ir::JoinKind::Right,
                $crate::query::filters::parse_table(table),
                |join| join.on(first, op, second),
            )
        }

        /// `RIGHT JOIN table ON ...` with the conditions built in `on`.
        pub fn right_join_on(
            self,
            table: &str,
            on: impl FnOnce($crate::JoinClause) -> $crate::JoinClause,
        ) -> Self {
            self.push_join(
                $crate::ir::JoinKind::Right,
                $crate::query::filters::parse_table(table),
                on,
            )
        }

        /// `CROSS JOIN table`.
        pub fn cross_join(self, table: &str) -> Self {
            self.push_join(
                $crate::ir::JoinKind::Cross,
                $crate::query::filters::parse_table(table),
                |join| join,
            )
        }

        /// `INNER JOIN (subquery) AS alias ON first op second`.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// // Each deal with the time of its latest activity.
        /// let latest = Activity::query()
        ///     .select([Activity::DEAL_ID])
        ///     .select_expr(Expr::max(Activity::CREATED_AT), "at")
        ///     .group_by([Activity::DEAL_ID]);
        /// let deals = Deal::query()
        ///     .join_sub(latest, "latest", Deal::ID, "=", "latest.deal_id")
        ///     .add_select(["latest.at"]);
        /// # let _ = deals;
        /// ```
        pub fn join_sub(
            self,
            query: impl $crate::IntoQuery,
            alias: impl Into<std::borrow::Cow<'static, str>>,
            first: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            second: impl $crate::IntoExpr,
        ) -> Self {
            let table = $crate::ir::TableRef::subquery($crate::IntoQuery::into_query(query), alias);
            self.push_join($crate::ir::JoinKind::Inner, table, |join| {
                join.on(first, op, second)
            })
        }

        /// `LEFT JOIN (subquery) AS alias ON first op second`.
        pub fn left_join_sub(
            self,
            query: impl $crate::IntoQuery,
            alias: impl Into<std::borrow::Cow<'static, str>>,
            first: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            second: impl $crate::IntoExpr,
        ) -> Self {
            let table = $crate::ir::TableRef::subquery($crate::IntoQuery::into_query(query), alias);
            self.push_join($crate::ir::JoinKind::Left, table, |join| {
                join.on(first, op, second)
            })
        }

        /// `RIGHT JOIN (subquery) AS alias ON first op second`.
        pub fn right_join_sub(
            self,
            query: impl $crate::IntoQuery,
            alias: impl Into<std::borrow::Cow<'static, str>>,
            first: impl $crate::IntoExpr,
            op: impl Into<$crate::ir::Operator>,
            second: impl $crate::IntoExpr,
        ) -> Self {
            let table = $crate::ir::TableRef::subquery($crate::IntoQuery::into_query(query), alias);
            self.push_join($crate::ir::JoinKind::Right, table, |join| {
                join.on(first, op, second)
            })
        }

        /// `CROSS JOIN (subquery) AS alias`: every row with every row of the
        /// subquery.
        pub fn cross_join_sub(
            self,
            query: impl $crate::IntoQuery,
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            let table = $crate::ir::TableRef::subquery($crate::IntoQuery::into_query(query), alias);
            self.push_join($crate::ir::JoinKind::Cross, table, |join| join)
        }

        /// `INNER JOIN LATERAL (subquery) AS alias ON true`: the subquery
        /// runs for each row and may use its columns (a correlated
        /// subquery returning rows). Rows for which it returns nothing are
        /// left out. PostgreSQL and MySQL 8.0.14+; `Unsupported` on SQLite.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// // Each company with its three biggest deals.
        /// let top = Deal::query()
        ///     .select([Deal::TITLE.expr(), Deal::AMOUNT_CENTS.expr()])
        ///     .where_column(Deal::COMPANY_ID, "=", Company::ID)
        ///     .order_by_desc(Deal::AMOUNT_CENTS)
        ///     .limit(3);
        /// let rows = Company::query().join_lateral(top, "top_deals").add_select(["top_deals.title"]);
        /// assert!(rows.to_sql(Dialect::Postgres)?.sql.contains(r#"INNER JOIN LATERAL (SELECT "deals"."title""#));
        /// # Ok::<(), navree_orm::OrmError>(())
        /// ```
        pub fn join_lateral(
            self,
            query: impl $crate::IntoQuery,
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            let table = $crate::ir::TableRef::subquery($crate::IntoQuery::into_query(query), alias);
            self.push_join($crate::ir::JoinKind::Lateral, table, |join| join)
        }

        /// `LEFT JOIN LATERAL (subquery) AS alias ON true`: like
        /// [`join_lateral`](Self::join_lateral), keeping the rows for which
        /// the subquery returns nothing (its columns are NULL).
        pub fn left_join_lateral(
            self,
            query: impl $crate::IntoQuery,
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            let table = $crate::ir::TableRef::subquery($crate::IntoQuery::into_query(query), alias);
            self.push_join($crate::ir::JoinKind::LeftLateral, table, |join| join)
        }

        /// At most `limit` rows per value of `column` (top N per group), in
        /// the query's order: `ROW_NUMBER() OVER (PARTITION BY column ORDER
        /// BY ..)` in a derived table (PostgreSQL, MySQL 8, MariaDB 10.2+,
        /// SQLite 3.25+). An offset skips rows per group; a limit caps the
        /// whole result. Rows come back by their rank within their group.
        ///
        /// ```no_run
        /// # use navree_orm::__doc_models::*;
        /// # async fn demo(db: Db) -> Result<()> {
        /// // The two latest activities of every deal, in one query.
        /// let recent = Activity::query().latest().group_limit(2, Activity::DEAL_ID).get(&db).await?;
        /// # Ok(()) }
        /// ```
        pub fn group_limit(mut self, limit: u64, column: impl $crate::IntoExpr) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self).group_limit =
                Some($crate::ir::GroupLimit {
                    limit,
                    column: $crate::IntoExpr::into_expr(column),
                });
            self
        }

        /// Keeps the rows whose vector column (an embedding) has cosine
        /// similarity `min_similarity` or more with `vector` (distance at
        /// most `1 - min_similarity`), most similar first. pgvector on
        /// PostgreSQL, MariaDB's vector functions; unsupported on SQLite.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// # let embedding = vec![0.1_f32, 0.7, 0.2];
        /// let similar = Deal::query().where_vector_similar_to("embedding", &embedding, 0.8).limit(5);
        /// assert!(similar.to_sql(Dialect::Postgres)?.sql.ends_with(
        ///     r#"WHERE ("embedding" <=> CAST($1 AS vector)) <= $2 ORDER BY ("embedding" <=> CAST($3 AS vector)) ASC LIMIT $4"#
        /// ));
        /// # Ok::<(), navree_orm::OrmError>(())
        /// ```
        pub fn where_vector_similar_to(
            self,
            column: impl $crate::IntoExpr,
            vector: &[f32],
            min_similarity: f64,
        ) -> Self {
            let column = $crate::IntoExpr::into_expr(column);
            self.where_vector_distance_less_than(column.clone(), vector, 1.0 - min_similarity)
                .order_by_vector_distance(column, vector)
        }

        /// `OR` the vector column has cosine similarity `min_similarity` or
        /// more with `vector`, most similar first.
        pub fn or_where_vector_similar_to(
            self,
            column: impl $crate::IntoExpr,
            vector: &[f32],
            min_similarity: f64,
        ) -> Self {
            let column = $crate::IntoExpr::into_expr(column);
            self.or_where_vector_distance_less_than(column.clone(), vector, 1.0 - min_similarity)
                .order_by_vector_distance(column, vector)
        }

        /// Adds the cosine distance between the vector column and `vector`
        /// to the select list as `alias`.
        pub fn select_vector_distance(
            self,
            column: impl $crate::IntoExpr,
            vector: &[f32],
            alias: impl Into<std::borrow::Cow<'static, str>>,
        ) -> Self {
            self.select_expr(
                $crate::Expr::vector_distance($crate::IntoExpr::into_expr(column), vector),
                alias,
            )
        }

        /// Orders by the cosine distance between the vector column and
        /// `vector`, nearest first.
        pub fn order_by_vector_distance(
            self,
            column: impl $crate::IntoExpr,
            vector: &[f32],
        ) -> Self {
            self.order($crate::ir::Order::asc($crate::Expr::vector_distance(
                $crate::IntoExpr::into_expr(column),
                vector,
            )))
        }

        /// `INNER JOIN (subquery) AS alias ON ...` with the conditions built
        /// in `on`.
        pub fn join_sub_on(
            self,
            query: impl $crate::IntoQuery,
            alias: impl Into<std::borrow::Cow<'static, str>>,
            on: impl FnOnce($crate::JoinClause) -> $crate::JoinClause,
        ) -> Self {
            let table = $crate::ir::TableRef::subquery($crate::IntoQuery::into_query(query), alias);
            self.push_join($crate::ir::JoinKind::Inner, table, on)
        }

        fn push_join(
            mut self,
            kind: $crate::ir::JoinKind,
            table: $crate::ir::TableRef,
            on: impl FnOnce($crate::JoinClause) -> $crate::JoinClause,
        ) -> Self {
            let join = on($crate::JoinClause::new(kind, table)).into_join();
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .joins
                .push(join);
            self
        }

        /// `GROUP BY columns` (added to earlier groups).
        pub fn group_by<C: $crate::IntoExpr>(
            mut self,
            columns: impl IntoIterator<Item = C>,
        ) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .groups
                .extend(columns.into_iter().map($crate::IntoExpr::into_expr));
            self
        }

        /// `GROUP BY` raw SQL with `?` placeholders.
        pub fn group_by_raw(
            mut self,
            sql: impl Into<std::borrow::Cow<'static, str>>,
            values: impl IntoIterator<Item = impl Into<$crate::Value>>,
        ) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .groups
                .push($crate::Expr::raw(sql, values));
            self
        }

        /// `HAVING condition` (`AND` with earlier ones):
        /// `.having(Expr::count_all().gt(3))`.
        pub fn having(mut self, filter: impl Into<$crate::Filter>) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .havings
                .push($crate::ir::WhereClause::and(filter.into().into_where()));
            self
        }

        /// `OR condition` in the `HAVING` list.
        pub fn or_having(mut self, filter: impl Into<$crate::Filter>) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .havings
                .push($crate::ir::WhereClause::or(filter.into().into_where()));
            self
        }

        /// `HAVING` raw SQL with `?` placeholders.
        pub fn having_raw(
            self,
            sql: impl Into<std::borrow::Cow<'static, str>>,
            values: impl IntoIterator<Item = impl Into<$crate::Value>>,
        ) -> Self {
            self.having($crate::Filter::raw(sql, values))
        }

        /// `OR` raw SQL in the `HAVING` list.
        pub fn or_having_raw(
            self,
            sql: impl Into<std::borrow::Cow<'static, str>>,
            values: impl IntoIterator<Item = impl Into<$crate::Value>>,
        ) -> Self {
            self.or_having($crate::Filter::raw(sql, values))
        }

        /// Orders by `column`, ascending (after earlier orders).
        pub fn order_by(self, column: impl $crate::IntoExpr) -> Self {
            self.order($crate::ir::Order::asc($crate::IntoExpr::into_expr(column)))
        }

        /// Orders by `column`, descending.
        pub fn order_by_desc(self, column: impl $crate::IntoExpr) -> Self {
            self.order($crate::ir::Order::desc($crate::IntoExpr::into_expr(column)))
        }

        /// Orders by `column` in `direction`.
        pub fn order_by_dir(
            self,
            column: impl $crate::IntoExpr,
            direction: $crate::ir::Direction,
        ) -> Self {
            self.order(match direction {
                $crate::ir::Direction::Asc => {
                    $crate::ir::Order::asc($crate::IntoExpr::into_expr(column))
                }
                $crate::ir::Direction::Desc => {
                    $crate::ir::Order::desc($crate::IntoExpr::into_expr(column))
                }
            })
        }

        /// Orders by raw SQL with `?` placeholders (direction included in
        /// the SQL).
        pub fn order_by_raw(
            self,
            sql: impl Into<std::borrow::Cow<'static, str>>,
            values: impl IntoIterator<Item = impl Into<$crate::Value>>,
        ) -> Self {
            self.order($crate::ir::Order::raw(sql, values))
        }

        /// Adds an order item: `.order(Deal::CLOSED_AT.desc().nulls_last())`.
        pub fn order(mut self, order: impl Into<$crate::ir::Order>) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .orders
                .push(order.into());
            self
        }

        /// Newest first by `column`.
        pub fn latest_by(self, column: impl $crate::IntoExpr) -> Self {
            self.order_by_desc(column)
        }

        /// Oldest first by `column`.
        pub fn oldest_by(self, column: impl $crate::IntoExpr) -> Self {
            self.order_by(column)
        }

        /// Random order.
        pub fn in_random_order(self) -> Self {
            self.order($crate::ir::Order::Random)
        }

        /// Removes every order.
        pub fn reorder(mut self) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .orders
                .clear();
            self
        }

        /// Replaces every order with `column` in `direction`.
        pub fn reorder_by(
            self,
            column: impl $crate::IntoExpr,
            direction: $crate::ir::Direction,
        ) -> Self {
            self.reorder().order_by_dir(column, direction)
        }

        /// At most `count` rows.
        pub fn limit(mut self, count: u64) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self).limit = Some(count);
            self
        }

        /// At most `count` rows (an alias of [`limit`](Self::limit)).
        pub fn take(self, count: u64) -> Self {
            self.limit(count)
        }

        /// Skips the first `count` rows.
        pub fn offset(mut self, count: u64) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self).offset = Some(count);
            self
        }

        /// Skips the first `count` rows (an alias of [`offset`](Self::offset)).
        pub fn skip(self, count: u64) -> Self {
            self.offset(count)
        }

        /// The rows of page `page` (1-based) of `per_page` rows.
        pub fn for_page(self, page: u64, per_page: u64) -> Self {
            let per_page = per_page.max(1);
            self.offset((page.max(1) - 1).saturating_mul(per_page))
                .limit(per_page)
        }

        /// `UNION` with `query` (duplicates removed).
        pub fn union(mut self, query: impl $crate::IntoQuery) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .unions
                .push($crate::ir::Union {
                    query: Box::new($crate::IntoQuery::into_query(query)),
                    all: false,
                });
            self
        }

        /// `UNION ALL` with `query`.
        pub fn union_all(mut self, query: impl $crate::IntoQuery) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self)
                .unions
                .push($crate::ir::Union {
                    query: Box::new($crate::IntoQuery::into_query(query)),
                    all: true,
                });
            self
        }

        /// Locks the selected rows for update until the transaction ends
        /// (`FOR UPDATE`; SQLite serialises write transactions instead).
        /// Run it in a unit of work: a locking read through `&Db` joins
        /// the task's unit of work (beginning its transaction), since a
        /// lock outside a transaction ends with the statement.
        ///
        /// ```no_run
        /// # use navree_orm::__doc_models::*;
        /// # async fn demo(uow: Uow, id: Uuid) -> Result<()> {
        /// let mut deal = Deal::query().where_key(id).lock_for_update().first_or_fail(&uow).await?;
        /// deal.views += 1;
        /// deal.save(&uow).await?; // nobody changed the row in between
        /// # Ok(()) }
        /// ```
        pub fn lock_for_update(self) -> Self {
            self.lock($crate::ir::LockMode::Update)
        }

        /// Locks the selected rows against changes until the transaction
        /// ends (`FOR SHARE`, `LOCK IN SHARE MODE` on MySQL).
        pub fn shared_lock(self) -> Self {
            self.lock($crate::ir::LockMode::Share)
        }

        /// Locks the selected rows in `mode` until the transaction ends
        /// (see [`Lock`](crate::ir::Lock) for each database's clause).
        pub fn lock(mut self, mode: $crate::ir::LockMode) -> Self {
            let query = $crate::query::clauses::HasQuery::query_mut(&mut self);
            let wait = query.lock.map(|lock| lock.wait).unwrap_or_default();
            query.lock = Some($crate::ir::Lock { mode, wait });
            self
        }

        /// Leaves out the rows another transaction has locked instead of
        /// waiting for them (`SKIP LOCKED`; PostgreSQL and MySQL 8). Locks
        /// for update when no lock is set yet.
        ///
        /// ```
        /// # use navree_orm::__doc_models::*;
        /// // Each worker claims ten events nobody else is working on.
        /// let claim = Event::query().oldest().limit(10).lock_for_update().skip_locked();
        /// assert_eq!(
        ///     claim.to_sql(Dialect::Postgres)?.sql,
        ///     r#"SELECT "events"."id", "events"."name", "events"."created_at", "events"."updated_at" FROM "events" ORDER BY "events"."created_at" ASC LIMIT $1 FOR UPDATE SKIP LOCKED"#
        /// );
        /// # Ok::<(), navree_orm::OrmError>(())
        /// ```
        pub fn skip_locked(self) -> Self {
            self.lock_wait($crate::ir::LockWait::SkipLocked)
        }

        /// Fails at once with the database's lock error when another
        /// transaction holds one of the rows (`NOWAIT`; PostgreSQL and
        /// MySQL 8). Locks for update when no lock is set yet.
        pub fn no_wait(self) -> Self {
            self.lock_wait($crate::ir::LockWait::NoWait)
        }

        fn lock_wait(mut self, wait: $crate::ir::LockWait) -> Self {
            let query = $crate::query::clauses::HasQuery::query_mut(&mut self);
            let mode = query
                .lock
                .map_or($crate::ir::LockMode::Update, |lock| lock.mode);
            query.lock = Some($crate::ir::Lock { mode, wait });
            self
        }

        /// Runs the query on connection `name` (a named connection of
        /// `[database.connections.<name>]`, or `default`), whatever the
        /// model's: given a [`Db`](navree_db::Db), the query goes to that
        /// connection; a unit of work of another connection refuses it.
        ///
        /// ```no_run
        /// # use navree_orm::__doc_models::*;
        /// # async fn demo(db: Db) -> Result<()> {
        /// let archived = Deal::query().on_connection("archive").count(&db).await?;
        /// # Ok(()) }
        /// ```
        pub fn on_connection(mut self, name: impl Into<std::borrow::Cow<'static, str>>) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self).connection = Some(name.into());
            self
        }

        /// Reads from the writer even where read replicas exist: for a
        /// read that must see a write made outside the task's unit of work
        /// (reads in a unit of work that wrote use the writer anyway).
        ///
        /// ```no_run
        /// # use navree_orm::__doc_models::*;
        /// # async fn demo(db: Db, id: Uuid) -> Result<()> {
        /// let fresh = Deal::query().use_write().find(&db, id).await?;
        /// # Ok(()) }
        /// ```
        pub fn use_write(mut self) -> Self {
            $crate::query::clauses::HasQuery::query_mut(&mut self).use_writer = true;
            self
        }
    };
}

pub(crate) use clauses;
