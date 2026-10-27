//! The statement guard of `query_database`: a statement is parsed before it
//! is sent, and only one read-only statement passes.
//!
//! What is sent is the parsed statement rendered back to SQL, never the text
//! the model wrote, so a quirk of quoting cannot smuggle a second statement
//! past the parser. The database then runs it in a read-only transaction
//! (PostgreSQL) or on a `query_only` connection (SQLite), with a statement
//! timeout and a row limit: the guard is the first of those layers, not the
//! only one.

use std::fmt;
use std::ops::ControlFlow;

use sqlparser::ast::{Expr, Query, Select, Statement, Visit, Visitor};
use sqlparser::dialect::{PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

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
}

/// Functions refused anywhere in a statement (matched on the unqualified,
/// lower-cased name; an entry ending in `_` is a prefix).
const DENIED_FUNCTIONS: &[&str] = &[
    // PostgreSQL: server control, files, locks, notifications, links.
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
    "set_config",
    "lo_",
    "dblink",
    "dblink_",
    "query_to_xml",
    "query_to_xml_and_xmlschema",
    "cursor_to_xml",
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
pub fn check(sql: &str, dialect: Dialect) -> Result<CheckedStatement, Refusal> {
    let parsed = match dialect {
        Dialect::Sqlite => Parser::parse_sql(&SQLiteDialect {}, sql),
        Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
    }
    .map_err(|error| Refusal::Parse {
        dialect,
        message: error.to_string(),
    })?;
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
    if let ControlFlow::Break(refusal) = statement.visit(&mut Guard) {
        return Err(refusal);
    }
    Ok(CheckedStatement {
        sql: statement.to_string(),
        kind,
    })
}

/// Walks every statement, query, select and expression of the tree.
struct Guard;

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
        if let Expr::Function(function) = expr {
            let qualified = function.name.to_string().to_lowercase();
            let name = qualified
                .rsplit('.')
                .next()
                .unwrap_or(&qualified)
                .trim_matches('"')
                .to_owned();
            if is_denied(&name) {
                return ControlFlow::Break(Refusal::Function { name });
            }
        }
        ControlFlow::Continue(())
    }
}

fn is_denied(name: &str) -> bool {
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
                    matches!(refusal, Refusal::Multiple { .. } | Refusal::Parse { .. }),
                    "{sql}: {refusal:?}"
                ),
            }
        }
        let checked = pg("SELECT 'it''s; DELETE FROM deals'").unwrap();
        assert_eq!(checked.sql, "SELECT 'it''s; DELETE FROM deals'");
    }
}
