//! The SQL buffer a grammar writes into: the text, the values bound to its
//! placeholders (`$1`, `$2`, .. on PostgreSQL, `?` elsewhere, numbered in
//! the order they are written, so nested subqueries, CTEs and compound
//! parts number correctly), identifiers quoted with their quote character
//! doubled, string literals escaped for the dialect, and raw fragments with
//! portable placeholders.

use super::ast::{Ident, ObjectName};
use crate::error::OrmError;
use crate::sql::{Dialect, Statement};
use crate::value::Value;

/// A statement being written for one dialect.
///
/// ```
/// use navree_orm::render::Writer;
/// use navree_orm::render::ast::ObjectName;
/// use navree_orm::{Dialect, Value};
///
/// let mut w = Writer::new(Dialect::Postgres);
/// w.push("SELECT * FROM ");
/// w.object_name(&ObjectName::parse("audit.events"));
/// w.push(" WHERE ");
/// w.ident("odd\"name");
/// w.push(" = ");
/// w.bind(Value::from(7_i64));
/// let statement = w.finish();
/// assert_eq!(statement.sql, r#"SELECT * FROM "audit"."events" WHERE "odd""name" = $1"#);
/// assert_eq!(statement.values, [Value::from(7_i64)]);
/// ```
#[derive(Debug)]
pub struct Writer {
    dialect: Dialect,
    sql: String,
    values: Vec<Value>,
    aliases: usize,
}

impl Writer {
    /// An empty statement for `dialect`.
    pub fn new(dialect: Dialect) -> Self {
        Writer {
            dialect,
            sql: String::with_capacity(128),
            values: Vec::new(),
            aliases: 0,
        }
    }

    /// The dialect being written.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// The SQL written so far.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// Appends SQL text as it is.
    pub fn push(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    /// The identifier quote of the dialect: `` ` `` on MySQL, `"`
    /// elsewhere.
    pub fn quote_char(&self) -> char {
        match self.dialect {
            Dialect::MySql => '`',
            Dialect::Postgres | Dialect::Sqlite => '"',
        }
    }

    /// Appends `name` as one quoted identifier (a quote inside it is
    /// doubled; a dot is part of the name).
    pub fn ident(&mut self, name: &str) {
        let quote = self.quote_char();
        self.sql.push(quote);
        for c in name.chars() {
            if c == quote {
                self.sql.push(quote);
            }
            self.sql.push(c);
        }
        self.sql.push(quote);
    }

    /// Appends an [`Ident`].
    pub fn identifier(&mut self, ident: &Ident) {
        self.ident(ident.as_str());
    }

    /// Appends a name of several parts, each quoted on its own:
    /// `"audit"."events"`.
    pub fn object_name(&mut self, name: &ObjectName) {
        for (i, part) in name.0.iter().enumerate() {
            if i > 0 {
                self.sql.push('.');
            }
            self.ident(part.as_str());
        }
    }

    /// Appends the next placeholder and binds `value` to it.
    pub fn bind(&mut self, value: Value) {
        self.values.push(value);
        match self.dialect {
            Dialect::Postgres => {
                self.sql.push('$');
                self.sql.push_str(&self.values.len().to_string());
            }
            Dialect::MySql | Dialect::Sqlite => self.sql.push('?'),
        }
    }

    /// Appends `text` as a string literal: quotes doubled; on PostgreSQL a
    /// text with a backslash is an escape string (`E'a\\b'`), on MySQL a
    /// backslash is doubled (its strings read backslash escapes).
    pub fn string_literal(&mut self, text: &str) {
        let escape_backslashes = match self.dialect {
            Dialect::Postgres if text.contains('\\') => {
                self.sql.push('E');
                true
            }
            Dialect::MySql => true,
            _ => false,
        };
        self.sql.push('\'');
        for c in text.chars() {
            match c {
                '\'' => self.sql.push_str("''"),
                '\\' if escape_backslashes => self.sql.push_str("\\\\"),
                other => self.sql.push(other),
            }
        }
        self.sql.push('\'');
    }

    /// Appends a fragment written by hand with portable placeholders: `?`
    /// binds the next of `values`, `??` is a literal `?`, and quoted text
    /// (`'..'`, `".."`, `` `..` ``) is left alone. The fragment is written
    /// once and never read again, so a PostgreSQL `$` needs no escaping.
    /// Fails when the placeholders and the values differ in number.
    pub fn raw(&mut self, sql: &str, values: &[Value]) -> Result<(), OrmError> {
        check_raw(sql, values.len(), self.dialect)?;
        let mut values = values.iter();
        for token in scan(sql, self.dialect) {
            match token {
                Token::Text(text) => self.sql.push_str(text),
                Token::Question => {
                    if let Some(value) = values.next() {
                        self.bind(value.clone());
                    }
                }
                Token::DoubleQuestion => self.sql.push('?'),
                Token::Hole => self.sql.push_str("{}"),
            }
        }
        Ok(())
    }

    /// A name for a generated alias, unique in the statement:
    /// `navree_union_1`, `navree_union_2`, ..
    pub fn next_alias(&mut self, prefix: &str) -> String {
        self.aliases += 1;
        format!("{prefix}_{}", self.aliases)
    }

    /// The statement.
    pub fn finish(self) -> Statement {
        Statement {
            sql: self.sql,
            values: self.values,
            dialect: self.dialect,
        }
    }
}

/// A piece of a raw fragment or template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    /// Text written as it is (quoted text included).
    Text(&'a str),
    /// `?` outside quotes.
    Question,
    /// `??` outside quotes.
    DoubleQuestion,
    /// `{}` outside quotes.
    Hole,
}

/// `sql` split into text, `?`, `??` and `{}` outside quotes. Quoted text
/// (`'..'`, `".."`, `` `..` ``, a doubled quote escaping itself; on MySQL
/// also a backslash escape) stays inside text tokens.
pub(crate) fn scan(sql: &str, dialect: Dialect) -> Vec<Token<'_>> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(open) = quote {
            if dialect == Dialect::MySql && c == b'\\' && open != b'`' {
                i += 2;
                continue;
            }
            if c == open {
                if bytes.get(i + 1) == Some(&open) {
                    i += 2;
                    continue;
                }
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' | b'"' | b'`' => {
                quote = Some(c);
                i += 1;
            }
            b'?' => {
                if start < i {
                    tokens.push(Token::Text(&sql[start..i]));
                }
                if bytes.get(i + 1) == Some(&b'?') {
                    tokens.push(Token::DoubleQuestion);
                    i += 2;
                } else {
                    tokens.push(Token::Question);
                    i += 1;
                }
                start = i;
            }
            b'{' if bytes.get(i + 1) == Some(&b'}') => {
                if start < i {
                    tokens.push(Token::Text(&sql[start..i]));
                }
                tokens.push(Token::Hole);
                i += 2;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < sql.len() {
        tokens.push(Token::Text(&sql[start.min(sql.len())..]));
    }
    tokens
}

/// Checks that a raw fragment has as many `?` placeholders (`??` is not
/// one) as values.
pub(crate) fn check_raw(sql: &str, values: usize, dialect: Dialect) -> Result<(), OrmError> {
    let placeholders = scan(sql, dialect)
        .iter()
        .filter(|token| matches!(token, Token::Question))
        .count();
    if placeholders == values {
        Ok(())
    } else {
        Err(OrmError::invalid(format!(
            "the raw SQL `{sql}` has {placeholders} placeholder(s) but {values} value(s)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(sql: &str, values: usize, dialect: Dialect) -> Result<String, OrmError> {
        let mut w = Writer::new(dialect);
        let values: Vec<Value> = (0..values).map(|i| Value::from(i as i64)).collect();
        w.raw(sql, &values)?;
        Ok(w.finish().sql)
    }

    #[test]
    fn raw_placeholders_are_portable() {
        assert_eq!(
            raw("a = ? AND b = ?", 2, Dialect::Postgres).unwrap(),
            "a = $1 AND b = $2"
        );
        assert_eq!(
            raw("a = ? AND b = ?", 2, Dialect::Sqlite).unwrap(),
            "a = ? AND b = ?"
        );
        assert_eq!(
            raw("data ?? 'k' AND x = ?", 1, Dialect::Postgres).unwrap(),
            "data ? 'k' AND x = $1"
        );
        assert_eq!(raw("data ?? 'k'", 0, Dialect::MySql).unwrap(), "data ? 'k'");
        assert_eq!(
            raw("name = '?' AND x = ?", 1, Dialect::Postgres).unwrap(),
            "name = '?' AND x = $1"
        );
        // Written once and never parsed again: `$` stays itself.
        assert_eq!(
            raw("price > 5$", 0, Dialect::Postgres).unwrap(),
            "price > 5$"
        );
        assert_eq!(
            raw("'it''s ?' = ?", 1, Dialect::Sqlite).unwrap(),
            "'it''s ?' = ?"
        );
        assert_eq!(
            raw(r"'it\'s ?' = ?", 1, Dialect::MySql).unwrap(),
            r"'it\'s ?' = ?"
        );
        assert_eq!(
            raw("\"a?\" = `b?` AND c = ?", 1, Dialect::Postgres).unwrap(),
            "\"a?\" = `b?` AND c = $1"
        );
        let error = raw("a = ?", 2, Dialect::Sqlite).unwrap_err().to_string();
        assert_eq!(
            error,
            "invalid query: the raw SQL `a = ?` has 1 placeholder(s) but 2 value(s)"
        );
    }

    #[test]
    fn identifiers_double_their_quotes() {
        let mut w = Writer::new(Dialect::MySql);
        w.ident("odd`name");
        w.push(" ");
        w.object_name(&ObjectName::parse("audit.events"));
        assert_eq!(w.sql(), "`odd``name` `audit`.`events`");
        let mut w = Writer::new(Dialect::Sqlite);
        w.ident("a\"b.c");
        assert_eq!(w.sql(), r#""a""b.c""#);
    }

    #[test]
    fn string_literals_escape_per_dialect() {
        let literal = |dialect, text: &str| {
            let mut w = Writer::new(dialect);
            w.string_literal(text);
            w.finish().sql
        };
        assert_eq!(literal(Dialect::Postgres, "it's"), "'it''s'");
        assert_eq!(literal(Dialect::Postgres, r"a\b'"), r"E'a\\b'''");
        assert_eq!(literal(Dialect::MySql, r#"$."a\"b""#), r#"'$."a\\"b"'"#);
        assert_eq!(literal(Dialect::Sqlite, r"a\b'"), r"'a\b'''");
    }

    #[test]
    fn placeholders_number_in_writing_order() {
        let mut w = Writer::new(Dialect::Postgres);
        w.bind(Value::from(1_i64));
        w.push(", ");
        w.raw("? + ?", &[Value::from(2_i64), Value::from(3_i64)])
            .unwrap();
        assert_eq!(w.finish().sql, "$1, $2 + $3");
    }

    #[test]
    fn templates_scan_holes_outside_quotes() {
        assert_eq!(
            scan("f({}, '{}', {})", Dialect::Postgres),
            [
                Token::Text("f("),
                Token::Hole,
                Token::Text(", '{}', "),
                Token::Hole,
                Token::Text(")"),
            ]
        );
    }
}
