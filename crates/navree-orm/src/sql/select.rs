//! `SELECT`, aggregates over a query, and `EXISTS`.

use sea_query::{
    Alias, Asterisk, Func, FunctionCall, IntoColumnRef, IntoIden, JoinType, NullOrdering,
    OrderedStatement, OverStatement, SelectStatement, TableName, UnionType, WindowStatement,
};

use super::{Compiler, Dialect, Statement, build, is_identifier};
use crate::error::OrmError;
use crate::ir::{
    Aggregate, ColumnRef, Direction, Distinct, Expr, GroupLimit, JoinKind, LockMode, LockWait,
    Nulls, Order, QueryIr, SelectItem, TableRef,
};

/// The derived table of a group limit, and its row number column.
const GROUP_TABLE: &str = "navree_table";
const GROUP_ROW: &str = "navree_row";

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
    let mut statement = build(&select, dialect);
    if dialect == Dialect::MySql
        && let Some(lock) = query.lock
        && matches!(lock.mode, LockMode::Share | LockMode::KeyShare)
        && lock.wait != LockWait::Wait
    {
        // MySQL 8 takes NOWAIT and SKIP LOCKED after `FOR SHARE` only.
        const LEGACY: &str = " LOCK IN SHARE MODE";
        if let Some(at) = statement.sql.rfind(LEGACY) {
            statement.sql.replace_range(at..at + LEGACY.len(), " FOR SHARE");
        }
    }
    timeout_hint(statement, query, dialect)
}

/// `statement` with MySQL's execution-time hint when `query` has a
/// timeout (`SELECT /*+ MAX_EXECUTION_TIME(ms) */ ..`). PostgreSQL's
/// timeout is set around the statement when it runs; SQLite has none.
fn timeout_hint(
    mut statement: Statement,
    query: &QueryIr,
    dialect: Dialect,
) -> Result<Statement, OrmError> {
    let Some(timeout) = query.timeout else {
        return Ok(statement);
    };
    if dialect != Dialect::MySql {
        return Ok(statement);
    }
    const SELECT: &str = "SELECT ";
    if !statement.sql.starts_with(SELECT) {
        return Err(OrmError::unsupported(
            "a statement timeout on a query that does not start with SELECT",
            "MySQL",
        ));
    }
    let millis = timeout_millis(timeout);
    statement
        .sql
        .insert_str(SELECT.len(), &format!("/*+ MAX_EXECUTION_TIME({millis}) */ "));
    Ok(statement)
}

/// `timeout` in whole milliseconds, rounded up (at least one).
pub(crate) fn timeout_millis(timeout: std::time::Duration) -> u128 {
    let millis = timeout.as_millis();
    if timeout > std::time::Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX)) {
        millis + 1
    } else {
        millis.max(1)
    }
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
        || query.offset.is_some()
        || query.group_limit.is_some();
    let select = if wrap {
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
    timeout_hint(build(&select, dialect), query, dialect)
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
    timeout_hint(build(&select, dialect), query, dialect)
}

/// Splits the trailing direction (and NULLs placement) off a raw order:
/// `"field(stage, ?) DESC NULLS LAST"` is `field(stage, ?)`, descending,
/// NULLs last. Without a direction the order is ascending, as in SQL.
fn split_direction(sql: &str) -> (&str, sea_query::Order, Option<NullOrdering>) {
    let mut body = sql.trim_end();
    let mut nulls = None;
    for (suffix, placement) in [
        (" nulls first", NullOrdering::First),
        (" nulls last", NullOrdering::Last),
    ] {
        if ends_with_ignore_case(body, suffix) {
            body = body[..body.len() - suffix.len()].trim_end();
            nulls = Some(placement);
            break;
        }
    }
    let mut direction = sea_query::Order::Asc;
    for (suffix, order) in [
        (" desc", sea_query::Order::Desc),
        (" asc", sea_query::Order::Asc),
    ] {
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
        match &query.group_limit {
            Some(group) => self.group_limited(query, group),
            None => self.select_core(query),
        }
    }

    /// A table, subquery or table function as sea-query names it.
    fn table_ref(&self, table: &TableRef) -> Result<sea_query::TableRef, OrmError> {
        let qualifier = || Alias::new(table.qualifier().to_owned()).into_iden();
        if let Some(subquery) = &table.subquery {
            return Ok(sea_query::TableRef::SubQuery(
                Box::new(self.select(subquery)?),
                qualifier(),
            ));
        }
        if let Some(function) = &table.function {
            return Ok(sea_query::TableRef::FunctionCall(
                self.function_call(function)?,
                qualifier(),
            ));
        }
        Ok(sea_query::TableRef::Table(
            TableName(None, Alias::new(table.name.to_string()).into_iden()),
            table
                .alias
                .as_ref()
                .map(|alias| Alias::new(alias.to_string()).into_iden()),
        ))
    }

    /// A set-returning function call used as a table.
    fn function_call(&self, function: &Expr) -> Result<FunctionCall, OrmError> {
        match function {
            Expr::Function { name, args } if is_identifier(name) => {
                let args = args
                    .iter()
                    .map(|arg| self.expr(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Func::cust(Alias::new(name.to_string())).args(args))
            }
            Expr::Function { name, .. } => Err(OrmError::invalid(format!(
                "`{name}` is not a valid SQL function name"
            ))),
            other => Err(OrmError::invalid(format!(
                "a table function must be a function call, not {other:?}"
            ))),
        }
    }

    /// Adds `orders` to `target` (a select or a window). With `unqualified`,
    /// plain columns lose their table: the orders of a union name its
    /// result columns.
    fn apply_orders<S: OrderedStatement>(
        &self,
        target: &mut S,
        orders: &[Order],
        unqualified_columns: bool,
    ) -> Result<(), OrmError> {
        for order in orders {
            match order {
                Order::By {
                    expr,
                    direction,
                    nulls,
                } => {
                    let expr = if unqualified_columns {
                        self.expr(&unqualified(expr))?
                    } else {
                        self.expr(expr)?
                    };
                    let direction = match direction {
                        Direction::Asc => sea_query::Order::Asc,
                        Direction::Desc => sea_query::Order::Desc,
                    };
                    match nulls {
                        None => {
                            target.order_by_expr(expr, direction);
                        }
                        Some(nulls) => {
                            let nulls = match nulls {
                                Nulls::First => NullOrdering::First,
                                Nulls::Last => NullOrdering::Last,
                            };
                            target.order_by_expr_with_nulls(expr, direction, nulls);
                        }
                    }
                }
                Order::Random => {
                    let random = match self.dialect {
                        Dialect::MySql => "RAND()",
                        Dialect::Postgres | Dialect::Sqlite => "RANDOM()",
                    };
                    target.order_by_expr(sea_query::Expr::cust(random), sea_query::Order::Asc);
                }
                Order::Raw { sql, values } => {
                    let (body, direction, nulls) = split_direction(sql);
                    let body = super::raw_sql(body, values.len(), self.dialect)?;
                    let expr = sea_query::Expr::cust_with_values(body, values.iter().cloned());
                    match nulls {
                        Some(nulls) => {
                            target.order_by_expr_with_nulls(expr, direction, nulls);
                        }
                        None => {
                            target.order_by_expr(expr, direction);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// A query with a per-group limit: the query with `ROW_NUMBER() OVER
    /// (PARTITION BY column ORDER BY <its orders>)` in a derived table, and
    /// the rows ranked within the limit (after the offset) of each group.
    fn group_limited(&self, query: &QueryIr, group: &GroupLimit) -> Result<SelectStatement, OrmError> {
        use sea_query::ExprTrait as _;

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
        let mut window = WindowStatement::new();
        window.add_partition_by(self.expr(&group.column)?);
        self.apply_orders(&mut window, &orders, false)?;
        ranked.expr_window_as(
            sea_query::Expr::cust("ROW_NUMBER()"),
            window,
            Alias::new(GROUP_ROW),
        );

        let mut outer = sea_query::Query::select();
        match names {
            Some(names) => {
                for name in names {
                    outer.column((Alias::new(GROUP_TABLE), Alias::new(name)));
                }
            }
            None => {
                outer.column(Asterisk);
            }
        }
        outer.from_subquery(ranked, Alias::new(GROUP_TABLE));
        let row = || sea_query::Expr::col((Alias::new(GROUP_TABLE), Alias::new(GROUP_ROW)));
        let bound = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        outer.and_where(row().lte(bound(group.limit.saturating_add(offset))));
        if offset > 0 {
            outer.and_where(row().gt(bound(offset)));
        }
        outer.order_by_expr(row(), sea_query::Order::Asc);
        if let Some(limit) = limit {
            outer.limit(std::cmp::min(limit, i64::MAX as u64));
        }
        Ok(outer)
    }

    /// A read query without a group limit.
    fn select_core(&self, query: &QueryIr) -> Result<SelectStatement, OrmError> {
        query.check()?;
        let mut select = sea_query::Query::select();
        select.from(self.table_ref(&query.table)?);

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
            let condition = self.clauses(&join.on)?;
            let kind = match join.kind {
                JoinKind::Inner | JoinKind::Lateral => JoinType::InnerJoin,
                JoinKind::Left | JoinKind::LeftLateral => JoinType::LeftJoin,
                // SQLite has RIGHT JOIN since 3.39 (the bundled library is newer).
                JoinKind::Right => JoinType::RightJoin,
                JoinKind::Cross => JoinType::CrossJoin,
            };
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
                let condition = if join.on.is_empty() {
                    sea_query::Condition::all().add(sea_query::Expr::cust("true"))
                } else {
                    condition
                };
                select.join_lateral(
                    kind,
                    self.select(subquery)?,
                    Alias::new(join.table.qualifier().to_owned()),
                    condition,
                );
                continue;
            }
            select.join(kind, self.table_ref(&join.table)?, condition);
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
        // The orders of a union sort its result: they name result columns.
        self.apply_orders(&mut select, &query.orders, !query.unions.is_empty())?;
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
                let kind = match (self.dialect, lock.mode) {
                    (Dialect::Postgres, LockMode::NoKeyUpdate) => sea_query::LockType::NoKeyUpdate,
                    (Dialect::Postgres, LockMode::KeyShare) => sea_query::LockType::KeyShare,
                    (_, LockMode::Share | LockMode::KeyShare) => sea_query::LockType::Share,
                    _ => sea_query::LockType::Update,
                };
                match lock.wait {
                    LockWait::Wait => {
                        select.lock(kind);
                    }
                    LockWait::NoWait => {
                        select.lock_with_behavior(kind, sea_query::LockBehavior::Nowait);
                    }
                    LockWait::SkipLocked => {
                        select.lock_with_behavior(kind, sea_query::LockBehavior::SkipLocked);
                    }
                }
            }
        }
        for (index, union) in query.unions.iter().enumerate() {
            let mut part = self.select(&union.query)?;
            // SQLite writes union parts bare, where an order or a limit of
            // their own would end the whole statement: such a part becomes
            // a derived table.
            let own_window = !union.query.orders.is_empty()
                || union.query.limit.is_some()
                || union.query.offset.is_some();
            if self.dialect == Dialect::Sqlite && own_window {
                let mut wrapped = sea_query::Query::select();
                wrapped
                    .column(Asterisk)
                    .from_subquery(part, Alias::new(format!("navree_union_{}", index + 1)));
                part = wrapped;
            }
            select.union(
                if union.all {
                    UnionType::All
                } else {
                    UnionType::Distinct
                },
                part,
            );
        }
        Ok(select)
    }
}

