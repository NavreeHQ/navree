//! navree-orm's own SQL renderer: a [syntax tree](ast) that is the same for
//! every database, [grammars](grammar) that write it for PostgreSQL /
//! TimescaleDB, MySQL / MariaDB and SQLite, and a [lowering](lower) from
//! the ORM's IR ([`render_select`], [`render_insert`], ...), which writes
//! every statement the ORM runs.
//!
//! - **The tree** ([`ast`]): statements (`SELECT` with CTEs, compounds,
//!   windows and locks; `INSERT` with conflict handling and `RETURNING`;
//!   `UPDATE` and `DELETE` over other tables; emptying a table) and
//!   expressions. Values are bound ([`ast::Expr::Value`]); literals,
//!   raw fragments with portable `?` placeholders and `{}` templates are
//!   written into the SQL.
//! - **The grammars** ([`grammar`]): [`Grammar`] holds the SQL the
//!   databases share as default methods, each dialect overrides what
//!   differs; the [`Writer`] quotes names (each part of `schema.table` on
//!   its own, quote characters doubled), escapes literals and numbers the
//!   placeholders (`$n` on PostgreSQL, `?` elsewhere) in writing order.
//!   What a dialect cannot express is [`OrmError::Unsupported`], a
//!   malformed tree [`OrmError::InvalidQuery`]; rendering never panics.
//! - **The lowering** ([`lower`]): the IR as trees, with each database's
//!   rules for the clauses where they differ.
//!
//! ```
//! use navree_orm::Dialect;
//! use navree_orm::render::ast::{Expr, Query, Select};
//! use navree_orm::render::render_query;
//!
//! let latest = Query::from(
//!     Select::from_table("deals")
//!         .column(Expr::col("title"))
//!         .filter(Expr::col("closed_at").is_null()),
//! )
//! .order_by(Expr::col("created_at").desc().nulls_last())
//! .offset(20);
//! assert_eq!(
//!     render_query(&latest, Dialect::MySql).unwrap().sql,
//!     "SELECT `title` FROM `deals` WHERE `closed_at` IS NULL \
//!      ORDER BY `created_at` IS NULL ASC, `created_at` DESC LIMIT ? OFFSET ?"
//! );
//! ```

pub mod ast;
pub mod grammar;
pub mod lower;
mod writer;

pub use grammar::Grammar;
pub use lower::{
    render_aggregate, render_delete, render_exists, render_insert, render_select, render_truncate,
    render_update, render_update_from,
};
pub use writer::Writer;

use crate::error::{OrmError, Result};
use crate::sql::{Dialect, Statement, Syntax};

/// `statement` written for `syntax` (a [`Dialect`], or MariaDB's
/// [`Syntax`]). A statement that takes several on `syntax` (emptying a
/// table on SQLite) is refused: use [`render_all`].
pub fn render(statement: &ast::Statement, syntax: impl Into<Syntax>) -> Result<Statement> {
    let syntax = syntax.into();
    let mut w = Writer::new(syntax);
    grammar::for_dialect(syntax.dialect()).statement(&mut w, statement)?;
    Ok(w.finish())
}

/// The statements that run `statement` on `dialect`, in order: one,
/// except when emptying a table on SQLite (resetting its sequence, then
/// deleting its rows).
///
/// ```
/// use navree_orm::Dialect;
/// use navree_orm::render::ast::{ObjectName, Statement, Truncate};
/// use navree_orm::render::render_all;
///
/// let truncate = Statement::Truncate(Truncate {
///     table: ObjectName::parse("deals"),
///     restart_identity: true,
///     cascade: false,
/// });
/// let sql: Vec<String> = render_all(&truncate, Dialect::Sqlite)
///     .unwrap()
///     .into_iter()
///     .map(|statement| statement.sql)
///     .collect();
/// assert_eq!(sql, [r#"DELETE FROM "sqlite_sequence" WHERE "name" = ?"#, r#"DELETE FROM "deals""#]);
/// ```
pub fn render_all(statement: &ast::Statement, syntax: impl Into<Syntax>) -> Result<Vec<Statement>> {
    let syntax = syntax.into();
    match statement {
        ast::Statement::Truncate(truncate) => {
            Ok(grammar::for_dialect(syntax.dialect()).truncate(truncate))
        }
        other => Ok(vec![render(other, syntax)?]),
    }
}

/// `query` written for `syntax`.
pub fn render_query(query: &ast::Query, syntax: impl Into<Syntax>) -> Result<Statement> {
    let syntax = syntax.into();
    let mut w = Writer::new(syntax);
    grammar::for_dialect(syntax.dialect()).query(&mut w, query)?;
    Ok(w.finish())
}

/// An expression alone, as a clause would hold it (for tests and
/// debugging).
///
/// ```
/// use navree_orm::Dialect;
/// use navree_orm::render::ast::{BinaryOp, Expr};
/// use navree_orm::render::render_expr;
///
/// let same = Expr::col("a").binary(BinaryOp::NullSafeEq, Expr::col("b"));
/// assert_eq!(render_expr(&same, Dialect::Postgres).unwrap().sql, r#""a" IS NOT DISTINCT FROM "b""#);
/// assert_eq!(render_expr(&same, Dialect::MySql).unwrap().sql, "`a` <=> `b`");
/// assert_eq!(render_expr(&same, Dialect::Sqlite).unwrap().sql, r#""a" IS "b""#);
/// ```
pub fn render_expr(expr: &ast::Expr, syntax: impl Into<Syntax>) -> Result<Statement> {
    let syntax = syntax.into();
    let mut w = Writer::new(syntax);
    grammar::for_dialect(syntax.dialect()).expr(&mut w, expr)?;
    Ok(w.finish())
}

/// `Unsupported` for `feature` on `dialect`, as the renderer reports it.
pub(crate) fn unsupported(
    dialect: Dialect,
    feature: impl Into<std::borrow::Cow<'static, str>>,
) -> OrmError {
    grammar::unsupported(dialect, feature)
}

/// `Unsupported` for `feature` on MariaDB.
pub(crate) fn unsupported_on_mariadb(
    feature: impl Into<std::borrow::Cow<'static, str>>,
) -> OrmError {
    OrmError::unsupported(feature, "MariaDB")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// The renderer stands on its own: no file of it names the query
    /// builder or the ORM it replaces.
    #[test]
    fn the_renderer_stands_on_its_own() {
        let forbidden = [["sea", "_query"].concat(), ["sea", "_orm"].concat()];
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("render");
        let mut files = Vec::new();
        let mut pending = vec![root];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).expect("reads the render sources") {
                let path = entry.expect("reads a directory entry").path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    files.push(path);
                }
            }
        }
        assert!(files.len() >= 10, "found the render sources: {files:?}");
        for file in files {
            let text = std::fs::read_to_string(&file).expect("reads a render source");
            for word in &forbidden {
                assert!(
                    !text.contains(word.as_str()),
                    "{} names {word}",
                    file.display()
                );
            }
        }
    }
}
