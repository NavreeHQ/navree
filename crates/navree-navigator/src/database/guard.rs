//! The statement guard of `query_database`: a statement is parsed before it
//! is sent, and only one read-only statement passes.
//!
//! The guard is the second layer. The first is the database engine: on
//! SQLite a file opened read-only, where SQLite itself prepares exactly one
//! read-only statement under an authorizer that allows only reading; on
//! PostgreSQL a read-only transaction run by a role without superuser (or a
//! way to become one), server-file, server-program or signal privileges,
//! and without functions it may execute that act beyond it
//! (`query_database` refuses such a role unless Navigator is started with
//! `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`; see the database module
//! for what an ordinary role can still reach). The guard
//! and SQLite do not tokenize alike (SQLite's `$name::x(..)` variables), so
//! on SQLite the guard's own checks are never what keeps a query read-only.
//!
//! What is sent is the parsed statement rendered back to SQL, and only when
//! that text parses again to exactly one statement with the same tree: a
//! value the renderer writes back differently from how it was read cannot
//! become a second statement or a different one. Literals the renderer
//! writes back raw (hex, bit, byte, raw, national and triple-quoted strings)
//! and custom types with modifiers are refused outright, and so are
//! parameters, which nothing binds. On SQLite the text must also hold no `;`
//! before its end, as far as a simple reading of SQLite's quoting finds. The
//! renderer writes standard SQL strings, which a PostgreSQL server
//! reads the same way only with `standard_conforming_strings = on`:
//! Navigator's connections pin that and check it, and send the statement as
//! one prepared statement, which cannot hold a second command. Changing a
//! setting is refused like any other statement that is not a query (`SET`,
//! `RESET`), and `set_config()` like the other denied functions, in an
//! expression or in FROM.

use std::fmt;
use std::ops::ControlFlow;

use sqlparser::ast::{
    ArrayElemTypeDef, DataType, DollarQuotedString, Expr, ObjectName, ObjectNamePart, Query,
    Select, Statement, TableFactor, Value, ValueWithSpan, Visit, Visitor,
};
use sqlparser::dialect::{PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use super::query::MAX_COLUMNS;

/// The SQL dialect a statement is parsed with: the app's database driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// SQLite.
    Sqlite,
    /// PostgreSQL.
    Postgres,
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Dialect::Sqlite => "SQLite",
            Dialect::Postgres => "PostgreSQL",
        })
    }
}

/// What kind of read-only statement passed the guard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatementKind {
    /// `SELECT`, `WITH ... SELECT`, `VALUES` or `TABLE`.
    Query,
    /// `EXPLAIN` of a query (`EXPLAIN QUERY PLAN` on SQLite).
    Explain,
    /// `SHOW` (PostgreSQL settings).
    Show,
}

/// A statement that passed the guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedStatement {
    /// The statement rendered from its parse tree: what is sent.
    pub sql: String,
    /// Its kind.
    pub kind: StatementKind,
}

/// Why a statement was refused before it was sent.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Refusal {
    /// Nothing but whitespace or comments.
    #[error("no SQL statement given")]
    Empty,
    /// The parser could not read it.
    #[error("the statement could not be parsed as {dialect} SQL: {message}")]
    Parse {
        /// The dialect used.
        dialect: Dialect,
        /// The parser's message.
        message: String,
    },
    /// More than one statement.
    #[error("send one statement at a time (got {count})")]
    Multiple {
        /// How many statements were found.
        count: usize,
    },
    /// A statement other than `SELECT`, `WITH`, `EXPLAIN` or `SHOW`.
    #[error(
        "only SELECT, WITH, EXPLAIN and SHOW statements are allowed; {statement} is not read-only"
    )]
    NotReadOnly {
        /// The statement's leading keywords (`DELETE`, `CREATE TABLE`).
        statement: String,
    },
    /// A data-changing statement inside a read-only one
    /// (`WITH d AS (DELETE ...) SELECT ...`, `EXPLAIN ANALYZE UPDATE ...`).
    #[error("the statement contains {statement}, which changes data")]
    Nested {
        /// The nested statement's leading keywords.
        statement: String,
    },
    /// `SELECT ... INTO`, which creates a table.
    #[error("SELECT ... INTO creates a table; drop the INTO clause")]
    SelectInto,
    /// `FOR UPDATE` / `FOR SHARE`, which take row locks.
    #[error("locking clauses (FOR UPDATE, FOR SHARE) are not allowed")]
    Locking,
    /// A function with side effects outside the transaction (cancelling
    /// backends, reading server files, advisory locks, remote links, ...).
    #[error("the function {name}() is not allowed: it acts outside the read-only transaction")]
    Function {
        /// The function's name, lower-cased.
        name: String,
    },
    /// A literal the guard does not send: hex (`X'..'`), bit or byte
    /// (`B'..'`), raw, national (`N'..'`) or triple-quoted strings.
    #[error("{kind} literals are not allowed; write the value as a plain '...' string")]
    Literal {
        /// What kind of literal.
        kind: &'static str,
    },
    /// A custom type with modifiers (`CAST(x AS name('..'))`).
    #[error("the type {name}(...) is not allowed: types with custom modifiers are not sent")]
    TypeModifier {
        /// The type's name.
        name: String,
    },
    /// The statement, rendered for sending, does not read back as the same
    /// single statement.
    #[error(
        "the statement does not read back as the same single statement once rendered, so it is not sent"
    )]
    Rendering,
    /// A statement longer than [`MAX_SQL_BYTES`].
    #[error(
        "the statement is {bytes} bytes long; send one under {} bytes",
        MAX_SQL_BYTES
    )]
    TooLong {
        /// Its length.
        bytes: usize,
    },
    /// Expressions nested deeper than [`MAX_EXPRESSION_DEPTH`].
    #[error(
        "the statement's expressions nest more than {} deep",
        MAX_EXPRESSION_DEPTH
    )]
    TooDeep,
    /// A query with more columns than [`MAX_COLUMNS`].
    #[error(
        "the query answers {count} columns; select at most {} of them",
        MAX_COLUMNS
    )]
    TooManyColumns {
        /// How many it answers.
        count: usize,
    },
    /// EXPLAIN on PostgreSQL, whose plan text holds any constant the query
    /// computes, at any size.
    #[error(
        "EXPLAIN is not run on PostgreSQL: the planner writes the constants it folds into the plan, \
         at any size, and the plan is one value Navigator would have to hold whole. Run EXPLAIN in \
         psql, or ask describe_schema about the tables and indexes"
    )]
    ExplainOnPostgres,
    /// The guard could not run: its thread could not start, or it failed.
    #[error("the statement could not be checked (the guard's thread failed), so it is not sent")]
    Unchecked,
    /// A parameter (`$1`, `?`, `:name`, `@name`, `$name`): nothing is bound.
    #[error(
        "parameters ({name}) are not allowed: query_database binds no values; write the value itself"
    )]
    Parameter {
        /// The parameter as written.
        name: String,
    },
    /// A dollar-quoted string whose tag starts with a digit (`$1$..$1$`),
    /// which PostgreSQL reads as a parameter.
    #[error("a dollar-quote tag may not start with a digit (`${tag}$`)")]
    DollarTag {
        /// The tag.
        tag: String,
    },
    /// SQLite reads more than one statement in the text.
    #[error("SQLite reads more than one statement in it; send one statement")]
    Trailing,
    /// SQLite says the statement writes.
    #[error("SQLite says the statement writes; only statements that read are allowed")]
    Writes,
    /// SQLite's authorizer refused something the statement does.
    #[error(
        "not authorized: {reason}. On SQLite only reading tables and calling functions is allowed \
         (no PRAGMA, ATTACH, writes or transactions, no pragma table functions, and no R*Tree tables, \
         which write when opened)"
    )]
    NotAuthorized {
        /// SQLite's message.
        reason: String,
    },
}

/// Functions refused anywhere in a statement (matched on the unqualified,
/// lower-cased name; an entry ending in `_` is a prefix).
///
/// This list is defense in depth and cannot be complete: an extension can
/// add a function that runs SQL text or reaches a file, a process or the
/// network under any name. The role and the database's own checks are the
/// first layer (see the module's documentation), which is why a privileged
/// role is refused.
const DENIED_FUNCTIONS: &[&str] = &[
    // PostgreSQL: functions that run SQL text the guard never parses.
    "ts_stat",
    "ts_rewrite",
    "query_to_",
    "cursor_to_",
    "table_to_xml",
    "table_to_xmlschema",
    "table_to_xml_and_xmlschema",
    "schema_to_",
    "database_to_",
    "pg_background_",
    // PostgreSQL: other sessions and the server.
    "pg_sleep",
    "pg_sleep_for",
    "pg_sleep_until",
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_stat_file",
    "pg_ls_",
    "pg_file_",
    "pg_advisory_",
    "pg_try_advisory_",
    "pg_notify",
    "pg_switch_wal",
    "pg_create_",
    "pg_drop_",
    "pg_logical_",
    "pg_replication_",
    "pg_promote",
    "pg_log_backend_memory_contexts",
    "pg_signal_backend",
    "pg_stat_reset",
    "pg_stat_reset_",
    "pg_wal_replay_",
    "pg_backup_",
    "pg_import_system_collations",
    "pg_sync_replication_slots",
    "set_config",
    // PostgreSQL: files, large objects, and other servers or the network
    // (dblink, the http extension).
    "lo_",
    "pg_read_",
    "dblink",
    "dblink_",
    "http",
    "http_",
    // SQLite: extensions and files.
    "load_extension",
    "fts3_tokenizer",
    "readfile",
    "writefile",
    "edit",
];

/// Parses `sql` with `dialect` and checks that it is exactly one read-only
/// statement: `SELECT` (with `WITH`, `VALUES` or `TABLE`), `EXPLAIN` of
/// such a query, or `SHOW`. Nested data-changing statements, `SELECT ...
/// INTO`, locking clauses and side-effecting functions are refused too.
///
/// A statement longer than [`MAX_SQL_BYTES`] is refused unread, and one
/// whose expressions nest deeper than [`MAX_EXPRESSION_DEPTH`] once parsed
/// (`1+1+...+1` thousands of times) is refused before it is rendered. The
/// parse tree is walked recursively (checking, rendering, comparing,
/// dropping), so all of it happens on a thread with a stack sized for the
/// longest statement allowed: a deep tree is refused, not a crash.
pub fn check(sql: &str, dialect: Dialect) -> Result<CheckedStatement, Refusal> {
    if sql.len() > MAX_SQL_BYTES {
        return Err(Refusal::TooLong { bytes: sql.len() });
    }
    let sql = sql.to_owned();
    std::thread::Builder::new()
        .name("navigator-sql-guard".to_owned())
        .stack_size(GUARD_STACK_BYTES)
        .spawn(move || check_here(&sql, dialect))
        .map_err(|_| Refusal::Unchecked)?
        .join()
        .unwrap_or(Err(Refusal::Unchecked))
}

/// The longest statement `query_database` reads: far more than a hand- or
/// model-written query needs.
pub const MAX_SQL_BYTES: usize = 64 * 1024;

/// How deep expressions may nest (operands of operands, arguments of
/// arguments).
pub const MAX_EXPRESSION_DEPTH: usize = 1_000;

/// The guard thread's stack: room for a tree of [`MAX_SQL_BYTES`] of text,
/// which nests at most about half as deep as it is long.
const GUARD_STACK_BYTES: usize = 64 * 1024 * 1024;

fn check_here(sql: &str, dialect: Dialect) -> Result<CheckedStatement, Refusal> {
    let parsed = parse(sql, dialect)?;
    let statement = match parsed.as_slice() {
        [] => return Err(Refusal::Empty),
        [statement] => statement,
        many => return Err(Refusal::Multiple { count: many.len() }),
    };
    let kind = match statement {
        Statement::Query(_) => StatementKind::Query,
        Statement::Explain { statement, .. } => match statement.as_ref() {
            Statement::Query(_) => StatementKind::Explain,
            other => {
                return Err(Refusal::Nested {
                    statement: leading_keywords(other),
                });
            }
        },
        Statement::ExplainTable { .. } => StatementKind::Explain,
        other if is_show(other) => StatementKind::Show,
        other => {
            return Err(Refusal::NotReadOnly {
                statement: leading_keywords(other),
            });
        }
    };
    if let ControlFlow::Break(refusal) = statement.visit(&mut Guard::default()) {
        return Err(refusal);
    }
    Ok(CheckedStatement {
        sql: rendered(statement, dialect)?,
        kind,
    })
}

/// `statement` as the text to send: exactly the text checked here. It must
/// read back as one statement with the same tree, and on SQLite end at its
/// first `;`.
fn rendered(statement: &Statement, dialect: Dialect) -> Result<String, Refusal> {
    let sql = statement.to_string();
    match parse(&sql, dialect).as_deref() {
        Ok([again]) if again == statement => {}
        _ => return Err(Refusal::Rendering),
    }
    if dialect == Dialect::Sqlite && !sqlite_single_statement(&sql) {
        return Err(Refusal::Rendering);
    }
    Ok(sql)
}

fn parse(sql: &str, dialect: Dialect) -> Result<Vec<Statement>, Refusal> {
    match dialect {
        Dialect::Sqlite => Parser::parse_sql(&SQLiteDialect {}, sql),
        Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
    }
    .map_err(|error| Refusal::Parse {
        dialect,
        message: error.to_string(),
    })
}

/// Whether `sql` holds no statement after its first, read as SQLite's own
/// tokenizer reads it: `'..'` and `"..."` with their quote doubled, `` `..` ``
/// and `[..]` identifiers, `--` and `/* */` comments. Nothing but whitespace
/// and `;` may follow the first `;` outside them.
pub(crate) fn sqlite_single_statement(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut ended = false;
    while i < bytes.len() {
        let byte = bytes[i];
        if ended {
            if !(byte.is_ascii_whitespace() || byte == b';') {
                return false;
            }
            i += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' | b'`' => {
                // Up to the closing quote; a doubled quote is part of the text.
                i += 1;
                loop {
                    match bytes.get(i) {
                        None => return false,
                        Some(&c) if c == byte => {
                            if bytes.get(i + 1) == Some(&byte) {
                                i += 2;
                            } else {
                                i += 1;
                                break;
                            }
                        }
                        Some(_) => i += 1,
                    }
                }
            }
            b'[' => match bytes[i..].iter().position(|&c| c == b']') {
                Some(end) => i += end + 1,
                None => return false,
            },
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i = bytes[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(bytes.len(), |end| i + end + 1);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                match bytes[i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(end) => i += 2 + end + 2,
                    None => i = bytes.len(),
                }
            }
            b';' => {
                ended = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    true
}

/// A literal the renderer writes back without escaping it.
fn raw_literal(value: &Value) -> Option<&'static str> {
    Some(match value {
        Value::HexStringLiteral(_) => "hex string (X'..')",
        Value::SingleQuotedByteStringLiteral(_)
        | Value::DoubleQuotedByteStringLiteral(_)
        | Value::TripleSingleQuotedByteStringLiteral(_)
        | Value::TripleDoubleQuotedByteStringLiteral(_) => "bit or byte string (B'..')",
        Value::SingleQuotedRawStringLiteral(_)
        | Value::DoubleQuotedRawStringLiteral(_)
        | Value::TripleSingleQuotedRawStringLiteral(_)
        | Value::TripleDoubleQuotedRawStringLiteral(_) => "raw string (R'..')",
        Value::NationalStringLiteral(_) | Value::NationalQuoteDelimitedStringLiteral(_) => {
            "national string (N'..')"
        }
        Value::QuoteDelimitedStringLiteral(_) => "quote-delimited string",
        Value::TripleSingleQuotedString(_) | Value::TripleDoubleQuotedString(_) => {
            "triple-quoted string"
        }
        _ => return None,
    })
}

/// The name of a custom type with modifiers in `data_type`, also as an
/// array's element type.
fn custom_with_modifiers(data_type: &DataType) -> Option<String> {
    match data_type {
        DataType::Custom(name, modifiers) if !modifiers.is_empty() => Some(name.to_string()),
        DataType::Array(
            ArrayElemTypeDef::AngleBracket(inner)
            | ArrayElemTypeDef::SquareBracket(inner, _)
            | ArrayElemTypeDef::Parenthesis(inner)
            | ArrayElemTypeDef::Qualified(inner, _),
        ) => custom_with_modifiers(inner),
        _ => None,
    }
}

/// Walks every statement, query, select and expression of the tree,
/// counting how deep expressions nest.
#[derive(Default)]
struct Guard {
    depth: usize,
}

impl Visitor for Guard {
    type Break = Refusal;

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Refusal> {
        match statement {
            Statement::Query(_) | Statement::Explain { .. } | Statement::ExplainTable { .. } => {
                ControlFlow::Continue(())
            }
            other if is_show(other) => ControlFlow::Continue(()),
            other => ControlFlow::Break(Refusal::Nested {
                statement: leading_keywords(other),
            }),
        }
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Refusal> {
        if query.locks.is_empty() {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(Refusal::Locking)
        }
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Refusal> {
        if select.into.is_some() {
            ControlFlow::Break(Refusal::SelectInto)
        } else {
            ControlFlow::Continue(())
        }
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Refusal> {
        self.depth += 1;
        if self.depth > MAX_EXPRESSION_DEPTH {
            return ControlFlow::Break(Refusal::TooDeep);
        }
        let data_type = match expr {
            Expr::Function(function) => return deny_function(&function.name),
            Expr::Cast { data_type, .. } => data_type,
            Expr::TypedString(typed) => &typed.data_type,
            _ => return ControlFlow::Continue(()),
        };
        match custom_with_modifiers(data_type) {
            Some(name) => ControlFlow::Break(Refusal::TypeModifier { name }),
            None => ControlFlow::Continue(()),
        }
    }

    fn post_visit_expr(&mut self, _expr: &Expr) -> ControlFlow<Refusal> {
        self.depth -= 1;
        ControlFlow::Continue(())
    }

    fn pre_visit_value(&mut self, value: &ValueWithSpan) -> ControlFlow<Refusal> {
        match &value.value {
            Value::Placeholder(name) => {
                ControlFlow::Break(Refusal::Parameter { name: name.clone() })
            }
            Value::DollarQuotedString(DollarQuotedString { tag: Some(tag), .. })
                if tag.starts_with(|c: char| c.is_ascii_digit()) =>
            {
                ControlFlow::Break(Refusal::DollarTag { tag: tag.clone() })
            }
            other => match raw_literal(other) {
                Some(kind) => ControlFlow::Break(Refusal::Literal { kind }),
                None => ControlFlow::Continue(()),
            },
        }
    }

    /// A function called in FROM (`SELECT * FROM set_config(..)`,
    /// `LATERAL pg_read_file(..)`) is a table factor, not an expression.
    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<Refusal> {
        match factor {
            TableFactor::Table {
                name,
                args: Some(_),
                ..
            }
            | TableFactor::Function { name, .. } => deny_function(name),
            _ => ControlFlow::Continue(()),
        }
    }
}

/// Breaks when the function `name` (qualified or not) is denied. The name
/// compared is the last part's identifier as written, without its quotes of
/// any style (`"f"`, `` `f` ``, `[f]`), lower-cased.
fn deny_function(name: &ObjectName) -> ControlFlow<Refusal> {
    let name = match name.0.last() {
        Some(ObjectNamePart::Identifier(ident)) => ident.value.to_lowercase(),
        Some(ObjectNamePart::Function(function)) => function.name.value.to_lowercase(),
        None => String::new(),
    };
    if is_denied(&name) {
        ControlFlow::Break(Refusal::Function { name })
    } else {
        ControlFlow::Continue(())
    }
}

/// Whether the function `name` (unqualified, lower-cased) is in
/// [`DENIED_FUNCTIONS`].
pub(crate) fn is_denied(name: &str) -> bool {
    DENIED_FUNCTIONS.iter().any(|denied| {
        if denied.ends_with('_') {
            name.starts_with(denied)
        } else {
            name == *denied
        }
    })
}

fn is_show(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::ShowFunctions { .. }
            | Statement::ShowVariable { .. }
            | Statement::ShowStatus { .. }
            | Statement::ShowVariables { .. }
            | Statement::ShowCreate { .. }
            | Statement::ShowColumns { .. }
            | Statement::ShowCatalogs { .. }
            | Statement::ShowDatabases { .. }
            | Statement::ShowSchemas { .. }
            | Statement::ShowCharset(_)
            | Statement::ShowObjects(_)
            | Statement::ShowTables { .. }
            | Statement::ShowViews { .. }
            | Statement::ShowCollation { .. }
    )
}

/// The first one or two keywords of a statement, for messages: `DELETE`,
/// `CREATE TABLE`, `PRAGMA`.
fn leading_keywords(statement: &Statement) -> String {
    let rendered = statement.to_string();
    let words: Vec<&str> = rendered
        .split_whitespace()
        .take_while(|word| word.chars().all(|c| c.is_ascii_alphabetic() || c == '_'))
        .take(2)
        .collect();
    let first = words.first().copied().unwrap_or("this statement");
    let two_word = ["CREATE", "DROP", "ALTER", "INSERT", "DELETE", "START"];
    if words.len() == 2 && two_word.contains(&first.to_ascii_uppercase().as_str()) {
        format!("{} {}", first, words[1]).to_ascii_uppercase()
    } else {
        first.to_ascii_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg(sql: &str) -> Result<CheckedStatement, Refusal> {
        check(sql, Dialect::Postgres)
    }

    fn lite(sql: &str) -> Result<CheckedStatement, Refusal> {
        check(sql, Dialect::Sqlite)
    }

    #[test]
    fn selects_pass_and_are_rendered_from_the_parse_tree() {
        let checked = pg("select stage, count(*) as deals from deals group by stage;").unwrap();
        assert_eq!(checked.kind, StatementKind::Query);
        assert_eq!(
            checked.sql,
            "SELECT stage, count(*) AS deals FROM deals GROUP BY stage"
        );
        assert_eq!(
            lite("SELECT 1 -- trailing comment").unwrap().sql,
            "SELECT 1"
        );
        assert_eq!(
            pg("WITH open AS (SELECT * FROM deals WHERE stage <> 'won') SELECT count(*) FROM open")
                .unwrap()
                .kind,
            StatementKind::Query
        );
        assert_eq!(lite("VALUES (1, 2)").unwrap().kind, StatementKind::Query);
        assert_eq!(
            pg("SELECT id FROM a UNION SELECT id FROM b").unwrap().kind,
            StatementKind::Query
        );
    }

    #[test]
    fn explain_of_a_query_passes() {
        assert_eq!(
            pg("EXPLAIN SELECT * FROM deals").unwrap().kind,
            StatementKind::Explain
        );
        assert_eq!(
            pg("EXPLAIN ANALYZE SELECT * FROM deals").unwrap().kind,
            StatementKind::Explain
        );
        assert_eq!(
            lite("EXPLAIN QUERY PLAN SELECT * FROM deals WHERE id = 1")
                .unwrap()
                .kind,
            StatementKind::Explain
        );
    }

    #[test]
    fn show_passes_on_postgres() {
        assert_eq!(pg("SHOW search_path").unwrap().kind, StatementKind::Show);
        assert_eq!(pg("SHOW ALL").unwrap().kind, StatementKind::Show);
    }

    #[test]
    fn writes_are_refused() {
        for (sql, statement) in [
            ("DELETE FROM deals", "DELETE FROM"),
            ("UPDATE deals SET stage = 'won'", "UPDATE"),
            ("INSERT INTO deals (title) VALUES ('x')", "INSERT INTO"),
            ("DROP TABLE deals", "DROP TABLE"),
            ("CREATE TABLE t (id int)", "CREATE TABLE"),
            ("ALTER TABLE deals ADD COLUMN x int", "ALTER TABLE"),
            ("TRUNCATE deals", "TRUNCATE"),
        ] {
            assert_eq!(
                pg(sql),
                Err(Refusal::NotReadOnly {
                    statement: statement.to_owned()
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn transaction_and_session_control_is_refused() {
        for sql in [
            "COMMIT",
            "BEGIN",
            "ROLLBACK",
            "SET default_transaction_read_only = off",
            "SET TRANSACTION READ WRITE",
            "RESET ALL",
            "LISTEN jobs",
            "VACUUM",
        ] {
            assert!(
                matches!(
                    pg(sql),
                    Err(Refusal::NotReadOnly { .. }) | Err(Refusal::Parse { .. })
                ),
                "{sql}: {:?}",
                pg(sql)
            );
        }
    }

    #[test]
    fn sqlite_pragmas_and_attach_are_refused() {
        assert_eq!(
            lite("PRAGMA query_only = 0"),
            Err(Refusal::NotReadOnly {
                statement: "PRAGMA".to_owned()
            })
        );
        assert!(matches!(
            lite("ATTACH DATABASE 'other.db' AS other"),
            Err(Refusal::NotReadOnly { .. })
        ));
    }

    #[test]
    fn multiple_statements_are_refused() {
        assert_eq!(
            pg("SELECT 1; DELETE FROM deals"),
            Err(Refusal::Multiple { count: 2 })
        );
        assert_eq!(
            lite("SELECT 1; SELECT 2"),
            Err(Refusal::Multiple { count: 2 })
        );
    }

    #[test]
    fn data_changing_ctes_are_refused() {
        let refused = pg("WITH gone AS (DELETE FROM deals RETURNING id) SELECT * FROM gone");
        assert!(
            matches!(&refused, Err(Refusal::Nested { statement }) if statement.starts_with("DELETE")),
            "{refused:?}"
        );
        let refused =
            pg("WITH moved AS (UPDATE deals SET stage = 'won' RETURNING id) SELECT * FROM moved");
        assert!(
            matches!(refused, Err(Refusal::Nested { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn explain_of_a_write_is_refused() {
        let refused = pg("EXPLAIN ANALYZE DELETE FROM deals");
        assert!(
            matches!(&refused, Err(Refusal::Nested { statement }) if statement.starts_with("DELETE")),
            "{refused:?}"
        );
    }

    #[test]
    fn select_into_is_refused() {
        assert_eq!(
            pg("SELECT * INTO copy FROM deals"),
            Err(Refusal::SelectInto)
        );
    }

    #[test]
    fn locking_clauses_are_refused() {
        assert_eq!(pg("SELECT * FROM deals FOR UPDATE"), Err(Refusal::Locking));
        assert_eq!(
            pg("SELECT * FROM (SELECT * FROM deals FOR SHARE) d"),
            Err(Refusal::Locking)
        );
    }

    #[test]
    fn side_effecting_functions_are_refused() {
        for (sql, name) in [
            ("SELECT pg_sleep(10)", "pg_sleep"),
            ("SELECT pg_terminate_backend(42)", "pg_terminate_backend"),
            (
                "SELECT pg_catalog.pg_read_file('/etc/passwd')",
                "pg_read_file",
            ),
            ("SELECT pg_advisory_lock(1)", "pg_advisory_lock"),
            ("SELECT set_config('x', 'y', false)", "set_config"),
            ("SELECT lo_import('/etc/passwd')", "lo_import"),
            (
                "SELECT * FROM deals WHERE id IN (SELECT pg_cancel_backend(1))",
                "pg_cancel_backend",
            ),
        ] {
            assert_eq!(
                pg(sql),
                Err(Refusal::Function {
                    name: name.to_owned()
                }),
                "{sql}"
            );
        }
        assert_eq!(
            lite("SELECT load_extension('evil')"),
            Err(Refusal::Function {
                name: "load_extension".to_owned()
            })
        );
        // Ordinary functions pass.
        assert!(pg("SELECT lower(title), now(), count(*) FROM deals GROUP BY 1, 2").is_ok());
    }

    #[test]
    fn side_effecting_functions_in_from_are_refused() {
        for (sql, name) in [
            (
                "SELECT * FROM set_config('standard_conforming_strings', 'off', false)",
                "set_config",
            ),
            (
                "SELECT * FROM pg_catalog.set_config('standard_conforming_strings', 'off', false) AS s",
                "set_config",
            ),
            ("SELECT * FROM pg_read_file('/etc/passwd')", "pg_read_file"),
            (
                "SELECT * FROM pg_terminate_backend(42)",
                "pg_terminate_backend",
            ),
            (
                "SELECT * FROM dblink('dbname=app', 'DROP TABLE deals')",
                "dblink",
            ),
            (
                "SELECT d.id FROM deals d, LATERAL pg_sleep(10) AS s",
                "pg_sleep",
            ),
            (
                "SELECT * FROM deals WHERE id IN (SELECT 1 FROM pg_advisory_lock(1))",
                "pg_advisory_lock",
            ),
        ] {
            assert_eq!(
                pg(sql),
                Err(Refusal::Function {
                    name: name.to_owned()
                }),
                "{sql}"
            );
        }
        // Set-returning functions that only read pass, and so does a table
        // whose name starts like a denied prefix.
        assert!(pg("SELECT * FROM generate_series(1, 3) AS g").is_ok());
        assert!(pg("SELECT * FROM lo_items").is_ok());
    }

    #[test]
    fn the_string_setting_cannot_be_changed() {
        for sql in [
            "SET standard_conforming_strings = off",
            "SET SESSION standard_conforming_strings = off",
            "SET LOCAL standard_conforming_strings TO off",
            "RESET standard_conforming_strings",
            "SELECT set_config('standard_conforming_strings', 'off', false)",
            "SELECT * FROM set_config('standard_conforming_strings', 'off', true)",
        ] {
            assert!(
                matches!(
                    pg(sql),
                    Err(Refusal::NotReadOnly { .. }
                        | Refusal::Parse { .. }
                        | Refusal::Function { .. })
                ),
                "{sql}: {:?}",
                pg(sql)
            );
        }
    }

    #[test]
    fn empty_and_unparsable_input_is_refused() {
        assert_eq!(pg("  -- nothing\n"), Err(Refusal::Empty));
        assert!(matches!(
            pg("SELEKT * FROM deals"),
            Err(Refusal::Parse {
                dialect: Dialect::Postgres,
                ..
            })
        ));
    }

    #[test]
    fn quoting_cannot_smuggle_a_second_statement() {
        // Whatever the parser made of the literal, what is sent is one
        // statement rendered from the tree, which parses back to one query.
        for sql in [
            r"SELECT 'a\'; DELETE FROM deals; --'",
            r"SELECT E'a\'; DELETE FROM deals; --'",
            "SELECT $$a; DELETE FROM deals$$",
        ] {
            match pg(sql) {
                Ok(checked) => assert_eq!(
                    check(&checked.sql, Dialect::Postgres).map(|c| c.kind),
                    Ok(StatementKind::Query),
                    "{sql} -> {}",
                    checked.sql
                ),
                Err(refusal) => assert!(
                    matches!(
                        refusal,
                        Refusal::Multiple { .. } | Refusal::Parse { .. } | Refusal::Rendering
                    ),
                    "{sql}: {refusal:?}"
                ),
            }
        }
        let checked = pg("SELECT 'it''s; DELETE FROM deals'").unwrap();
        assert_eq!(checked.sql, "SELECT 'it''s; DELETE FROM deals'");
    }

    /// Statements whose literals or types the renderer writes back
    /// differently from how they were read.
    const RENDER_SMUGGLING: &[&str] = &[
        "SELECT X'00''; PRAGMA query_only = OFF; DROP TABLE users; --'",
        "SELECT CAST(1 AS x('1)); PRAGMA query_only=OFF; DROP TABLE users; SELECT CAST(1 AS x(1'))",
        "SELECT X'0'', pg_terminate_backend(pid), ''' FROM pg_stat_activity",
        "SELECT X'0'', dblink_exec(''dbname=app'', ''DROP TABLE users''), '''",
        "SELECT X'0''; COMMIT; BEGIN READ WRITE; DROP TABLE users; COMMIT; --'",
    ];

    #[test]
    fn literals_and_types_written_back_raw_are_refused() {
        for sql in RENDER_SMUGGLING {
            for dialect in [Dialect::Sqlite, Dialect::Postgres] {
                let refused = check(sql, dialect);
                assert!(
                    matches!(
                        refused,
                        Err(Refusal::Literal { .. }
                            | Refusal::TypeModifier { .. }
                            | Refusal::Rendering
                            | Refusal::Multiple { .. }
                            | Refusal::Parse { .. })
                    ),
                    "{dialect}: {sql}: {refused:?}"
                );
            }
        }
        assert!(matches!(lite("SELECT X'00'"), Err(Refusal::Literal { .. })));
        assert!(matches!(
            pg("SELECT CAST(1 AS x('1'))"),
            Err(Refusal::TypeModifier { .. })
        ));
        // Plain strings, numbers and ordinary types pass.
        assert!(pg("SELECT 'a''b', 1.5, CAST('1' AS integer), E'x\\'y'").is_ok());
        assert!(lite("SELECT 'a''b', CAST('1' AS INTEGER)").is_ok());
    }

    #[test]
    fn a_statement_that_does_not_render_back_is_refused() {
        // The tree as the parser read it, taken past the visitor: rendered, it
        // becomes several statements, so it is not sent.
        for (sql, dialect) in [
            (RENDER_SMUGGLING[0], Dialect::Sqlite),
            (RENDER_SMUGGLING[1], Dialect::Sqlite),
            (RENDER_SMUGGLING[4], Dialect::Postgres),
        ] {
            let parsed = parse(sql, dialect).unwrap();
            assert_eq!(parsed.len(), 1, "{sql}");
            assert_eq!(
                rendered(&parsed[0], dialect),
                Err(Refusal::Rendering),
                "{sql}"
            );
        }
        let parsed = parse("SELECT 'it''s'", Dialect::Sqlite).unwrap();
        assert_eq!(
            rendered(&parsed[0], Dialect::Sqlite).as_deref(),
            Ok("SELECT 'it''s'")
        );
    }

    #[test]
    fn sqlite_text_ends_at_its_first_semicolon() {
        for sql in [
            "SELECT 1",
            "SELECT 1;",
            "SELECT 1; \n ;",
            "SELECT 'a;b', \"c;d\", `e;f`, [g;h] -- ;x\n",
            "SELECT 1 /* ; */",
            "SELECT 'it''s; still one'",
        ] {
            assert!(sqlite_single_statement(sql), "{sql}");
        }
        for sql in [
            "SELECT 1; SELECT 2",
            "SELECT X'00'; PRAGMA query_only = OFF; --'",
            "SELECT 'a''; DROP TABLE t; --'; DELETE FROM t",
            "SELECT 1 /* x */; DELETE FROM t",
            "SELECT 'unterminated",
        ] {
            assert!(!sqlite_single_statement(sql), "{sql}");
        }
    }

    #[test]
    fn functions_that_run_sql_text_are_refused() {
        for (sql, name) in [
            (
                "SELECT * FROM ts_stat('SELECT to_tsvector(pg_terminate_backend(pid)::text) FROM pg_stat_activity')",
                "ts_stat",
            ),
            (
                "SELECT ts_rewrite(to_tsquery('a'), 'SELECT pg_sleep(10)')",
                "ts_rewrite",
            ),
            (
                "SELECT query_to_xml('SELECT 1', true, false, '')",
                "query_to_xml",
            ),
            (
                "SELECT query_to_xmlschema('SELECT 1', true, false, '')",
                "query_to_xmlschema",
            ),
            ("SELECT database_to_xml(true, false, '')", "database_to_xml"),
            (
                "SELECT * FROM pg_background_launch('DROP TABLE users')",
                "pg_background_launch",
            ),
            ("SELECT http_get('http://example.com')", "http_get"),
            (
                "SELECT dblink_exec('dbname=app', 'DROP TABLE users')",
                "dblink_exec",
            ),
            ("SELECT lo_export(1, '/tmp/x')", "lo_export"),
        ] {
            assert_eq!(
                pg(sql),
                Err(Refusal::Function {
                    name: name.to_owned()
                }),
                "{sql}"
            );
        }
    }

    #[test]
    fn sqlite_variables_and_parameters_are_refused() {
        // SQLite reads `$a::ENUM('x)')` as one variable token, and the rest as
        // more statements; whatever the guard makes of it, it is refused.
        for variable in ["$a", ":a", "@a", "#a"] {
            let sql = format!(
                "SELECT {variable}::ENUM('x)'), '; PRAGMA query_only = OFF; VACUUM INTO char(47,116,109,112,47,120); --'"
            );
            assert!(lite(&sql).is_err(), "{sql}: {:?}", lite(&sql));
        }
        assert!(matches!(lite("SELECT ?"), Err(Refusal::Parameter { .. })));
        assert!(matches!(
            lite("SELECT :name"),
            Err(Refusal::Parameter { .. })
        ));
        assert!(matches!(pg("SELECT $1"), Err(Refusal::Parameter { .. })));
    }

    #[test]
    fn a_dollar_tag_that_starts_with_a_digit_is_refused() {
        assert!(matches!(
            pg("SELECT $1$a; DELETE FROM deals$1$"),
            Err(Refusal::DollarTag { .. } | Refusal::Parameter { .. } | Refusal::Parse { .. })
        ));
        assert!(pg("SELECT $tag$a; b$tag$").is_ok());
    }

    #[test]
    fn denied_functions_are_matched_whatever_their_quotes() {
        for (sql, name) in [
            ("SELECT [load_extension]('/x')", "load_extension"),
            ("SELECT `fts3_tokenizer`('simple')", "fts3_tokenizer"),
            ("SELECT \"readfile\"('/etc/passwd')", "readfile"),
        ] {
            assert_eq!(
                lite(sql),
                Err(Refusal::Function {
                    name: name.to_owned()
                }),
                "{sql}"
            );
        }
        assert_eq!(
            pg("SELECT \"PG_SLEEP\"(1)"),
            Err(Refusal::Function {
                name: "pg_sleep".to_owned()
            })
        );
    }

    #[test]
    fn long_or_deep_statements_are_refused_not_a_crash() {
        let deep = format!("SELECT 1{}", "+1".repeat(20_000));
        assert!(deep.len() < MAX_SQL_BYTES);
        for dialect in [Dialect::Sqlite, Dialect::Postgres] {
            assert_eq!(check(&deep, dialect), Err(Refusal::TooDeep), "{dialect}");
        }
        let long = format!("SELECT '{}'", "x".repeat(MAX_SQL_BYTES));
        assert!(matches!(
            check(&long, Dialect::Postgres),
            Err(Refusal::TooLong { .. })
        ));
        let fine = format!("SELECT 1{}", "+1".repeat(500));
        assert!(pg(&fine).is_ok());
    }
}
