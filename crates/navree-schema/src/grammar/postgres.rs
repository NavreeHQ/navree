//! The PostgreSQL (and TimescaleDB) grammar.

use crate::blueprint::{Blueprint, Command};
use crate::column::{ColumnDefinition, ColumnType};
use crate::error::{SchemaError, SchemaResult};
use crate::grammar::timescale::{self, CreatePlan};
use crate::grammar::{
    Compiled, Context, columnize, default_value, ident, schema_of, string, strings, table,
    unqualified,
};
use crate::index::{ForeignKeyDefinition, IndexDefinition, IndexKind};
use crate::target::Dialect;

const D: Dialect = Dialect::Postgres;

pub(crate) fn compile(
    blueprint: &Blueprint,
    commands: &[Command],
    ctx: &Context<'_>,
) -> SchemaResult<Compiled> {
    let mut out = Compiled::default();
    let name = blueprint.table();
    let mut plan = None;
    let mut primary = crate::grammar::primary_command(commands).cloned();
    if blueprint.creating {
        if let Some(hypertable) = &blueprint.hypertable {
            let created = timescale::plan_create(blueprint, hypertable, commands, ctx)?;
            primary = created.primary.clone();
            out.notices.extend(created.notices.iter().cloned());
            plan = Some(created);
        }
        if let Some(CreatePlan {
            install_extension: true,
            ..
        }) = &plan
        {
            out.push(timescale::CREATE_EXTENSION);
        }
        out.push(create_table(blueprint, primary.as_ref(), ctx)?);
        if let Some(plan) = &plan {
            out.statements.extend(plan.after_create.iter().cloned());
        }
    }
    let has_primary = primary.is_some();
    for command in commands {
        match command {
            Command::Index(index) if index.kind == IndexKind::Primary && blueprint.creating => {
                // Part of CREATE TABLE.
            }
            command => compile_command(blueprint, command, has_primary, ctx, &mut out)?,
        }
    }
    for column in blueprint.columns() {
        if let Some(comment) = comment_statement(name, column) {
            out.push(comment);
        }
    }
    if let Some(plan) = plan {
        out.statements.extend(plan.policies);
    }
    Ok(out)
}

fn compile_command(
    blueprint: &Blueprint,
    command: &Command,
    has_primary: bool,
    ctx: &Context<'_>,
    out: &mut Compiled,
) -> SchemaResult {
    let name = blueprint.table();
    let t = table(D, name);
    match command {
        Command::Column(index) => {
            let column = &blueprint.columns()[*index];
            if column.change {
                out.push(change_column(blueprint, column, ctx)?);
            } else {
                out.push(format!(
                    "ALTER TABLE {t} ADD COLUMN {}",
                    column_definition(blueprint, column, ctx, !has_primary)?
                ));
            }
        }
        Command::Index(index) => out.statements.extend(index_statements(name, index, ctx)?),
        Command::Foreign(foreign) => out.push(foreign_statement(name, foreign)),
        Command::DropColumn(columns) => out.push(format!(
            "ALTER TABLE {t} {}",
            columns
                .iter()
                .map(|c| format!("DROP COLUMN {}", ident(D, c)))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Command::RenameColumn { from, to } => out.push(format!(
            "ALTER TABLE {t} RENAME COLUMN {} TO {}",
            ident(D, from),
            ident(D, to)
        )),
        Command::DropIndex {
            kind, name: index, ..
        } => match kind {
            IndexKind::Primary => {
                let constraint = index
                    .clone()
                    .unwrap_or_else(|| format!("{}_pkey", unqualified(name)));
                out.push(format!(
                    "ALTER TABLE {t} DROP CONSTRAINT {}",
                    ident(D, &constraint)
                ));
            }
            IndexKind::Unique => out.push(format!(
                "ALTER TABLE {t} DROP CONSTRAINT {}",
                ident(D, index.as_deref().unwrap_or_default())
            )),
            IndexKind::Index | IndexKind::Fulltext | IndexKind::Spatial => out.push(format!(
                "DROP INDEX {}",
                qualified_index(name, index.as_deref().unwrap_or_default())
            )),
        },
        Command::DropForeign { name: foreign, .. } => out.push(format!(
            "ALTER TABLE {t} DROP CONSTRAINT {}",
            ident(D, foreign)
        )),
        Command::RenameIndex { from, to } => out.push(format!(
            "ALTER INDEX {} RENAME TO {}",
            qualified_index(name, from),
            ident(D, to)
        )),
        Command::Rename(to) => out.push(format!(
            "ALTER TABLE {t} RENAME TO {}",
            ident(D, unqualified(to))
        )),
        Command::Drop => out.push(format!("DROP TABLE {t}")),
        Command::DropIfExists => out.push(format!("DROP TABLE IF EXISTS {t}")),
        Command::Comment(comment) => {
            out.push(format!("COMMENT ON TABLE {t} IS {}", string(D, comment)))
        }
    }
    Ok(())
}

fn create_table(
    blueprint: &Blueprint,
    primary: Option<&IndexDefinition>,
    ctx: &Context<'_>,
) -> SchemaResult<String> {
    let mut body = Vec::new();
    for column in crate::grammar::added_columns(blueprint) {
        body.push(column_definition_in_create(
            blueprint,
            column,
            primary.is_some(),
            ctx,
        )?);
    }
    if let Some(primary) = primary {
        body.push(format!("PRIMARY KEY ({})", columnize(D, &primary.columns)));
    }
    Ok(format!(
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
        table(D, blueprint.table()),
        body.join(", ")
    ))
}

fn column_definition_in_create(
    blueprint: &Blueprint,
    column: &ColumnDefinition,
    has_primary: bool,
    ctx: &Context<'_>,
) -> SchemaResult<String> {
    column_definition(blueprint, column, ctx, !has_primary)
}

/// `"name" TYPE [COLLATE] [NULL|NOT NULL] [DEFAULT] [GENERATED ...]
/// [PRIMARY KEY]`. `inline_primary`: an identity column without an explicit
/// primary key becomes the primary key.
pub(crate) fn column_definition(
    blueprint: &Blueprint,
    column: &ColumnDefinition,
    ctx: &Context<'_>,
    inline_primary: bool,
) -> SchemaResult<String> {
    let mut sql = format!("{} {}", ident(D, &column.name), type_sql(column, ctx)?);
    if let Some(collation) = &column.collation {
        sql.push_str(&format!(" COLLATE {}", ident(D, collation)));
    }
    sql.push_str(if column.is_nullable() {
        " NULL"
    } else {
        " NOT NULL"
    });
    if let Some(default) = effective_default(column) {
        sql.push_str(&format!(" DEFAULT {default}"));
    }
    if let Some(expression) = &column.virtual_as {
        if !ctx.target.at_least(18) {
            return Err(SchemaError::unsupported(
                D,
                format!(
                    "virtual generated columns before version 18 (column `{}`, server {})",
                    column.name,
                    ctx.target.version()
                ),
                "use `stored_as`, which PostgreSQL 12 and later support",
            ));
        }
        sql.push_str(&format!(" GENERATED ALWAYS AS ({expression}) VIRTUAL"));
    }
    if let Some(expression) = &column.stored_as {
        sql.push_str(&format!(" GENERATED ALWAYS AS ({expression}) STORED"));
    }
    if let Some(identity) = identity_clause(column) {
        sql.push_str(&format!(" {identity}"));
        if inline_primary && column.is_auto_increment() {
            sql.push_str(" PRIMARY KEY");
        }
    }
    let _ = blueprint;
    Ok(sql)
}

/// `GENERATED BY DEFAULT AS IDENTITY [(options)]` for auto-incrementing and
/// identity columns.
fn identity_clause(column: &ColumnDefinition) -> Option<String> {
    if !(column.is_auto_increment() || column.identity.is_some()) {
        return None;
    }
    let identity = column.identity.clone().unwrap_or_default();
    let mut options = Vec::new();
    if let Some(given) = &identity.options {
        options.push(given.clone());
    }
    if let Some(start) = column.starting_value
        && !identity
            .options
            .as_deref()
            .is_some_and(|o| o.to_ascii_uppercase().contains("START"))
    {
        options.push(format!("START WITH {start}"));
    }
    Some(format!(
        "GENERATED {} AS IDENTITY{}",
        if identity.always {
            "ALWAYS"
        } else {
            "BY DEFAULT"
        },
        if options.is_empty() {
            String::new()
        } else {
            format!(" ({})", options.join(" "))
        }
    ))
}

/// The default clause value: the explicit default, else the current
/// date/time for `use_current`.
fn effective_default(column: &ColumnDefinition) -> Option<String> {
    if let Some(default) = &column.default {
        return Some(default_value(D, default));
    }
    if column.use_current {
        return Some(
            match column.kind {
                ColumnType::Date => "CURRENT_DATE",
                ColumnType::Time { .. } | ColumnType::TimeTz { .. } => "CURRENT_TIME",
                ColumnType::Year => "EXTRACT(YEAR FROM CURRENT_DATE)",
                _ => "CURRENT_TIMESTAMP",
            }
            .to_owned(),
        );
    }
    None
}

fn precision(p: Option<u8>) -> String {
    match p {
        Some(p) if p != 6 => format!("({p})"),
        _ => String::new(),
    }
}

/// The PostgreSQL type of a column.
pub(crate) fn type_sql(column: &ColumnDefinition, ctx: &Context<'_>) -> SchemaResult<String> {
    let _ = ctx;
    Ok(match &column.kind {
        ColumnType::Char { length } => format!("CHAR({length})"),
        ColumnType::String { length } => format!("VARCHAR({length})"),
        ColumnType::TinyText => "VARCHAR(255)".to_owned(),
        ColumnType::Text | ColumnType::MediumText | ColumnType::LongText => "TEXT".to_owned(),
        ColumnType::TinyInteger | ColumnType::SmallInteger => "SMALLINT".to_owned(),
        ColumnType::MediumInteger | ColumnType::Integer => "INTEGER".to_owned(),
        ColumnType::BigInteger => "BIGINT".to_owned(),
        ColumnType::Float { precision } => format!("FLOAT({precision})"),
        ColumnType::Double => "DOUBLE PRECISION".to_owned(),
        ColumnType::Decimal { precision, scale } => format!("DECIMAL({precision}, {scale})"),
        ColumnType::Boolean => "BOOLEAN".to_owned(),
        ColumnType::Enum { values } => format!(
            "VARCHAR(255) CHECK ({} IN ({}))",
            ident(D, &column.name),
            strings(D, values)
        ),
        ColumnType::Set { .. } => {
            return Err(SchemaError::unsupported(
                D,
                format!("`set` columns (column `{}`)", column.name),
                "`set` is a MySQL type; use a `jsonb` array or a join table",
            ));
        }
        ColumnType::Json => "JSON".to_owned(),
        ColumnType::Jsonb => "JSONB".to_owned(),
        ColumnType::Date => "DATE".to_owned(),
        ColumnType::Time { precision: p } => format!("TIME{} WITHOUT TIME ZONE", precision(*p)),
        ColumnType::TimeTz { precision: p } => format!("TIME{} WITH TIME ZONE", precision(*p)),
        ColumnType::DateTime { precision: p } | ColumnType::Timestamp { precision: p } => {
            format!("TIMESTAMP{} WITHOUT TIME ZONE", precision(*p))
        }
        ColumnType::DateTimeTz { precision: p } | ColumnType::TimestampTz { precision: p } => {
            format!("TIMESTAMP{} WITH TIME ZONE", precision(*p))
        }
        ColumnType::Year => "INTEGER".to_owned(),
        ColumnType::Binary { .. } => "BYTEA".to_owned(),
        ColumnType::Uuid => "UUID".to_owned(),
        ColumnType::IpAddress => "INET".to_owned(),
        ColumnType::MacAddress => "MACADDR".to_owned(),
        ColumnType::Geometry { subtype, srid } => match subtype {
            Some(subtype) => format!(
                "GEOMETRY({}{})",
                subtype.to_uppercase(),
                srid.map(|s| format!(", {s}")).unwrap_or_default()
            ),
            None => "GEOMETRY".to_owned(),
        },
        ColumnType::Geography { subtype, srid } => match subtype {
            Some(subtype) => format!("GEOGRAPHY({}, {srid})", subtype.to_uppercase()),
            None => "GEOGRAPHY".to_owned(),
        },
        ColumnType::Vector { dimensions } => match dimensions {
            Some(n) => format!("VECTOR({n})"),
            None => "VECTOR".to_owned(),
        },
        ColumnType::TsVector => "TSVECTOR".to_owned(),
        ColumnType::Raw { definition } => definition.clone(),
    })
}

/// `ALTER TABLE ... ALTER COLUMN ...` for a changed column: type
/// (with `USING`), nullability, default, identity, generated expression.
///
/// The definition is complete: a default left out is dropped. An
/// auto-incrementing column keeps its identity (and its sequence's
/// position); `generated_as()` adds one to a plain column; any other column
/// loses an identity it had.
fn change_column(
    blueprint: &Blueprint,
    column: &ColumnDefinition,
    ctx: &Context<'_>,
) -> SchemaResult<String> {
    let t = table(D, blueprint.table());
    let c = ident(D, &column.name);
    let mut changes = Vec::new();
    let mut ty = format!("TYPE {}", type_sql(column, ctx)?);
    if let Some(collation) = &column.collation {
        ty.push_str(&format!(" COLLATE {}", ident(D, collation)));
    }
    if let Some(using) = &column.using {
        ty.push_str(&format!(" USING {using}"));
    }
    changes.push(ty);
    changes.push(
        if column.is_nullable() {
            "DROP NOT NULL"
        } else {
            "SET NOT NULL"
        }
        .to_owned(),
    );
    if column.identity.is_some() {
        if let Some(identity) = identity_clause(column) {
            changes.push(format!("ADD {identity}"));
        }
    } else if column.is_auto_increment() {
        // Keeps the existing identity column as it is.
    } else {
        changes.push(match effective_default(column) {
            Some(default) => format!("SET DEFAULT {default}"),
            None => "DROP DEFAULT".to_owned(),
        });
        changes.push("DROP IDENTITY IF EXISTS".to_owned());
    }
    if let Some(expression) = column.stored_as.as_ref().or(column.virtual_as.as_ref()) {
        if !ctx.target.at_least(17) {
            return Err(SchemaError::unsupported(
                D,
                format!(
                    "changing a generated column's expression before version 17 (column `{}`)",
                    column.name
                ),
                "drop the column and add it again",
            ));
        }
        changes.push(format!("SET EXPRESSION AS ({expression})"));
    }
    Ok(format!(
        "ALTER TABLE {t} {}",
        changes
            .iter()
            .map(|change| format!("ALTER COLUMN {c} {change}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn comment_statement(table_name: &str, column: &ColumnDefinition) -> Option<String> {
    if column.comment.is_none() && !column.change {
        return None;
    }
    Some(format!(
        "COMMENT ON COLUMN {}.{} IS {}",
        table(D, table_name),
        ident(D, &column.name),
        column
            .comment
            .as_deref()
            .map_or_else(|| "NULL".to_owned(), |c| string(D, c))
    ))
}

/// An index name qualified with the table's schema, when it has one.
fn qualified_index(table_name: &str, index: &str) -> String {
    match schema_of(table_name) {
        Some(schema) => format!("{}.{}", ident(D, schema), ident(D, index)),
        None => ident(D, index),
    }
}

fn index_columns(index: &IndexDefinition) -> String {
    if let Some(expression) = &index.expression {
        return expression.as_str().to_owned();
    }
    match &index.operator_class {
        Some(class) => index
            .columns
            .iter()
            .map(|c| format!("{} {class}", ident(D, c)))
            .collect::<Vec<_>>()
            .join(", "),
        None => columnize(D, &index.columns),
    }
}

fn deferral(deferrable: Option<bool>, initially_immediate: Option<bool>) -> String {
    let mut sql = String::new();
    if let Some(deferrable) = deferrable {
        sql.push_str(if deferrable {
            " DEFERRABLE"
        } else {
            " NOT DEFERRABLE"
        });
        if deferrable && let Some(immediate) = initially_immediate {
            sql.push_str(if immediate {
                " INITIALLY IMMEDIATE"
            } else {
                " INITIALLY DEFERRED"
            });
        }
    }
    sql
}

/// The statements creating `index` on `table_name`.
pub(crate) fn index_statements(
    table_name: &str,
    index: &IndexDefinition,
    ctx: &Context<'_>,
) -> SchemaResult<Vec<String>> {
    let t = table(D, table_name);
    let name = ident(D, &index.name);
    let concurrently = if index.online { "CONCURRENTLY " } else { "" };
    let using = |algorithm: Option<&str>| {
        algorithm
            .map(|a| format!(" USING {}", a.to_uppercase()))
            .unwrap_or_default()
    };
    let predicate = index
        .predicate
        .as_ref()
        .map(|p| format!(" WHERE {p}"))
        .unwrap_or_default();
    Ok(match index.kind {
        IndexKind::Primary => vec![format!(
            "ALTER TABLE {t} ADD PRIMARY KEY ({})",
            columnize(D, &index.columns)
        )],
        IndexKind::Unique => {
            if index.nulls_not_distinct.is_some() && !ctx.target.at_least(15) {
                return Err(SchemaError::unsupported(
                    D,
                    format!(
                        "NULLS [NOT] DISTINCT before version 15 (index `{}`)",
                        index.name
                    ),
                    "upgrade PostgreSQL or use a partial unique index",
                ));
            }
            let nulls = match index.nulls_not_distinct {
                Some(true) => " NULLS NOT DISTINCT",
                Some(false) => " NULLS DISTINCT",
                None => "",
            };
            if index.predicate.is_some() || index.expression.is_some() {
                vec![format!(
                    "CREATE UNIQUE INDEX {concurrently}{name} ON {t}{}{nulls} ({}){predicate}",
                    using(index.algorithm.as_deref()),
                    index_columns(index)
                )]
            } else if index.online || index.algorithm.is_some() {
                vec![
                    format!(
                        "CREATE UNIQUE INDEX {concurrently}{name} ON {t}{} ({})",
                        using(index.algorithm.as_deref()),
                        index_columns(index)
                    ),
                    format!(
                        "ALTER TABLE {t} ADD CONSTRAINT {name} UNIQUE USING INDEX {name}{}",
                        deferral(index.deferrable, index.initially_immediate)
                    ),
                ]
            } else {
                vec![format!(
                    "ALTER TABLE {t} ADD CONSTRAINT {name} UNIQUE{nulls} ({}){}",
                    columnize(D, &index.columns),
                    deferral(index.deferrable, index.initially_immediate)
                )]
            }
        }
        IndexKind::Index => vec![format!(
            "CREATE INDEX {concurrently}{name} ON {t}{} ({}){predicate}",
            using(index.algorithm.as_deref()),
            index_columns(index)
        )],
        IndexKind::Fulltext => {
            let language = string(D, index.language.as_deref().unwrap_or("english"));
            let vectors = index
                .columns
                .iter()
                .map(|c| format!("to_tsvector({language}, {})", ident(D, c)))
                .collect::<Vec<_>>()
                .join(" || ");
            vec![format!(
                "CREATE INDEX {concurrently}{name} ON {t} USING GIN (({vectors})){predicate}"
            )]
        }
        IndexKind::Spatial => vec![format!(
            "CREATE INDEX {concurrently}{name} ON {t}{} ({}){predicate}",
            using(Some(index.algorithm.as_deref().unwrap_or("gist"))),
            index_columns(index)
        )],
    })
}

/// `ALTER TABLE ... ADD CONSTRAINT ... FOREIGN KEY ...`.
fn foreign_statement(table_name: &str, foreign: &ForeignKeyDefinition) -> String {
    let mut sql = format!(
        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        table(D, table_name),
        ident(D, &foreign.name),
        columnize(D, &foreign.columns),
        table(D, foreign.on.as_deref().unwrap_or_default()),
        columnize(D, &foreign.references)
    );
    if let Some(action) = foreign.on_delete {
        sql.push_str(&format!(" ON DELETE {action}"));
    }
    if let Some(action) = foreign.on_update {
        sql.push_str(&format!(" ON UPDATE {action}"));
    }
    sql.push_str(&deferral(foreign.deferrable, foreign.initially_immediate));
    if foreign.not_valid {
        sql.push_str(" NOT VALID");
    }
    sql
}
