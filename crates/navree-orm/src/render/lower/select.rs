//! `SELECT` (derived tables, table functions, lateral joins, limits per
//! group, unions), aggregates over a query, and `EXISTS`.

use std::time::Duration;

use super::{Lowerer, aggregate_call, filter, unqualify};
use crate::error::{OrmError, Result};
use crate::ir::{
    Aggregate, Distinct, Expr, GroupLimit, JoinKind, LockMode, LockWait, QueryIr, SelectItem,
    TableRef,
};
use crate::render::ast;
use crate::render::grammar::is_identifier;
use crate::sql::{Dialect, Syntax};
use crate::value::Value;

/// The derived table of a group limit, and its row number column.
const GROUP_TABLE: &str = "navree_table";
const GROUP_ROW: &str = "navree_row";

/// A read query as a tree (MySQL's timeout hint included).
pub fn select(query: &QueryIr, syntax: impl Into<Syntax>) -> Result<ast::Query> {
    let lowerer = Lowerer::new(syntax);
    let mut lowered = lowerer.select(query)?;
    lowerer.hint(&mut lowered, query);
    Ok(lowered)
}

/// An aggregate over a query's rows, in a column named `aggregate`.
/// Queries with groups, `HAVING`, `DISTINCT`, unions, a limit, an offset
/// or a per-group limit are aggregated as a derived table, so the result
/// counts what the query returns; others replace the select list (and
/// drop the order).
pub fn aggregate(
    query: &QueryIr,
    aggregate: &Aggregate,
    syntax: impl Into<Syntax>,
) -> Result<ast::Query> {
    let lowerer = Lowerer::new(syntax);
    query.check()?;
    let wrap = !query.groups.is_empty()
        || !query.havings.is_empty()
        || !query.unions.is_empty()
        || !matches!(query.distinct, Distinct::No)
        || query.limit.is_some()
        || query.offset.is_some()
        || query.group_limit.is_some();
    let mut lowered = if wrap {
        let mut inner = query.clone();
        // Orders matter when they pick the rows: under a limit, an offset
        // or a per-group limit.
        if inner.limit.is_none() && inner.offset.is_none() && inner.group_limit.is_none() {
            inner.orders.clear();
        }
        inner.lock = None;
        // A grouped query without a select list counts its groups.
        if inner.columns.is_empty() && !inner.groups.is_empty() {
            inner.columns = inner
                .groups
                .iter()
                .cloned()
                .map(|expr| SelectItem { expr, alias: None })
                .collect();
        }
        let inner = lowerer.select(&inner)?;
        let arg = match aggregate.arg.as_ref().map(unqualify) {
            Some(arg) => lowerer.expr(&arg)?,
            None => ast::Expr::Wildcard(None),
        };
        let expr = aggregate_call(aggregate.function, arg, aggregate.distinct);
        ast::Query::from(
            ast::Select::from_source(ast::Source::derived(inner, "aggregate_table"))
                .column_as(expr, "aggregate"),
        )
    } else {
        let mut stripped = query.clone();
        stripped.columns.clear();
        stripped.orders.clear();
        stripped.lock = None;
        let mut lowered = lowerer.select(&stripped)?;
        let arg = match &aggregate.arg {
            Some(arg) => lowerer.expr(arg)?,
            None => ast::Expr::Wildcard(None),
        };
        let expr = aggregate_call(aggregate.function, arg, aggregate.distinct);
        if let Some(select) = leftmost_select(&mut lowered.body) {
            select.items = vec![ast::SelectItem::aliased(expr, "aggregate")];
        }
        lowered
    };
    lowerer.hint(&mut lowered, query);
    Ok(lowered)
}

/// `SELECT EXISTS(query) AS "exists"`.
pub fn exists(query: &QueryIr, syntax: impl Into<Syntax>) -> Result<ast::Query> {
    let lowerer = Lowerer::new(syntax);
    let mut inner = query.clone();
    inner.orders.clear();
    inner.lock = None;
    filter::exists_select_list(&mut inner);
    let inner = lowerer.select(&inner)?;
    let mut lowered =
        ast::Query::from(ast::Select::new().column_as(ast::Expr::exists(inner), "exists"));
    lowerer.hint(&mut lowered, query);
    Ok(lowered)
}

/// The first `SELECT` of a body (where a select list is replaced, or the
/// row number of a group limit added).
fn leftmost_select(body: &mut ast::SetExpr) -> Option<&mut ast::Select> {
    match body {
        ast::SetExpr::Select(select) => Some(select),
        ast::SetExpr::Query(query) => leftmost_select(&mut query.body),
        ast::SetExpr::Operation { left, .. } => leftmost_select(left),
    }
}

/// The name a select item comes back under: its alias, or a plain column's
/// name; `None` for an unnamed expression.
fn output_name(item: &SelectItem) -> Option<String> {
    if let Some(alias) = &item.alias {
        return Some(alias.to_string());
    }
    match &item.expr {
        Expr::Column(column) if column.path.is_empty() => Some(column.name.to_string()),
        _ => None,
    }
}

/// `timeout` in whole milliseconds, rounded up (at least one).
pub(crate) fn timeout_millis(timeout: Duration) -> u64 {
    let millis = timeout.as_millis();
    let rounded = if timeout > Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX)) {
        millis + 1
    } else {
        millis.max(1)
    };
    u64::try_from(rounded).unwrap_or(u64::MAX)
}

impl Lowerer {
    /// MySQL's execution-time hint for a query with a timeout; PostgreSQL
    /// sets its timeout around the statement when it runs, SQLite has
    /// none.
    fn hint(&self, lowered: &mut ast::Query, query: &QueryIr) {
        if self.dialect == Dialect::MySql
            && let Some(timeout) = query.timeout
        {
            lowered.max_execution_time = Some(timeout_millis(timeout));
        }
    }

    /// A read query.
    pub(crate) fn select(&self, query: &QueryIr) -> Result<ast::Query> {
        match &query.group_limit {
            Some(group) => self.group_limited(query, group),
            None => self.select_core(query),
        }
    }

    /// A table, derived table or table function.
    fn table_ref(&self, table: &TableRef) -> Result<ast::Source> {
        let qualifier = || ast::Ident::new(table.qualifier().to_owned());
        if let Some(subquery) = &table.subquery {
            return Ok(ast::Source::Derived {
                query: Box::new(self.select(subquery)?),
                alias: qualifier(),
                lateral: false,
            });
        }
        if let Some(function) = &table.function {
            return Ok(ast::Source::Function {
                call: Box::new(self.function_call(function)?),
                alias: Some(qualifier()),
            });
        }
        Ok(ast::Source::Table {
            name: ast::ObjectName::parse(table.name.clone()),
            alias: table.alias.clone().map(ast::Ident),
        })
    }

    /// A set-returning function call used as a table.
    fn function_call(&self, function: &Expr) -> Result<ast::Expr> {
        match function {
            Expr::Function { name, args } if is_identifier(name) => {
                let args = args
                    .iter()
                    .map(|arg| self.expr(arg))
                    .collect::<Result<Vec<_>>>()?;
                Ok(ast::Expr::func(name.clone(), args))
            }
            Expr::Function { name, .. } => Err(OrmError::invalid(format!(
                "`{name}` is not a valid SQL function name"
            ))),
            other => Err(OrmError::invalid(format!(
                "a table function must be a function call, not {other:?}"
            ))),
        }
    }

    /// A query with a per-group limit: the query with `ROW_NUMBER() OVER
    /// (PARTITION BY column ORDER BY <its orders>)` in a derived table, and
    /// the rows ranked within the limit (after the offset) of each group.
    fn group_limited(&self, query: &QueryIr, group: &GroupLimit) -> Result<ast::Query> {
        query.check()?;
        let mut inner = query.clone();
        inner.group_limit = None;
        inner.lock = None;
        let orders = std::mem::take(&mut inner.orders);
        let offset = inner.offset.take().unwrap_or(0);
        let limit = inner.limit.take();
        if inner.columns.is_empty() {
            inner.columns = vec![SelectItem {
                expr: Expr::All {
                    table: Some(inner.table.qualifier().to_owned().into()),
                },
                alias: None,
            }];
        }
        let names: Option<Vec<String>> = inner.columns.iter().map(output_name).collect();
        let mut ranked = self.select_core(&inner)?;
        let window = ast::Expr::row_number().over(
            vec![self.expr(&group.column)?],
            self.orders(&orders, false)?,
        );
        if let Some(select) = leftmost_select(&mut ranked.body) {
            select
                .items
                .push(ast::SelectItem::aliased(window, GROUP_ROW));
        }

        let mut outer = ast::Select::from_source(ast::Source::derived(ranked, GROUP_TABLE));
        match names {
            Some(names) => {
                for name in names {
                    outer = outer.column(ast::Expr::qualified(
                        ast::ObjectName::single(GROUP_TABLE),
                        name,
                    ));
                }
            }
            None => outer = outer.column(ast::Expr::Wildcard(None)),
        }
        let row = || ast::Expr::qualified(ast::ObjectName::single(GROUP_TABLE), GROUP_ROW);
        let bound = |n: u64| Value::from(i64::try_from(n).unwrap_or(i64::MAX));
        outer = outer.filter(row().lte(bound(group.limit.saturating_add(offset))));
        if offset > 0 {
            outer = outer.filter(row().gt(bound(offset)));
        }
        let mut lowered = ast::Query::from(outer).order_by(row().asc());
        lowered.limit = limit;
        Ok(lowered)
    }

    /// A read query without a group limit.
    fn select_core(&self, query: &QueryIr) -> Result<ast::Query> {
        query.check()?;
        let mut select = ast::Select::from_source(self.table_ref(&query.table)?);

        if query.columns.is_empty() {
            select.items.push(ast::SelectItem::new(ast::Expr::Wildcard(
                (!query.joins.is_empty())
                    .then(|| ast::ObjectName::parse(query.table.qualifier().to_owned())),
            )));
        }
        for item in &query.columns {
            select.items.push(ast::SelectItem {
                expr: self.expr(&item.expr)?,
                alias: item.alias.clone().map(ast::Ident),
            });
        }

        select.distinct = match &query.distinct {
            Distinct::No => ast::Distinct::All,
            Distinct::Yes => ast::Distinct::Distinct,
            Distinct::On(exprs) => {
                if self.dialect != Dialect::Postgres {
                    return Err(self.unsupported("DISTINCT ON"));
                }
                let mut columns = Vec::with_capacity(exprs.len());
                for expr in exprs {
                    match expr {
                        Expr::Column(column) if column.path.is_empty() => {
                            columns.push(self.plain_column(column));
                        }
                        other => {
                            return Err(OrmError::invalid(format!(
                                "DISTINCT ON takes columns, not {other:?}"
                            )));
                        }
                    }
                }
                ast::Distinct::On(columns)
            }
        };

        for join in &query.joins {
            let condition = self.clauses(&join.on)?;
            let on = (!join.on.is_empty()).then_some(condition);
            if matches!(join.kind, JoinKind::Lateral | JoinKind::LeftLateral) {
                if self.dialect == Dialect::Sqlite {
                    return Err(self.unsupported("a lateral join"));
                }
                let Some(subquery) = &join.table.subquery else {
                    return Err(OrmError::invalid(format!(
                        "a lateral join takes a subquery, not the table `{}`",
                        join.table.name
                    )));
                };
                select.joins.push(ast::Join {
                    kind: if join.kind == JoinKind::Lateral {
                        ast::JoinKind::Inner
                    } else {
                        ast::JoinKind::Left
                    },
                    source: ast::Source::Derived {
                        query: Box::new(self.select(subquery)?),
                        alias: ast::Ident::new(join.table.qualifier().to_owned()),
                        lateral: true,
                    },
                    on,
                });
                continue;
            }
            let kind = match join.kind {
                // A cross join with conditions is an inner join on them.
                JoinKind::Cross if on.is_some() => ast::JoinKind::Inner,
                JoinKind::Cross => ast::JoinKind::Cross,
                JoinKind::Left => ast::JoinKind::Left,
                // SQLite has RIGHT JOIN since 3.39 (the bundled library is newer).
                JoinKind::Right => ast::JoinKind::Right,
                _ => ast::JoinKind::Inner,
            };
            select.joins.push(ast::Join {
                kind,
                source: self.table_ref(&join.table)?,
                on,
            });
        }

        if !query.wheres.is_empty() {
            select.filter = Some(self.clauses(&query.wheres)?);
        }
        for group in &query.groups {
            select.group_by.push(self.expr(group)?);
        }
        if !query.havings.is_empty() {
            select.having = Some(self.clauses(&query.havings)?);
        }

        let mut lowered = ast::Query::from(select);
        // The orders of a union sort its result: they name result columns.
        lowered.order_by = self.orders(&query.orders, !query.unions.is_empty())?;
        lowered.limit = query.limit;
        lowered.offset = query.offset;
        if let Some(lock) = query.lock
            && self.dialect != Dialect::Sqlite
        {
            // SQLite has no row locks: its write transactions are serialised
            // (navree-db begins them IMMEDIATE).
            lowered.lock = Some(ast::Lock {
                strength: match lock.mode {
                    LockMode::NoKeyUpdate => ast::LockStrength::NoKeyUpdate,
                    LockMode::Share => ast::LockStrength::Share,
                    LockMode::KeyShare => ast::LockStrength::KeyShare,
                    _ => ast::LockStrength::Update,
                },
                of: Vec::new(),
                wait: match lock.wait {
                    LockWait::NoWait => ast::LockWait::NoWait,
                    LockWait::SkipLocked => ast::LockWait::SkipLocked,
                    _ => ast::LockWait::Wait,
                },
            });
        }
        for union in &query.unions {
            let part = self.select(&union.query)?;
            let body = std::mem::replace(&mut lowered.body, ast::SetExpr::Select(Box::default()));
            lowered.body = ast::SetExpr::Operation {
                op: ast::SetOperator::Union,
                all: union.all,
                left: Box::new(body),
                right: Box::new(part.into_set_expr()),
            };
        }
        Ok(lowered)
    }
}
