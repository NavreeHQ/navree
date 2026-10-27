//! SQL scripts: [`split_statements`] cuts a script (a schema dump, a
//! hand-written `.sql` file) into the statements to run one by one.

use crate::target::Dialect;

/// Splits an SQL script into its statements, without their terminating
/// `;`, for `dialect`.
///
/// It understands what each database's own client understands, so a script
/// that loads with `psql`, `mysql` or `sqlite3` splits the same way:
///
/// - strings (`'it''s'`, MySQL's backslash escapes and double-quoted
///   strings, PostgreSQL's `E'...'`), quoted identifiers (`"..."`,
///   `` `...` ``, `[...]`), line comments (`--`, MySQL's `#`) and block
///   comments (nested on PostgreSQL);
/// - PostgreSQL dollar quotes (`$body$ ... $body$`) and SQL-standard
///   function bodies (`BEGIN ATOMIC ... END`);
/// - SQLite trigger bodies (`BEGIN ... END`, `CASE ... END` inside);
/// - MySQL's `DELIMITER` directive, which compound statements (triggers,
///   procedures) use: `DELIMITER ;;` ... `END;;` ... `DELIMITER ;`.
///
/// Comments before a statement are dropped; empty statements are skipped.
///
/// ```
/// use navree_schema::{Dialect, split_statements};
///
/// let script = "CREATE TABLE notes (body TEXT DEFAULT 'a;b'); -- the table\n\
///               INSERT INTO notes VALUES ('it''s;');";
/// assert_eq!(
///     split_statements(Dialect::Sqlite, script),
///     ["CREATE TABLE notes (body TEXT DEFAULT 'a;b')", "INSERT INTO notes VALUES ('it''s;')"]
/// );
/// ```
pub fn split_statements(dialect: Dialect, sql: &str) -> Vec<String> {
    Splitter::new(dialect, sql).run()
}

struct Splitter<'a> {
    dialect: Dialect,
    sql: &'a str,
    bytes: &'a [u8],
    at: usize,
    /// Where the current statement's first token starts.
    start: Option<usize>,
    /// The statement terminator (MySQL's `DELIMITER` changes it).
    delimiter: String,
    /// The first words of the current statement, upper-cased.
    words: Vec<String>,
    /// `BEGIN`/`CASE` ... `END` nesting inside a body.
    depth: u32,
    statements: Vec<String>,
}

impl<'a> Splitter<'a> {
    fn new(dialect: Dialect, sql: &'a str) -> Splitter<'a> {
        Splitter {
            dialect,
            sql,
            bytes: sql.as_bytes(),
            at: 0,
            start: None,
            delimiter: ";".to_owned(),
            words: Vec::new(),
            depth: 0,
            statements: Vec::new(),
        }
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.at + offset).copied()
    }

    fn begin_statement(&mut self) {
        if self.start.is_none() {
            self.start = Some(self.at);
        }
    }

    fn finish_statement(&mut self, end: usize) {
        if let Some(start) = self.start.take() {
            let statement = self.sql[start..end].trim();
            if !statement.is_empty() {
                self.statements.push(statement.to_owned());
            }
        }
        self.words.clear();
        self.depth = 0;
    }

    fn run(mut self) -> Vec<String> {
        while self.at < self.bytes.len() {
            if self.dialect == Dialect::MySql && self.start.is_none() && self.at_line_start() {
                if let Some(next) = self.delimiter_directive() {
                    self.at = next;
                    continue;
                }
            }
            if self.depth == 0 && self.sql[self.at..].starts_with(self.delimiter.as_str()) {
                let end = self.at;
                self.at += self.delimiter.len();
                self.finish_statement(end);
                continue;
            }
            let byte = self.bytes[self.at];
            match byte {
                b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' => self.at += 1,
                b'-' if self.peek(1) == Some(b'-') && self.dash_comment() => self.skip_line(),
                b'#' if self.dialect == Dialect::MySql => self.skip_line(),
                b'/' if self.peek(1) == Some(b'*') => self.skip_block_comment(),
                b'\'' => {
                    self.begin_statement();
                    let backslashes = self.dialect == Dialect::MySql || self.escape_string_prefix();
                    self.skip_quoted(b'\'', backslashes);
                }
                b'"' => {
                    self.begin_statement();
                    self.skip_quoted(b'"', self.dialect == Dialect::MySql);
                }
                b'`' => {
                    self.begin_statement();
                    self.skip_quoted(b'`', false);
                }
                b'[' if self.dialect == Dialect::Sqlite => {
                    self.begin_statement();
                    self.skip_until(b']');
                }
                b'$' if self.dialect == Dialect::Postgres => {
                    self.begin_statement();
                    self.dollar();
                }
                byte if byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80 => {
                    self.begin_statement();
                    self.word();
                }
                _ => {
                    self.begin_statement();
                    self.at += 1;
                }
            }
        }
        let end = self.bytes.len();
        self.finish_statement(end);
        self.statements
    }

    fn at_line_start(&self) -> bool {
        self.sql[..self.at]
            .rfind('\n')
            .map_or(&self.sql[..self.at], |nl| &self.sql[nl + 1..self.at])
            .trim()
            .is_empty()
    }

    /// `DELIMITER <token>` on its own line: sets the delimiter and returns
    /// where the next line starts.
    fn delimiter_directive(&mut self) -> Option<usize> {
        let line_end = self.sql[self.at..]
            .find('\n')
            .map_or(self.sql.len(), |n| self.at + n);
        let line = self.sql[self.at..line_end].trim();
        let (keyword, rest) = line.split_once(char::is_whitespace)?;
        if !keyword.eq_ignore_ascii_case("DELIMITER") {
            return None;
        }
        let token = rest.trim();
        if token.is_empty() {
            return None;
        }
        self.delimiter = token.to_owned();
        Some((line_end + 1).min(self.sql.len()))
    }

    /// `--` starts a comment, except on MySQL when no space follows (`1--1`).
    fn dash_comment(&self) -> bool {
        self.dialect != Dialect::MySql
            || self
                .peek(2)
                .is_none_or(|b| b.is_ascii_whitespace())
    }

    fn skip_line(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at] != b'\n' {
            self.at += 1;
        }
    }

    fn skip_block_comment(&mut self) {
        let nested = self.dialect == Dialect::Postgres;
        let mut depth = 0;
        while self.at < self.bytes.len() {
            if self.bytes[self.at] == b'/' && self.peek(1) == Some(b'*') {
                if depth > 0 && !nested {
                    self.at += 2;
                    continue;
                }
                depth += 1;
                self.at += 2;
            } else if self.bytes[self.at] == b'*' && self.peek(1) == Some(b'/') {
                depth -= 1;
                self.at += 2;
                if depth == 0 {
                    return;
                }
            } else {
                self.at += 1;
            }
        }
    }

    /// Whether the `'` at the cursor opens a PostgreSQL `E'...'` string.
    fn escape_string_prefix(&self) -> bool {
        self.dialect == Dialect::Postgres
            && self.at > 0
            && matches!(self.bytes[self.at - 1], b'E' | b'e')
            && (self.at < 2 || !is_word_byte(self.bytes[self.at - 2]))
    }

    fn skip_quoted(&mut self, quote: u8, backslashes: bool) {
        self.at += 1;
        while self.at < self.bytes.len() {
            let byte = self.bytes[self.at];
            if backslashes && byte == b'\\' {
                self.at += 2;
                continue;
            }
            if byte == quote {
                if self.peek(1) == Some(quote) {
                    self.at += 2;
                    continue;
                }
                self.at += 1;
                return;
            }
            self.at += 1;
        }
    }

    fn skip_until(&mut self, close: u8) {
        while self.at < self.bytes.len() && self.bytes[self.at] != close {
            self.at += 1;
        }
        self.at = (self.at + 1).min(self.bytes.len());
    }

    /// A PostgreSQL dollar quote (`$$`, `$body$`), or a lone `$` (`$1`).
    fn dollar(&mut self) {
        let rest = &self.sql[self.at + 1..];
        let tag_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80)
            .count();
        let starts_with_digit = rest.as_bytes().first().is_some_and(u8::is_ascii_digit);
        if starts_with_digit || rest.as_bytes().get(tag_len) != Some(&b'$') {
            self.at += 1;
            return;
        }
        let tag = &self.sql[self.at..self.at + tag_len + 2];
        let body_start = self.at + tag.len();
        match self.sql[body_start..].find(tag) {
            Some(offset) => self.at = body_start + offset + tag.len(),
            None => self.at = self.bytes.len(),
        }
    }

    fn word(&mut self) {
        let start = self.at;
        while self.at < self.bytes.len() && is_word_byte(self.bytes[self.at]) {
            self.at += 1;
        }
        let word = self.sql[start..self.at].to_ascii_uppercase();
        if self.words.len() < 4 {
            self.words.push(word.clone());
        }
        if self.has_body() {
            match word.as_str() {
                "BEGIN" | "CASE" => self.depth += 1,
                "END" => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    /// Whether the statement can hold `;`-separated statements in a
    /// `BEGIN ... END` body: a SQLite trigger, or a PostgreSQL function or
    /// procedure (SQL-standard body).
    fn has_body(&self) -> bool {
        let words: Vec<&str> = self.words.iter().map(String::as_str).collect();
        match self.dialect {
            Dialect::Sqlite => matches!(
                words.as_slice(),
                ["CREATE", "TRIGGER", ..] | ["CREATE", "TEMP" | "TEMPORARY", "TRIGGER", ..]
            ),
            Dialect::Postgres => matches!(
                words.as_slice(),
                ["CREATE", "FUNCTION" | "PROCEDURE", ..]
                    | ["CREATE", "OR", "REPLACE", "FUNCTION" | "PROCEDURE"]
            ),
            Dialect::MySql => false,
        }
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_outside_strings_and_comments() {
        let script = "-- leading comment; dropped\n\
                      CREATE TABLE a (x TEXT DEFAULT 'a;b', \"we;ird\" INT); -- trailing; comment\n\
                      INSERT INTO a (x) VALUES ('it''s;');\n\
                      /* block; */ SELECT 1;;\n   \n";
        assert_eq!(
            split_statements(Dialect::Sqlite, script),
            [
                "CREATE TABLE a (x TEXT DEFAULT 'a;b', \"we;ird\" INT)",
                "INSERT INTO a (x) VALUES ('it''s;')",
                "SELECT 1",
            ]
        );
        assert_eq!(
            split_statements(Dialect::Sqlite, "SELECT 1"),
            ["SELECT 1"],
            "the last statement needs no terminator"
        );
        assert!(split_statements(Dialect::Postgres, " -- only a comment\n").is_empty());
    }

    #[test]
    fn postgres_dollar_quotes_escape_strings_and_atomic_bodies() {
        let script = "CREATE FUNCTION touch() RETURNS trigger AS $body$\n\
                      BEGIN NEW.updated_at := now(); RETURN NEW; END;\n\
                      $body$ LANGUAGE plpgsql;\n\
                      CREATE OR REPLACE FUNCTION two() RETURNS int LANGUAGE sql\n\
                      BEGIN ATOMIC SELECT CASE WHEN true THEN 1 END; SELECT 2; END;\n\
                      SELECT E'it\\'s;', $$a;b$$, $1::text, 'c\\';\n\
                      /* outer /* nested; */ still; */ SELECT 3;";
        assert_eq!(
            split_statements(Dialect::Postgres, script),
            [
                "CREATE FUNCTION touch() RETURNS trigger AS $body$\nBEGIN NEW.updated_at := now(); RETURN NEW; END;\n$body$ LANGUAGE plpgsql",
                "CREATE OR REPLACE FUNCTION two() RETURNS int LANGUAGE sql\nBEGIN ATOMIC SELECT CASE WHEN true THEN 1 END; SELECT 2; END",
                "SELECT E'it\\'s;', $$a;b$$, $1::text, 'c\\'",
                "SELECT 3",
            ]
        );
    }

    #[test]
    fn sqlite_trigger_bodies_and_bracketed_names() {
        let script = "CREATE TRIGGER touch AFTER UPDATE ON notes BEGIN\n\
                      UPDATE notes SET rank = CASE WHEN new.rank < 0 THEN 0 END;\n\
                      DELETE FROM [old;log];\n\
                      END;\n\
                      CREATE INDEX notes_rank ON notes (rank);";
        assert_eq!(
            split_statements(Dialect::Sqlite, script),
            [
                "CREATE TRIGGER touch AFTER UPDATE ON notes BEGIN\nUPDATE notes SET rank = CASE WHEN new.rank < 0 THEN 0 END;\nDELETE FROM [old;log];\nEND",
                "CREATE INDEX notes_rank ON notes (rank)",
            ]
        );
    }

    #[test]
    fn mysql_delimiters_backslashes_and_hash_comments() {
        let script = "SET FOREIGN_KEY_CHECKS=0;\n\
                      DELIMITER ;;\n\
                      CREATE TRIGGER t BEFORE INSERT ON a FOR EACH ROW BEGIN SET NEW.x = 'a;b\\'c'; END;;\n\
                      DELIMITER ;\n\
                      INSERT INTO a VALUES (\"x;y\", 1--1); # a comment;\n\
                      SET FOREIGN_KEY_CHECKS=1;";
        assert_eq!(
            split_statements(Dialect::MySql, script),
            [
                "SET FOREIGN_KEY_CHECKS=0",
                "CREATE TRIGGER t BEFORE INSERT ON a FOR EACH ROW BEGIN SET NEW.x = 'a;b\\'c'; END",
                "INSERT INTO a VALUES (\"x;y\", 1--1)",
                "SET FOREIGN_KEY_CHECKS=1",
            ]
        );
    }
}
