//! SQLite table rebuilds: the state of a table as read from the database,
//! and the small SQL tokenizer that keeps column definitions, constraints,
//! indexes and triggers intact while the table is rebuilt.

use crate::index::ReferentialAction;

/// A column of a table being rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateColumn {
    /// The column name.
    pub name: String,
    /// Everything after the name in its definition (type, `NOT NULL`,
    /// `DEFAULT`, `COLLATE`, `CHECK`, `GENERATED ...`), without the primary
    /// key and `REFERENCES` clauses, which the rebuild declares at table
    /// level.
    pub definition: String,
    /// A generated column (never copied).
    pub generated: bool,
    /// The definition declares `UNIQUE` inline.
    pub unique: bool,
}

/// An index of a table being rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateIndex {
    /// The index name.
    pub name: String,
    /// A unique index.
    pub unique: bool,
    /// The indexed columns (`None` for an expression).
    pub columns: Vec<Option<String>>,
    /// Its `CREATE INDEX` statement (`None` for indexes SQLite creates for
    /// `UNIQUE` and `PRIMARY KEY` constraints).
    pub sql: Option<String>,
}

impl StateIndex {
    /// Whether the index was created with `CREATE INDEX` (not by a
    /// constraint).
    pub fn is_explicit(&self) -> bool {
        self.sql.is_some()
    }

    fn uses(&self, column: &str) -> bool {
        self.columns
            .iter()
            .any(|c| c.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(column)))
            || self.sql.as_deref().is_some_and(|sql| mentions(sql, column))
    }
}

/// A foreign key of a table being rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateForeign {
    /// The referencing columns.
    pub columns: Vec<String>,
    /// The referenced table.
    pub table: String,
    /// The referenced columns (empty: the referenced table's primary key).
    pub references: Vec<String>,
    /// The action on update.
    pub on_update: Option<ReferentialAction>,
    /// The action on delete.
    pub on_delete: Option<ReferentialAction>,
}

/// A SQLite table as it is now, read before a rebuild.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableState {
    /// The columns, in table order.
    pub columns: Vec<StateColumn>,
    /// The primary key's columns (empty: none declared).
    pub primary: Vec<String>,
    /// The column declared `PRIMARY KEY AUTOINCREMENT`.
    pub autoincrement: Option<String>,
    /// The foreign keys.
    pub foreign_keys: Vec<StateForeign>,
    /// Table-level `UNIQUE` and `CHECK` constraints, as written.
    pub constraints: Vec<String>,
    /// Table options after the column list (`WITHOUT ROWID`, `STRICT`).
    pub options: String,
    /// The indexes.
    pub indexes: Vec<StateIndex>,
    /// `CREATE TRIGGER` statements on the table.
    pub triggers: Vec<String>,
    /// Whether `PRAGMA foreign_keys` is on for the connection.
    pub foreign_keys_enabled: bool,
    /// Tables whose foreign keys reference this table.
    pub referenced_by: Vec<String>,
    /// Whether the table has rows.
    pub has_rows: bool,
}

impl TableState {
    /// Builds the state from a `CREATE TABLE` statement (columns, inline and
    /// table constraints); foreign keys, indexes and triggers come from the
    /// pragmas.
    pub fn from_create_sql(sql: &str) -> TableState {
        let parsed = parse_create_table(sql);
        let mut state = TableState::default();
        for column in parsed.columns {
            if column.primary {
                state.primary.push(column.name.clone());
            }
            if column.autoincrement {
                state.autoincrement = Some(column.name.clone());
            }
            state.columns.push(StateColumn {
                name: column.name,
                generated: column.generated,
                unique: column.unique,
                definition: column.definition,
            });
        }
        if let Some(primary) = parsed.primary {
            state.primary = primary;
        }
        state.constraints = parsed.constraints;
        state.options = parsed.options;
        state
    }

    /// The column named `name`.
    pub fn column(&self, name: &str) -> Option<&StateColumn> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub(crate) fn rename_column(&mut self, from: &str, to: &str) {
        for column in &mut self.columns {
            if column.name.eq_ignore_ascii_case(from) {
                column.name = to.to_owned();
            }
            column.definition = rename_identifier(&column.definition, from, to);
        }
        rename_in(&mut self.primary, from, to);
        if self
            .autoincrement
            .as_deref()
            .is_some_and(|c| c.eq_ignore_ascii_case(from))
        {
            self.autoincrement = Some(to.to_owned());
        }
        for foreign in &mut self.foreign_keys {
            rename_in(&mut foreign.columns, from, to);
        }
        for constraint in &mut self.constraints {
            *constraint = rename_identifier(constraint, from, to);
        }
        for index in &mut self.indexes {
            for column in index.columns.iter_mut().flatten() {
                if column.eq_ignore_ascii_case(from) {
                    *column = to.to_owned();
                }
            }
            if let Some(sql) = &mut index.sql {
                *sql = rename_identifier(sql, from, to);
            }
        }
        for trigger in &mut self.triggers {
            *trigger = rename_identifier(trigger, from, to);
        }
    }

    /// Forgets column `name` and everything that uses it (indexes, foreign
    /// keys, table constraints); returns the names of the dropped explicit
    /// indexes.
    pub(crate) fn drop_column(&mut self, name: &str) -> Vec<String> {
        self.columns.retain(|c| !c.name.eq_ignore_ascii_case(name));
        self.primary.retain(|c| !c.eq_ignore_ascii_case(name));
        if self
            .autoincrement
            .as_deref()
            .is_some_and(|c| c.eq_ignore_ascii_case(name))
        {
            self.autoincrement = None;
        }
        self.foreign_keys
            .retain(|f| !f.columns.iter().any(|c| c.eq_ignore_ascii_case(name)));
        self.constraints.retain(|c| !mentions(c, name));
        let mut dropped = Vec::new();
        self.indexes.retain(|index| {
            let uses = index.uses(name);
            if uses && index.is_explicit() {
                dropped.push(index.name.clone());
            }
            !uses
        });
        dropped
    }

    /// Whether dropping `name` needs a rebuild: it is part of the primary
    /// key, a foreign key or a table constraint, or declared `UNIQUE`.
    pub(crate) fn column_is_entangled(&self, name: &str) -> bool {
        self.primary.iter().any(|c| c.eq_ignore_ascii_case(name))
            || self
                .foreign_keys
                .iter()
                .any(|f| f.columns.iter().any(|c| c.eq_ignore_ascii_case(name)))
            || self.constraints.iter().any(|c| mentions(c, name))
            || self.column(name).is_some_and(|c| c.unique)
            || self
                .indexes
                .iter()
                .any(|i| !i.is_explicit() && i.uses(name))
    }

    /// The explicit indexes using column `name`.
    pub(crate) fn indexes_using(&self, name: &str) -> Vec<String> {
        self.indexes
            .iter()
            .filter(|i| i.is_explicit() && i.uses(name))
            .map(|i| i.name.clone())
            .collect()
    }
}

fn rename_in(columns: &mut [String], from: &str, to: &str) {
    for column in columns {
        if column.eq_ignore_ascii_case(from) {
            *column = to.to_owned();
        }
    }
}

// ----- tokenizer -------------------------------------------------------------

/// A token of SQLite SQL, with its byte span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// A bare word: keyword or identifier.
    Word,
    /// `"..."`, `` `...` `` or `[...]`.
    QuotedIdent,
    /// `'...'`.
    String,
    /// A number.
    Number,
    /// `(`.
    Open,
    /// `)`.
    Close,
    /// `,`.
    Comma,
    /// `;`.
    Semicolon,
    /// Anything else (operators).
    Other,
}

/// Splits `sql` into tokens, skipping whitespace and comments.
pub(crate) fn tokenize(sql: &str) -> Vec<Token> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        let kind = match c {
            b' ' | b'\t' | b'\n' | b'\r' => {
                i += 1;
                continue;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                continue;
            }
            b'\'' | b'"' | b'`' => {
                let quote = c;
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
                i = (i + 1).min(bytes.len());
                if quote == b'\'' {
                    TokenKind::String
                } else {
                    TokenKind::QuotedIdent
                }
            }
            b'[' => {
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
                TokenKind::QuotedIdent
            }
            b'(' => {
                i += 1;
                TokenKind::Open
            }
            b')' => {
                i += 1;
                TokenKind::Close
            }
            b',' => {
                i += 1;
                TokenKind::Comma
            }
            b';' => {
                i += 1;
                TokenKind::Semicolon
            }
            c if c.is_ascii_digit() => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                    i += 1;
                }
                TokenKind::Number
            }
            c if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 => {
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric()
                        || bytes[i] == b'_'
                        || bytes[i] == b'$'
                        || bytes[i] >= 0x80)
                {
                    i += 1;
                }
                TokenKind::Word
            }
            _ => {
                i += 1;
                TokenKind::Other
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: i,
        });
    }
    tokens
}

/// The identifier a token names: a word as is, a quoted identifier
/// unquoted.
pub(crate) fn identifier(sql: &str, token: &Token) -> Option<String> {
    let text = &sql[token.start..token.end];
    match token.kind {
        TokenKind::Word => Some(text.to_owned()),
        TokenKind::QuotedIdent => {
            let inner = &text[1..text.len().saturating_sub(1)];
            Some(match text.as_bytes()[0] {
                b'"' => inner.replace("\"\"", "\""),
                b'`' => inner.replace("``", "`"),
                _ => inner.to_owned(),
            })
        }
        _ => None,
    }
}

fn is_word(sql: &str, token: Option<&Token>, word: &str) -> bool {
    token.is_some_and(|t| {
        t.kind == TokenKind::Word && sql[t.start..t.end].eq_ignore_ascii_case(word)
    })
}

/// Whether `sql` mentions identifier `name` (bare or quoted, any case).
pub(crate) fn mentions(sql: &str, name: &str) -> bool {
    tokenize(sql).iter().any(|t| {
        matches!(t.kind, TokenKind::Word | TokenKind::QuotedIdent)
            && identifier(sql, t).is_some_and(|i| i.eq_ignore_ascii_case(name))
    })
}

/// `sql` with every identifier `from` replaced by `"to"`.
pub(crate) fn rename_identifier(sql: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut last = 0;
    for token in tokenize(sql) {
        if matches!(token.kind, TokenKind::Word | TokenKind::QuotedIdent)
            && identifier(sql, &token).is_some_and(|i| i.eq_ignore_ascii_case(from))
        {
            out.push_str(&sql[last..token.start]);
            out.push_str(&format!("\"{}\"", to.replace('"', "\"\"")));
            last = token.end;
        }
    }
    out.push_str(&sql[last..]);
    out
}

/// `CREATE INDEX` SQL with the index renamed to `to`.
pub(crate) fn rename_index_sql(sql: &str, to: &str) -> String {
    let tokens = tokenize(sql);
    let mut i = 0;
    while i < tokens.len() && !is_word(sql, tokens.get(i), "INDEX") {
        i += 1;
    }
    i += 1;
    if is_word(sql, tokens.get(i), "IF") {
        i += 3;
    }
    match tokens.get(i) {
        Some(name) => {
            // A schema-qualified name: `"main"."old"`.
            let name = if tokens
                .get(i + 1)
                .is_some_and(|t| &sql[t.start..t.end] == ".")
            {
                tokens.get(i + 2).unwrap_or(name)
            } else {
                name
            };
            format!(
                "{}\"{}\"{}",
                &sql[..name.start],
                to.replace('"', "\"\""),
                &sql[name.end..]
            )
        }
        None => sql.to_owned(),
    }
}

// ----- CREATE TABLE parsing --------------------------------------------------

/// A column of a parsed `CREATE TABLE`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedColumn {
    pub(crate) name: String,
    pub(crate) definition: String,
    pub(crate) primary: bool,
    pub(crate) autoincrement: bool,
    pub(crate) generated: bool,
    pub(crate) unique: bool,
}

/// A parsed `CREATE TABLE`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ParsedTable {
    pub(crate) columns: Vec<ParsedColumn>,
    /// The table-level primary key's columns.
    pub(crate) primary: Option<Vec<String>>,
    /// Table-level `UNIQUE` and `CHECK` constraints.
    pub(crate) constraints: Vec<String>,
    /// What follows the column list: `WITHOUT ROWID`, `STRICT`.
    pub(crate) options: String,
}

const CONSTRAINT_STARTS: [&str; 5] = ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"];

/// Splits the body of a `CREATE TABLE` into column definitions and table
/// constraints.
pub(crate) fn parse_create_table(sql: &str) -> ParsedTable {
    let tokens = tokenize(sql);
    let Some(open) = tokens.iter().position(|t| t.kind == TokenKind::Open) else {
        return ParsedTable::default();
    };
    // Items: token ranges separated by top-level commas.
    let mut items: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut item_start = open + 1;
    let mut end = tokens.len();
    for (i, token) in tokens.iter().enumerate().skip(open + 1) {
        match token.kind {
            TokenKind::Open => depth += 1,
            TokenKind::Close if depth == 0 => {
                end = i;
                break;
            }
            TokenKind::Close => depth -= 1,
            TokenKind::Comma if depth == 0 => {
                items.push((item_start, i));
                item_start = i + 1;
            }
            _ => {}
        }
    }
    items.push((item_start, end));

    let mut parsed = ParsedTable::default();
    if let Some(close) = tokens.get(end) {
        parsed.options = sql[close.end..]
            .trim()
            .trim_end_matches(';')
            .trim()
            .to_owned();
    }
    for (from, to) in items {
        if from >= to {
            continue;
        }
        let item = &tokens[from..to];
        let text = &sql[item[0].start..item[item.len() - 1].end];
        let first = &item[0];
        let is_constraint = first.kind == TokenKind::Word
            && CONSTRAINT_STARTS
                .iter()
                .any(|k| sql[first.start..first.end].eq_ignore_ascii_case(k));
        if is_constraint {
            classify_constraint(sql, item, text, &mut parsed);
        } else if let Some(name) = identifier(sql, first) {
            parsed.columns.push(parse_column(sql, name, &item[1..]));
        }
    }
    parsed
}

fn classify_constraint(sql: &str, item: &[Token], text: &str, parsed: &mut ParsedTable) {
    let mut i = 0;
    if is_word(sql, item.first(), "CONSTRAINT") {
        i = 2;
    }
    if is_word(sql, item.get(i), "PRIMARY") {
        // PRIMARY KEY ( columns )
        let columns = item[i..]
            .iter()
            .skip_while(|t| t.kind != TokenKind::Open)
            .skip(1)
            .take_while(|t| t.kind != TokenKind::Close)
            .filter(|t| matches!(t.kind, TokenKind::Word | TokenKind::QuotedIdent))
            .filter(|t| {
                !["ASC", "DESC", "COLLATE"]
                    .iter()
                    .any(|k| sql[t.start..t.end].eq_ignore_ascii_case(k))
            })
            .filter_map(|t| identifier(sql, t))
            .collect();
        parsed.primary = Some(columns);
    } else if is_word(sql, item.get(i), "FOREIGN") {
        // Read from `pragma_foreign_key_list`.
    } else {
        parsed.constraints.push(text.to_owned());
    }
}

/// Parses one column definition (the tokens after its name), removing the
/// primary key and `REFERENCES` clauses.
fn parse_column(sql: &str, name: String, tokens: &[Token]) -> ParsedColumn {
    let mut column = ParsedColumn {
        name,
        definition: String::new(),
        primary: false,
        autoincrement: false,
        generated: false,
        unique: false,
    };
    let mut removed: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        let word = |t: Option<&Token>, w: &str| is_word(sql, t, w);
        // An optional `CONSTRAINT name` before a clause.
        let (clause_start, at) = if word(Some(token), "CONSTRAINT") {
            (i, i + 2)
        } else {
            (i, i)
        };
        if word(tokens.get(at), "PRIMARY") && word(tokens.get(at + 1), "KEY") {
            column.primary = true;
            let mut j = at + 2;
            if word(tokens.get(j), "ASC") || word(tokens.get(j), "DESC") {
                j += 1;
            }
            if word(tokens.get(j), "ON") && word(tokens.get(j + 1), "CONFLICT") {
                j += 3;
            }
            if word(tokens.get(j), "AUTOINCREMENT") {
                column.autoincrement = true;
                j += 1;
            }
            removed.push((clause_start, j.min(tokens.len())));
            i = j;
            continue;
        }
        if word(tokens.get(at), "REFERENCES") {
            let mut j = at + 2;
            if tokens.get(j).is_some_and(|t| t.kind == TokenKind::Open) {
                while j < tokens.len() && tokens[j].kind != TokenKind::Close {
                    j += 1;
                }
                j += 1;
            }
            loop {
                if word(tokens.get(j), "ON") {
                    j += 2;
                    if word(tokens.get(j), "SET") || word(tokens.get(j), "NO") {
                        j += 2;
                    } else {
                        j += 1;
                    }
                } else if word(tokens.get(j), "MATCH") {
                    j += 2;
                } else if word(tokens.get(j), "DEFERRABLE")
                    || (word(tokens.get(j), "NOT") && word(tokens.get(j + 1), "DEFERRABLE"))
                {
                    j += if word(tokens.get(j), "NOT") { 2 } else { 1 };
                    if word(tokens.get(j), "INITIALLY") {
                        j += 2;
                    }
                } else {
                    break;
                }
            }
            removed.push((clause_start, j.min(tokens.len())));
            i = j;
            continue;
        }
        if word(tokens.get(at), "UNIQUE") {
            column.unique = true;
        }
        if word(tokens.get(at), "AS")
            || (word(tokens.get(at), "GENERATED") && word(tokens.get(at + 1), "ALWAYS"))
        {
            column.generated = true;
        }
        i += 1;
    }
    let mut parts = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some((_, end)) = removed.iter().find(|(start, _)| *start == i) {
            i = *end;
            continue;
        }
        parts.push(i);
        i += 1;
    }
    // Rebuild the text from the kept token runs, keeping their spacing.
    let mut definition = String::new();
    let mut previous: Option<usize> = None;
    for index in parts {
        let token = &tokens[index];
        if let Some(p) = previous {
            if p + 1 == index {
                definition.push_str(&sql[tokens[p].end..token.start]);
            } else {
                definition.push(' ');
            }
        }
        definition.push_str(&sql[token.start..token.end]);
        previous = Some(index);
    }
    column.definition = definition.trim().to_owned();
    column
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_columns_and_constraints() {
        let sql = r#"CREATE TABLE "deals" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "company_id" INTEGER NOT NULL REFERENCES "companies" ("id") ON DELETE CASCADE, "title" VARCHAR NOT NULL COLLATE NOCASE, "status" VARCHAR CHECK ("status" IN ('a', 'b')) DEFAULT 'a', "total" INTEGER AS (1 + 2) STORED, "code" VARCHAR UNIQUE, CONSTRAINT "positive" CHECK ("total" > 0), UNIQUE ("title", "code"), FOREIGN KEY("company_id") REFERENCES "companies"("id"))"#;
        let parsed = parse_create_table(sql);
        let names: Vec<&str> = parsed.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["id", "company_id", "title", "status", "total", "code"]
        );
        let id = &parsed.columns[0];
        assert!(id.primary && id.autoincrement);
        assert_eq!(id.definition, "INTEGER NOT NULL");
        assert_eq!(parsed.columns[1].definition, "INTEGER NOT NULL");
        assert_eq!(
            parsed.columns[2].definition,
            "VARCHAR NOT NULL COLLATE NOCASE"
        );
        assert_eq!(
            parsed.columns[3].definition,
            "VARCHAR CHECK (\"status\" IN ('a', 'b')) DEFAULT 'a'"
        );
        assert!(parsed.columns[4].generated);
        assert!(parsed.columns[5].unique);
        assert_eq!(
            parsed.constraints,
            [
                "CONSTRAINT \"positive\" CHECK (\"total\" > 0)",
                "UNIQUE (\"title\", \"code\")"
            ]
        );
        assert_eq!(parsed.primary, None);
    }

    #[test]
    fn parses_table_primary_keys() {
        let parsed = parse_create_table(
            "CREATE TABLE members (workspace_id TEXT NOT NULL, user_id TEXT NOT NULL, PRIMARY KEY (workspace_id, user_id))",
        );
        assert_eq!(
            parsed.primary,
            Some(vec!["workspace_id".to_owned(), "user_id".to_owned()])
        );
        assert_eq!(parsed.columns[0].definition, "TEXT NOT NULL");
        let strict = parse_create_table("CREATE TABLE t (a INTEGER) WITHOUT ROWID, STRICT;");
        assert_eq!(strict.options, "WITHOUT ROWID, STRICT");
    }

    #[test]
    fn renames_identifiers_in_sql() {
        assert_eq!(
            rename_identifier(r#"CHECK ("status" IN ('status'))"#, "status", "stage"),
            r#"CHECK ("stage" IN ('status'))"#
        );
        assert_eq!(
            rename_identifier("CREATE INDEX i ON t (status, kind)", "STATUS", "stage"),
            r#"CREATE INDEX i ON t ("stage", kind)"#
        );
        assert!(mentions("CHECK (`total` > 0)", "total"));
        assert!(!mentions("CHECK ('total' > 0)", "total"));
    }

    #[test]
    fn renames_indexes_in_sql() {
        assert_eq!(
            rename_index_sql(
                r#"CREATE UNIQUE INDEX "old" ON "t" ("a") WHERE a > 0"#,
                "new"
            ),
            r#"CREATE UNIQUE INDEX "new" ON "t" ("a") WHERE a > 0"#
        );
        assert_eq!(
            rename_index_sql("CREATE INDEX IF NOT EXISTS old ON t (a)", "new"),
            r#"CREATE INDEX IF NOT EXISTS "new" ON t (a)"#
        );
    }

    #[test]
    fn dropping_a_column_drops_what_uses_it() {
        let mut state = TableState::from_create_sql(
            r#"CREATE TABLE "t" ("id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "a" VARCHAR, "b" VARCHAR, CHECK ("b" <> ''))"#,
        );
        state.indexes.push(StateIndex {
            name: "t_a_index".into(),
            unique: false,
            columns: vec![Some("a".into())],
            sql: Some(r#"CREATE INDEX "t_a_index" ON "t" ("a")"#.into()),
        });
        assert!(!state.column_is_entangled("a"));
        assert!(state.column_is_entangled("b"), "a CHECK uses it");
        assert!(state.column_is_entangled("id"), "primary key");
        assert_eq!(state.indexes_using("a"), ["t_a_index"]);
        assert_eq!(state.drop_column("a"), ["t_a_index"]);
        assert!(state.indexes.is_empty());
        state.drop_column("b");
        assert!(state.constraints.is_empty());
        assert_eq!(state.autoincrement.as_deref(), Some("id"));
    }
}
