//! Grammars: how each dialect writes the [syntax tree](super::ast).
//!
//! [`Grammar`] holds the SQL every database shares as default methods;
//! [`Postgres`], [`MySql`] and [`Sqlite`] override what differs (quoting
//! and placeholders live in the [`Writer`]). Expressions are parenthesized
//! by [`precedence`]: an operand binding more loosely than its position
//! needs is wrapped, raw fragments and templates whenever they are an
//! operand.

use std::borrow::Cow;

use super::ast::{
    Assignment, BinaryOp, ConflictAction, Delete, Distinct, Expr, Function, Ident, Insert,
    InsertSource, Join, JoinKind, JsonMode, LikeCase, Literal, Lock, LockStrength, LockWait,
    ObjectName, OnConflict, OrderBy, Query, Select, SelectItem, SetExpr, SetOperator, Source,
    SqlType, Statement, Truncate, Update, With,
};
use super::writer::{Token, Writer, scan};
use crate::error::{OrmError, Result};
use crate::ir::JsonSegment;
use crate::sql::{self, Dialect};

mod mysql;
mod postgres;
mod sqlite;

pub use mysql::MySql;
pub use postgres::Postgres;
pub use sqlite::Sqlite;

/// How tightly an expression binds: an operand whose precedence is below
/// what its position needs is parenthesized.
pub mod precedence {
    /// A raw fragment or template: parenthesized as any operand.
    pub const LOWEST: u8 = 0;
    /// `OR`.
    pub const OR: u8 = 1;
    /// `AND`.
    pub const AND: u8 = 2;
    /// `NOT`.
    pub const NOT: u8 = 3;
    /// Comparisons, `IS NULL`, `IN`, `BETWEEN`, `LIKE`.
    pub const COMPARISON: u8 = 4;
    /// Bit operators, concatenation and operators written as is: their
    /// operands are parenthesized unless they are atoms (the databases
    /// rank them differently).
    pub const OTHER: u8 = 5;
    /// `+`, `-`.
    pub const ADDITIVE: u8 = 6;
    /// `*`, `/`, `%`.
    pub const MULTIPLICATIVE: u8 = 7;
    /// A negation, `-x`.
    pub const UNARY: u8 = 8;
    /// Columns, values, calls, `CAST`, `CASE`, subqueries: never
    /// parenthesized.
    pub const ATOM: u8 = 9;
}

use precedence::{ADDITIVE, AND, ATOM, COMPARISON, LOWEST, MULTIPLICATIVE, NOT, OTHER, UNARY};

/// The grammar of `dialect`.
pub fn for_dialect(dialect: Dialect) -> &'static dyn Grammar {
    match dialect {
        Dialect::Postgres => &Postgres,
        Dialect::MySql => &MySql,
        Dialect::Sqlite => &Sqlite,
    }
}

/// `feature` is not supported on `dialect`.
pub(crate) fn unsupported(dialect: Dialect, feature: impl Into<Cow<'static, str>>) -> OrmError {
    OrmError::unsupported(feature, dialect.name())
}

/// Whether `name` is a plain (optionally schema-qualified) SQL identifier,
/// as function names must be.
pub(crate) fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// Writes the syntax tree for one dialect. Every method has the shared
/// SQL as its default; the dialects override what differs.
pub trait Grammar: Send + Sync {
    /// The dialect written.
    fn dialect(&self) -> Dialect;

    /// A statement. A truncation renders through
    /// [`truncate`](Grammar::truncate) (several statements on SQLite).
    fn statement(&self, w: &mut Writer, statement: &Statement) -> Result<()> {
        match statement {
            Statement::Query(query) => self.query(w, query),
            Statement::Insert(insert) => self.insert(w, insert),
            Statement::Update(update) => self.update(w, update),
            Statement::Delete(delete) => self.delete(w, delete),
            Statement::Truncate(truncate) => {
                let mut statements = self.truncate(truncate);
                if statements.len() != 1 {
                    return Err(OrmError::invalid(format!(
                        "emptying a table is {} statements on {}: render it with render_all",
                        statements.len(),
                        self.dialect().name()
                    )));
                }
                let statement = statements.remove(0);
                w.push(&statement.sql);
                for value in statement.values {
                    // A truncation binds nothing on databases with
                    // `TRUNCATE`; kept for completeness.
                    w.bind(value);
                }
                Ok(())
            }
        }
    }

    /// A query: `WITH`, the body, `ORDER BY`, `LIMIT`/`OFFSET`, the lock.
    fn query(&self, w: &mut Writer, query: &Query) -> Result<()> {
        if let Some(with) = &query.with {
            self.with(w, with)?;
        }
        self.set_expr(w, &query.body, query.max_execution_time)?;
        self.order_by(w, &query.order_by)?;
        self.limit_offset(w, query.limit, query.offset)?;
        if let Some(lock) = &query.lock {
            self.lock(w, lock)?;
        }
        Ok(())
    }

    /// `WITH [RECURSIVE] name (columns) AS (query), .. ` (with a trailing
    /// space).
    fn with(&self, w: &mut Writer, with: &With) -> Result<()> {
        if with.ctes.is_empty() {
            return Ok(());
        }
        w.push(if with.recursive {
            "WITH RECURSIVE "
        } else {
            "WITH "
        });
        for (i, cte) in with.ctes.iter().enumerate() {
            if i > 0 {
                w.push(", ");
            }
            w.identifier(&cte.name);
            if !cte.columns.is_empty() {
                w.push(" (");
                for (j, column) in cte.columns.iter().enumerate() {
                    if j > 0 {
                        w.push(", ");
                    }
                    w.identifier(column);
                }
                w.push(")");
            }
            w.push(" AS (");
            self.query(w, &cte.query)?;
            w.push(")");
        }
        w.push(" ");
        Ok(())
    }

    /// A query body. `hint` is the execution-time hint of the statement,
    /// written in its first `SELECT`.
    fn set_expr(&self, w: &mut Writer, body: &SetExpr, hint: Option<u64>) -> Result<()> {
        match body {
            SetExpr::Select(select) => self.select(w, select, hint),
            SetExpr::Query(_) => self.set_operand(w, body),
            SetExpr::Operation {
                op,
                all,
                left,
                right,
            } => {
                // Compounds read left to right; a left operand the dialect
                // would regroup, and any compound on the right, keep their
                // grouping in parentheses.
                match left.as_ref() {
                    SetExpr::Operation { op: inner, .. }
                        if !self.groups_left_operand(*op, *inner) =>
                    {
                        self.set_expr(w, left, hint)?;
                    }
                    SetExpr::Select(select) => self.select(w, select, hint)?,
                    other => self.set_operand(w, other)?,
                }
                w.push(self.set_operator(*op, *all)?);
                match right.as_ref() {
                    SetExpr::Select(select) => self.select(w, select, None),
                    other => self.set_operand(w, other),
                }
            }
        }
    }

    /// Whether a compound of `inner` on the left of `outer` needs its own
    /// parentheses: `INTERSECT` binds tighter than `UNION` and `EXCEPT`, so
    /// another operator keeps its grouping.
    fn groups_left_operand(&self, outer: SetOperator, inner: SetOperator) -> bool {
        outer != inner
    }

    /// A compound operator with its spaces: ` UNION ALL `.
    fn set_operator(&self, op: SetOperator, all: bool) -> Result<&'static str> {
        Ok(match (op, all) {
            (SetOperator::Union, false) => " UNION ",
            (SetOperator::Union, true) => " UNION ALL ",
            (SetOperator::Intersect, false) => " INTERSECT ",
            (SetOperator::Intersect, true) => " INTERSECT ALL ",
            (SetOperator::Except, false) => " EXCEPT ",
            (SetOperator::Except, true) => " EXCEPT ALL ",
        })
    }

    /// An operand of a compound that keeps clauses or a grouping of its
    /// own: `(SELECT .. LIMIT ?)` (SQLite: a derived table).
    fn set_operand(&self, w: &mut Writer, operand: &SetExpr) -> Result<()> {
        w.push("(");
        match operand {
            SetExpr::Query(query) => self.query(w, query)?,
            other => self.set_expr(w, other, None)?,
        }
        w.push(")");
        Ok(())
    }

    /// A `SELECT` without its order and limit.
    fn select(&self, w: &mut Writer, select: &Select, hint: Option<u64>) -> Result<()> {
        w.push("SELECT ");
        if let Some(millis) = hint {
            self.execution_time_hint(w, millis);
        }
        self.distinct(w, &select.distinct)?;
        if select.items.is_empty() {
            w.push("*");
        }
        self.select_items(w, &select.items)?;
        if !select.from.is_empty() {
            w.push(" FROM ");
            for (i, source) in select.from.iter().enumerate() {
                if i > 0 {
                    w.push(", ");
                }
                self.source(w, source)?;
            }
        }
        for join in &select.joins {
            self.join(w, join)?;
        }
        if let Some(filter) = &select.filter {
            w.push(" WHERE ");
            self.expr(w, filter)?;
        }
        if !select.group_by.is_empty() {
            w.push(" GROUP BY ");
            self.expr_list(w, &select.group_by)?;
        }
        if let Some(having) = &select.having {
            w.push(" HAVING ");
            self.expr(w, having)?;
        }
        Ok(())
    }

    /// A statement's execution-time hint (MySQL's `/*+
    /// MAX_EXECUTION_TIME(ms) */`); nothing by default.
    fn execution_time_hint(&self, w: &mut Writer, millis: u64) {
        let _ = (w, millis);
    }

    /// `DISTINCT ` (with a trailing space); `DISTINCT ON` is PostgreSQL's.
    fn distinct(&self, w: &mut Writer, distinct: &Distinct) -> Result<()> {
        match distinct {
            Distinct::All => Ok(()),
            Distinct::Distinct => {
                w.push("DISTINCT ");
                Ok(())
            }
            Distinct::On(_) => Err(unsupported(self.dialect(), "DISTINCT ON")),
        }
    }

    /// A select list (or `RETURNING` list): `expr [AS alias], ..`.
    fn select_items(&self, w: &mut Writer, items: &[SelectItem]) -> Result<()> {
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                w.push(", ");
            }
            self.expr(w, &item.expr)?;
            if let Some(alias) = &item.alias {
                w.push(" AS ");
                w.identifier(alias);
            }
        }
        Ok(())
    }

    /// Expressions separated by commas.
    fn expr_list(&self, w: &mut Writer, exprs: &[Expr]) -> Result<()> {
        for (i, expr) in exprs.iter().enumerate() {
            if i > 0 {
                w.push(", ");
            }
            self.expr(w, expr)?;
        }
        Ok(())
    }

    /// A table, derived table or table function.
    fn source(&self, w: &mut Writer, source: &Source) -> Result<()> {
        match source {
            Source::Table { name, alias } => {
                w.object_name(name);
                if let Some(alias) = alias {
                    w.push(" AS ");
                    w.identifier(alias);
                }
            }
            Source::Derived {
                query,
                alias,
                lateral,
            } => {
                if *lateral {
                    self.lateral(w)?;
                }
                w.push("(");
                self.query(w, query)?;
                w.push(") AS ");
                w.identifier(alias);
            }
            Source::Function { call, alias } => {
                let Expr::Function(function) = call.as_ref() else {
                    return Err(OrmError::invalid(format!(
                        "a table function must be a function call, not {call:?}"
                    )));
                };
                self.function(w, function)?;
                if let Some(alias) = alias {
                    w.push(" AS ");
                    w.identifier(alias);
                }
            }
        }
        Ok(())
    }

    /// `LATERAL ` (with a trailing space).
    fn lateral(&self, w: &mut Writer) -> Result<()> {
        w.push("LATERAL ");
        Ok(())
    }

    /// ` KIND JOIN source ON condition`; a join without a condition joins
    /// on `TRUE` (a cross join takes none).
    fn join(&self, w: &mut Writer, join: &Join) -> Result<()> {
        w.push(self.join_keyword(join.kind)?);
        self.source(w, &join.source)?;
        match (&join.on, join.kind) {
            (Some(_), JoinKind::Cross) => Err(OrmError::invalid(
                "a cross join takes no ON condition (use an inner join)",
            )),
            (Some(on), _) => {
                w.push(" ON ");
                self.expr(w, on)
            }
            (None, JoinKind::Cross) => Ok(()),
            (None, _) => {
                w.push(" ON ");
                self.bool_literal(w, true);
                Ok(())
            }
        }
    }

    /// The keyword of a join kind, with its spaces: ` INNER JOIN `.
    fn join_keyword(&self, kind: JoinKind) -> Result<&'static str> {
        Ok(match kind {
            JoinKind::Inner => " INNER JOIN ",
            JoinKind::Left => " LEFT JOIN ",
            JoinKind::Right => " RIGHT JOIN ",
            JoinKind::Full => " FULL JOIN ",
            JoinKind::Cross => " CROSS JOIN ",
        })
    }

    /// ` ORDER BY items` (nothing when there are none).
    fn order_by(&self, w: &mut Writer, items: &[OrderBy]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        w.push(" ORDER BY ");
        self.order_items(w, items)
    }

    /// Order items separated by commas.
    fn order_items(&self, w: &mut Writer, items: &[OrderBy]) -> Result<()> {
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                w.push(", ");
            }
            self.order_item(w, item)?;
        }
        Ok(())
    }

    /// `expr ASC|DESC [NULLS FIRST|LAST]`.
    fn order_item(&self, w: &mut Writer, item: &OrderBy) -> Result<()> {
        self.expr(w, &item.expr)?;
        w.push(if item.descending { " DESC" } else { " ASC" });
        match item.nulls {
            Some(super::ast::NullsOrder::First) => w.push(" NULLS FIRST"),
            Some(super::ast::NullsOrder::Last) => w.push(" NULLS LAST"),
            None => {}
        }
        Ok(())
    }

    /// Whether an `OFFSET` needs a `LIMIT` before it (MySQL, SQLite).
    fn offset_needs_limit(&self) -> bool {
        false
    }

    /// ` LIMIT ? OFFSET ?`, bound as 64-bit integers (larger counts mean
    /// "no limit" anyway).
    fn limit_offset(&self, w: &mut Writer, limit: Option<u64>, offset: Option<u64>) -> Result<()> {
        const LARGEST: u64 = i64::MAX as u64;
        let limit = match (limit, offset) {
            (None, Some(_)) if self.offset_needs_limit() => Some(LARGEST),
            (limit, _) => limit,
        };
        if let Some(limit) = limit {
            w.push(" LIMIT ");
            w.bind(crate::value::Value::from(limit.min(LARGEST)));
        }
        if let Some(offset) = offset {
            w.push(" OFFSET ");
            w.bind(crate::value::Value::from(offset.min(LARGEST)));
        }
        Ok(())
    }

    /// ` FOR UPDATE [OF ..] [NOWAIT | SKIP LOCKED]`.
    fn lock(&self, w: &mut Writer, lock: &Lock) -> Result<()> {
        w.push(match lock.strength {
            LockStrength::Update => " FOR UPDATE",
            LockStrength::NoKeyUpdate => " FOR NO KEY UPDATE",
            LockStrength::Share => " FOR SHARE",
            LockStrength::KeyShare => " FOR KEY SHARE",
        });
        self.lock_tail(w, lock);
        Ok(())
    }

    /// A lock's ` OF tables` and ` NOWAIT` / ` SKIP LOCKED`.
    fn lock_tail(&self, w: &mut Writer, lock: &Lock) {
        if !lock.of.is_empty() {
            w.push(" OF ");
            for (i, table) in lock.of.iter().enumerate() {
                if i > 0 {
                    w.push(", ");
                }
                w.object_name(table);
            }
        }
        match lock.wait {
            LockWait::Wait => {}
            LockWait::NoWait => w.push(" NOWAIT"),
            LockWait::SkipLocked => w.push(" SKIP LOCKED"),
        }
    }

    /// An `INSERT`.
    fn insert(&self, w: &mut Writer, insert: &Insert) -> Result<()> {
        w.push(self.insert_keyword(insert));
        w.object_name(&insert.table);
        let column_list = |w: &mut Writer| {
            if !insert.columns.is_empty() {
                w.push(" (");
                for (i, column) in insert.columns.iter().enumerate() {
                    if i > 0 {
                        w.push(", ");
                    }
                    w.identifier(column);
                }
                w.push(")");
            }
        };
        match &insert.source {
            InsertSource::Values(rows) => {
                if rows.is_empty() {
                    return Err(OrmError::invalid(format!(
                        "an insert into `{}` has no rows",
                        name_text(&insert.table)
                    )));
                }
                column_list(w);
                w.push(" VALUES ");
                for (i, row) in rows.iter().enumerate() {
                    if !insert.columns.is_empty() && row.len() != insert.columns.len() {
                        return Err(OrmError::invalid(format!(
                            "an insert into `{}` has {} columns but a row of {} values",
                            name_text(&insert.table),
                            insert.columns.len(),
                            row.len()
                        )));
                    }
                    if i > 0 {
                        w.push(", ");
                    }
                    w.push("(");
                    self.expr_list(w, row)?;
                    w.push(")");
                }
            }
            InsertSource::Query(query) => {
                column_list(w);
                w.push(" ");
                self.insert_query(w, query, insert.on_conflict.is_some())?;
            }
            InsertSource::DefaultValues => {
                if !insert.columns.is_empty() {
                    return Err(OrmError::invalid(format!(
                        "an insert of default values into `{}` names columns",
                        name_text(&insert.table)
                    )));
                }
                self.default_values(w);
            }
        }
        if let Some(conflict) = &insert.on_conflict {
            self.on_conflict(w, conflict)?;
        }
        self.returning(w, &insert.returning)
    }

    /// `INSERT INTO ` (MySQL: `INSERT IGNORE INTO ` for a skipping insert).
    fn insert_keyword(&self, insert: &Insert) -> &'static str {
        let _ = insert;
        "INSERT INTO "
    }

    /// The query of `INSERT .. SELECT`; `conflict` when a conflict clause
    /// follows (SQLite then needs a `WHERE`).
    fn insert_query(&self, w: &mut Writer, query: &Query, conflict: bool) -> Result<()> {
        let _ = conflict;
        self.query(w, query)
    }

    /// ` DEFAULT VALUES` (MySQL: ` () VALUES ()`).
    fn default_values(&self, w: &mut Writer) {
        w.push(" DEFAULT VALUES");
    }

    /// ` ON CONFLICT [(target)] DO NOTHING | DO UPDATE SET .. [WHERE ..]`.
    fn on_conflict(&self, w: &mut Writer, conflict: &OnConflict) -> Result<()> {
        w.push(" ON CONFLICT");
        if !conflict.target.is_empty() {
            w.push(" (");
            for (i, column) in conflict.target.iter().enumerate() {
                if i > 0 {
                    w.push(", ");
                }
                w.identifier(column);
            }
            w.push(")");
        }
        match &conflict.action {
            ConflictAction::Ignore => {
                w.push(" DO NOTHING");
                Ok(())
            }
            ConflictAction::Update { sets, filter } => {
                if sets.is_empty() {
                    return Err(OrmError::invalid("an upsert updates no column"));
                }
                if conflict.target.is_empty() && self.upsert_needs_target() {
                    return Err(OrmError::invalid(format!(
                        "an upsert on {} needs the unique columns its rows conflict on",
                        self.dialect().name()
                    )));
                }
                w.push(" DO UPDATE SET ");
                self.assignments(w, sets, None)?;
                if let Some(filter) = filter {
                    w.push(" WHERE ");
                    self.expr(w, filter)?;
                }
                Ok(())
            }
        }
    }

    /// Whether `DO UPDATE` needs a conflict target (PostgreSQL).
    fn upsert_needs_target(&self) -> bool {
        true
    }

    /// `column = value, ..`, the columns qualified by `qualifier` when
    /// given (MySQL's multi-table updates).
    fn assignments(
        &self,
        w: &mut Writer,
        sets: &[Assignment],
        qualifier: Option<&ObjectName>,
    ) -> Result<()> {
        for (i, set) in sets.iter().enumerate() {
            if i > 0 {
                w.push(", ");
            }
            if let Some(qualifier) = qualifier {
                w.object_name(qualifier);
                w.push(".");
            }
            w.identifier(&set.column);
            w.push(" = ");
            self.expr(w, &set.value)?;
        }
        Ok(())
    }

    /// ` RETURNING items` (nothing when there are none).
    fn returning(&self, w: &mut Writer, items: &[SelectItem]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        w.push(" RETURNING ");
        self.select_items(w, items)
    }

    /// An `UPDATE`: `UPDATE t [AS a] SET .. [FROM ..] [WHERE ..]
    /// [RETURNING ..]`.
    fn update(&self, w: &mut Writer, update: &Update) -> Result<()> {
        if update.sets.is_empty() {
            return Err(OrmError::invalid(format!(
                "an update of `{}` sets no column",
                name_text(&update.table)
            )));
        }
        if !update.order_by.is_empty() || update.limit.is_some() {
            return Err(unsupported(
                self.dialect(),
                "an update with ORDER BY or LIMIT (narrow the rows with a subquery)",
            ));
        }
        w.push("UPDATE ");
        w.object_name(&update.table);
        if let Some(alias) = &update.alias {
            w.push(" AS ");
            w.identifier(alias);
        }
        w.push(" SET ");
        self.assignments(w, &update.sets, None)?;
        if !update.from.is_empty() {
            w.push(" FROM ");
            for (i, source) in update.from.iter().enumerate() {
                if i > 0 {
                    w.push(", ");
                }
                self.source(w, source)?;
            }
        }
        if let Some(filter) = &update.filter {
            w.push(" WHERE ");
            self.expr(w, filter)?;
        }
        self.returning(w, &update.returning)
    }

    /// A `DELETE`: `DELETE FROM t [AS a] [USING ..] [WHERE ..] [RETURNING
    /// ..]`.
    fn delete(&self, w: &mut Writer, delete: &Delete) -> Result<()> {
        if !delete.order_by.is_empty() || delete.limit.is_some() {
            return Err(unsupported(
                self.dialect(),
                "a delete with ORDER BY or LIMIT (narrow the rows with a subquery)",
            ));
        }
        w.push("DELETE FROM ");
        w.object_name(&delete.table);
        if let Some(alias) = &delete.alias {
            w.push(" AS ");
            w.identifier(alias);
        }
        if !delete.using.is_empty() {
            w.push(" USING ");
            for (i, source) in delete.using.iter().enumerate() {
                if i > 0 {
                    w.push(", ");
                }
                self.source(w, source)?;
            }
        }
        if let Some(filter) = &delete.filter {
            w.push(" WHERE ");
            self.expr(w, filter)?;
        }
        self.returning(w, &delete.returning)
    }

    /// The statements that empty a table: `TRUNCATE TABLE t [RESTART
    /// IDENTITY] [CASCADE]`.
    fn truncate(&self, truncate: &Truncate) -> Vec<sql::Statement> {
        let mut w = Writer::new(self.dialect());
        w.push("TRUNCATE TABLE ");
        w.object_name(&truncate.table);
        if truncate.restart_identity {
            w.push(" RESTART IDENTITY");
        }
        if truncate.cascade {
            w.push(" CASCADE");
        }
        vec![w.finish()]
    }

    /// An expression where anything may stand (a clause, an argument, a
    /// list item).
    fn expr(&self, w: &mut Writer, expr: &Expr) -> Result<()> {
        match expr {
            Expr::Column(column) => {
                if let Some(table) = &column.table {
                    w.object_name(table);
                    w.push(".");
                }
                w.identifier(&column.name);
            }
            Expr::Value(value) => w.bind(value.clone()),
            Expr::Literal(literal) => self.literal(w, literal),
            Expr::Raw { sql, values } => w.raw(sql, values)?,
            Expr::Template { sql, args } => self.template(w, sql, args)?,
            Expr::Function(function) => self.function(w, function)?,
            Expr::Wildcard(table) => {
                if let Some(table) = table {
                    w.object_name(table);
                    w.push(".");
                }
                w.push("*");
            }
            Expr::Neg(operand) => {
                w.push("-");
                self.operand(w, operand, ATOM)?;
            }
            Expr::Binary { left, op, right } => self.binary(w, left, op, right)?,
            Expr::And(items) => self.junction(w, items, " AND ", AND, true)?,
            Expr::Or(items) => self.junction(w, items, " OR ", NOT, false)?,
            Expr::Not(operand) => {
                w.push("NOT ");
                self.operand(w, operand, ATOM)?;
            }
            Expr::IsNull { expr, negated } => {
                self.operand(w, expr, OTHER)?;
                w.push(if *negated { " IS NOT NULL" } else { " IS NULL" });
            }
            Expr::InList {
                expr,
                list,
                negated,
            } => {
                if list.is_empty() {
                    // `IN ()` is not SQL: an empty list matches nothing,
                    // and its negation everything.
                    w.push(if *negated { "1 = 1" } else { "1 = 0" });
                } else {
                    self.operand(w, expr, OTHER)?;
                    w.push(if *negated { " NOT IN (" } else { " IN (" });
                    self.expr_list(w, list)?;
                    w.push(")");
                }
            }
            Expr::InQuery {
                expr,
                query,
                negated,
            } => {
                self.operand(w, expr, OTHER)?;
                w.push(if *negated { " NOT IN (" } else { " IN (" });
                self.query(w, query)?;
                w.push(")");
            }
            Expr::Between {
                expr,
                low,
                high,
                negated,
            } => {
                self.operand(w, expr, OTHER)?;
                w.push(if *negated {
                    " NOT BETWEEN "
                } else {
                    " BETWEEN "
                });
                self.operand(w, low, OTHER)?;
                w.push(" AND ");
                self.operand(w, high, OTHER)?;
            }
            Expr::Like {
                expr,
                pattern,
                escape,
                negated,
                case,
            } => self.like(w, expr, pattern, *escape, *negated, *case)?,
            Expr::Exists { query, negated } => {
                w.push(if *negated { "NOT EXISTS (" } else { "EXISTS (" });
                self.query(w, query)?;
                w.push(")");
            }
            Expr::Cast { expr, ty } => self.cast(w, expr, ty)?,
            Expr::Subquery(query) => {
                w.push("(");
                self.query(w, query)?;
                w.push(")");
            }
            Expr::Tuple(items) => {
                w.push("(");
                self.expr_list(w, items)?;
                w.push(")");
            }
            Expr::Case {
                operand,
                whens,
                otherwise,
            } => {
                if whens.is_empty() {
                    return Err(OrmError::invalid("a CASE needs at least one WHEN"));
                }
                w.push("CASE");
                if let Some(operand) = operand {
                    w.push(" ");
                    self.expr(w, operand)?;
                }
                for (condition, result) in whens {
                    w.push(" WHEN ");
                    self.expr(w, condition)?;
                    w.push(" THEN ");
                    self.expr(w, result)?;
                }
                if let Some(otherwise) = otherwise {
                    w.push(" ELSE ");
                    self.expr(w, otherwise)?;
                }
                w.push(" END");
            }
            Expr::JsonPath { expr, path, mode } => self.json_path(w, expr, path, *mode)?,
            Expr::Excluded(column) => self.excluded(w, column)?,
            Expr::Window {
                function,
                partition_by,
                order_by,
            } => {
                self.operand(w, function, ATOM)?;
                w.push(" OVER (");
                if !partition_by.is_empty() {
                    w.push("PARTITION BY ");
                    self.expr_list(w, partition_by)?;
                }
                if !order_by.is_empty() {
                    if !partition_by.is_empty() {
                        w.push(" ");
                    }
                    w.push("ORDER BY ");
                    self.order_items(w, order_by)?;
                }
                w.push(")");
            }
            Expr::Random => self.random(w),
            Expr::Default => self.default_keyword(w)?,
        }
        Ok(())
    }

    /// `items` joined by `separator`, each an operand of at least `min`;
    /// one item stands alone, none is `TRUE` (`and`) or `FALSE`.
    fn junction(
        &self,
        w: &mut Writer,
        items: &[Expr],
        separator: &str,
        min: u8,
        and: bool,
    ) -> Result<()> {
        match items {
            [] => {
                self.bool_literal(w, and);
                Ok(())
            }
            [single] => self.expr(w, single),
            items => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        w.push(separator);
                    }
                    self.operand(w, item, min)?;
                }
                Ok(())
            }
        }
    }

    /// `expr` as an operand needing precedence `min`: parenthesized when
    /// it binds more loosely.
    fn operand(&self, w: &mut Writer, expr: &Expr, min: u8) -> Result<()> {
        if self.precedence(expr) < min {
            w.push("(");
            self.expr(w, expr)?;
            w.push(")");
            Ok(())
        } else {
            self.expr(w, expr)
        }
    }

    /// How tightly `expr` binds, as written by this grammar.
    fn precedence(&self, expr: &Expr) -> u8 {
        match expr {
            Expr::Raw { .. } | Expr::Template { .. } => LOWEST,
            Expr::Literal(Literal::Int(value)) if *value < 0 => UNARY,
            Expr::Neg(_) => UNARY,
            Expr::Binary { op, .. } => self.binary_precedence(op),
            Expr::And(items) => match items.as_slice() {
                [] => ATOM,
                [single] => self.precedence(single),
                _ => AND,
            },
            Expr::Or(items) => match items.as_slice() {
                [] => ATOM,
                [single] => self.precedence(single),
                _ => precedence::OR,
            },
            Expr::Not(_) | Expr::Exists { negated: true, .. } => NOT,
            Expr::IsNull { .. }
            | Expr::InList { .. }
            | Expr::InQuery { .. }
            | Expr::Between { .. }
            | Expr::Like { .. } => COMPARISON,
            Expr::JsonPath {
                mode: JsonMode::Json,
                path,
                ..
            } if !path.is_empty() => self.json_precedence(),
            _ => ATOM,
        }
    }

    /// How tightly a binary operator binds, as written by this grammar.
    fn binary_precedence(&self, op: &BinaryOp) -> u8 {
        match op {
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
            | BinaryOp::NullSafeEq
            | BinaryOp::NullSafeNotEq => COMPARISON,
            BinaryOp::Add | BinaryOp::Sub => ADDITIVE,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => MULTIPLICATIVE,
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::ShiftLeft
            | BinaryOp::ShiftRight
            | BinaryOp::Concat
            | BinaryOp::Custom(_) => OTHER,
        }
    }

    /// How tightly a JSON path read as JSON binds (an operator on
    /// PostgreSQL and SQLite, a call on MySQL).
    fn json_precedence(&self) -> u8 {
        OTHER
    }

    /// A constant.
    fn literal(&self, w: &mut Writer, literal: &Literal) {
        match literal {
            Literal::Null => w.push("NULL"),
            Literal::Bool(value) => self.bool_literal(w, *value),
            Literal::Int(value) => w.push(&value.to_string()),
            Literal::String(text) => w.string_literal(text),
        }
    }

    /// `TRUE` or `FALSE`.
    fn bool_literal(&self, w: &mut Writer, value: bool) {
        w.push(if value { "TRUE" } else { "FALSE" });
    }

    /// `name([DISTINCT] args)`; the name must be a plain identifier.
    fn function(&self, w: &mut Writer, function: &Function) -> Result<()> {
        if !is_identifier(&function.name) {
            return Err(OrmError::invalid(format!(
                "`{}` is not a valid SQL function name",
                function.name
            )));
        }
        w.push(&function.name);
        w.push("(");
        if function.distinct {
            w.push("DISTINCT ");
        }
        self.expr_list(w, &function.args)?;
        w.push(")");
        Ok(())
    }

    /// A template: its text with each `{}` hole filled by the next
    /// argument (parenthesized unless an atom). A `?` outside quotes is a
    /// placeholder on MySQL and SQLite, so only PostgreSQL takes one.
    fn template(&self, w: &mut Writer, sql: &str, args: &[Expr]) -> Result<()> {
        let tokens = scan(sql, self.dialect());
        let holes = tokens
            .iter()
            .filter(|token| matches!(token, Token::Hole))
            .count();
        if holes != args.len() {
            return Err(OrmError::invalid(format!(
                "the template `{sql}` has {holes} hole(s) but {} argument(s)",
                args.len()
            )));
        }
        let question = tokens
            .iter()
            .any(|token| matches!(token, Token::Question | Token::DoubleQuestion));
        if question && self.dialect() != Dialect::Postgres {
            return Err(OrmError::invalid(format!(
                "the template `{sql}` holds a `?`, a placeholder on {}: bind values through its arguments",
                self.dialect().name()
            )));
        }
        let mut args = args.iter();
        for token in tokens {
            match token {
                Token::Text(text) => w.push(text),
                Token::Question => w.push("?"),
                Token::DoubleQuestion => w.push("??"),
                Token::Hole => {
                    if let Some(arg) = args.next() {
                        self.operand(w, arg, ATOM)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// `left op right`.
    fn binary(&self, w: &mut Writer, left: &Expr, op: &BinaryOp, right: &Expr) -> Result<()> {
        let symbol = match op {
            BinaryOp::Eq => " = ",
            BinaryOp::NotEq => " <> ",
            BinaryOp::Lt => " < ",
            BinaryOp::LtEq => " <= ",
            BinaryOp::Gt => " > ",
            BinaryOp::GtEq => " >= ",
            BinaryOp::NullSafeEq => return self.null_safe(w, left, right, false),
            BinaryOp::NullSafeNotEq => return self.null_safe(w, left, right, true),
            BinaryOp::Add | BinaryOp::Sub => {
                self.operand(w, left, ADDITIVE)?;
                w.push(if *op == BinaryOp::Add { " + " } else { " - " });
                return self.operand(w, right, MULTIPLICATIVE);
            }
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                self.operand(w, left, MULTIPLICATIVE)?;
                w.push(match op {
                    BinaryOp::Mul => " * ",
                    BinaryOp::Div => " / ",
                    _ => " % ",
                });
                return self.operand(w, right, UNARY);
            }
            BinaryOp::BitXor => return self.bit_xor(w, left, right),
            BinaryOp::Concat => return self.concat(w, left, right),
            BinaryOp::BitAnd => return self.atomic_binary(w, left, "&", right),
            BinaryOp::BitOr => return self.atomic_binary(w, left, "|", right),
            BinaryOp::ShiftLeft => return self.atomic_binary(w, left, "<<", right),
            BinaryOp::ShiftRight => return self.atomic_binary(w, left, ">>", right),
            BinaryOp::Custom(op) => {
                check_operator(op, self.dialect())?;
                return self.atomic_binary(w, left, op, right);
            }
        };
        self.operand(w, left, OTHER)?;
        w.push(symbol);
        self.operand(w, right, OTHER)
    }

    /// `left op right` with both operands atoms (parenthesized otherwise).
    fn atomic_binary(&self, w: &mut Writer, left: &Expr, op: &str, right: &Expr) -> Result<()> {
        self.operand(w, left, ATOM)?;
        w.push(" ");
        w.push(op);
        w.push(" ");
        self.operand(w, right, ATOM)
    }

    /// Null-safe equality: `IS [NOT] DISTINCT FROM`.
    fn null_safe(&self, w: &mut Writer, left: &Expr, right: &Expr, negated: bool) -> Result<()> {
        self.operand(w, left, OTHER)?;
        w.push(if negated {
            " IS DISTINCT FROM "
        } else {
            " IS NOT DISTINCT FROM "
        });
        self.operand(w, right, OTHER)
    }

    /// Exclusive or: `#`.
    fn bit_xor(&self, w: &mut Writer, left: &Expr, right: &Expr) -> Result<()> {
        self.atomic_binary(w, left, "#", right)
    }

    /// Concatenation: `||`.
    fn concat(&self, w: &mut Writer, left: &Expr, right: &Expr) -> Result<()> {
        self.atomic_binary(w, left, "||", right)
    }

    /// `expr [NOT] LIKE pattern [ESCAPE 'c']`; ignoring case is `ILIKE`.
    fn like(
        &self,
        w: &mut Writer,
        expr: &Expr,
        pattern: &Expr,
        escape: Option<char>,
        negated: bool,
        case: LikeCase,
    ) -> Result<()> {
        self.operand(w, expr, OTHER)?;
        w.push(match (case, negated) {
            (LikeCase::Ignore, false) => " ILIKE ",
            (LikeCase::Ignore, true) => " NOT ILIKE ",
            (_, false) => " LIKE ",
            (_, true) => " NOT LIKE ",
        });
        self.operand(w, pattern, OTHER)?;
        self.like_escape(w, escape);
        Ok(())
    }

    /// ` ESCAPE 'c'` when there is an escape character.
    fn like_escape(&self, w: &mut Writer, escape: Option<char>) {
        if let Some(escape) = escape {
            w.push(" ESCAPE ");
            w.string_literal(&escape.to_string());
        }
    }

    /// `CAST(expr AS type)`.
    fn cast(&self, w: &mut Writer, expr: &Expr, ty: &SqlType) -> Result<()> {
        let name = self.type_name(ty)?;
        w.push("CAST(");
        self.expr(w, expr)?;
        w.push(" AS ");
        w.push(&name);
        w.push(")");
        Ok(())
    }

    /// The name of a type in `CAST`.
    fn type_name(&self, ty: &SqlType) -> Result<Cow<'static, str>> {
        Ok(match ty {
            SqlType::Text => "TEXT".into(),
            SqlType::Integer => "BIGINT".into(),
            SqlType::Double => "DOUBLE PRECISION".into(),
            SqlType::Decimal { precision, scale } => {
                format!("NUMERIC({precision}, {scale})").into()
            }
            SqlType::Date => "DATE".into(),
            SqlType::Time => "TIME".into(),
            SqlType::DateTime => "TIMESTAMP".into(),
            SqlType::Json => "JSONB".into(),
            SqlType::Custom(name) => custom_type(name)?,
        })
    }

    /// The JSON value at `path` inside `expr`: `(expr->'a'->>'b')` as
    /// text, `expr->'a'->'b'` as JSON.
    fn json_path(
        &self,
        w: &mut Writer,
        expr: &Expr,
        path: &[JsonSegment],
        mode: JsonMode,
    ) -> Result<()> {
        if path.is_empty() {
            return match mode {
                JsonMode::Json => self.expr(w, expr),
                JsonMode::Text => {
                    w.push("(");
                    self.operand(w, expr, ATOM)?;
                    w.push(" #>> '{}')");
                    Ok(())
                }
            };
        }
        if mode == JsonMode::Text {
            w.push("(");
        }
        self.operand(w, expr, ATOM)?;
        for (i, segment) in path.iter().enumerate() {
            let last = i + 1 == path.len();
            w.push(if last && mode == JsonMode::Text {
                "->>"
            } else {
                "->"
            });
            match segment {
                JsonSegment::Key(key) => w.string_literal(key),
                // A negative index counts from the end; parenthesized so it
                // never reads as part of the operator.
                JsonSegment::Index(index) if *index < 0 => w.push(&format!("({index})")),
                JsonSegment::Index(index) => w.push(&index.to_string()),
            }
        }
        if mode == JsonMode::Text {
            w.push(")");
        }
        Ok(())
    }

    /// The value the conflicting row proposed: `"excluded"."column"`.
    fn excluded(&self, w: &mut Writer, column: &Ident) -> Result<()> {
        w.ident("excluded");
        w.push(".");
        w.identifier(column);
        Ok(())
    }

    /// A random number: `RANDOM()`.
    fn random(&self, w: &mut Writer) {
        w.push("RANDOM()");
    }

    /// `DEFAULT`.
    fn default_keyword(&self, w: &mut Writer) -> Result<()> {
        w.push("DEFAULT");
        Ok(())
    }
}

/// A JSON path for MySQL and SQLite, before it is quoted as a string:
/// `$."key"[0]` (a key's `"` and `\` escaped). An index from the end
/// (negative) is `[last]`, `[last-1]`, .. on MySQL and `[#-1]`, `[#-2]`,
/// .. on SQLite.
pub(crate) fn json_path_text(path: &[JsonSegment], dialect: Dialect) -> String {
    let mut text = String::from("$");
    for segment in path {
        match segment {
            JsonSegment::Key(key) => {
                text.push_str(".\"");
                text.push_str(&key.replace('\\', "\\\\").replace('"', "\\\""));
                text.push('"');
            }
            JsonSegment::Index(index) if *index < 0 => {
                let back = index.unsigned_abs();
                match dialect {
                    Dialect::MySql if back == 1 => text.push_str("[last]"),
                    Dialect::MySql => text.push_str(&format!("[last-{}]", back - 1)),
                    _ => text.push_str(&format!("[#-{back}]")),
                }
            }
            JsonSegment::Index(index) => {
                text.push('[');
                text.push_str(&index.to_string());
                text.push(']');
            }
        }
    }
    text
}

/// A name as text for messages: `audit.events`.
pub(crate) fn name_text(name: &ObjectName) -> String {
    name.0
        .iter()
        .map(Ident::as_str)
        .collect::<Vec<_>>()
        .join(".")
}

/// A custom type name, checked: letters, digits, `_`, spaces, parentheses
/// and commas.
fn custom_type(name: &str) -> Result<Cow<'static, str>> {
    let valid = !name.trim().is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | '(' | ')' | ','));
    if valid {
        Ok(Cow::Owned(name.to_owned()))
    } else {
        Err(OrmError::invalid(format!(
            "`{name}` is not a type name CAST takes"
        )))
    }
}

/// An operator written as is, checked: operator characters and words, no
/// quotes, comments or statement ends; `?` only on PostgreSQL (elsewhere
/// it is a placeholder).
fn check_operator(op: &str, dialect: Dialect) -> Result<()> {
    let valid = !op.trim().is_empty()
        && op.chars().all(|c| {
            c.is_ascii_alphabetic()
                || c == ' '
                || matches!(
                    c,
                    '!' | '#'
                        | '%'
                        | '&'
                        | '*'
                        | '+'
                        | '-'
                        | '/'
                        | '<'
                        | '='
                        | '>'
                        | '?'
                        | '@'
                        | '^'
                        | '|'
                        | '~'
                )
        })
        && !op.contains("--")
        && !op.contains("/*");
    if !valid {
        return Err(OrmError::invalid(format!(
            "`{op}` is not an operator the renderer writes"
        )));
    }
    if op.contains('?') && dialect != Dialect::Postgres {
        return Err(OrmError::invalid(format!(
            "the operator `{op}` holds a `?`, a placeholder on {}",
            dialect.name()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers() {
        assert!(is_identifier("time_bucket"));
        assert!(is_identifier("public.my_fn"));
        assert!(!is_identifier("drop table x; --"));
        assert!(!is_identifier("1abc"));
        assert!(!is_identifier(""));
    }

    #[test]
    fn json_paths_are_escaped_and_count_from_the_end() {
        let path = [
            JsonSegment::Key("a".into()),
            JsonSegment::Index(2),
            JsonSegment::Key("it's \"x\"".into()),
        ];
        assert_eq!(
            json_path_text(&path, Dialect::MySql),
            r#"$."a"[2]."it's \"x\"""#
        );
        let from_end = [JsonSegment::Index(-1), JsonSegment::Index(-3)];
        assert_eq!(json_path_text(&from_end, Dialect::MySql), "$[last][last-2]");
        assert_eq!(json_path_text(&from_end, Dialect::Sqlite), "$[#-1][#-3]");
    }

    #[test]
    fn operators_and_types_are_checked() {
        assert!(check_operator("@>", Dialect::Postgres).is_ok());
        assert!(check_operator("SOUNDS LIKE", Dialect::MySql).is_ok());
        assert!(check_operator("?|", Dialect::Postgres).is_ok());
        assert!(check_operator("?|", Dialect::Sqlite).is_err());
        assert!(check_operator("= 1; DROP TABLE x", Dialect::Postgres).is_err());
        assert!(check_operator("-- x", Dialect::Postgres).is_err());
        assert!(custom_type("DATETIME(6)").is_ok());
        assert!(custom_type("vector").is_ok());
        assert!(custom_type("int); DROP TABLE x").is_err());
    }
}
