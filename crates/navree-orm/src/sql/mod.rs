//! SQL for PostgreSQL / TimescaleDB, MySQL and SQLite: the [`Dialect`]s,
//! the [`Statement`]s the ORM runs, and the functions that write the IR as
//! statements ([`render_select`], [`render_insert`], ...).
//!
//! The SQL is navree-orm's own: the IR is lowered into the
//! [render tree](crate::render::ast) and written by the dialect's
//! [grammar](crate::render::grammar), which quotes identifiers, binds every
//! value and numbers the placeholders (`$n` on PostgreSQL, `?` elsewhere).

use std::fmt;

use navree_db::Backend;

use crate::error::OrmError;
use crate::ir::Where;
use crate::value::Value;

pub(crate) use crate::render::grammar::is_identifier;
pub(crate) use crate::render::lower::timeout_millis;
pub use crate::render::{
    render_aggregate, render_delete, render_exists, render_insert, render_select, render_truncate,
    render_update, render_update_from,
};

/// An SQL dialect.
///
/// TimescaleDB is PostgreSQL with the extension: its helpers
/// (`time_bucket`, `first`, `last`) compile on [`Dialect::Postgres`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// PostgreSQL and TimescaleDB.
    Postgres,
    /// MySQL and MariaDB.
    MySql,
    /// SQLite.
    Sqlite,
}

impl Dialect {
    /// navree-db's backend of this dialect.
    pub fn backend(self) -> Backend {
        match self {
            Dialect::Postgres => Backend::Postgres,
            Dialect::MySql => Backend::MySql,
            Dialect::Sqlite => Backend::Sqlite,
        }
    }

    /// The database's name in messages.
    pub fn name(self) -> &'static str {
        match self {
            Dialect::Postgres => "PostgreSQL",
            Dialect::MySql => "MySQL",
            Dialect::Sqlite => "SQLite",
        }
    }

    /// Whether `INSERT`, `UPDATE` and `DELETE` can return rows.
    pub fn supports_returning(self) -> bool {
        !matches!(self, Dialect::MySql)
    }

    /// The most values one statement can bind: 65,535 on PostgreSQL and
    /// MySQL, 32,766 on SQLite. Multi-row inserts are split to stay under
    /// it.
    pub fn max_bind_values(self) -> usize {
        match self {
            Dialect::Postgres | Dialect::MySql => 65_535,
            Dialect::Sqlite => 32_766,
        }
    }

    /// `name` quoted as an identifier (`"deals"`, `` `deals` ``), each part
    /// of a schema-qualified name on its own.
    ///
    /// ```
    /// use navree_orm::Dialect;
    ///
    /// assert_eq!(Dialect::Postgres.quote_identifier("audit.events"), r#""audit"."events""#);
    /// assert_eq!(Dialect::MySql.quote_identifier("odd`name"), "`odd``name`");
    /// ```
    pub fn quote_identifier(self, name: &str) -> String {
        quote_identifier(name, self)
    }
}

/// `name` quoted as an identifier for `dialect`.
pub(crate) fn quote_identifier(name: &str, dialect: Dialect) -> String {
    let quote = match dialect {
        Dialect::MySql => '`',
        Dialect::Postgres | Dialect::Sqlite => '"',
    };
    name.split('.')
        .map(|part| {
            let escaped = part.replace(quote, &format!("{quote}{quote}"));
            format!("{quote}{escaped}{quote}")
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The dialect a navree-db backend speaks.
///
/// ```
/// use navree_db::Backend;
/// use navree_orm::Dialect;
///
/// assert_eq!(Dialect::from(Backend::MySql), Dialect::MySql);
/// assert_eq!(Dialect::Postgres.backend(), Backend::Postgres);
/// ```
impl From<Backend> for Dialect {
    fn from(backend: Backend) -> Self {
        match backend {
            Backend::Postgres => Dialect::Postgres,
            Backend::MySql => Dialect::MySql,
            Backend::Sqlite => Dialect::Sqlite,
        }
    }
}

impl From<Dialect> for Backend {
    fn from(dialect: Dialect) -> Self {
        dialect.backend()
    }
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A compiled statement: SQL with placeholders and the values bound to
/// them.
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    /// The SQL (`$1` placeholders on PostgreSQL, `?` elsewhere).
    pub sql: String,
    /// The bound values, in placeholder order.
    pub values: Vec<Value>,
    /// The dialect it was compiled for.
    pub dialect: Dialect,
}

impl Statement {
    /// The statement as navree-db runs it (its SQL, written in the
    /// dialect's own placeholders, and its values).
    pub fn to_db(&self) -> navree_db::Statement {
        navree_db::Statement::new(
            self.dialect.backend(),
            self.sql.clone(),
            self.values.clone(),
        )
    }
}

impl From<Statement> for navree_db::Statement {
    fn from(statement: Statement) -> Self {
        navree_db::Statement::new(statement.dialect.backend(), statement.sql, statement.values)
    }
}

impl fmt::Display for Statement {
    /// The SQL followed by the values, one statement per line:
    /// `SELECT ... WHERE "id" = $1 -- [I64(1)]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sql)?;
        if !self.values.is_empty() {
            write!(f, " -- {:?}", self.values)?;
        }
        Ok(())
    }
}

/// A filter alone, as `WHERE` would hold it (for tests and debugging).
pub(crate) fn render_filter(filter: &Where, dialect: Dialect) -> Result<Statement, OrmError> {
    let condition = crate::render::lower::Lowerer::new(dialect).where_(filter)?;
    crate::render::render_expr(&condition, dialect)
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
    fn filters_render_alone() {
        let filter = Where::Raw {
            sql: "a = ?".into(),
            values: vec![Value::from(1)],
        };
        let statement = render_filter(&filter, Dialect::Postgres).unwrap();
        assert_eq!(statement.sql, "a = $1");
        assert_eq!(statement.values, [Value::from(1)]);
    }
}
