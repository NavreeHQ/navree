//! Schema dumps: the structure of a database as an SQL script (what
//! `navree schema:dump` writes), and loading such a script.
//!
//! The dump is read from the database's own catalogs, so no client tool
//! (`pg_dump`, `mysqldump`, `sqlite3`) is needed:
//!
//! - SQLite: the `CREATE` statements kept in `sqlite_master`;
//! - MySQL and MariaDB: `SHOW CREATE` for tables, views, routines and
//!   triggers, without definers and `AUTO_INCREMENT` counters;
//! - PostgreSQL: schemas, extensions, types, functions, sequences, tables,
//!   constraints, indexes, views, triggers, comments and row-level security
//!   from the catalogs; with TimescaleDB, its hypertables, dimensions,
//!   compression settings, policies and continuous aggregates, re-created
//!   through TimescaleDB's own functions.
//!
//! Rows of the tables named in [`DumpOptions::data`] follow as `INSERT`s
//! (the migrator dumps its migrations table, so a database loaded from the
//! dump knows which migrations it already has).

mod mysql;
mod postgres;
mod sqlite;


use crate::connection::{SchemaConnection, SchemaRow};
use crate::error::SchemaResult;
use crate::grammar::{columnize, ident, table};
use crate::inspect::{self, text};
use crate::target::{Dialect, Target};

/// What a schema dump includes besides the structure.
///
/// ```
/// use navree_schema::DumpOptions;
///
/// let options = DumpOptions::default().with_data("navree_migrations");
/// assert_eq!(options.data, ["navree_migrations"]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DumpOptions {
    /// Tables whose rows are dumped as `INSERT` statements.
    pub data: Vec<String>,
}

impl DumpOptions {
    /// The same options, dumping the rows of `table` too.
    pub fn with_data(mut self, table: impl Into<String>) -> DumpOptions {
        self.data.push(table.into());
        self
    }
}

/// An SQL script being written: sections of statements, each ending with
/// `;` (compound MySQL statements are wrapped in `DELIMITER ;;`).
pub(crate) struct Script {
    dialect: Dialect,
    out: String,
}

impl Script {
    pub(crate) fn new(dialect: Dialect, target: &Target) -> Script {
        let mut out = format!(
            "-- Schema of a {} database, written by `navree schema:dump`.\n\
             -- `navree migrate` loads it into an empty database, in one transaction,\n\
             -- then runs the migrations it does not record.\n",
            database_name(target)
        );
        if dialect == Dialect::Postgres {
            out.push_str("\nSET LOCAL check_function_bodies = false;\n");
        }
        Script { dialect, out }
    }

    /// A titled section of statements (nothing when `statements` is empty).
    pub(crate) fn section(&mut self, title: &str, statements: &[String]) {
        if statements.is_empty() {
            return;
        }
        self.out.push_str(&format!("\n-- {title}\n"));
        for statement in statements {
            self.statement(statement);
        }
    }

    /// Comment lines about what the dump leaves out, under the last
    /// section.
    pub(crate) fn notes(&mut self, notes: &[String]) {
        for note in notes {
            for line in note.lines() {
                self.out.push_str(&format!("-- {line}\n"));
            }
        }
    }

    /// One statement, terminated.
    pub(crate) fn statement(&mut self, sql: &str) {
        let sql = sql.trim().trim_end_matches(';').trim_end();
        self.out.push_str(sql);
        self.out.push_str(";\n");
    }

    /// A titled section of compound statements (bodies with `;` inside):
    /// MySQL needs them between `DELIMITER ;;` and `DELIMITER ;`.
    pub(crate) fn compound_section(&mut self, title: &str, statements: &[String]) {
        if statements.is_empty() {
            return;
        }
        if self.dialect != Dialect::MySql {
            self.section(title, statements);
            return;
        }
        self.out.push_str(&format!("\n-- {title}\nDELIMITER ;;\n"));
        for statement in statements {
            self.out
                .push_str(statement.trim().trim_end_matches(';').trim_end());
            self.out.push_str(";;\n");
        }
        self.out.push_str("DELIMITER ;\n");
    }

    pub(crate) fn finish(self) -> String {
        self.out
    }
}

fn database_name(target: &Target) -> String {
    match (target.dialect(), target.is_mariadb(), target.timescaledb()) {
        (Dialect::Postgres, _, Some(_)) => "PostgreSQL (TimescaleDB)".to_owned(),
        (Dialect::MySql, true, _) => "MariaDB".to_owned(),
        (dialect, _, _) => dialect.to_string(),
    }
}

/// The dump of the database behind `conn`.
pub(crate) async fn dump(
    conn: &dyn SchemaConnection,
    target: &Target,
    options: &DumpOptions,
) -> SchemaResult<String> {
    let mut script = Script::new(target.dialect(), target);
    match target.dialect() {
        Dialect::Sqlite => sqlite::dump(conn, &mut script).await?,
        Dialect::MySql => mysql::dump(conn, &mut script).await?,
        Dialect::Postgres => postgres::dump(conn, target, &mut script).await?,
    }
    let mut inserts = Vec::new();
    for name in &options.data {
        inserts.extend(rows_as_inserts(conn, target.dialect(), name).await?);
    }
    script.section("Data", &inserts);
    if target.dialect() == Dialect::MySql {
        script.section(
            "Foreign key checks back on",
            &["SET FOREIGN_KEY_CHECKS=1".to_owned()],
        );
    }
    Ok(script.finish())
}

/// `INSERT` statements for every row of `table_name`, in key order, with
/// each value written as an SQL literal by the database itself (`quote`,
/// `quote_nullable`, `QUOTE`). Generated columns are left out.
async fn rows_as_inserts(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
    table_name: &str,
) -> SchemaResult<Vec<String>> {
    if !inspect::has_table(conn, dialect, table_name).await? {
        return Ok(Vec::new());
    }
    let columns: Vec<String> = inspect::columns(conn, dialect, table_name)
        .await?
        .into_iter()
        .filter(|c| c.generation.is_none())
        .map(|c| c.name)
        .collect();
    if columns.is_empty() {
        return Ok(Vec::new());
    }
    let mut order: Vec<String> = inspect::indexes(conn, dialect, table_name)
        .await?
        .into_iter()
        .find(|i| i.primary)
        .map(|i| i.columns)
        .unwrap_or_default();
    if order.is_empty() {
        order = columns.clone();
    }
    let literal = |column: &String| {
        let c = ident(dialect, column);
        match dialect {
            Dialect::Sqlite => format!("quote({c})"),
            Dialect::Postgres => format!("quote_nullable({c}::text)"),
            Dialect::MySql => format!("QUOTE({c})"),
        }
    };
    let values = match dialect {
        Dialect::MySql => format!(
            "CONCAT_WS(', ', {})",
            columns.iter().map(literal).collect::<Vec<_>>().join(", ")
        ),
        Dialect::Postgres | Dialect::Sqlite => columns
            .iter()
            .map(literal)
            .collect::<Vec<_>>()
            .join(" || ', ' || "),
    };
    let sql = format!(
        "SELECT {values} AS row_values FROM {} ORDER BY {}",
        table(dialect, table_name),
        columnize(dialect, &order)
    );
    let found: Vec<SchemaRow> = inspect::rows(conn, &sql, Vec::new()).await?;
    let insert = format!(
        "INSERT INTO {} ({}) VALUES",
        table(dialect, table_name),
        columnize(dialect, &columns)
    );
    Ok(found
        .iter()
        .filter_map(|row| text(row, "row_values"))
        .map(|values| format!("{insert} ({values})"))
        .collect())
}
