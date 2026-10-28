//! MySQL and MariaDB: backquoted names, `?` placeholders, a `LIMIT` before
//! every `OFFSET`, NULLs sorted with a leading `expr IS NULL` key, `LOCK IN
//! SHARE MODE` (`FOR SHARE` with `NOWAIT`, `SKIP LOCKED` or `OF`), `INSERT
//! IGNORE` and `ON DUPLICATE KEY UPDATE`, no `RETURNING`, multi-table
//! `UPDATE`/`DELETE` with commas, `json_extract`, `<=>`, `CONCAT`, `RAND()`
//! and the `MAX_EXECUTION_TIME` hint.

use std::borrow::Cow;

use super::{Grammar, json_path_text, name_text, unsupported};
use crate::error::{OrmError, Result};
use crate::ir::JsonSegment;
use crate::render::Writer;
use crate::render::ast::{
    BinaryOp, ConflictAction, Delete, Expr, Ident, Insert, JoinKind, JsonMode, LikeCase, Lock,
    LockStrength, LockWait, ObjectName, OnConflict, OrderBy, SelectItem, SqlType, Truncate, Update,
};
use crate::sql::{self, Dialect};

use super::precedence::{ATOM, NOT, OTHER};

/// The MySQL (and MariaDB) grammar.
#[derive(Clone, Copy, Debug, Default)]
pub struct MySql;

impl Grammar for MySql {
    fn dialect(&self) -> Dialect {
        Dialect::MySql
    }

    fn execution_time_hint(&self, w: &mut Writer, millis: u64) {
        w.push(&format!("/*+ MAX_EXECUTION_TIME({millis}) */ "));
    }

    fn join_keyword(&self, kind: JoinKind) -> Result<&'static str> {
        Ok(match kind {
            JoinKind::Inner => " INNER JOIN ",
            JoinKind::Left => " LEFT JOIN ",
            JoinKind::Right => " RIGHT JOIN ",
            JoinKind::Full => return Err(unsupported(Dialect::MySql, "a full join")),
            JoinKind::Cross => " CROSS JOIN ",
        })
    }

    /// MySQL has no `NULLS FIRST`/`LAST`: a leading `expr IS NULL` key
    /// sorts the NULLs there.
    fn order_item(&self, w: &mut Writer, item: &OrderBy) -> Result<()> {
        if let Some(nulls) = item.nulls {
            self.operand(w, &item.expr, OTHER)?;
            w.push(match nulls {
                crate::render::ast::NullsOrder::First => " IS NULL DESC, ",
                crate::render::ast::NullsOrder::Last => " IS NULL ASC, ",
            });
        }
        self.expr(w, &item.expr)?;
        w.push(if item.descending { " DESC" } else { " ASC" });
        Ok(())
    }

    fn offset_needs_limit(&self) -> bool {
        true
    }

    /// `FOR UPDATE` for both update strengths; a shared lock is `LOCK IN
    /// SHARE MODE` (MariaDB has no `FOR SHARE`), or `FOR SHARE` when it
    /// does not wait or names its tables (MySQL 8 takes those after `FOR
    /// SHARE` only).
    fn lock(&self, w: &mut Writer, lock: &Lock) -> Result<()> {
        match lock.strength {
            LockStrength::Update | LockStrength::NoKeyUpdate => w.push(" FOR UPDATE"),
            LockStrength::Share | LockStrength::KeyShare => {
                if lock.wait == LockWait::Wait && lock.of.is_empty() {
                    w.push(" LOCK IN SHARE MODE");
                    return Ok(());
                }
                w.push(" FOR SHARE");
            }
        }
        self.lock_tail(w, lock);
        Ok(())
    }

    fn insert_keyword(&self, insert: &Insert) -> &'static str {
        match &insert.on_conflict {
            Some(OnConflict {
                action: ConflictAction::Ignore,
                ..
            }) => "INSERT IGNORE INTO ",
            _ => "INSERT INTO ",
        }
    }

    fn default_values(&self, w: &mut Writer) {
        w.push(" () VALUES ()");
    }

    /// A skipping insert is `INSERT IGNORE` (above); an update is `ON
    /// DUPLICATE KEY UPDATE`, on any unique index (the target is not
    /// written), without a condition.
    fn on_conflict(&self, w: &mut Writer, conflict: &OnConflict) -> Result<()> {
        match &conflict.action {
            ConflictAction::Ignore => Ok(()),
            ConflictAction::Update { sets, filter } => {
                if filter.is_some() {
                    return Err(unsupported(
                        Dialect::MySql,
                        "a condition on an upsert's update (DO UPDATE .. WHERE)",
                    ));
                }
                if sets.is_empty() {
                    return Err(OrmError::invalid("an upsert updates no column"));
                }
                w.push(" ON DUPLICATE KEY UPDATE ");
                self.assignments(w, sets, None)
            }
        }
    }

    fn upsert_needs_target(&self) -> bool {
        false
    }

    fn returning(&self, w: &mut Writer, items: &[SelectItem]) -> Result<()> {
        let _ = w;
        if items.is_empty() {
            Ok(())
        } else {
            Err(unsupported(Dialect::MySql, "RETURNING"))
        }
    }

    /// `UPDATE t [AS a][, other ..] SET .. WHERE .. [ORDER BY ..] [LIMIT
    /// ?]`; with other tables the assigned columns are qualified and an
    /// order or limit is refused (MySQL takes them for one table only).
    fn update(&self, w: &mut Writer, update: &Update) -> Result<()> {
        if update.sets.is_empty() {
            return Err(OrmError::invalid(format!(
                "an update of `{}` sets no column",
                name_text(&update.table)
            )));
        }
        self.returning(w, &update.returning)?;
        let multi = !update.from.is_empty();
        if multi && (!update.order_by.is_empty() || update.limit.is_some()) {
            return Err(unsupported(
                Dialect::MySql,
                "an update of several tables with ORDER BY or LIMIT",
            ));
        }
        w.push("UPDATE ");
        w.object_name(&update.table);
        if let Some(alias) = &update.alias {
            w.push(" AS ");
            w.identifier(alias);
        }
        for source in &update.from {
            w.push(", ");
            self.source(w, source)?;
        }
        w.push(" SET ");
        let qualifier = multi.then(|| target_name(&update.table, update.alias.as_ref()));
        self.assignments(w, &update.sets, qualifier.as_ref())?;
        if let Some(filter) = &update.filter {
            w.push(" WHERE ");
            self.expr(w, filter)?;
        }
        self.order_by(w, &update.order_by)?;
        if let Some(limit) = update.limit {
            w.push(" LIMIT ");
            w.bind(crate::value::Value::from(limit.min(i64::MAX as u64)));
        }
        Ok(())
    }

    /// `DELETE FROM t [AS a] WHERE .. [ORDER BY ..] [LIMIT ?]`, or with
    /// other tables `DELETE t FROM t, other .. WHERE ..`.
    fn delete(&self, w: &mut Writer, delete: &Delete) -> Result<()> {
        self.returning(w, &delete.returning)?;
        if delete.using.is_empty() {
            w.push("DELETE FROM ");
            w.object_name(&delete.table);
            if let Some(alias) = &delete.alias {
                w.push(" AS ");
                w.identifier(alias);
            }
        } else {
            if !delete.order_by.is_empty() || delete.limit.is_some() {
                return Err(unsupported(
                    Dialect::MySql,
                    "a delete from several tables with ORDER BY or LIMIT",
                ));
            }
            w.push("DELETE ");
            w.object_name(&target_name(&delete.table, delete.alias.as_ref()));
            w.push(" FROM ");
            w.object_name(&delete.table);
            if let Some(alias) = &delete.alias {
                w.push(" AS ");
                w.identifier(alias);
            }
            for source in &delete.using {
                w.push(", ");
                self.source(w, source)?;
            }
        }
        if let Some(filter) = &delete.filter {
            w.push(" WHERE ");
            self.expr(w, filter)?;
        }
        self.order_by(w, &delete.order_by)?;
        if let Some(limit) = delete.limit {
            w.push(" LIMIT ");
            w.bind(crate::value::Value::from(limit.min(i64::MAX as u64)));
        }
        Ok(())
    }

    /// `TRUNCATE TABLE t` (it restarts the identity; it commits the open
    /// transaction, as DDL does on MySQL; tables pointing at it are not
    /// emptied).
    fn truncate(&self, truncate: &Truncate) -> Vec<sql::Statement> {
        let mut w = Writer::new(Dialect::MySql);
        w.push("TRUNCATE TABLE ");
        w.object_name(&truncate.table);
        vec![w.finish()]
    }

    fn binary_precedence(&self, op: &BinaryOp) -> u8 {
        match op {
            // `NOT (a <=> b)`.
            BinaryOp::NullSafeNotEq => NOT,
            // `CONCAT(a, b)`.
            BinaryOp::Concat => ATOM,
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
            | BinaryOp::NullSafeEq => super::precedence::COMPARISON,
            BinaryOp::Add | BinaryOp::Sub => super::precedence::ADDITIVE,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => super::precedence::MULTIPLICATIVE,
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::ShiftLeft
            | BinaryOp::ShiftRight
            | BinaryOp::Custom(_) => OTHER,
        }
    }

    fn null_safe(&self, w: &mut Writer, left: &Expr, right: &Expr, negated: bool) -> Result<()> {
        if negated {
            w.push("NOT (");
        }
        self.operand(w, left, OTHER)?;
        w.push(" <=> ");
        self.operand(w, right, OTHER)?;
        if negated {
            w.push(")");
        }
        Ok(())
    }

    fn bit_xor(&self, w: &mut Writer, left: &Expr, right: &Expr) -> Result<()> {
        self.atomic_binary(w, left, "^", right)
    }

    /// `CONCAT(a, b)`: `||` is `OR` on MySQL.
    fn concat(&self, w: &mut Writer, left: &Expr, right: &Expr) -> Result<()> {
        w.push("CONCAT(");
        self.expr(w, left)?;
        w.push(", ");
        self.expr(w, right)?;
        w.push(")");
        Ok(())
    }

    /// The column's collation decides by default; `LOWER(..) LIKE
    /// LOWER(..)` ignores case under every collation, `LIKE BINARY`
    /// matches it.
    fn like(
        &self,
        w: &mut Writer,
        expr: &Expr,
        pattern: &Expr,
        escape: Option<char>,
        negated: bool,
        case: LikeCase,
    ) -> Result<()> {
        let keyword = if negated { " NOT LIKE " } else { " LIKE " };
        match case {
            LikeCase::Database => {
                self.operand(w, expr, OTHER)?;
                w.push(keyword);
                self.operand(w, pattern, OTHER)?;
            }
            LikeCase::Ignore => {
                w.push("LOWER(");
                self.expr(w, expr)?;
                w.push(")");
                w.push(keyword);
                w.push("LOWER(");
                self.expr(w, pattern)?;
                w.push(")");
            }
            LikeCase::Match => {
                self.operand(w, expr, OTHER)?;
                w.push(keyword);
                w.push("BINARY ");
                self.operand(w, pattern, ATOM)?;
            }
        }
        self.like_escape(w, escape);
        Ok(())
    }

    fn type_name(&self, ty: &SqlType) -> Result<Cow<'static, str>> {
        Ok(match ty {
            SqlType::Text => "CHAR".into(),
            SqlType::Integer => "SIGNED".into(),
            SqlType::Double => "DOUBLE".into(),
            SqlType::Decimal { precision, scale } => {
                format!("DECIMAL({precision}, {scale})").into()
            }
            SqlType::Date => "DATE".into(),
            SqlType::Time => "TIME".into(),
            SqlType::DateTime => "DATETIME(6)".into(),
            SqlType::Json => "JSON".into(),
            SqlType::Custom(name) => super::custom_type(name)?,
        })
    }

    /// `json_extract(expr, '$."a"[0]')`, unquoted for text
    /// (`json_unquote(..)`); an index from the end is `[last]`,
    /// `[last-1]`, ...
    fn json_path(
        &self,
        w: &mut Writer,
        expr: &Expr,
        path: &[JsonSegment],
        mode: JsonMode,
    ) -> Result<()> {
        if path.is_empty() && mode == JsonMode::Json {
            return self.expr(w, expr);
        }
        if mode == JsonMode::Text {
            w.push("json_unquote(");
        }
        w.push("json_extract(");
        self.expr(w, expr)?;
        w.push(", ");
        w.string_literal(&json_path_text(path, Dialect::MySql));
        w.push(")");
        if mode == JsonMode::Text {
            w.push(")");
        }
        Ok(())
    }

    fn json_precedence(&self) -> u8 {
        ATOM
    }

    /// `VALUES(column)`.
    fn excluded(&self, w: &mut Writer, column: &Ident) -> Result<()> {
        w.push("VALUES(");
        w.identifier(column);
        w.push(")");
        Ok(())
    }

    fn random(&self, w: &mut Writer) {
        w.push("RAND()");
    }
}

/// The name a multi-table statement qualifies the written table's columns
/// with: its alias, else its name.
fn target_name(table: &ObjectName, alias: Option<&Ident>) -> ObjectName {
    match alias {
        Some(alias) => ObjectName(vec![alias.clone()]),
        None => table.clone(),
    }
}
