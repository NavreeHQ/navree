//! Differential tests of navree-orm's own SQL renderer, on real databases.
//!
//! - **Versus the compiler**: every case of `corpus.rs` (IR statements) is
//!   compiled by `sql::compile_*` and by `render::render_*`, both run on the
//!   same fixture, and their results compared: the rows (in order when the
//!   statement orders them), the rows changed and returned, and every
//!   fixture table afterwards. A case a database refuses must be refused
//!   (or fail) on both paths.
//! - **Across databases**: the statements of `across.rs`, built as syntax
//!   trees, return the rows written there on every database (or are refused
//!   where the dialect lacks the feature).
//!
//! SQLite always runs; PostgreSQL (with TimescaleDB) and MySQL run with the
//! crate's `postgres` / `mysql` features when `NAVREE_TEST_POSTGRES_URL` /
//! `NAVREE_TEST_MYSQL_URL` is set (or in `target/test-databases.env`):
//! `cargo test -p navree-orm --features postgres,mysql --test differential
//! -- --nocapture` prints the tallies.

mod across;
mod corpus;
mod exec;
mod support;

use std::fmt::Write as _;

use navree_orm::{Dialect, OrmError, Statement, render, sql};
use serde_json::Value as Json;

use crate::corpus::{Case, Kind};
use crate::exec::{Exec, Row};

#[tokio::test]
async fn sqlite_renders_what_the_compiler_runs() {
    compare_paths(Dialect::Sqlite).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_renders_what_the_compiler_runs() {
    compare_paths(Dialect::Postgres).await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn mysql_renders_what_the_compiler_runs() {
    compare_paths(Dialect::MySql).await;
}

#[tokio::test]
async fn sqlite_runs_the_portable_statements() {
    portable(Dialect::Sqlite).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_runs_the_portable_statements() {
    portable(Dialect::Postgres).await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn mysql_runs_the_portable_statements() {
    portable(Dialect::MySql).await;
}

/// What a path did with a case.
#[derive(Debug)]
enum Outcome {
    /// It would not compile the case.
    Refused(String),
    /// The database refused the statement.
    Failed(String),
    /// The rows of a read.
    Rows(Vec<Row>),
    /// A write: the rows changed and returned, and the tables afterwards.
    Written {
        affected: u64,
        returned: Vec<Row>,
        tables: Vec<(String, Vec<Row>)>,
    },
}

/// The statements of `kind` as the compiler builds them.
fn compiled(kind: &Kind, dialect: Dialect) -> Result<Vec<Statement>, OrmError> {
    Ok(match kind {
        Kind::Select(query) | Kind::Locked(query) => vec![sql::compile_select(query, dialect)?],
        Kind::Aggregate(query, aggregate) => {
            vec![sql::compile_aggregate(query, aggregate, dialect)?]
        }
        Kind::Exists(query) => vec![sql::compile_exists(query, dialect)?],
        Kind::Insert(insert) => vec![sql::compile_insert(insert, dialect)?],
        Kind::Update(update) => vec![sql::compile_update(update, dialect)?],
        Kind::UpdateFrom {
            table,
            sets,
            joins,
            wheres,
        } => vec![sql::compile_update_from(table, sets, joins, wheres, dialect)?],
        Kind::Delete(delete) => vec![sql::compile_delete(delete, dialect)?],
        Kind::Truncate { table, cascade } => sql::compile_truncate(table, *cascade, dialect),
    })
}

/// The statements of `kind` as the renderer writes them.
fn rendered(kind: &Kind, dialect: Dialect) -> Result<Vec<Statement>, OrmError> {
    Ok(match kind {
        Kind::Select(query) | Kind::Locked(query) => vec![render::render_select(query, dialect)?],
        Kind::Aggregate(query, aggregate) => {
            vec![render::render_aggregate(query, aggregate, dialect)?]
        }
        Kind::Exists(query) => vec![render::render_exists(query, dialect)?],
        Kind::Insert(insert) => vec![render::render_insert(insert, dialect)?],
        Kind::Update(update) => vec![render::render_update(update, dialect)?],
        Kind::UpdateFrom {
            table,
            sets,
            joins,
            wheres,
        } => vec![render::render_update_from(
            table, sets, joins, wheres, dialect,
        )?],
        Kind::Delete(delete) => vec![render::render_delete(delete, dialect)?],
        Kind::Truncate { table, cascade } => render::render_truncate(table, *cascade, dialect),
    })
}

/// Runs a case's statements (resetting the fixture first for a write).
async fn run(exec: &Exec, case: &Case, statements: Result<Vec<Statement>, OrmError>) -> Outcome {
    let statements = match statements {
        Ok(statements) => statements,
        Err(error) => return Outcome::Refused(error.to_string()),
    };
    let dialect = exec.dialect();
    let result = match &case.kind {
        Kind::Select(_) | Kind::Aggregate(..) | Kind::Exists(_) => {
            exec.rows(&statements[0]).await.map(Outcome::Rows)
        }
        Kind::Locked(_) => exec
            .rows_in_transaction(&statements[0])
            .await
            .map(Outcome::Rows),
        kind => {
            support::reset(exec).await;
            let returning = matches!(
                kind,
                Kind::Insert(insert) if !insert.returning.is_empty() && dialect.supports_returning()
            );
            let mut affected = 0;
            let mut returned = Vec::new();
            let mut failure = None;
            for statement in &statements {
                let step = if returning {
                    exec.rows(statement).await.map(|rows| {
                        let count = rows.len() as u64;
                        returned.extend(rows);
                        count
                    })
                } else {
                    exec.execute(statement).await
                };
                match step {
                    Ok(count) => affected += count,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            match failure {
                Some(error) => Err(error),
                None => {
                    let mut tables = Vec::new();
                    for (table, order) in support::TABLES {
                        let rows = exec
                            .dump(table, order)
                            .await
                            .unwrap_or_else(|error| panic!("reads {table}: {error}"));
                        tables.push((table.to_owned(), rows));
                    }
                    Ok(Outcome::Written {
                        affected,
                        returned,
                        tables,
                    })
                }
            }
        }
    };
    result.unwrap_or_else(Outcome::Failed)
}

/// A column name worth comparing: a plain identifier (an unnamed
/// expression is named after its text on MySQL, which differs between the
/// paths by parentheses).
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The values of a row, as JSON text (for sorting and messages).
fn values(row: &Row) -> String {
    serde_json::to_string(&row.iter().map(|(_, value)| value).collect::<Vec<_>>())
        .unwrap_or_default()
}

/// Whether two row sets agree: the same values in each column, the same
/// plain column names, in order when `ordered`.
fn same_rows(left: &[Row], right: &[Row], ordered: bool) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let names = |rows: &[Row]| -> Vec<String> {
        rows.first()
            .map(|row| row.iter().map(|(name, _)| name.to_ascii_lowercase()).collect())
            .unwrap_or_default()
    };
    let (left_names, right_names) = (names(left), names(right));
    if left_names.len() != right_names.len()
        || left_names
            .iter()
            .zip(&right_names)
            .any(|(a, b)| plain_name(a) && plain_name(b) && a != b)
    {
        return false;
    }
    let mut a: Vec<String> = left.iter().map(values).collect();
    let mut b: Vec<String> = right.iter().map(values).collect();
    if !ordered {
        a.sort();
        b.sort();
    }
    a == b
}

/// How the two paths compare on a case.
enum Verdict {
    SameRows,
    SameWrite,
    RefusedByBoth,
    FailedOnBoth,
    Mismatch(String),
}

fn compare(case: &Case, compiler: &Outcome, renderer: &Outcome) -> Verdict {
    match (compiler, renderer) {
        (Outcome::Refused(_), Outcome::Refused(_)) => Verdict::RefusedByBoth,
        (
            Outcome::Refused(_) | Outcome::Failed(_),
            Outcome::Refused(_) | Outcome::Failed(_),
        ) => Verdict::FailedOnBoth,
        (Outcome::Rows(a), Outcome::Rows(b)) => {
            if same_rows(a, b, case.ordered) {
                Verdict::SameRows
            } else {
                Verdict::Mismatch("different rows".to_owned())
            }
        }
        (
            Outcome::Written {
                affected: a_affected,
                returned: a_returned,
                tables: a_tables,
            },
            Outcome::Written {
                affected: b_affected,
                returned: b_returned,
                tables: b_tables,
            },
        ) => {
            if a_affected != b_affected {
                return Verdict::Mismatch(format!(
                    "{a_affected} rows changed against {b_affected}"
                ));
            }
            if !same_rows(a_returned, b_returned, false) {
                return Verdict::Mismatch("different rows returned".to_owned());
            }
            for ((table, a), (_, b)) in a_tables.iter().zip(b_tables) {
                if !same_rows(a, b, true) {
                    return Verdict::Mismatch(format!("`{table}` differs afterwards"));
                }
            }
            Verdict::SameWrite
        }
        _ => Verdict::Mismatch("one path ran, the other did not".to_owned()),
    }
}

fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Refused(error) => format!("refused: {error}"),
        Outcome::Failed(error) => format!("failed: {error}"),
        Outcome::Rows(rows) => format!(
            "{} row(s): {}",
            rows.len(),
            rows.iter().map(values).collect::<Vec<_>>().join(" ")
        ),
        Outcome::Written {
            affected,
            returned,
            tables,
        } => format!(
            "{affected} changed, returned {}; {}",
            returned.iter().map(values).collect::<Vec<_>>().join(" "),
            tables
                .iter()
                .map(|(table, rows)| format!(
                    "{table}: {}",
                    rows.iter().map(values).collect::<Vec<_>>().join(" ")
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    }
}

fn sql_of(statements: &Result<Vec<Statement>, OrmError>) -> String {
    match statements {
        Ok(statements) => statements
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ; "),
        Err(error) => format!("error: {error}"),
    }
}

async fn compare_paths(dialect: Dialect) {
    support::run(dialect, |exec| async move {
        let mut cases = corpus::reads(dialect);
        cases.extend(corpus::writes(dialect));
        let (mut rows, mut writes, mut refused, mut failed) = (0, 0, 0, 0);
        let mut mismatches = String::new();
        let mut failures_on_both = String::new();
        for case in &cases {
            let compiler_sql = compiled(&case.kind, dialect);
            let renderer_sql = rendered(&case.kind, dialect);
            let compiler = run(&exec, case, clone(&compiler_sql)).await;
            let renderer = run(&exec, case, clone(&renderer_sql)).await;
            match compare(case, &compiler, &renderer) {
                Verdict::SameRows => rows += 1,
                Verdict::SameWrite => writes += 1,
                Verdict::RefusedByBoth => refused += 1,
                Verdict::FailedOnBoth => {
                    failed += 1;
                    writeln!(
                        failures_on_both,
                        "  {}: {} | {}",
                        case.name,
                        describe(&compiler),
                        describe(&renderer)
                    )
                    .unwrap();
                }
                Verdict::Mismatch(why) => {
                    writeln!(
                        mismatches,
                        "\n[{}] {why}\n  compiler: {}\n    => {}\n  renderer: {}\n    => {}",
                        case.name,
                        sql_of(&compiler_sql),
                        describe(&compiler),
                        sql_of(&renderer_sql),
                        describe(&renderer),
                    )
                    .unwrap();
                }
            }
        }
        if writes > 0 {
            support::reset(&exec).await;
        }
        eprintln!(
            "differential ({dialect}): {} cases: {rows} same rows, {writes} same writes, {refused} refused by both, {failed} failed on both, {} mismatches",
            cases.len(),
            cases.len() - rows - writes - refused - failed,
        );
        if !failures_on_both.is_empty() {
            eprintln!("failed on both ({dialect}):\n{failures_on_both}");
        }
        assert!(
            mismatches.is_empty(),
            "the renderer and the compiler differ on {dialect}:{mismatches}"
        );
    })
    .await;
}

fn clone(
    statements: &Result<Vec<Statement>, OrmError>,
) -> Result<Vec<Statement>, OrmError> {
    match statements {
        Ok(statements) => Ok(statements.clone()),
        Err(error) => Err(OrmError::InvalidQuery(error.to_string())),
    }
}

async fn portable(dialect: Dialect) {
    support::run(dialect, |exec| async move {
        let mut problems = String::new();
        let (mut ran, mut refused) = (0, 0);
        let statements = across::statements();
        for portable in &statements {
            let mut rendered = Vec::new();
            let mut refusal = None;
            for step in &portable.steps {
                match render::render_all(step, dialect) {
                    Ok(statements) => rendered.extend(statements),
                    Err(error) => {
                        refusal = Some(error);
                        break;
                    }
                }
            }
            let expect_refusal = portable.refused_on.contains(&dialect);
            match (refusal, expect_refusal) {
                (Some(error), true) => {
                    refused += 1;
                    assert!(
                        matches!(error, OrmError::Unsupported { .. } | OrmError::InvalidQuery(_)),
                        "{}: {error}",
                        portable.name
                    );
                    continue;
                }
                (Some(error), false) => {
                    writeln!(problems, "\n[{}] refused: {error}", portable.name).unwrap();
                    continue;
                }
                (None, true) => {
                    writeln!(
                        problems,
                        "\n[{}] rendered where {dialect} should refuse: {}",
                        portable.name,
                        rendered
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(" ; ")
                    )
                    .unwrap();
                    continue;
                }
                (None, false) => {}
            }
            let writes = portable.check.is_some()
                || portable
                    .steps
                    .iter()
                    .any(|step| !matches!(step, render::ast::Statement::Query(_)));
            if writes {
                support::reset(&exec).await;
            }
            let rows = match &portable.check {
                None if portable.locked => exec.rows_in_transaction(&rendered[0]).await,
                None => exec.rows(&rendered[0]).await,
                Some(check) => {
                    let mut failure = None;
                    for statement in &rendered {
                        if let Err(error) = exec.execute(statement).await {
                            failure = Some(format!("{statement}: {error}"));
                            break;
                        }
                    }
                    match failure {
                        Some(error) => Err(error),
                        None => match render::render_query(check, dialect) {
                            Ok(check) => exec.rows(&check).await,
                            Err(error) => Err(error.to_string()),
                        },
                    }
                }
            };
            ran += 1;
            let sql = rendered
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ; ");
            match rows {
                Ok(rows) => {
                    let got: Vec<Json> = rows
                        .iter()
                        .map(|row| Json::Array(row.iter().map(|(_, v)| v.clone()).collect()))
                        .collect();
                    if Json::Array(got.clone()) != portable.expect {
                        writeln!(
                            problems,
                            "\n[{}] {sql}\n  expected {}\n  got      {}",
                            portable.name,
                            portable.expect,
                            Json::Array(got)
                        )
                        .unwrap();
                    }
                }
                Err(error) => {
                    writeln!(problems, "\n[{}] {sql}\n  failed: {error}", portable.name).unwrap();
                }
            }
        }
        if ran > 0 {
            support::reset(&exec).await;
        }
        eprintln!(
            "portable ({dialect}): {} statements: {ran} ran, {refused} refused as expected",
            statements.len()
        );
        assert!(
            problems.is_empty(),
            "portable statements on {dialect}:{problems}"
        );
    })
    .await;
}
