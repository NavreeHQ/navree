//! [`TableBuilder`]: queries of any table, without a model.

use std::borrow::Cow;
use std::hash::Hash;

use indexmap::IndexMap;

use crate::column::{AsExpr, Assignment, IntoQuery};
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::ir::{
    Aggregate, AggregateFunction, ArithmeticOp, ColumnRef, DeleteIr, Distinct, Expr, InsertIr,
    QueryIr, SelectItem, TableRef, UpdateIr, WhereClause,
};
use crate::pagination::{Page, PageRequest};
use crate::query::clauses::{HasQuery, clauses};
use crate::query::fetch::Selectable;
use crate::query::filters::{HasWheres, filters, parse_table};
use crate::row::{Row, decode_error};
use crate::sql::{Dialect, Statement, compile_select};
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

    /// The statement the query compiles to for `dialect`, without running
    /// it.
    pub fn to_sql(&self, dialect: Dialect) -> Result<Statement> {
        compile_select(&self.ir, dialect)
    }

    /// The query's rows.
    pub async fn get<E: Executor + ?Sized>(self, exec: &E) -> Result<Vec<Row>> {
        let target = exec.target(Access::Read).await?;
        target.fetch(&self.ir).await
    }

    /// The first row, if any.
    pub async fn first<E: Executor + ?Sized>(self, exec: &E) -> Result<Option<Row>> {
        Ok(self.limit(1).get(exec).await?.into_iter().next())
    }

    async fn select_rows(mut self, target: &Target<'_>, items: Vec<SelectItem>) -> Result<Vec<Row>> {
        self.ir.columns = items;
        target.fetch(&self.ir).await
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
        let target = exec.target(Access::Read).await?;
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
        let target = exec.target(Access::Read).await?;
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
        let target = exec.target(Access::Read).await?;
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
        let target = exec.target(Access::Read).await?;
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
        column: impl AsExpr,
    ) -> Result<T> {
        self.aggregate::<T>(exec, AggregateFunction::Sum, Some(column.as_expr()), false)
            .await
            .map(Option::unwrap_or_default)
    }

    /// The average of `column`; `None` when there are no rows.
    pub async fn avg<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl AsExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(exec, AggregateFunction::Avg, Some(column.as_expr()), false)
            .await
    }

    /// The smallest value of `column`.
    pub async fn min<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl AsExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(exec, AggregateFunction::Min, Some(column.as_expr()), false)
            .await
    }

    /// The largest value of `column`.
    pub async fn max<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl AsExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(exec, AggregateFunction::Max, Some(column.as_expr()), false)
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
        let target = exec.target(Access::Read).await?;
        let name = self.table_name();
        let aggregate = Aggregate {
            function,
            arg: argument,
            distinct,
        };
        let value = target.aggregate(&self.ir, &aggregate).await?;
        <Option<T>>::from_value(value).map_err(|error| {
            decode_error(name, function.sql_name(), T::type_name(), &error)
        })
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
        let target = exec.target(Access::Read).await?;
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

    /// The table a write goes to, and its conditions. Writes take plain
    /// conditions only: a table query has no key to narrow joined or
    /// limited rows by.
    fn write_target(self, what: &str) -> Result<(TableRef, Vec<WhereClause>)> {
        let ir = self.ir;
        ir.check()?;
        if ir.table.subquery.is_some()
            || !ir.joins.is_empty()
            || !ir.groups.is_empty()
            || !ir.havings.is_empty()
            || !ir.unions.is_empty()
            || ir.limit.is_some()
            || ir.offset.is_some()
            || !matches!(ir.distinct, Distinct::No)
        {
            return Err(OrmError::invalid(format!(
                "a table {what} takes where conditions only (no joins, groups, limits or subquery tables); use a model query for those"
            )));
        }
        Ok((TableRef::new(ir.table.name), ir.wheres))
    }

    /// Inserts `rows` (each a list of `(column, value)` pairs, every row
    /// with the same columns) in one statement; returns how many were
    /// inserted.
    ///
    /// ```ignore
    /// table("deal_tag").insert(&uow, [[("deal_id", Value::from(deal)), ("tag_id", Value::from(tag))]]).await?;
    /// ```
    pub async fn insert<E, R, C, V>(self, exec: &E, rows: impl IntoIterator<Item = R>) -> Result<u64>
    where
        E: Executor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>,
    {
        let (table, _) = self.write_target("insert")?;
        let mut columns: Vec<Cow<'static, str>> = Vec::new();
        let mut values: Vec<Vec<Expr>> = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            let pairs: Vec<(Cow<'static, str>, Value)> = row
                .into_iter()
                .map(|(column, value)| (column.into(), value.into()))
                .collect();
            if index == 0 {
                columns = pairs.iter().map(|(column, _)| column.clone()).collect();
            }
            if pairs.len() != columns.len() {
                return Err(OrmError::invalid(format!(
                    "row {} of an insert into `{}` has {} columns, the first has {}",
                    index + 1,
                    table.name,
                    pairs.len(),
                    columns.len()
                )));
            }
            let mut ordered = Vec::with_capacity(columns.len());
            for column in &columns {
                let value = pairs
                    .iter()
                    .find(|(name, _)| name == column)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| {
                        OrmError::invalid(format!(
                            "row {} of an insert into `{}` has no `{column}` column",
                            index + 1,
                            table.name
                        ))
                    })?;
                ordered.push(Expr::Value(value));
            }
            values.push(ordered);
        }
        if values.is_empty() {
            return Ok(0);
        }
        let insert = InsertIr {
            table: table.name,
            columns,
            rows: values,
            returning: Vec::new(),
        };
        let target = exec.target(Access::Write).await?;
        Ok(target.insert(&insert).await?.rows_affected)
    }

    /// Sets columns on every matching row; returns how many changed.
    pub async fn update<E: Executor + ?Sized>(
        self,
        exec: &E,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> Result<u64> {
        let sets = assignments
            .into_iter()
            .map(|assignment| assignment.into_set().map_err(OrmError::invalid))
            .collect::<Result<Vec<_>>>()?;
        let (table, wheres) = self.write_target("update")?;
        let target = exec.target(Access::Write).await?;
        target.update(&UpdateIr { table, sets, wheres }).await
    }

    /// Deletes every matching row; returns how many were deleted.
    pub async fn delete<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        let (table, wheres) = self.write_target("delete")?;
        let target = exec.target(Access::Write).await?;
        target.delete(&DeleteIr { table, wheres }).await
    }

    /// Adds `amount` to `column` on every matching row.
    pub async fn increment<E: Executor + ?Sized>(
        self,
        exec: &E,
        column: &str,
        amount: impl Into<Value>,
    ) -> Result<u64> {
        self.step(exec, column, amount.into(), ArithmeticOp::Add).await
    }

    /// Subtracts `amount` from `column` on every matching row.
    pub async fn decrement<E: Executor + ?Sized>(
        self,
        exec: &E,
        column: &str,
        amount: impl Into<Value>,
    ) -> Result<u64> {
        self.step(exec, column, amount.into(), ArithmeticOp::Sub).await
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
        let (table, wheres) = self.write_target("update")?;
        let target = exec.target(Access::Write).await?;
        target.update(&UpdateIr { table, sets, wheres }).await
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
