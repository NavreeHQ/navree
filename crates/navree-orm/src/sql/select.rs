//! `SELECT`, aggregates over a query, and `EXISTS`.

use sea_query::{Alias, Asterisk, JoinType, NullOrdering, SelectStatement, UnionType};

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
    let inner = compiler.select(&inner)?;
    let mut select = sea_query::Query::select();
    select.expr_as(sea_query::Expr::exists(inner), Alias::new("exists"));
    Ok(build(&select, dialect))
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
        let table = Alias::new(query.table.name.to_string());
        match &query.table.alias {
            Some(alias) => select.from_as(table, Alias::new(alias.to_string())),
            None => select.from(table),
        };

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
                            columns.push(match &column.table {
                                Some(table) => sea_query::ColumnRef::TableColumn(
                                    Alias::new(table.to_string()).into(),
                                    Alias::new(column.name.to_string()).into(),
                                ),
                                None => sea_query::ColumnRef::Column(
                                    Alias::new(column.name.to_string()).into(),
                                ),
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
            other => return Err(self.unsupported(format!("{other:?}"))),
        }

        for join in &query.joins {
            let kind = match join.kind {
                JoinKind::Inner => JoinType::InnerJoin,
                JoinKind::Left => JoinType::LeftJoin,
                // SQLite has RIGHT JOIN since 3.39 (the bundled library is newer).
                JoinKind::Right => JoinType::RightJoin,
                JoinKind::Cross => JoinType::CrossJoin,
                other => return Err(self.unsupported(format!("{other:?} joins"))),
            };
            let condition = self.clauses(&join.on)?;
            let table = Alias::new(join.table.name.to_string());
            match &join.table.alias {
                Some(alias) => {
                    select.join_as(kind, table, Alias::new(alias.to_string()), condition);
                }
                None => {
                    select.join(kind, table, condition);
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
                other => return Err(self.unsupported(format!("the order {other:?}"))),
            }
        }
        if let Some(limit) = query.limit {
            select.limit(limit);
        }
        if let Some(offset) = query.offset {
            if query.limit.is_none() && self.dialect != Dialect::Postgres {
                // MySQL and SQLite need a LIMIT before OFFSET.
                select.limit(u64::MAX >> 1);
            }
            select.offset(offset);
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
                    other => return Err(self.unsupported(format!("the lock {other:?}"))),
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
