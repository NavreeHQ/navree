//! Raw SQL: [`Raw`] fragments and statements with portable `?`
//! placeholders, and the functions that run raw statements.
//!
//! A `?` binds the next value (`$1`, `$2`, ... on PostgreSQL), `??` is a
//! literal `?` (PostgreSQL's `?`, `?|` and `?&` JSON operators), and quoted
//! text is left alone. The number of placeholders must match the number of
//! values; a mismatch is an [`OrmError::InvalidQuery`] before anything runs.
//!
//! A [`Raw`] goes anywhere an expression or a condition does
//! (`select_expr`, `where_`, `order_by`, ...) and runs as a statement of
//! its own:
//!
//! | Function | Runs | Returns |
//! |---|---|---|
//! | [`select`] | a query | its rows |
//! | [`select_one`] | a query | its first row |
//! | [`scalar`] | a query | the first column of its first row |
//! | [`statement`] / [`affecting_statement`] | any statement | the number of rows it changed |
//! | [`unprepared`] | statements without values, `;`-separated | nothing |
//! | [`cursor`](crate::cursor) | a query | its rows as a stream |
//!
//! Reads go where the executor reads (`&Db` joins the task's open unit of
//! work), statements where it writes. Document stores have no SQL: they
//! answer with [`OrmError::Unsupported`].
//!
//! ```no_run
//! # use navree_orm::__doc_models::*;
//! use navree_orm::{raw, scalar, select, statement};
//!
//! # async fn demo(db: Db, uow: Uow) -> Result<()> {
//! let open: Option<i64> = scalar(&db, raw("SELECT COUNT(*) FROM deals WHERE stage = ?", ["open"])).await?;
//! let rows = select(&db, raw("SELECT id, title FROM deals ORDER BY created_at DESC LIMIT ?", [10])).await?;
//! let archived = statement(&uow, raw("UPDATE deals SET stage = ? WHERE closed_at < ?", [
//!     Value::from("archived"),
//!     Value::from(Utc::now() - Duration::days(365)),
//! ]))
//! .await?;
//! # let _ = (open, rows, archived);
//! # Ok(()) }
//! ```

use std::borrow::Cow;
use std::fmt;

use crate::column::{Filter, IntoExpr};
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, SqlTarget, Target};
use crate::ir::{Expr, SelectItem};
use crate::query::IntoSelect;
use crate::render::Writer;
use crate::row::{Row, decode_error};
use crate::sql::{Dialect, Statement};
use crate::value::{Field, Value};

/// Raw SQL with portable `?` placeholders and the values bound to them: a
/// fragment (an expression, a condition, a select item) or a whole
/// statement. Built with [`raw`].
///
/// ```
/// use navree_orm::{Dialect, raw};
///
/// let fragment = raw("lower(title) = ? AND meta ?? 'vip'", ["renewal"]);
/// let statement = fragment.to_statement(Dialect::Postgres).unwrap();
/// assert_eq!(statement.sql, "lower(title) = $1 AND meta ? 'vip'");
///
/// let error = raw("a = ? AND b = ?", [1]).to_statement(Dialect::Sqlite).unwrap_err();
/// assert_eq!(error.to_string(), "invalid query: the raw SQL `a = ? AND b = ?` has 2 placeholder(s) but 1 value(s)");
/// ```
#[derive(Clone, PartialEq)]
pub struct Raw {
    sql: Cow<'static, str>,
    values: Vec<Value>,
}

/// Raw SQL with `?` placeholders for `values` (`??` for a literal `?`).
///
/// ```
/// use navree_orm::{Dialect, raw, table};
///
/// let query = table("deals").where_(raw("amount_cents > ? * 100", [50]));
/// assert_eq!(
///     query.to_sql(Dialect::Postgres).unwrap().sql,
///     r#"SELECT * FROM "deals" WHERE amount_cents > $1 * 100"#
/// );
/// ```
pub fn raw(
    sql: impl Into<Cow<'static, str>>,
    values: impl IntoIterator<Item = impl Into<Value>>,
) -> Raw {
    Raw::new(sql, values)
}

impl Raw {
    /// Raw SQL with `?` placeholders for `values` (see [`raw`]).
    pub fn new(
        sql: impl Into<Cow<'static, str>>,
        values: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Self {
        Raw {
            sql: sql.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// The SQL as written (with portable `?` placeholders).
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// The values, in placeholder order.
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// The statement for `dialect`: placeholders rewritten for it (`$n` on
    /// PostgreSQL) and their number checked against the values.
    pub fn to_statement(&self, dialect: Dialect) -> Result<Statement> {
        let mut w = Writer::new(dialect);
        w.raw(&self.sql, &self.values)?;
        Ok(w.finish())
    }
}

impl fmt::Debug for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Raw")
            .field("sql", &self.sql)
            .field("values", &self.values)
            .finish()
    }
}

/// A raw fragment is an expression ([`Expr::Raw`]).
impl IntoExpr for Raw {
    fn into_expr(self) -> Expr {
        Expr::Raw {
            sql: self.sql,
            values: self.values,
        }
    }
}

impl From<Raw> for Expr {
    fn from(raw: Raw) -> Self {
        raw.into_expr()
    }
}

/// A raw fragment is a condition ([`Filter::raw`]).
impl From<Raw> for Filter {
    fn from(raw: Raw) -> Self {
        Filter::raw(raw.sql, raw.values)
    }
}

/// A raw fragment is a select item (name the result with `AS`).
impl IntoSelect for Raw {
    fn into_select(self) -> SelectItem {
        SelectItem {
            expr: self.into_expr(),
            alias: None,
        }
    }
}

/// The SQL target of `target`, or the error a document store answers raw
/// SQL with.
pub(crate) fn sql_target<'t, 'a>(target: &'t Target<'a>) -> Result<&'t SqlTarget<'a>> {
    match target {
        Target::Sql(sql) => Ok(sql),
        Target::Document(_) => Err(OrmError::unsupported(
            "raw SQL",
            target.context().database(),
        )),
    }
}

/// The rows of raw query `query`.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db) -> Result<()> {
/// let rows = navree_orm::select(&db, navree_orm::raw("SELECT stage, COUNT(*) AS n FROM deals GROUP BY stage", [] as [i64; 0])).await?;
/// for row in &rows {
///     println!("{}: {}", row.get::<String>("stage")?, row.get::<i64>("n")?);
/// }
/// # Ok(()) }
/// ```
pub async fn select<E: Executor + ?Sized>(exec: &E, query: Raw) -> Result<Vec<Row>> {
    let target = exec.target(Access::Read).await?;
    let sql = sql_target(&target)?;
    let statement = query.to_statement(sql.dialect())?;
    Ok(sql
        .query_all(&statement)
        .await?
        .into_iter()
        .map(Row::from_sql)
        .collect())
}

/// The first row of raw query `query`, if any.
pub async fn select_one<E: Executor + ?Sized>(exec: &E, query: Raw) -> Result<Option<Row>> {
    let target = exec.target(Access::Read).await?;
    let sql = sql_target(&target)?;
    let statement = query.to_statement(sql.dialect())?;
    Ok(sql.query_one(&statement).await?.map(Row::from_sql))
}

/// The first column of the first row of raw query `query`, read as `T`;
/// `None` when the query returns no row.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db) -> Result<()> {
/// let version: Option<String> = navree_orm::scalar(&db, navree_orm::raw("SELECT version()", [] as [i64; 0])).await?;
/// # Ok(()) }
/// ```
pub async fn scalar<T: Field, E: Executor + ?Sized>(exec: &E, query: Raw) -> Result<Option<T>> {
    let Some(row) = select_one(exec, query).await? else {
        return Ok(None);
    };
    let columns = row.columns();
    let Some(first) = columns.first() else {
        return Err(OrmError::invalid("the raw query selects no column"));
    };
    row.field::<T>(first)
        .map(Some)
        .map_err(|error| decode_error("the raw query", first, T::type_name(), &error))
}

/// Runs raw statement `statement` where the executor writes (inside the
/// unit of work); returns how many rows it changed.
pub async fn statement<E: Executor + ?Sized>(exec: &E, statement: Raw) -> Result<u64> {
    affecting_statement(exec, statement).await
}

/// Runs raw statement `statement` where the executor writes; returns how
/// many rows it changed (inserted, updated or deleted).
pub async fn affecting_statement<E: Executor + ?Sized>(exec: &E, statement: Raw) -> Result<u64> {
    let target = exec.target(Access::Write).await?;
    let sql = sql_target(&target)?;
    let compiled = statement.to_statement(sql.dialect())?;
    Ok(sql.execute(&compiled).await?.rows_affected)
}

/// Runs `sql` as written, without binding values: several statements may
/// be separated by `;`. For DDL and scripts; never put user input in it.
pub async fn unprepared<E: Executor + ?Sized>(exec: &E, sql: &str) -> Result<()> {
    let target = exec.target(Access::Write).await?;
    let sql_target = sql_target(&target)?;
    sql_target.execute_unprepared(sql).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::render_select;

    #[test]
    fn raw_fragments_are_expressions_conditions_and_select_items() {
        let mut query = crate::ir::QueryIr::new("deals");
        query
            .columns
            .push(raw("count(*) AS n", [] as [i64; 0]).into_select());
        query.wheres.push(crate::ir::WhereClause::and(
            Filter::from(raw("a = ?", [1])).into_where(),
        ));
        let statement = render_select(&query, Dialect::Postgres).unwrap();
        assert_eq!(
            statement.sql,
            r#"SELECT count(*) AS n FROM "deals" WHERE a = $1"#
        );
        assert_eq!(statement.values, [Value::from(1)]);
    }

    #[test]
    fn statements_check_their_placeholders() {
        let statement = raw("SELECT ? + ?", [1, 2])
            .to_statement(Dialect::MySql)
            .unwrap();
        assert_eq!(statement.sql, "SELECT ? + ?");
        assert_eq!(statement.values.len(), 2);
        assert!(
            raw("SELECT ?", [] as [i64; 0])
                .to_statement(Dialect::Sqlite)
                .is_err()
        );
    }
}
