//! [`TableBuilder`]: queries of any table, without a model.

use std::borrow::Cow;
use std::hash::Hash;

use indexmap::IndexMap;

use crate::column::{Assignment, IntoExpr, IntoQuery};
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::ir::{
    Aggregate, AggregateFunction, ArithmeticOp, ColumnRef, DeleteIr, Distinct, Expr, InValues,
    QueryIr, SelectItem, TableRef, UpdateIr, Where, WhereClause,
};
use crate::pagination::{Page, PageRequest};
use crate::query::clauses::{HasQuery, clauses};
use crate::query::fetch::Selectable;
use crate::query::filters::{HasWheres, filters, parse_table};
use crate::row::{Row, decode_error};
use crate::sql::{Dialect, Statement, render_select};
use crate::value::{Field, Value};

/// The page size of [`TableBuilder::paginate`] given a page number alone.
const TABLE_PER_PAGE: u64 = 15;

/// A query of any table: the same where, select, join, group, order and
/// limit methods as a model query, returning [`Row`]s, plus inserts,
/// updates and deletes of rows. For pivot tables, reports and tables
/// without a model.
///
/// ```
/// use navree_orm::{Dialect, Expr, table};
///
/// let report = table("deals")
///     .select(["stage"])
///     .select_expr(Expr::count_all(), "deals")
///     .group_by(["stage"])
///     .having(Expr::count_all().gt(5));
/// assert_eq!(
///     report.to_sql(Dialect::Sqlite).unwrap().sql,
///     r#"SELECT "stage", COUNT(*) AS "deals" FROM "deals" GROUP BY "stage" HAVING COUNT(*) > ?"#
/// );
/// ```
#[derive(Clone, Debug)]
pub struct TableBuilder {
    ir: QueryIr,
}

/// A query of table `name` (`"deals"`, `"deals as d"`).
pub fn table(name: &str) -> TableBuilder {
    TableBuilder::new(name)
}

impl TableBuilder {
    /// A query of table `name` (`"deals"`, `"deals as d"`).
    pub fn new(name: &str) -> Self {
        let mut ir = QueryIr::new(Cow::Owned(String::new()));
        ir.table = parse_table(name);
        TableBuilder { ir }
    }

    /// A query of the rows of `query`, named `alias` (`SELECT ... FROM
    /// (subquery) AS alias`).
    pub fn from_sub(query: impl IntoQuery, alias: impl Into<Cow<'static, str>>) -> Self {
        let mut ir = QueryIr::new(Cow::Owned(String::new()));
        ir.table = TableRef::subquery(query.into_query(), alias);
        TableBuilder { ir }
    }

    filters!();
    clauses!();

    /// Names the query's table `alias` (`FROM deals AS d`); qualify its
    /// columns with the alias (`d.title`).
    ///
    /// ```
    /// use navree_orm::{Dialect, col, table};
    ///
    /// let sql = table("deals").from_as("d").where_(col("d.stage").eq("won")).to_sql(Dialect::Sqlite).unwrap();
    /// assert_eq!(sql.sql, r#"SELECT * FROM "deals" AS "d" WHERE "d"."stage" = ?"#);
    /// ```
    pub fn from_as(mut self, alias: impl Into<Cow<'static, str>>) -> Self {
        let alias = alias.into();
        let table = &mut self.ir.table;
        if table.subquery.is_some() || table.function.is_some() {
            table.name = alias.clone();
        }
        table.alias = Some(alias);
        self
    }

    /// Reads the rows of the set-returning function call `call` (an
    /// [`Expr::func`]) instead of a table, named `alias`: `FROM
    /// generate_series(..) AS alias`. SQL databases only.
    ///
    /// ```
    /// use navree_orm::{Dialect, Expr, table};
    ///
    /// let numbers = table("ignored").from_function(Expr::func("generate_series", [Expr::val(1), Expr::val(3)]), "n");
    /// assert_eq!(
    ///     numbers.to_sql(Dialect::Postgres).unwrap().sql,
    ///     r#"SELECT * FROM generate_series($1, $2) AS "n""#
    /// );
    /// ```
    pub fn from_function(
        mut self,
        call: impl Into<Expr>,
        alias: impl Into<Cow<'static, str>>,
    ) -> Self {
        match call.into() {
            call @ Expr::Function { .. } => self.ir.table = TableRef::function(call, alias),
            other => self.ir.errors.push(format!(
                "from_function takes a function call (Expr::func), not {other:?}"
            )),
        }
        self
    }

    /// Reads the rows of a table function written as raw SQL, with `?`
    /// placeholders for `values`: `name(arguments) [AS] alias` (the alias
    /// defaults to the function's name). For a derived table, use
    /// [`from_sub`](Self::from_sub).
    ///
    /// ```
    /// use navree_orm::{Dialect, table};
    ///
    /// let days = table("ignored").from_raw("generate_series(?::date, ?::date, '1 day') AS day", ["2026-10-01", "2026-10-31"]);
    /// assert_eq!(
    ///     days.to_sql(Dialect::Postgres).unwrap().sql,
    ///     r#"SELECT * FROM generate_series($1::date, $2::date, '1 day') AS "day""#
    /// );
    /// ```
    pub fn from_raw(
        mut self,
        sql: impl Into<Cow<'static, str>>,
        values: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Self {
        let sql = sql.into();
        let values: Vec<Value> = values.into_iter().map(Into::into).collect();
        match crate::query::clauses::parse_table_function(&sql) {
            Ok((name, arguments, alias)) => {
                let args = if arguments.trim().is_empty() && values.is_empty() {
                    Vec::new()
                } else {
                    vec![Expr::Raw {
                        sql: arguments.into(),
                        values,
                    }]
                };
                let call = Expr::Function {
                    name: name.into(),
                    args,
                };
                self.ir.table = TableRef::function(call, alias);
            }
            Err(message) => self.ir.errors.push(message),
        }
        self
    }

    /// Newest first by `created_at`.
    pub fn latest(self) -> Self {
        self.latest_by("created_at")
    }

    /// Oldest first by `created_at`.
    pub fn oldest(self) -> Self {
        self.oldest_by("created_at")
    }

    /// The query.
    pub fn ir(&self) -> &QueryIr {
        &self.ir
    }

    /// The query, by value.
    pub fn into_ir(self) -> QueryIr {
        self.ir
    }

    /// Where the query asks to run: the connection it names and whether
    /// its reads go to the writer.
    pub(crate) fn route(&self) -> crate::Route<'_> {
        self.ir.route(None)
    }

    /// The statement the query compiles to for `dialect`, without running
    /// it.
    pub fn to_sql(&self, dialect: Dialect) -> Result<Statement> {
        render_select(&self.ir, dialect)
    }

    /// The statement [`update`](Self::update) would run on `dialect` with
    /// `assignments`, without running it.
    ///
    /// ```
    /// use navree_orm::{Dialect, col, table};
    ///
    /// let sql = table("deals")
    ///     .where_(col("stage").eq("open"))
    ///     .order_by("created_at")
    ///     .limit(100)
    ///     .to_update_sql(Dialect::Postgres, [col("stage").set("stale")])
    ///     .unwrap();
    /// assert_eq!(
    ///     sql.sql,
    ///     r#"UPDATE "deals" SET "stage" = $1 WHERE "ctid" IN (SELECT "deals"."ctid" FROM "deals" WHERE "stage" = $2 ORDER BY "created_at" ASC LIMIT $3)"#
    /// );
    /// ```
    pub fn to_update_sql(
        &self,
        dialect: Dialect,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> Result<Statement> {
        let sets = assignments
            .into_iter()
            .map(|assignment| assignment.into_set().map_err(OrmError::invalid))
            .collect::<Result<Vec<_>>>()?;
        let (table, wheres) = self.clone().write_target("update", Some(dialect))?;
        crate::sql::render_update(
            &UpdateIr {
                table,
                sets,
                wheres,
            },
            dialect,
        )
    }

    /// The statement [`delete`](Self::delete) would run on `dialect`,
    /// without running it.
    pub fn to_delete_sql(&self, dialect: Dialect) -> Result<Statement> {
        let (table, wheres) = self.clone().write_target("delete", Some(dialect))?;
        crate::sql::render_delete(&DeleteIr { table, wheres }, dialect)
    }

    /// The query's rows.
    pub async fn get<E: Executor + ?Sized>(self, exec: &E) -> Result<Vec<Row>> {
        let target = exec
            .target_routed(crate::query::read_access(&self.ir), self.route())
            .await?;
        target.fetch(&self.ir).await
    }

    /// The first row, if any.
    pub async fn first<E: Executor + ?Sized>(self, exec: &E) -> Result<Option<Row>> {
        Ok(self.limit(1).get(exec).await?.into_iter().next())
    }

    async fn select_rows(self, target: &Target<'_>, items: Vec<SelectItem>) -> Result<Vec<Row>> {
        let query = crate::query::reading_values(self.ir, items, Vec::new());
        target.fetch(&query).await
    }

    fn table_name(&self) -> String {
        self.ir.table.name.to_string()
    }

    /// The value of `column` in the first row; `None` when there is no row.
    /// A column name reads a [`Value`]; [`col_as`](crate::col_as) or a
    /// typed column reads its type.
    pub async fn value<E, C>(self, exec: &E, column: C) -> Result<Option<C::Output>>
    where
        E: Executor + ?Sized,
        C: Selectable,
    {
        let target = exec
            .target_routed(crate::query::read_access(&self.ir), self.route())
            .await?;
        let name = self.table_name();
        let item = SelectItem {
            expr: column.selected(),
            alias: Some(Cow::Borrowed("value")),
        };
        let rows = self.limit(1).select_rows(&target, vec![item]).await?;
        match rows.first() {
            Some(row) => column
                .read(row, "value")
                .map(Some)
                .map_err(|error| decode_error(name, "value", "the selected value".into(), &error)),
            None => Ok(None),
        }
    }

    /// The values of `column` in every row, in order.
    pub async fn pluck<E, C>(self, exec: &E, column: C) -> Result<Vec<C::Output>>
    where
        E: Executor + ?Sized,
        C: Selectable,
    {
        let target = exec
            .target_routed(crate::query::read_access(&self.ir), self.route())
            .await?;
        let name = self.table_name();
        let item = SelectItem {
            expr: column.selected(),
            alias: Some(Cow::Borrowed("value")),
        };
        let rows = self.select_rows(&target, vec![item]).await?;
        rows.iter()
            .map(|row| {
                column.read(row, "value").map_err(|error| {
                    decode_error(name.clone(), "value", "the selected value".into(), &error)
                })
            })
            .collect()
    }

    /// The values of `column` keyed by the values of `key`, in order.
    pub async fn pluck_keyed<E, V, K>(
        self,
        exec: &E,
        column: V,
        key: K,
    ) -> Result<IndexMap<K::Output, V::Output>>
    where
        E: Executor + ?Sized,
        V: Selectable,
        K: Selectable,
        K::Output: Hash + Eq,
    {
        let target = exec
            .target_routed(crate::query::read_access(&self.ir), self.route())
            .await?;
        let name = self.table_name();
        let items = vec![
            SelectItem {
                expr: column.selected(),
                alias: Some(Cow::Borrowed("value")),
            },
            SelectItem {
                expr: key.selected(),
                alias: Some(Cow::Borrowed("key")),
            },
        ];
        let rows = self.select_rows(&target, items).await?;
        let mut map = IndexMap::with_capacity(rows.len());
        for row in &rows {
            let value = column.read(row, "value").map_err(|error| {
                decode_error(name.clone(), "value", "the selected value".into(), &error)
            })?;
            let key = key.read(row, "key").map_err(|error| {
                decode_error(name.clone(), "key", "the selected key".into(), &error)
            })?;
            map.insert(key, value);
        }
        Ok(map)
    }

    /// Whether the query has any row.
    pub async fn exists<E: Executor + ?Sized>(self, exec: &E) -> Result<bool> {
        let target = exec.target_routed(Access::Read, self.route()).await?;
        target.exists(&self.ir).await
    }

    /// Whether the query has no row.
    pub async fn doesnt_exist<E: Executor + ?Sized>(self, exec: &E) -> Result<bool> {
        Ok(!self.exists(exec).await?)
    }

    /// The number of rows (of groups for a grouped query).
    pub async fn count<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        self.aggregate::<u64>(exec, AggregateFunction::Count, None, false)
            .await
            .map(Option::unwrap_or_default)
    }

    /// The sum of `column` (zero when there are no rows).
    pub async fn sum<T: Field + Default>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<T> {
        self.aggregate::<T>(
            exec,
            AggregateFunction::Sum,
            Some(column.into_expr()),
            false,
        )
        .await
        .map(Option::unwrap_or_default)
    }

    /// The average of `column`; `None` when there are no rows.
    pub async fn avg<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(
            exec,
            AggregateFunction::Avg,
            Some(column.into_expr()),
            false,
        )
        .await
    }

    /// The smallest value of `column`.
    pub async fn min<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(
            exec,
            AggregateFunction::Min,
            Some(column.into_expr()),
            false,
        )
        .await
    }

    /// The largest value of `column`.
    pub async fn max<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(
            exec,
            AggregateFunction::Max,
            Some(column.into_expr()),
            false,
        )
        .await
    }

    /// An aggregate of the rows read as `T`; `None` when the database
    /// returns NULL.
    pub async fn aggregate<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        function: AggregateFunction,
        argument: Option<Expr>,
        distinct: bool,
    ) -> Result<Option<T>> {
        let target = exec.target_routed(Access::Read, self.route()).await?;
        let name = self.table_name();
        let aggregate = Aggregate {
            function,
            arg: argument,
            distinct,
        };
        let value = target.aggregate(&self.ir, &aggregate).await?;
        <Option<T>>::from_value(value)
            .map_err(|error| decode_error(name, function.sql_name(), T::type_name(), &error))
    }

    /// Page `page` of the rows and their total (15 rows per page for a
    /// page number alone).
    pub async fn paginate<E: Executor + ?Sized>(
        self,
        exec: &E,
        page: impl Into<PageRequest>,
    ) -> Result<Page<Row>> {
        let page = page.into();
        let query = navree_db::PageQuery {
            page: page.page.max(1),
            per_page: page.per_page.unwrap_or(TABLE_PER_PAGE).max(1),
        };
        let target = exec.target_routed(Access::Read, self.route()).await?;
        let aggregate = Aggregate {
            function: AggregateFunction::Count,
            arg: None,
            distinct: false,
        };
        let total = u64::from_value(target.aggregate(&self.ir, &aggregate).await?)
            .map_err(|error| decode_error(self.table_name(), "COUNT", "u64".into(), &error))?;
        let past_the_end = (query.page - 1)
            .checked_mul(query.per_page)
            .is_none_or(|offset| offset >= total);
        let data = if past_the_end {
            Vec::new()
        } else {
            let paged = self
                .offset((query.page - 1) * query.per_page)
                .limit(query.per_page);
            target.fetch(&paged.ir).await?
        };
        Ok(Page::new(data, query, total))
    }

    /// The table a write on `dialect` goes to, and its conditions. Plain
    /// conditions go into the `WHERE` as they are. A query with joins,
    /// orders, a limit, an offset or `DISTINCT` narrows the rows by their
    /// physical row id instead: `ctid IN (SELECT t.ctid ..)` on PostgreSQL,
    /// `rowid IN (SELECT t.rowid ..)` on SQLite. MySQL has no row id
    /// (model queries narrow by key there), and groups, unions and
    /// subquery tables never write.
    pub(crate) fn write_target(
        self,
        what: &str,
        dialect: Option<Dialect>,
    ) -> Result<(TableRef, Vec<WhereClause>)> {
        let ir = self.ir;
        ir.check()?;
        if ir.table.subquery.is_some()
            || !ir.groups.is_empty()
            || !ir.havings.is_empty()
            || !ir.unions.is_empty()
        {
            return Err(OrmError::invalid(format!(
                "a table {what} cannot have groups, unions or a subquery as its table; use a model query"
            )));
        }
        let simple = ir.joins.is_empty()
            && ir.limit.is_none()
            && ir.offset.is_none()
            && matches!(ir.distinct, Distinct::No);
        if simple {
            return Ok((TableRef::new(ir.table.name), ir.wheres));
        }
        let row_id = match dialect {
            Some(Dialect::Postgres) => "ctid",
            Some(Dialect::Sqlite) => "rowid",
            Some(Dialect::MySql) => {
                return Err(OrmError::unsupported(
                    format!(
                        "a table {what} with joins, a limit or an offset (MySQL has no row id; a model query narrows the rows by key)"
                    ),
                    "MySQL",
                ));
            }
            None => {
                return Err(OrmError::unsupported(
                    format!("a table {what} with joins, a limit or an offset"),
                    "a document store",
                ));
            }
        };
        let table = TableRef::new(ir.table.name.clone());
        let mut rows = ir;
        rows.columns = vec![SelectItem {
            expr: Expr::Column(ColumnRef::new(
                Some(rows.table.qualifier().to_owned()),
                row_id,
            )),
            alias: None,
        }];
        rows.lock = None;
        if rows.limit.is_none() && rows.offset.is_none() {
            rows.orders.clear();
        }
        Ok((
            table,
            vec![WhereClause::and(Where::In {
                expr: Expr::Column(ColumnRef::new(None::<&str>, row_id)),
                values: InValues::Subquery(Box::new(rows)),
                not: false,
            })],
        ))
    }

    /// Sets columns on every matching row; returns how many changed. An
    /// assignment to a JSON path (`col("meta->status").set("won")`)
    /// changes that value inside the column. A query with joins, orders or
    /// a limit updates the rows it selects (see the row ids above).
    pub async fn update<E: Executor + ?Sized>(
        self,
        exec: &E,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> Result<u64> {
        let sets = assignments
            .into_iter()
            .map(|assignment| assignment.into_set().map_err(OrmError::invalid))
            .collect::<Result<Vec<_>>>()?;
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let (table, wheres) = self.write_target("update", target.dialect())?;
        target
            .update(&UpdateIr {
                table,
                sets,
                wheres,
            })
            .await
    }

    /// Deletes every matching row; returns how many were deleted. A query
    /// with joins, orders or a limit deletes the rows it selects.
    pub async fn delete<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let (table, wheres) = self.write_target("delete", target.dialect())?;
        target.delete(&DeleteIr { table, wheres }).await
    }

    /// Adds `amount` to `column` on every matching row.
    pub async fn increment<E: Executor + ?Sized>(
        self,
        exec: &E,
        column: &str,
        amount: impl Into<Value>,
    ) -> Result<u64> {
        self.step(exec, column, amount.into(), ArithmeticOp::Add)
            .await
    }

    /// Subtracts `amount` from `column` on every matching row.
    pub async fn decrement<E: Executor + ?Sized>(
        self,
        exec: &E,
        column: &str,
        amount: impl Into<Value>,
    ) -> Result<u64> {
        self.step(exec, column, amount.into(), ArithmeticOp::Sub)
            .await
    }

    async fn step<E: Executor + ?Sized>(
        self,
        exec: &E,
        column: &str,
        amount: Value,
        op: ArithmeticOp,
    ) -> Result<u64> {
        let column = column.to_owned();
        let sets = vec![(
            Cow::Owned(column.clone()),
            Expr::Arithmetic {
                left: Box::new(Expr::Column(ColumnRef::new(None::<&str>, column))),
                op,
                right: Box::new(Expr::Value(amount)),
            },
        )];
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let (table, wheres) = self.write_target("update", target.dialect())?;
        target
            .update(&UpdateIr {
                table,
                sets,
                wheres,
            })
            .await
    }
}

impl HasWheres for TableBuilder {
    fn wheres_mut(&mut self) -> &mut Vec<WhereClause> {
        &mut self.ir.wheres
    }
}

impl HasQuery for TableBuilder {
    fn query_mut(&mut self) -> &mut QueryIr {
        &mut self.ir
    }

    fn default_select(&self) -> Vec<SelectItem> {
        Vec::new()
    }
}

impl IntoQuery for TableBuilder {
    fn into_query(self) -> QueryIr {
        self.ir
    }
}
