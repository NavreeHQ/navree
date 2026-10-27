//! The SQLite grammar: `ALTER TABLE` where SQLite allows it, table
//! rebuilds where it does not.

use std::collections::HashSet;

use crate::blueprint::{Blueprint, Command};
use crate::column::{ColumnDefinition, ColumnType};
use crate::error::{SchemaError, SchemaResult};
use crate::grammar::timescale;
use crate::grammar::{
    Compiled, Context, columnize, default_value, ident, schema_of, string, strings, table,
    unqualified,
};
use crate::index::{ForeignKeyDefinition, IndexDefinition, IndexKind};
use crate::rebuild::{StateColumn, StateForeign, StateIndex, TableState, rename_index_sql};
use crate::target::Dialect;

const D: Dialect = Dialect::Sqlite;

pub(crate) fn compile(
    blueprint: &Blueprint,
    commands: &[Command],
    ctx: &Context<'_>,
) -> SchemaResult<Compiled> {
    if blueprint.creating {
        create(blueprint, commands, ctx)
    } else {
        Alter::new(blueprint, ctx).run(commands)
    }
}

/// Whether compiling `commands` needs the table's current state.
pub(crate) fn needs_state(blueprint: &Blueprint, commands: &[Command]) -> bool {
    !blueprint.creating
        && commands.iter().any(|command| match command {
            Command::Column(index) => {
                let column = &blueprint.columns()[*index];
                column.change || !can_add_in_place(column)
            }
            Command::Index(index) => index.kind == IndexKind::Primary,
            Command::DropIndex { kind, .. } => *kind == IndexKind::Primary,
            Command::Foreign(_)
            | Command::DropForeign { .. }
            | Command::DropColumn(_)
            | Command::RenameIndex { .. } => true,
            _ => false,
        })
}

fn create(
    blueprint: &Blueprint,
    commands: &[Command],
    ctx: &Context<'_>,
) -> SchemaResult<Compiled> {
    let mut out = Compiled::default();
    let name = blueprint.table();
    let mut primary = crate::grammar::primary_command(commands).cloned();
    let mut policies = Vec::new();
    if let Some(hypertable) = &blueprint.hypertable {
        let plan = timescale::plan_create(blueprint, hypertable, commands, ctx)?;
        primary = plan.primary;
        out.notices.extend(plan.notices);
        policies = plan.policies;
    }
    let auto = blueprint
        .columns()
        .iter()
        .find(|c| c.is_auto_increment() && !c.change);
    if let (Some(auto), Some(key)) = (auto, &primary) {
        if key.columns == [auto.name.clone()] {
            primary = None;
        } else {
            return Err(SchemaError::definition(
                name,
                format!(
                    "SQLite's AUTOINCREMENT column `{}` must be the whole primary key, not part of ({}); use a plain integer column or drop the composite key",
                    auto.name,
                    key.columns.join(", ")
                ),
            ));
        }
    }
    let mut body = Vec::new();
    for column in crate::grammar::added_columns(blueprint) {
        body.push(column_definition(column, true)?);
    }
    for command in commands {
        if let Command::Foreign(foreign) = command {
            body.push(foreign_clause(foreign));
        }
    }
    if let Some(primary) = &primary {
        body.push(format!("PRIMARY KEY ({})", columnize(D, &primary.columns)));
    }
    out.push(format!(
        "CREATE {}TABLE {}{} ({})",
        if blueprint.temporary {
            "TEMPORARY "
        } else {
            ""
        },
        if blueprint.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        },
        table(D, name),
        body.join(", ")
    ));
    for command in commands {
        match command {
            Command::Index(index) if index.kind == IndexKind::Primary => {}
            Command::Index(index) => out.push(index_statement(name, index)?),
            Command::Foreign(_) | Command::Comment(_) => {}
            Command::Drop => out.push(format!("DROP TABLE {}", table(D, name))),
            Command::DropIfExists => out.push(format!("DROP TABLE IF EXISTS {}", table(D, name))),
            other => {
                return Err(SchemaError::definition(
                    name,
                    format!("{other:?} cannot be part of creating a table"),
                ));
            }
        }
    }
    out.statements.extend(policies);
    Ok(out)
}

/// Whether SQLite can `ADD COLUMN` this column in place: not a primary key
/// or auto-increment, not a stored generated column, no expression
/// default, and nullable or defaulted.
fn can_add_in_place(column: &ColumnDefinition) -> bool {
    if column.is_auto_increment() || column.stored_as.is_some() || column.use_current {
        return false;
    }
    if column
        .default
        .as_ref()
        .is_some_and(crate::value::DefaultValue::is_expression)
    {
        return false;
    }
    column.is_nullable() || column.default.is_some() || column.is_generated()
}

/// An alter blueprint compiled against the table's state, rebuilding the
/// table when a command needs it.
struct Alter<'a> {
    blueprint: &'a Blueprint,
    ctx: &'a Context<'a>,
    table: String,
    state: Option<TableState>,
    physical: HashSet<String>,
    pending: bool,
    out: Compiled,
}

impl<'a> Alter<'a> {
    fn new(blueprint: &'a Blueprint, ctx: &'a Context<'a>) -> Alter<'a> {
        let state = ctx.state.cloned();
        let physical = state
            .iter()
            .flat_map(|s| s.columns.iter().map(|c| c.name.to_lowercase()))
            .collect();
        Alter {
            blueprint,
            ctx,
            table: blueprint.table().to_owned(),
            state,
            physical,
            pending: false,
            out: Compiled::default(),
        }
    }

    fn t(&self) -> String {
        table(D, &self.table)
    }

    fn state(&mut self, what: &str) -> SchemaResult<&mut TableState> {
        let table_name = self.table.clone();
        self.state.as_mut().ok_or_else(|| SchemaError::Offline {
            operation: format!(
                "{what} on SQLite (it rebuilds `{table_name}` from its current columns, indexes and foreign keys)"
            ),
        })
    }

    /// Emits a pending rebuild before an in-place statement.
    fn in_place(&mut self, statement: String) -> SchemaResult {
        self.flush()?;
        self.out.push(statement);
        Ok(())
    }

    fn rebuild_needed(&mut self) {
        self.pending = true;
    }

    fn flush(&mut self) -> SchemaResult {
        if !self.pending {
            return Ok(());
        }
        self.pending = false;
        let state = self.state.clone().expect("a pending rebuild has a state");
        let statements = rebuild_statements(&self.table, &state, &self.physical)?;
        self.out.statements.extend(statements);
        self.physical = state
            .columns
            .iter()
            .map(|c| c.name.to_lowercase())
            .collect();
        Ok(())
    }

    fn run(mut self, commands: &[Command]) -> SchemaResult<Compiled> {
        for command in commands {
            self.command(command)?;
        }
        self.flush()?;
        Ok(self.out)
    }

    fn command(&mut self, command: &Command) -> SchemaResult {
        let blueprint = self.blueprint;
        match command {
            Command::Column(index) => {
                let column = &blueprint.columns()[*index];
                if column.change {
                    let body = column_body(column, false)?;
                    let state = self.state(&format!("changing column `{}`", column.name))?;
                    let Some(existing) = state
                        .columns
                        .iter_mut()
                        .find(|c| c.name.eq_ignore_ascii_case(&column.name))
                    else {
                        return Err(SchemaError::missing(format!(
                            "cannot change column `{}`: `{}` has no such column",
                            column.name,
                            blueprint.table()
                        )));
                    };
                    existing.definition = body;
                    existing.generated = column.is_generated();
                    if column.is_auto_increment() {
                        state.autoincrement = Some(column.name.clone());
                        state.primary = vec![column.name.clone()];
                    }
                    self.rebuild_needed();
                } else if can_add_in_place(column) {
                    let definition = column_definition(column, false)?;
                    self.in_place(format!("ALTER TABLE {} ADD COLUMN {definition}", self.t()))?;
                    let body = column_body(column, false)?;
                    if let Some(state) = self.state.as_mut() {
                        state.columns.push(StateColumn {
                            name: column.name.clone(),
                            definition: body,
                            generated: column.is_generated(),
                            unique: false,
                        });
                    }
                    self.physical.insert(column.name.to_lowercase());
                } else {
                    let body = column_body(column, false)?;
                    let table_name = self.table.clone();
                    let state = self.state(&format!("adding column `{}`", column.name))?;
                    if state.has_rows
                        && !column.is_nullable()
                        && column.default.is_none()
                        && !column.use_current
                        && !column.is_generated()
                        && !column.is_auto_increment()
                    {
                        return Err(SchemaError::definition(
                            table_name,
                            format!(
                                "column `{}` is NOT NULL without a default, and the table has rows: give it a default or make it nullable",
                                column.name
                            ),
                        ));
                    }
                    state.columns.push(StateColumn {
                        name: column.name.clone(),
                        definition: body,
                        generated: column.is_generated(),
                        unique: false,
                    });
                    if column.is_auto_increment() {
                        state.autoincrement = Some(column.name.clone());
                        state.primary = vec![column.name.clone()];
                    }
                    self.rebuild_needed();
                }
            }
            Command::Index(index) => match index.kind {
                IndexKind::Primary => {
                    let state = self.state("adding a primary key")?;
                    state.primary = index.columns.clone();
                    if state
                        .autoincrement
                        .as_ref()
                        .is_some_and(|a| index.columns != [a.clone()])
                    {
                        state.autoincrement = None;
                    }
                    self.rebuild_needed();
                }
                _ => {
                    let statement = index_statement(&self.table, index)?;
                    self.in_place(statement)?;
                    if let Some(state) = self.state.as_mut() {
                        state.indexes.push(StateIndex {
                            name: index.name.clone(),
                            unique: index.kind == IndexKind::Unique,
                            columns: index.columns.iter().cloned().map(Some).collect(),
                            sql: Some(index_statement(&self.table, index)?),
                        });
                    }
                }
            },
            Command::Foreign(foreign) => {
                let state = self.state("adding a foreign key")?;
                state.foreign_keys.push(StateForeign {
                    columns: foreign.columns.clone(),
                    table: foreign.on.clone().unwrap_or_default(),
                    references: foreign.references.clone(),
                    on_update: foreign.on_update,
                    on_delete: foreign.on_delete,
                });
                self.rebuild_needed();
            }
            Command::DropColumn(columns) => {
                for column in columns {
                    self.drop_column(column)?;
                }
            }
            Command::RenameColumn { from, to } => {
                let statement = format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {}",
                    self.t(),
                    ident(D, from),
                    ident(D, to)
                );
                self.in_place(statement)?;
                if let Some(state) = self.state.as_mut() {
                    state.rename_column(from, to);
                }
                if self.physical.remove(&from.to_lowercase()) {
                    self.physical.insert(to.to_lowercase());
                }
            }
            Command::DropIndex { kind, name, .. } => {
                if *kind == IndexKind::Primary {
                    let state = self.state("dropping the primary key")?;
                    state.primary.clear();
                    state.autoincrement = None;
                    self.rebuild_needed();
                } else {
                    if matches!(kind, IndexKind::Fulltext | IndexKind::Spatial) {
                        return Err(SchemaError::unsupported(
                            D,
                            format!("{kind}es"),
                            "SQLite has no such indexes to drop",
                        ));
                    }
                    let name = name.clone().unwrap_or_default();
                    let statement = format!("DROP INDEX {}", qualified_index(&self.table, &name));
                    self.in_place(statement)?;
                    if let Some(state) = self.state.as_mut() {
                        state
                            .indexes
                            .retain(|i| !i.name.eq_ignore_ascii_case(&name));
                    }
                }
            }
            Command::DropForeign { name, columns } => {
                if columns.is_empty() {
                    return Err(SchemaError::unsupported(
                        D,
                        format!("dropping foreign key `{name}` by name"),
                        "SQLite foreign keys have no names: drop it by its columns, `drop_foreign([\"company_id\"])`",
                    ));
                }
                let state = self.state("dropping a foreign key")?;
                let before = state.foreign_keys.len();
                state.foreign_keys.retain(|f| {
                    !(f.columns.len() == columns.len()
                        && f.columns
                            .iter()
                            .zip(columns)
                            .all(|(a, b)| a.eq_ignore_ascii_case(b)))
                });
                if state.foreign_keys.len() == before {
                    return Err(SchemaError::missing(format!(
                        "`{}` has no foreign key on ({})",
                        blueprint.table(),
                        columns.join(", ")
                    )));
                }
                self.rebuild_needed();
            }
            Command::RenameIndex { from, to } => {
                let table_name = self.table.clone();
                let state = self.state(&format!("renaming index `{from}`"))?;
                let Some(index) = state
                    .indexes
                    .iter_mut()
                    .find(|i| i.name.eq_ignore_ascii_case(from))
                else {
                    return Err(SchemaError::missing(format!(
                        "index `{from}` does not exist on `{table_name}`"
                    )));
                };
                let Some(sql) = index.sql.clone() else {
                    return Err(SchemaError::unsupported(
                        D,
                        format!("renaming index `{from}`, which a constraint created"),
                        "drop the constraint and declare an index with the new name",
                    ));
                };
                let created = rename_index_sql(&sql, to);
                index.name = to.clone();
                index.sql = Some(created.clone());
                let drop = format!("DROP INDEX {}", qualified_index(&table_name, from));
                self.in_place(drop)?;
                self.in_place(created)?;
            }
            Command::Rename(to) => {
                let statement = format!(
                    "ALTER TABLE {} RENAME TO {}",
                    self.t(),
                    ident(D, unqualified(to))
                );
                self.in_place(statement)?;
                self.table = match schema_of(&self.table) {
                    Some(schema) => format!("{schema}.{}", unqualified(to)),
                    None => to.clone(),
                };
            }
            Command::Drop => {
                let statement = format!("DROP TABLE {}", self.t());
                self.in_place(statement)?;
            }
            Command::DropIfExists => {
                let statement = format!("DROP TABLE IF EXISTS {}", self.t());
                self.in_place(statement)?;
            }
            Command::Comment(_) => {}
        }
        Ok(())
    }

    fn drop_column(&mut self, column: &str) -> SchemaResult {
        let modern = self.ctx.target.at_least((3, 35));
        let Some(state) = self.state.as_mut() else {
            if modern {
                let statement =
                    format!("ALTER TABLE {} DROP COLUMN {}", self.t(), ident(D, column));
                return self.in_place(statement);
            }
            return Err(SchemaError::Offline {
                operation: format!(
                    "dropping column `{column}` on SQLite before 3.35 (it rebuilds the table)"
                ),
            });
        };
        if state.column(column).is_none() {
            return Err(SchemaError::missing(format!(
                "cannot drop column `{column}`: `{}` has no such column",
                self.blueprint.table()
            )));
        }
        if self.pending {
            // A rebuild is coming anyway: it leaves the column out.
            state.drop_column(column);
            return Ok(());
        }
        if modern && !state.column_is_entangled(column) {
            let indexes = state.indexes_using(column);
            state.drop_column(column);
            for index in indexes {
                let statement = format!("DROP INDEX {}", qualified_index(&self.table, &index));
                self.in_place(statement)?;
            }
            let statement = format!("ALTER TABLE {} DROP COLUMN {}", self.t(), ident(D, column));
            self.in_place(statement)?;
            self.physical.remove(&column.to_lowercase());
        } else {
            state.drop_column(column);
            self.rebuild_needed();
        }
        Ok(())
    }
}

/// The statements rebuilding `table_name` into `state`. `physical`: the
/// (lower-cased) columns the table has now, which are copied.
fn rebuild_statements(
    table_name: &str,
    state: &TableState,
    physical: &HashSet<String>,
) -> SchemaResult<Vec<String>> {
    if state.foreign_keys_enabled && !state.referenced_by.is_empty() {
        return Err(SchemaError::unsupported(
            D,
            format!(
                "rebuilding `{table_name}` while foreign keys are on and {} reference{} it",
                state.referenced_by.join(", "),
                if state.referenced_by.len() == 1 {
                    "s"
                } else {
                    ""
                }
            ),
            "dropping the old table would run their ON DELETE actions; run the change through the migrator (it migrates SQLite with foreign keys off and checks them before committing), or turn `PRAGMA foreign_keys` off outside a transaction",
        ));
    }
    let temp_name = match schema_of(table_name) {
        Some(schema) => format!("{schema}.__temp__{}", unqualified(table_name)),
        None => format!("__temp__{table_name}"),
    };
    let temp = table(D, &temp_name);
    let original = table(D, table_name);
    let autoincrement = state
        .autoincrement
        .as_ref()
        .filter(|a| state.primary.len() == 1 && state.primary[0].eq_ignore_ascii_case(a));
    let mut body = Vec::new();
    for column in &state.columns {
        let mut definition = format!("{} {}", ident(D, &column.name), column.definition);
        if autoincrement.is_some_and(|a| a.eq_ignore_ascii_case(&column.name)) {
            definition.push_str(" PRIMARY KEY AUTOINCREMENT");
        }
        body.push(definition.trim_end().to_owned());
    }
    body.extend(state.constraints.iter().cloned());
    for foreign in &state.foreign_keys {
        body.push(state_foreign_clause(foreign));
    }
    if !state.primary.is_empty() && autoincrement.is_none() {
        body.push(format!("PRIMARY KEY ({})", columnize(D, &state.primary)));
    }
    let mut create = format!("CREATE TABLE {temp} ({})", body.join(", "));
    if !state.options.is_empty() {
        create.push(' ');
        create.push_str(&state.options);
    }
    let copied: Vec<String> = state
        .columns
        .iter()
        .filter(|c| !c.generated && physical.contains(&c.name.to_lowercase()))
        .map(|c| c.name.clone())
        .collect();
    let mut statements = vec![create];
    if !copied.is_empty() {
        let columns = columnize(D, &copied);
        statements.push(format!(
            "INSERT INTO {temp} ({columns}) SELECT {columns} FROM {original}"
        ));
    }
    statements.push(format!("DROP TABLE {original}"));
    statements.push("PRAGMA legacy_alter_table = ON".to_owned());
    statements.push(format!(
        "ALTER TABLE {temp} RENAME TO {}",
        ident(D, unqualified(table_name))
    ));
    statements.push("PRAGMA legacy_alter_table = OFF".to_owned());
    for index in &state.indexes {
        if let Some(sql) = &index.sql {
            statements.push(sql.clone());
        }
    }
    statements.extend(state.triggers.iter().cloned());
    Ok(statements)
}

fn state_foreign_clause(foreign: &StateForeign) -> String {
    let mut sql = format!(
        "FOREIGN KEY ({}) REFERENCES {}",
        columnize(D, &foreign.columns),
        table(D, &foreign.table)
    );
    if !foreign.references.is_empty() {
        sql.push_str(&format!(" ({})", columnize(D, &foreign.references)));
    }
    for (clause, action) in [("DELETE", foreign.on_delete), ("UPDATE", foreign.on_update)] {
        if let Some(action) = action
            && action != crate::index::ReferentialAction::NoAction
        {
            sql.push_str(&format!(" ON {clause} {action}"));
        }
    }
    sql
}

fn foreign_clause(foreign: &ForeignKeyDefinition) -> String {
    state_foreign_clause(&StateForeign {
        columns: foreign.columns.clone(),
        table: foreign.on.clone().unwrap_or_default(),
        references: foreign.references.clone(),
        on_update: foreign.on_update,
        on_delete: foreign.on_delete,
    })
}

/// An index name qualified with the table's schema, when it has one.
fn qualified_index(table_name: &str, index: &str) -> String {
    match schema_of(table_name) {
        Some(schema) => format!("{}.{}", ident(D, schema), ident(D, index)),
        None => ident(D, index),
    }
}

fn index_statement(table_name: &str, index: &IndexDefinition) -> SchemaResult<String> {
    let unique = match index.kind {
        IndexKind::Unique => "UNIQUE ",
        IndexKind::Index => "",
        IndexKind::Fulltext | IndexKind::Spatial => {
            return Err(SchemaError::unsupported(
                D,
                format!("{}es (index `{}`)", index.kind, index.name),
                match index.kind {
                    IndexKind::Fulltext => "use an FTS5 virtual table, or PostgreSQL or MySQL",
                    _ => "use PostgreSQL with PostGIS, or MySQL",
                },
            ));
        }
        IndexKind::Primary => unreachable!("primary keys are part of the table"),
    };
    let parts = match &index.expression {
        Some(expression) => expression.as_str().to_owned(),
        None => columnize(D, &index.columns),
    };
    let predicate = index
        .predicate
        .as_ref()
        .map(|p| format!(" WHERE {p}"))
        .unwrap_or_default();
    let name = qualified_index(table_name, &index.name);
    Ok(format!(
        "CREATE {unique}INDEX {name} ON {} ({parts}){predicate}",
        ident(D, unqualified(table_name))
    ))
}

/// `"name" TYPE [PRIMARY KEY AUTOINCREMENT] [NOT NULL] [DEFAULT] [COLLATE]
/// [AS (...)]`.
pub(crate) fn column_definition(
    column: &ColumnDefinition,
    with_increment: bool,
) -> SchemaResult<String> {
    Ok(format!(
        "{} {}",
        ident(D, &column.name),
        column_body(column, with_increment)?
    ))
}

/// Everything after the name: the type and modifiers.
fn column_body(column: &ColumnDefinition, with_increment: bool) -> SchemaResult<String> {
    let mut sql = type_sql(column)?;
    if with_increment && column.is_auto_increment() {
        sql.push_str(" PRIMARY KEY AUTOINCREMENT");
    }
    if column.is_generated() {
        if column.nullable == Some(false) {
            sql.push_str(" NOT NULL");
        }
    } else if !column.is_nullable() {
        sql.push_str(" NOT NULL");
    }
    if !column.is_generated() {
        if let Some(default) = &column.default {
            let value = default_value(D, default);
            sql.push_str(&format!(
                " DEFAULT {}",
                if default.is_expression() && !value.starts_with('(') {
                    format!("({value})")
                } else {
                    value
                }
            ));
        } else if column.use_current {
            sql.push_str(match column.kind {
                ColumnType::Date => " DEFAULT CURRENT_DATE",
                ColumnType::Time { .. } | ColumnType::TimeTz { .. } => " DEFAULT CURRENT_TIME",
                ColumnType::Year => " DEFAULT (CAST(strftime('%Y', 'now') AS INTEGER))",
                _ => " DEFAULT CURRENT_TIMESTAMP",
            });
        }
    }
    if let Some(collation) = &column.collation {
        sql.push_str(&format!(" COLLATE {}", string(D, collation)));
    }
    if let Some(expression) = &column.virtual_as {
        sql.push_str(&format!(" AS ({expression})"));
    }
    if let Some(expression) = &column.stored_as {
        sql.push_str(&format!(" AS ({expression}) STORED"));
    }
    Ok(sql)
}

/// The SQLite type of a column.
pub(crate) fn type_sql(column: &ColumnDefinition) -> SchemaResult<String> {
    Ok(match &column.kind {
        ColumnType::Char { .. }
        | ColumnType::String { .. }
        | ColumnType::Uuid
        | ColumnType::IpAddress
        | ColumnType::MacAddress => "VARCHAR".to_owned(),
        ColumnType::TinyText | ColumnType::Text | ColumnType::MediumText | ColumnType::LongText => {
            "TEXT".to_owned()
        }
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::MediumInteger
        | ColumnType::Integer
        | ColumnType::BigInteger
        | ColumnType::Year => "INTEGER".to_owned(),
        ColumnType::Float { .. } => "FLOAT".to_owned(),
        ColumnType::Double => "DOUBLE".to_owned(),
        ColumnType::Decimal { .. } => "NUMERIC".to_owned(),
        ColumnType::Boolean => "TINYINT(1)".to_owned(),
        ColumnType::Enum { values } => format!(
            "VARCHAR CHECK ({} IN ({}))",
            ident(D, &column.name),
            strings(D, values)
        ),
        ColumnType::Set { .. } => {
            return Err(SchemaError::unsupported(
                D,
                format!("`set` columns (column `{}`)", column.name),
                "`set` is a MySQL type; use a `json` array or a join table",
            ));
        }
        ColumnType::Json | ColumnType::Jsonb => "TEXT".to_owned(),
        ColumnType::Date => "DATE".to_owned(),
        ColumnType::DateTime { .. }
        | ColumnType::DateTimeTz { .. }
        | ColumnType::Timestamp { .. }
        | ColumnType::TimestampTz { .. } => "DATETIME".to_owned(),
        ColumnType::Time { .. } | ColumnType::TimeTz { .. } => "TIME".to_owned(),
        ColumnType::Binary { .. } => "BLOB".to_owned(),
        ColumnType::Geometry { .. } | ColumnType::Geography { .. } => "GEOMETRY".to_owned(),
        ColumnType::Vector { .. } => {
            return Err(SchemaError::unsupported(
                D,
                format!("vector columns (column `{}`)", column.name),
                "store embeddings in a `binary` column, or use PostgreSQL with pgvector",
            ));
        }
        ColumnType::TsVector => {
            return Err(SchemaError::unsupported(
                D,
                format!("tsvector columns (column `{}`)", column.name),
                "use an FTS5 virtual table, or PostgreSQL",
            ));
        }
        ColumnType::Raw { definition } => definition.clone(),
    })
}
