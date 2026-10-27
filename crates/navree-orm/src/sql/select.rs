//! `SELECT`, aggregates over a query, and `EXISTS`.

use sea_query::{
    Alias, Asterisk, IntoColumnRef, JoinType, NullOrdering, SelectStatement, UnionType,
};

use super::{Compiler, Dialect, Statement, build};
use crate::error::OrmError;
use crate::ir::{
    Aggregate, ColumnRef, Direction, Distinct, Expr, JoinKind, Lock, Nulls, Order, QueryIr,
};

/// The SQL of a read query.
///
/// ```
/// use navree_orm::ir::QueryIr;
/// use navree_orm::sql::{Dialect, compile_select};
///
/// let query = QueryIr::new("deals");
/// assert_eq!(compile_select(&query, Dialect::MySql).unwrap().sql, "SELECT * FROM `deals`");
/// ```
pub fn compile_select(query: &QueryIr, dialect: Dialect) -> Result<Statement, OrmError> {
    let select = Compiler::new(dialect).select(query)?;
    Ok(build(&select, dialect))
}

/// The SQL of an aggregate over a query's rows, in a column named
/// `aggregate`. Queries with groups, `HAVING`, `DISTINCT`, unions, a limit
/// or an offset are aggregated as a subquery, so the result counts what the
/// query returns; others replace the select list (and drop the order).
pub fn compile_aggregate(
    query: &QueryIr,
    aggregate: &Aggregate,
    dialect: Dialect,
) -> Result<Statement, OrmError> {
    let compiler = Compiler::new(dialect);
    query.check()?;
    let wrap = !query.groups.is_empty()
        || !query.havings.is_empty()
        || !query.unions.is_empty()
        || !matches!(query.distinct, Distinct::No)
        || query.limit.is_some()
        || query.offset.is_some();
    let select = if wrap {
        let mut inner = query.clone();
        if inner.limit.is_none() && inner.offset.is_none() {
            inner.orders.clear();
        }
        inner.lock = None;
        // A grouped query without a select list counts its groups.
        if inner.columns.is_empty() && !inner.groups.is_empty() {
            inner.columns = inner
                .groups
                .iter()
                .cloned()
                .map(|expr| crate::ir::SelectItem { expr, alias: None })
                .collect();
        }
        let inner = compiler.select(&inner)?;
        let arg = aggregate.arg.as_ref().map(unqualified);
        let expr = compiler.expr(&Expr::Aggregate {
            function: aggregate.function,
            arg: arg.map(Box::new),
            distinct: aggregate.distinct,
        })?;
        let mut outer = sea_query::Query::select();
        outer
            .expr_as(expr, Alias::new("aggregate"))
            .from_subquery(inner, Alias::new("aggregate_table"));
        outer
    } else {
        let mut stripped = query.clone();
        stripped.columns.clear();
        stripped.orders.clear();
        stripped.lock = None;
        let mut select = compiler.select(&stripped)?;
        select.clear_selects();
        let expr = compiler.expr(&Expr::Aggregate {
            function: aggregate.function,
            arg: aggregate.arg.clone().map(Box::new),
            distinct: aggregate.distinct,
        })?;
        select.expr_as(expr, Alias::new("aggregate"));
        select
    };
    Ok(build(&select, dialect))
}

/// `SELECT EXISTS(query) AS "exists"`.
pub fn compile_exists(query: &QueryIr, dialect: Dialect) -> Result<Statement, OrmError> {
    let compiler = Compiler::new(dialect);
    let mut inner = query.clone();
    inner.orders.clear();
    inner.lock = None;
    super::filter::exists_select_list(&mut inner);
    let inner = compiler.select(&inner)?;
    let mut select = sea_query::Query::select();
    select.expr_as(sea_query::Expr::exists(inner), Alias::new("exists"));
    Ok(build(&select, dialect))
}

/// Splits the trailing direction (and NULLs placement) off a raw order:
/// `"field(stage, ?) DESC NULLS LAST"` is `field(stage, ?)`, descending,
/// NULLs last. Without a direction the order is ascending, as in SQL.
fn split_direction(sql: &str) -> (&str, sea_query::Order, Option<NullOrdering>) {
    let mut body = sql.trim_end();
    let mut nulls = None;
    for (suffix, placement) in [(" nulls first", NullOrdering::First), (" nulls last", NullOrdering::Last)] {
        if ends_with_ignore_case(body, suffix) {
            body = body[..body.len() - suffix.len()].trim_end();
            nulls = Some(placement);
            break;
        }
    }
    let mut direction = sea_query::Order::Asc;
    for (suffix, order) in [(" desc", sea_query::Order::Desc), (" asc", sea_query::Order::Asc)] {
        if ends_with_ignore_case(body, suffix) {
            body = body[..body.len() - suffix.len()].trim_end();
            direction = order;
            break;
        }
    }
    (body, direction, nulls)
}

fn ends_with_ignore_case(text: &str, suffix: &str) -> bool {
    text.len() >= suffix.len()
        && text.is_char_boundary(text.len() - suffix.len())
        && text[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// A column reference without its table (outside a subquery, the columns
/// of `aggregate_table` are unqualified).
fn unqualified(expr: &Expr) -> Expr {
    match expr {
        Expr::Column(column) => Expr::Column(ColumnRef {
            table: None,
            ..column.clone()
        }),
        other => other.clone(),
    }
}

impl Compiler {
    /// A read query as a sea-query statement.
    pub(crate) fn select(&self, query: &QueryIr) -> Result<SelectStatement, OrmError> {
        query.check()?;
        let mut select = sea_query::Query::select();
        match (&query.table.subquery, &query.table.alias) {
            (Some(subquery), _) => {
                let subquery = self.select(subquery)?;
                select.from_subquery(subquery, Alias::new(query.table.qualifier().to_owned()));
            }
            (None, Some(alias)) => {
                select.from_as(
                    Alias::new(query.table.name.to_string()),
                    Alias::new(alias.to_string()),
                );
            }
            (None, None) => {
                select.from(Alias::new(query.table.name.to_string()));
            }
        }

        if query.columns.is_empty() {
            if query.joins.is_empty() {
                select.column(Asterisk);
            } else {
                select.column((Alias::new(query.table.qualifier().to_owned()), Asterisk));
            }
        }
        for item in &query.columns {
            let expr = self.expr(&item.expr)?;
            match &item.alias {
                Some(alias) => select.expr_as(expr, Alias::new(alias.to_string())),
                None => select.expr(expr),
            };
        }

        match &query.distinct {
            Distinct::No => {}
            Distinct::Yes => {
                select.distinct();
            }
            Distinct::On(exprs) => {
                if self.dialect != Dialect::Postgres {
                    return Err(self.unsupported("DISTINCT ON"));
                }
                let mut columns = Vec::with_capacity(exprs.len());
                for expr in exprs {
                    match expr {
                        Expr::Column(column) if column.path.is_empty() => {
                            let name = Alias::new(column.name.to_string());
                            columns.push(match &column.table {
                                Some(table) => {
                                    (Alias::new(table.to_string()), name).into_column_ref()
                                }
                                None => name.into_column_ref(),
                            });
                        }
                        other => {
                            return Err(OrmError::invalid(format!(
                                "DISTINCT ON takes columns, not {other:?}"
                            )));
                        }
                    }
                }
                select.distinct_on(columns);
            }
        }

        for join in &query.joins {
            let kind = match join.kind {
                JoinKind::Inner => JoinType::InnerJoin,
                JoinKind::Left => JoinType::LeftJoin,
                // SQLite has RIGHT JOIN since 3.39 (the bundled library is newer).
                JoinKind::Right => JoinType::RightJoin,
                JoinKind::Cross => JoinType::CrossJoin,
            };
            let condition = self.clauses(&join.on)?;
            match (&join.table.subquery, &join.table.alias) {
                (Some(subquery), _) => {
                    let subquery = self.select(subquery)?;
                    select.join_subquery(
                        kind,
                        subquery,
                        Alias::new(join.table.qualifier().to_owned()),
                        condition,
                    );
                }
                (None, Some(alias)) => {
                    select.join_as(
                        kind,
                        Alias::new(join.table.name.to_string()),
                        Alias::new(alias.to_string()),
                        condition,
                    );
                }
                (None, None) => {
                    select.join(kind, Alias::new(join.table.name.to_string()), condition);
                }
            }
        }

        if !query.wheres.is_empty() {
            select.cond_where(self.clauses(&query.wheres)?);
        }
        if !query.groups.is_empty() {
            let groups = query
                .groups
                .iter()
                .map(|group| self.expr(group))
                .collect::<Result<Vec<_>, _>>()?;
            select.add_group_by(groups);
        }
        if !query.havings.is_empty() {
            select.cond_having(self.clauses(&query.havings)?);
        }
        for order in &query.orders {
            match order {
                Order::By {
                    expr,
                    direction,
                    nulls,
                } => {
                    let expr = self.expr(expr)?;
                    let direction = match direction {
                        Direction::Asc => sea_query::Order::Asc,
                        Direction::Desc => sea_query::Order::Desc,
                    };
                    match nulls {
                        None => {
                            select.order_by_expr(expr, direction);
                        }
                        Some(nulls) => {
                            let nulls = match nulls {
                                Nulls::First => NullOrdering::First,
                                Nulls::Last => NullOrdering::Last,
                            };
                            select.order_by_expr_with_nulls(expr, direction, nulls);
                        }
                    }
                }
                Order::Random => {
                    let random = match self.dialect {
                        Dialect::MySql => "RAND()",
                        Dialect::Postgres | Dialect::Sqlite => "RANDOM()",
                    };
                    select.order_by_expr(sea_query::Expr::cust(random), sea_query::Order::Asc);
                }
                Order::Raw { sql, values } => {
                    let (body, direction, nulls) = split_direction(sql);
                    let body = super::raw_sql(body, values.len(), self.dialect)?;
                    let expr = sea_query::Expr::cust_with_values(body, values.iter().cloned());
                    match nulls {
                        Some(nulls) => {
                            select.order_by_expr_with_nulls(expr, direction, nulls);
                        }
                        None => {
                            select.order_by_expr(expr, direction);
                        }
                    }
                }
            }
        }
        // Bound as signed 64-bit integers on every database: larger counts
        // mean "no limit" anyway.
        const LARGEST: u64 = i64::MAX as u64;
        if let Some(limit) = query.limit {
            select.limit(limit.min(LARGEST));
        }
        if let Some(offset) = query.offset {
            if query.limit.is_none() && self.dialect != Dialect::Postgres {
                // MySQL and SQLite need a LIMIT before OFFSET.
                select.limit(LARGEST);
            }
            select.offset(offset.min(LARGEST));
        }
        if let Some(lock) = query.lock {
            // SQLite has no row locks: its write transactions are serialised
            // (navree-db begins them IMMEDIATE).
            if self.dialect != Dialect::Sqlite {
                match lock {
                    Lock::Update => {
                        select.lock(sea_query::LockType::Update);
                    }
                    Lock::Share => {
                        select.lock_shared();
                    }
                }
            }
        }
        for union in &query.unions {
            let other = self.select(&union.query)?;
            select.union(
                if union.all {
                    UnionType::All
                } else {
                    UnionType::Distinct
                },
                other,
            );
        }
        Ok(select)
    }
}
