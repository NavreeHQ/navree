//! SQLite: no lateral joins, no row locks (its write transactions are
//! serialised), a `LIMIT` before every `OFFSET`, compound operands that
//! cannot be parenthesized (a part with its own order or limit becomes a
//! derived table), a `WHERE` before an `INSERT .. SELECT`'s conflict
//! clause, no `USING` in deletes, `json_extract` and `->`, `IS` for
//! null-safe equality, an emulated exclusive or, `GLOB` for case-sensitive
//! patterns, date functions for date casts, and emptying a table as two
//! statements.

use std::borrow::Cow;

use super::{Grammar, json_path_text, unsupported};
use crate::error::Result;
use crate::ir::JsonSegment;
use crate::render::Writer;
use crate::render::ast::{
    BinaryOp, Delete, Expr, JsonMode, LikeCase, Lock, ObjectName, Query, SetExpr, SetOperator,
    SqlType, Truncate,
};
use crate::sql::{self, Dialect};
use crate::value::Value;

use super::precedence::{ATOM, OTHER};

/// The SQLite grammar.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sqlite;

impl Grammar for Sqlite {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    /// SQLite writes compound operands bare, where an order or a limit of
    /// their own would end the whole statement: such an operand is a
    /// derived table, `SELECT * FROM (..) AS "navree_union_1"`.
    fn set_operand(&self, w: &mut Writer, operand: &SetExpr) -> Result<()> {
        w.push("SELECT * FROM (");
        match operand {
            SetExpr::Query(query) => self.query(w, query)?,
            other => self.set_expr(w, other, None)?,
        }
        w.push(") AS ");
        let alias = w.next_alias("navree_union");
        w.ident(&alias);
        Ok(())
    }

    /// SQLite's compound operators share one precedence and read left to
    /// right: a compound on the left never needs regrouping.
    fn groups_left_operand(&self, outer: SetOperator, inner: SetOperator) -> bool {
        let _ = (outer, inner);
        false
    }

    /// SQLite keeps duplicates in `UNION ALL` only.
    fn set_operator(&self, op: SetOperator, all: bool) -> Result<&'static str> {
        Ok(match (op, all) {
            (SetOperator::Union, false) => " UNION ",
            (SetOperator::Union, true) => " UNION ALL ",
            (SetOperator::Intersect, false) => " INTERSECT ",
            (SetOperator::Except, false) => " EXCEPT ",
            (SetOperator::Intersect, true) => {
                return Err(unsupported(Dialect::Sqlite, "INTERSECT ALL"));
            }
            (SetOperator::Except, true) => {
                return Err(unsupported(Dialect::Sqlite, "EXCEPT ALL"));
            }
        })
    }

    fn lateral(&self, w: &mut Writer) -> Result<()> {
        let _ = w;
        Err(unsupported(Dialect::Sqlite, "a lateral join"))
    }

    fn offset_needs_limit(&self) -> bool {
        true
    }

    /// No row locks: SQLite's write transactions are serialised (navree-db
    /// begins them `IMMEDIATE`), so the clause is dropped.
    fn lock(&self, w: &mut Writer, lock: &Lock) -> Result<()> {
        let _ = (w, lock);
        Ok(())
    }

    /// SQLite reads `ON` right after a `FROM` as a join constraint: before
    /// a conflict clause the query gets a `WHERE` (`WHERE TRUE` when it has
    /// none; a compound or ordered query becomes a derived table).
    fn insert_query(&self, w: &mut Writer, query: &Query, conflict: bool) -> Result<()> {
        if !conflict {
            return self.query(w, query);
        }
        match &query.body {
            SetExpr::Select(select) if query.with.is_none() => {
                if select.filter.is_some() {
                    self.query(w, query)
                } else {
                    let mut select = select.as_ref().clone();
                    select.filter = Some(Expr::bool(true));
                    let mut query = query.clone();
                    query.body = SetExpr::Select(Box::new(select));
                    self.query(w, &query)
                }
            }
            _ => {
                w.push("SELECT * FROM (");
                self.query(w, query)?;
                w.push(") AS ");
                let alias = w.next_alias("navree_rows");
                w.ident(&alias);
                w.push(" WHERE TRUE");
                Ok(())
            }
        }
    }

    /// SQLite 3.35+ takes a `DO UPDATE` without a target in the last
    /// conflict clause.
    fn upsert_needs_target(&self) -> bool {
        false
    }

    fn delete(&self, w: &mut Writer, delete: &Delete) -> Result<()> {
        if !delete.using.is_empty() {
            return Err(unsupported(
                Dialect::Sqlite,
                "a delete matching other tables (narrow the rows with an IN subquery)",
            ));
        }
        if !delete.order_by.is_empty() || delete.limit.is_some() {
            return Err(unsupported(
                Dialect::Sqlite,
                "a delete with ORDER BY or LIMIT (narrow the rows with a subquery)",
            ));
        }
        w.push("DELETE FROM ");
        w.object_name(&delete.table);
        if let Some(alias) = &delete.alias {
            w.push(" AS ");
            w.identifier(alias);
        }
        if let Some(filter) = &delete.filter {
            w.push(" WHERE ");
            self.expr(w, filter)?;
        }
        self.returning(w, &delete.returning)
    }

    /// Resetting the table's `AUTOINCREMENT` sequence (run only when the
    /// database has a sequence table), then deleting every row.
    fn truncate(&self, truncate: &Truncate) -> Vec<sql::Statement> {
        let mut parts = truncate.table.0.clone();
        let table = parts.pop().unwrap_or_else(|| "".into());
        let mut sequence = Writer::new(Dialect::Sqlite);
        sequence.push("DELETE FROM ");
        parts.push("sqlite_sequence".into());
        sequence.object_name(&ObjectName(parts));
        sequence.push(" WHERE ");
        sequence.ident("name");
        sequence.push(" = ");
        sequence.bind(Value::from(table.as_str().to_owned()));
        let mut rows = Writer::new(Dialect::Sqlite);
        rows.push("DELETE FROM ");
        rows.object_name(&truncate.table);
        vec![sequence.finish(), rows.finish()]
    }

    fn binary_precedence(&self, op: &BinaryOp) -> u8 {
        match op {
            // `((a | b) - (a & b))`.
            BinaryOp::BitXor => ATOM,
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
            | BinaryOp::NullSafeEq
            | BinaryOp::NullSafeNotEq => super::precedence::COMPARISON,
            BinaryOp::Add | BinaryOp::Sub => super::precedence::ADDITIVE,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => super::precedence::MULTIPLICATIVE,
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::ShiftLeft
            | BinaryOp::ShiftRight
            | BinaryOp::Concat
            | BinaryOp::Custom(_) => OTHER,
        }
    }

    /// `IS` / `IS NOT`.
    fn null_safe(&self, w: &mut Writer, left: &Expr, right: &Expr, negated: bool) -> Result<()> {
        self.operand(w, left, OTHER)?;
        w.push(if negated { " IS NOT " } else { " IS " });
        self.operand(w, right, OTHER)
    }

    /// SQLite has no exclusive or: `((a | b) - (a & b))`.
    fn bit_xor(&self, w: &mut Writer, left: &Expr, right: &Expr) -> Result<()> {
        w.push("((");
        self.atomic_binary(w, left, "|", right)?;
        w.push(") - (");
        self.atomic_binary(w, left, "&", right)?;
        w.push("))");
        Ok(())
    }

    /// `LIKE` ignores ASCII case already; matching case is a `GLOB` of the
    /// pattern converted in SQL (`%` → `*`, `_` → `?`, and GLOB's own
    /// wildcards matching themselves), so any pattern expression works.
    fn like(
        &self,
        w: &mut Writer,
        expr: &Expr,
        pattern: &Expr,
        escape: Option<char>,
        negated: bool,
        case: LikeCase,
    ) -> Result<()> {
        if case != LikeCase::Match {
            self.operand(w, expr, OTHER)?;
            w.push(if negated { " NOT LIKE " } else { " LIKE " });
            self.operand(w, pattern, OTHER)?;
            self.like_escape(w, escape);
            return Ok(());
        }
        if escape.is_some() {
            return Err(unsupported(
                Dialect::Sqlite,
                "a case-sensitive LIKE with an ESCAPE character",
            ));
        }
        self.operand(w, expr, OTHER)?;
        w.push(if negated { " NOT GLOB " } else { " GLOB " });
        w.push("replace(replace(replace(replace(replace(");
        self.expr(w, pattern)?;
        w.push(", '[', '[[]'), '*', '[*]'), '?', '[?]'), '%', '*'), '_', '?')");
        Ok(())
    }

    /// Dates and times are text: `date(..)`, `time(..)`, `datetime(..)`,
    /// and JSON is `json(..)`.
    fn cast(&self, w: &mut Writer, expr: &Expr, ty: &SqlType) -> Result<()> {
        let function = match ty {
            SqlType::Date => "date(",
            SqlType::Time => "time(",
            SqlType::DateTime => "datetime(",
            SqlType::Json => "json(",
            other => {
                let name = self.type_name(other)?;
                w.push("CAST(");
                self.expr(w, expr)?;
                w.push(" AS ");
                w.push(&name);
                w.push(")");
                return Ok(());
            }
        };
        w.push(function);
        self.expr(w, expr)?;
        w.push(")");
        Ok(())
    }

    fn type_name(&self, ty: &SqlType) -> Result<Cow<'static, str>> {
        Ok(match ty {
            SqlType::Text => "TEXT".into(),
            SqlType::Integer => "INTEGER".into(),
            SqlType::Double => "REAL".into(),
            SqlType::Decimal { .. } => "NUMERIC".into(),
            SqlType::Date | SqlType::Time | SqlType::DateTime => "TEXT".into(),
            SqlType::Json => "TEXT".into(),
            SqlType::Custom(name) => super::custom_type(name)?,
        })
    }

    /// `json_extract(expr, '$."a"[0]')` as text, `expr -> '$."a"[0]'` as
    /// JSON; an index from the end is `[#-1]`, `[#-2]`, ...
    fn json_path(
        &self,
        w: &mut Writer,
        expr: &Expr,
        path: &[JsonSegment],
        mode: JsonMode,
    ) -> Result<()> {
        match mode {
            JsonMode::Text => {
                w.push("json_extract(");
                self.expr(w, expr)?;
                w.push(", ");
                w.string_literal(&json_path_text(path, Dialect::Sqlite));
                w.push(")");
            }
            JsonMode::Json if path.is_empty() => self.expr(w, expr)?,
            JsonMode::Json => {
                self.operand(w, expr, ATOM)?;
                w.push(" -> ");
                w.string_literal(&json_path_text(path, Dialect::Sqlite));
            }
        }
        Ok(())
    }

    fn default_keyword(&self, w: &mut Writer) -> Result<()> {
        let _ = w;
        Err(unsupported(
            Dialect::Sqlite,
            "DEFAULT in a list of values (leave the column out instead)",
        ))
    }
}
