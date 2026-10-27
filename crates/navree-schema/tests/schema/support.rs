//! Helpers shared by the navree-schema tests.

#![allow(dead_code)]

use navree_schema::{Schema, SchemaResult, Target};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tempfile::TempDir;

/// PostgreSQL 17 without TimescaleDB.
pub fn postgres() -> Target {
    Target::postgres(17)
}

/// PostgreSQL 17 with TimescaleDB 2.17.
pub fn timescaledb() -> Target {
    Target::postgres(17).with_timescaledb((2, 17))
}

/// MySQL 8.4.
pub fn mysql() -> Target {
    Target::mysql((8, 4))
}

/// MariaDB 11.4.
pub fn mariadb() -> Target {
    Target::mariadb((11, 4))
}

/// SQLite 3.46.
pub fn sqlite() -> Target {
    Target::sqlite((3, 46))
}

/// The SQL databases, without TimescaleDB.
pub fn dialects() -> Vec<(&'static str, Target)> {
    vec![
        ("postgres", postgres()),
        ("mysql", mysql()),
        ("mariadb", mariadb()),
        ("sqlite", sqlite()),
    ]
}

/// Every target, TimescaleDB included.
pub fn everything() -> Vec<(&'static str, Target)> {
    vec![
        ("timescaledb", timescaledb()),
        ("postgres", postgres()),
        ("mysql", mysql()),
        ("mariadb", mariadb()),
        ("sqlite", sqlite()),
    ]
}

/// Renders what one offline schema recorded: its statements (each ending
/// with `;`), its notices, or the error.
pub fn render(label: &str, schema: &Schema<'_>, result: SchemaResult) -> String {
    let mut out = format!("-- {label}\n");
    for statement in schema.statements() {
        out.push_str(&statement);
        out.push_str(";\n");
    }
    for notice in schema.notices() {
        out.push_str(&format!("-- notice: {notice}\n"));
    }
    if let Err(error) = result {
        out.push_str(&format!("-- error: {error}\n"));
    }
    out
}

/// Runs `$body` (using `$schema`, an offline schema) for each target of
/// `$targets` and renders the statements, notices and errors of each.
#[macro_export]
macro_rules! sql_for {
    ($targets:expr, |$schema:ident| $body:block) => {{
        let mut out = String::new();
        for (label, target) in $targets {
            let $schema = navree_schema::Schema::offline(target.clone());
            let result: navree_schema::SchemaResult = async { $body }.await;
            out.push_str(&$crate::support::render(label, &$schema, result));
            out.push('\n');
        }
        out
    }};
}

/// A fresh temporary SQLite database. Keep it alive for the test.
pub struct TempSqlite {
    pub dir: TempDir,
    pub url: String,
}

impl TempSqlite {
    pub fn new() -> TempSqlite {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("schema.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        TempSqlite { dir, url }
    }

    /// A pool on the database (foreign keys on, as `navree-db` opens it).
    pub async fn connect(&self) -> DatabaseConnection {
        self.connect_with_foreign_keys(true).await
    }

    /// A one-connection pool with foreign keys on or off.
    pub async fn connect_with_foreign_keys(&self, on: bool) -> DatabaseConnection {
        let mut options = ConnectOptions::new(self.url.clone());
        options.max_connections(1).sqlx_logging(false);
        options.map_sqlx_sqlite_opts(move |o| o.foreign_keys(on));
        Database::connect(options).await.expect("sqlite connects")
    }
}
