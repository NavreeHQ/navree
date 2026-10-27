//! Compilers from a [`Blueprint`] to SQL, one per dialect, plus the
//! TimescaleDB statements. Pure functions of the blueprint, the [`Target`]
//! and (for SQLite rebuilds and hypertable conversions) the table's current
//! state, so every dialect is testable without a server.

pub(crate) mod mysql;
pub(crate) mod postgres;
pub(crate) mod sqlite;
pub(crate) mod timescale;

use std::collections::HashSet;

use crate::blueprint::{Blueprint, Command};
use crate::column::{ColumnDefinition, ColumnType};
use crate::error::{SchemaError, SchemaResult};
use crate::index::IndexKind;
use crate::options::SchemaOptions;
use crate::rebuild::TableState;
use crate::target::{Dialect, Target};
use crate::value::DefaultValue;

/// The statements a blueprint compiles to, and the notices to report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Compiled {
    pub(crate) statements: Vec<String>,
    pub(crate) notices: Vec<String>,
}

impl Compiled {
    pub(crate) fn push(&mut self, statement: impl Into<String>) {
        self.statements.push(statement.into());
    }

    pub(crate) fn notice(&mut self, notice: impl Into<String>) {
        self.notices.push(notice.into());
    }
}

/// What a compiler knows besides the blueprint.
pub(crate) struct Context<'a> {
    pub(crate) target: &'a Target,
    pub(crate) options: &'a SchemaOptions,
    /// The table as it is now (SQLite rebuilds, hypertable conversions).
    pub(crate) state: Option<&'a TableState>,
}

/// Compiles `blueprint` for `ctx.target`.
pub(crate) fn compile(blueprint: &Blueprint, ctx: &Context<'_>) -> SchemaResult<Compiled> {
    validate(blueprint, ctx)?;
    let commands = blueprint.resolved_commands();
    match ctx.target.dialect() {
        Dialect::Postgres => postgres::compile(blueprint, &commands, ctx),
        Dialect::MySql => mysql::compile(blueprint, &commands, ctx),
        Dialect::Sqlite => sqlite::compile(blueprint, &commands, ctx),
    }
}

/// Checks a blueprint for mistakes no database would accept, with messages
/// that say how to fix them.
fn validate(blueprint: &Blueprint, ctx: &Context<'_>) -> SchemaResult {
    let table = blueprint.table();
    let fail = |message: String| Err(SchemaError::definition(table, message));
    if table.trim().is_empty() {
        return fail("the table name is empty".to_owned());
    }
    let mut seen = HashSet::new();
    for column in blueprint.columns() {
        if let Some(problem) = column.problems.first() {
            return fail(problem.clone());
        }
        if column.name.trim().is_empty() {
            return fail("a column name is empty".to_owned());
        }
        if !column.change && !seen.insert(column.name.as_str()) {
            return fail(format!("column `{}` is declared twice", column.name));
        }
        if column.virtual_as.is_some() && column.stored_as.is_some() {
            return fail(format!(
                "column `{}` is both `virtual_as` and `stored_as`; pick one",
                column.name
            ));
        }
        if column.is_generated() && column.default.is_some() {
            return fail(format!(
                "generated column `{}` cannot have a default",
                column.name
            ));
        }
        if let ColumnType::Enum { values } | ColumnType::Set { values } = &column.kind
            && values.is_empty()
        {
            return fail(format!(
                "{} column `{}` needs at least one value",
                column.kind.name(),
                column.name
            ));
        }
        if let ColumnType::Decimal { precision, scale } = column.kind
            && (precision == 0 || scale > precision)
        {
            return fail(format!(
                "decimal column `{}` needs 1 or more digits and a scale no larger than them (got {precision}, {scale})",
                column.name
            ));
        }
        if let Some(precision) = column.kind.precision()
            && precision > 6
        {
            return fail(format!(
                "column `{}` asks for {precision} fractional-second digits; databases keep at most 6",
                column.name
            ));
        }
        if let Some(DefaultValue::Float(value)) = &column.default
            && !value.is_finite()
        {
            return fail(format!(
                "column `{}` has a non-finite default ({value}); use a raw expression",
                column.name
            ));
        }
    }
    let commands = blueprint.resolved_commands();
    let primaries = commands
        .iter()
        .filter(|c| matches!(c, Command::Index(i) if i.kind == IndexKind::Primary))
        .count();
    if primaries > 1 {
        return fail(
            "more than one primary key is declared; list every key column in one `primary([...])`"
                .to_owned(),
        );
    }
    for command in &commands {
        if let Command::Foreign(foreign) = command {
            if foreign.on.is_none() {
                return fail(format!(
                    "foreign key `{}` on ({}) names no table: add `.on(\"table\")` (or use `constrained()`)",
                    foreign.name,
                    foreign.columns.join(", ")
                ));
            }
            if foreign.references.is_empty() {
                return fail(format!(
                    "foreign key `{}` on ({}) names no referenced column: add `.references(\"id\")`",
                    foreign.name,
                    foreign.columns.join(", ")
                ));
            }
            if foreign.references.len() != foreign.columns.len() {
                return fail(format!(
                    "foreign key `{}` has {} column(s) but references {}",
                    foreign.name,
                    foreign.columns.len(),
                    foreign.references.len()
                ));
            }
        }
        if let Command::Index(index) = command
            && index.columns.is_empty()
            && index.expression.is_none()
        {
            return fail(format!("{} `{}` has no columns", index.kind, index.name));
        }
    }
    if blueprint.creating && blueprint.columns().is_empty() && blueprint.hypertable.is_none() {
        return fail("a table needs at least one column".to_owned());
    }
    let _ = ctx;
    Ok(())
}

// ----- quoting --------------------------------------------------------------

/// `"name"` (PostgreSQL, SQLite) or `` `name` `` (MySQL), with the quote
/// character doubled inside.
pub(crate) fn ident(dialect: Dialect, name: &str) -> String {
    match dialect {
        Dialect::MySql => format!("`{}`", name.replace('`', "``")),
        Dialect::Postgres | Dialect::Sqlite => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// A table name, quoted part by part: `analytics.events` → `"analytics"."events"`.
pub(crate) fn table(dialect: Dialect, name: &str) -> String {
    name.split('.')
        .map(|part| ident(dialect, part))
        .collect::<Vec<_>>()
        .join(".")
}

/// The unqualified part of a table name: `analytics.events` → `events`.
pub(crate) fn unqualified(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// The schema part of a table name, if any.
pub(crate) fn schema_of(name: &str) -> Option<&str> {
    name.rsplit_once('.').map(|(schema, _)| schema)
}

/// Quoted, comma-separated columns.
pub(crate) fn columnize(dialect: Dialect, columns: &[String]) -> String {
    columns
        .iter()
        .map(|c| ident(dialect, c))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A string literal: `'it''s'`; on MySQL backslashes are escaped too.
pub(crate) fn string(dialect: Dialect, value: &str) -> String {
    let escaped = value.replace('\'', "''");
    match dialect {
        Dialect::MySql => format!("'{}'", escaped.replace('\\', "\\\\")),
        Dialect::Postgres | Dialect::Sqlite => format!("'{escaped}'"),
    }
}

/// Comma-separated string literals.
pub(crate) fn strings(dialect: Dialect, values: &[String]) -> String {
    values
        .iter()
        .map(|v| string(dialect, v))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The SQL of a default value.
pub(crate) fn default_value(dialect: Dialect, value: &DefaultValue) -> String {
    match value {
        DefaultValue::Null => "NULL".to_owned(),
        DefaultValue::Bool(b) => match (dialect, b) {
            (Dialect::Postgres, true) => "TRUE".to_owned(),
            (Dialect::Postgres, false) => "FALSE".to_owned(),
            (_, true) => "1".to_owned(),
            (_, false) => "0".to_owned(),
        },
        DefaultValue::Int(i) => i.to_string(),
        DefaultValue::UInt(u) => u.to_string(),
        DefaultValue::Float(f) => {
            let text = f.to_string();
            if text.contains('.') || text.contains('e') {
                text
            } else {
                format!("{text}.0")
            }
        }
        DefaultValue::Text(text) => string(dialect, text),
        DefaultValue::Expression(expression) => expression.as_str().to_owned(),
    }
}

/// The columns a created table's primary key uses: an explicit primary
/// command, else a column marked `.primary()` (already resolved into a
/// command), else none.
pub(crate) fn primary_command(commands: &[Command]) -> Option<&crate::index::IndexDefinition> {
    commands.iter().find_map(|c| match c {
        Command::Index(index) if index.kind == IndexKind::Primary => Some(index),
        _ => None,
    })
}

/// The column definitions added (not changed) by a blueprint.
pub(crate) fn added_columns(blueprint: &Blueprint) -> impl Iterator<Item = &ColumnDefinition> {
    blueprint.columns().iter().filter(|c| !c.change)
}
