//! [`Statement`]: SQL with the values bound to its placeholders.

use std::borrow::Cow;
use std::fmt;

use crate::engine::{Backend, DbError, Value};

/// How a statement's placeholders are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placeholders {
    /// The backend's own: `$1`, `$2` on PostgreSQL, `?` elsewhere.
    Native(Backend),
    /// `?` on every backend, rewritten for the backend that runs it.
    Portable,
}

/// SQL and the values bound to its placeholders, in order.
///
/// [`Statement::new`] takes SQL written for one backend (`$1` on
/// PostgreSQL, `?` on MySQL and SQLite), as a renderer produces it.
/// [`Statement::portable`] takes SQL with `?` placeholders that runs on any
/// backend: on PostgreSQL each `?` becomes `$n` when it runs, `??` is a
/// literal `?` (PostgreSQL's JSON operators), and quoted text (`'..'`,
/// `".."`, `` `..` ``) is left alone. Values are never written into the
/// SQL.
///
/// ```
/// use navree_db::{Backend, Statement};
///
/// let statement = Statement::portable("SELECT * FROM deals WHERE stage = ? AND amount > ?", ["won".into(), 100_i64.into()]);
/// assert_eq!(
///     statement.for_backend(Backend::Postgres).unwrap().sql(),
///     "SELECT * FROM deals WHERE stage = $1 AND amount > $2"
/// );
/// assert_eq!(
///     statement.to_string(),
///     "SELECT * FROM deals WHERE stage = 'won' AND amount > 100"
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    sql: String,
    values: Vec<Value>,
    placeholders: Placeholders,
}

impl Statement {
    /// SQL written for `backend` (its own placeholders) with its values.
    pub fn new(
        backend: Backend,
        sql: impl Into<String>,
        values: impl IntoIterator<Item = Value>,
    ) -> Statement {
        Statement {
            sql: sql.into(),
            values: values.into_iter().collect(),
            placeholders: Placeholders::Native(backend),
        }
    }

    /// SQL with portable `?` placeholders, for any backend.
    pub fn portable(sql: impl Into<String>, values: impl IntoIterator<Item = Value>) -> Statement {
        Statement {
            sql: sql.into(),
            values: values.into_iter().collect(),
            placeholders: Placeholders::Portable,
        }
    }

    /// SQL that binds nothing, for any backend (its `?`, if any, must sit
    /// inside quotes or be doubled).
    pub fn text(sql: impl Into<String>) -> Statement {
        Statement::portable(sql, [])
    }

    /// The SQL as written.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// The bound values, in placeholder order.
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// The backend the SQL was written for; `None` for portable SQL.
    pub fn backend(&self) -> Option<Backend> {
        match self.placeholders {
            Placeholders::Native(backend) => Some(backend),
            Placeholders::Portable => None,
        }
    }

    /// The SQL and the values.
    pub fn into_parts(self) -> (String, Vec<Value>) {
        (self.sql, self.values)
    }

    /// The statement as `backend` runs it: portable placeholders rewritten,
    /// the number of placeholders checked against the values. A statement
    /// written for another backend is an error.
    pub fn for_backend(&self, backend: Backend) -> Result<Cow<'_, Statement>, DbError> {
        match self.placeholders {
            Placeholders::Native(own) if own == backend => Ok(Cow::Borrowed(self)),
            Placeholders::Native(own) => Err(DbError::invalid(format!(
                "a statement written for {own} cannot run on {backend}"
            ))),
            Placeholders::Portable => {
                let sql = rewrite_placeholders(&self.sql, self.values.len(), backend)?;
                Ok(Cow::Owned(Statement {
                    sql,
                    values: self.values.clone(),
                    placeholders: Placeholders::Native(backend),
                }))
            }
        }
    }
}

impl fmt::Display for Statement {
    /// The SQL with its values written in, for reading (logs, test
    /// output); statements run with their values bound instead.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backend = self.backend().unwrap_or(Backend::Sqlite);
        f.write_str(&inline_values(&self.sql, &self.values, backend))
    }
}

/// `sql` with each placeholder replaced by its value as a literal: `$n`
/// on PostgreSQL, `?` elsewhere (`??` stays a literal `?`). Quoted text is
/// left alone.
pub(crate) fn inline_values(sql: &str, values: &[Value], backend: Backend) -> String {
    let mut out = String::with_capacity(sql.len() + values.len() * 8);
    let mut next = 0;
    scan(sql, |piece| match piece {
        Piece::Text(text) => out.push_str(text),
        Piece::Question => {
            match values.get(next) {
                Some(value) => out.push_str(&value.to_sql_literal()),
                None => out.push('?'),
            }
            next += 1;
        }
        Piece::DoubleQuestion => out.push_str("??"),
        Piece::Dollar(n) if backend == Backend::Postgres => {
            match n.checked_sub(1).and_then(|i| values.get(i)) {
                Some(value) => out.push_str(&value.to_sql_literal()),
                None => {
                    out.push('$');
                    out.push_str(&n.to_string());
                }
            }
        }
        Piece::Dollar(n) => {
            out.push('$');
            out.push_str(&n.to_string());
        }
    });
    out
}

/// A piece of SQL text, as [`scan`] cuts it.
enum Piece<'a> {
    /// Text to keep as it is (quoted text included).
    Text(&'a str),
    /// `?`.
    Question,
    /// `??`.
    DoubleQuestion,
    /// `$n`.
    Dollar(usize),
}

/// Cuts `sql` into text and placeholders, leaving quoted text (`'..'`,
/// `".."`, `` `..` ``, with doubled quotes inside) and comments alone.
fn scan<'a>(sql: &'a str, mut piece: impl FnMut(Piece<'a>)) {
    let bytes = sql.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == quote {
                        if bytes.get(i + 1) == Some(&quote) {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i += 2;
            }
            b'?' => {
                piece(Piece::Text(&sql[start..i]));
                if bytes.get(i + 1) == Some(&b'?') {
                    piece(Piece::DoubleQuestion);
                    i += 2;
                } else {
                    piece(Piece::Question);
                    i += 1;
                }
                start = i;
            }
            b'$' if bytes.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                let digits = bytes[i + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                piece(Piece::Text(&sql[start..i]));
                let n = sql[i + 1..i + 1 + digits].parse().unwrap_or(0);
                piece(Piece::Dollar(n));
                i += 1 + digits;
                start = i;
            }
            _ => i += 1,
        }
    }
    piece(Piece::Text(&sql[start.min(sql.len())..]));
}

/// Rewrites portable `?` placeholders for `backend` (`$n` on PostgreSQL)
/// and `??` into a literal `?`, checking that there are `values` of them.
fn rewrite_placeholders(sql: &str, values: usize, backend: Backend) -> Result<String, DbError> {
    let mut out = String::with_capacity(sql.len() + values * 2);
    let mut count = 0;
    scan(sql, |piece| match piece {
        Piece::Text(text) => out.push_str(text),
        Piece::Question => {
            count += 1;
            out.push_str(&backend.placeholder(count));
        }
        Piece::DoubleQuestion => out.push('?'),
        Piece::Dollar(n) => {
            out.push('$');
            out.push_str(&n.to_string());
        }
    });
    if count != values {
        return Err(DbError::invalid(format!(
            "the SQL `{sql}` has {count} placeholder(s) but {values} value(s)"
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_placeholders_become_the_backends_own() {
        let statement = Statement::portable("a = ? AND b = ?", [1_i64.into(), 2_i64.into()]);
        assert_eq!(
            statement.for_backend(Backend::Postgres).unwrap().sql(),
            "a = $1 AND b = $2"
        );
        assert_eq!(
            statement.for_backend(Backend::MySql).unwrap().sql(),
            "a = ? AND b = ?"
        );
        assert_eq!(
            Statement::portable("data ?? 'k' AND x = ?", [1_i64.into()])
                .for_backend(Backend::Postgres)
                .unwrap()
                .sql(),
            "data ? 'k' AND x = $1"
        );
        assert_eq!(
            Statement::portable("name = '?' AND \"q?\" = ? -- what?\n", [1_i64.into()])
                .for_backend(Backend::Postgres)
                .unwrap()
                .sql(),
            "name = '?' AND \"q?\" = $1 -- what?\n"
        );
        assert_eq!(
            Statement::portable("'it''s ?' = ? /* ? */", [1_i64.into()])
                .for_backend(Backend::Postgres)
                .unwrap()
                .sql(),
            "'it''s ?' = $1 /* ? */"
        );
    }

    #[test]
    fn placeholder_counts_must_match() {
        let error = Statement::portable("a = ?", [1_i64.into(), 2_i64.into()])
            .for_backend(Backend::Sqlite)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid statement: the SQL `a = ?` has 1 placeholder(s) but 2 value(s)"
        );
    }

    #[test]
    fn native_statements_run_on_their_backend_only() {
        let statement = Statement::new(Backend::Postgres, "SELECT $1", [1_i64.into()]);
        assert_eq!(statement.backend(), Some(Backend::Postgres));
        assert!(statement.for_backend(Backend::Postgres).is_ok());
        assert!(statement.for_backend(Backend::MySql).is_err());
    }

    #[test]
    fn values_are_written_in_for_reading() {
        let statement = Statement::new(
            Backend::Postgres,
            "UPDATE t SET a = $2 WHERE b = $1 AND c = '$1'",
            ["x".into(), Value::Null(crate::Kind::I64)],
        );
        assert_eq!(
            statement.to_string(),
            "UPDATE t SET a = NULL WHERE b = 'x' AND c = '$1'"
        );
        let mysql = Statement::new(Backend::MySql, "SELECT ?, ?", [true.into(), 1.5_f64.into()]);
        assert_eq!(mysql.to_string(), "SELECT TRUE, 1.5");
    }
}
