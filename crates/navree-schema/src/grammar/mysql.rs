//! The MySQL and MariaDB grammar.

use crate::blueprint::{Blueprint, Command};
use crate::column::{ColumnDefinition, ColumnType};
use crate::error::{SchemaError, SchemaResult};
use crate::grammar::timescale;
use crate::grammar::{Compiled, Context, columnize, default_value, ident, string, strings, table};
use crate::index::{ForeignKeyDefinition, IndexDefinition, IndexKind};
use crate::target::Dialect;
use crate::value::DefaultValue;

const D: Dialect = Dialect::MySql;

pub(crate) fn compile(
    blueprint: &Blueprint,
    commands: &[Command],
    ctx: &Context<'_>,
) -> SchemaResult<Compiled> {
    let mut out = Compiled::default();
    let name = blueprint.table();
    let t = table(D, name);
    let mut primary = crate::grammar::primary_command(commands).cloned();
    let mut policies = Vec::new();
    if blueprint.creating {
        if let Some(hypertable) = &blueprint.hypertable {
            let plan = timescale::plan_create(blueprint, hypertable, commands, ctx)?;
            primary = plan.primary;
            out.notices.extend(plan.notices);
            policies = plan.policies;
        }
        out.push(create_table(blueprint, primary.as_ref(), ctx)?);
    }
    let has_primary = primary.is_some();
    for command in commands {
        match command {
            Command::Index(index) if index.kind == IndexKind::Primary && blueprint.creating => {}
            Command::Index(index)
                if index.kind == IndexKind::Primary && inlined_primary(blueprint, index) => {}
            Command::Column(index) => {
                let column = &blueprint.columns()[*index];
                let definition = column_definition(blueprint, column, has_primary, ctx)?;
                if column.change {
                    out.push(format!("ALTER TABLE {t} MODIFY {definition}"));
                } else {
                    out.push(format!("ALTER TABLE {t} ADD {definition}"));
                }
            }
            Command::Index(index) => out.push(index_statement(name, index, ctx)?),
            Command::Foreign(foreign) => out.push(foreign_statement(name, foreign)),
            Command::DropColumn(columns) => out.push(format!(
                "ALTER TABLE {t} {}",
                columns
                    .iter()
                    .map(|c| format!("DROP {}", ident(D, c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Command::RenameColumn { from, to } => {
                let supported = if ctx.target.is_mariadb() {
                    ctx.target.at_least((10, 5, 2))
                } else {
                    ctx.target.at_least((8, 0, 3))
                };
                if !supported {
                    return Err(SchemaError::unsupported(
                        D,
                        format!(
                            "RENAME COLUMN on {} (renaming `{from}`)",
                            ctx.target.describe()
                        ),
                        "upgrade to MySQL 8.0.3 or MariaDB 10.5.2, or redefine the column under its new name with `.change()` and raw SQL",
                    ));
                }
                out.push(format!(
                    "ALTER TABLE {t} RENAME COLUMN {} TO {}",
                    ident(D, from),
                    ident(D, to)
                ));
            }
            Command::DropIndex {
                kind, name: index, ..
            } => match (kind, index) {
                (IndexKind::Primary, _) => out.push(format!("ALTER TABLE {t} DROP PRIMARY KEY")),
                (_, index) => out.push(format!(
                    "ALTER TABLE {t} DROP INDEX {}",
                    ident(D, index.as_deref().unwrap_or_default())
                )),
            },
            Command::DropForeign { name: foreign, .. } => out.push(format!(
                "ALTER TABLE {t} DROP FOREIGN KEY {}",
                ident(D, foreign)
            )),
            Command::RenameIndex { from, to } => out.push(format!(
                "ALTER TABLE {t} RENAME INDEX {} TO {}",
                ident(D, from),
                ident(D, to)
            )),
            Command::Rename(to) => out.push(format!("RENAME TABLE {t} TO {}", table(D, to))),
            Command::Drop => out.push(format!("DROP TABLE {t}")),
            Command::DropIfExists => out.push(format!("DROP TABLE IF EXISTS {t}")),
            Command::Comment(comment) => {
                out.push(format!("ALTER TABLE {t} COMMENT = {}", string(D, comment)))
            }
        }
    }
    for column in blueprint.columns() {
        if column.is_auto_increment()
            && let Some(start) = column.starting_value
        {
            out.push(format!("ALTER TABLE {t} AUTO_INCREMENT = {start}"));
        }
    }
    out.statements.extend(policies);
    Ok(out)
}

/// A changed auto-incrementing column declared `.primary()` carries
/// `PRIMARY KEY` in its own definition.
fn inlined_primary(blueprint: &Blueprint, index: &IndexDefinition) -> bool {
    !blueprint.creating
        && index.columns.len() == 1
        && blueprint.columns().iter().any(|c| {
            c.name == index.columns[0] && c.change && c.is_auto_increment() && c.primary.is_some()
        })
}

fn create_table(
    blueprint: &Blueprint,
    primary: Option<&IndexDefinition>,
    ctx: &Context<'_>,
) -> SchemaResult<String> {
    let mut body = Vec::new();
    for column in crate::grammar::added_columns(blueprint) {
        body.push(column_definition(
            blueprint,
            column,
            primary.is_some(),
            ctx,
        )?);
    }
    if let Some(primary) = primary {
        body.push(format!(
            "PRIMARY KEY {}({})",
            primary
                .algorithm
                .as_deref()
                .map(|a| format!("USING {} ", a.to_uppercase()))
                .unwrap_or_default(),
            columnize(D, &primary.columns)
        ));
    }
    let mut sql = format!(
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
    );
    let options = ctx.options;
    if let Some(charset) = blueprint
        .charset
        .as_ref()
        .or(options.mysql_charset.as_ref())
    {
        sql.push_str(&format!(" DEFAULT CHARACTER SET {charset}"));
    }
    if let Some(collation) = blueprint
        .collation
        .as_ref()
        .or(options.mysql_collation.as_ref())
    {
        sql.push_str(&format!(" COLLATE {}", string(D, collation)));
    }
    if let Some(engine) = blueprint.engine.as_ref().or(options.mysql_engine.as_ref()) {
        sql.push_str(&format!(" ENGINE = {engine}"));
    }
    Ok(sql)
}

/// Whether MySQL needs an expression default (in parentheses) for the type.
fn needs_expression_default(kind: &ColumnType) -> bool {
    matches!(
        kind,
        ColumnType::TinyText
            | ColumnType::Text
            | ColumnType::MediumText
            | ColumnType::LongText
            | ColumnType::Json
            | ColumnType::Jsonb
            | ColumnType::Binary { length: None, .. }
            | ColumnType::Geometry { .. }
            | ColumnType::Geography { .. }
    )
}

fn expression_defaults(ctx: &Context<'_>) -> bool {
    ctx.target.is_mariadb() || ctx.target.at_least((8, 0, 13))
}

fn current_time(precision: Option<u8>) -> String {
    match precision {
        Some(p) if p > 0 => format!("CURRENT_TIMESTAMP({p})"),
        _ => "CURRENT_TIMESTAMP".to_owned(),
    }
}

fn effective_default(column: &ColumnDefinition, ctx: &Context<'_>) -> SchemaResult<Option<String>> {
    if let Some(default) = &column.default {
        let sql = default_value(D, default);
        if needs_expression_default(&column.kind) && !matches!(default, DefaultValue::Expression(_))
        {
            if !expression_defaults(ctx) {
                return Err(SchemaError::unsupported(
                    D,
                    format!(
                        "a default on {} column `{}` before 8.0.13",
                        column.kind.name(),
                        column.name
                    ),
                    "upgrade MySQL, or default the value in the application",
                ));
            }
            return Ok(Some(format!("({sql})")));
        }
        return Ok(Some(sql));
    }
    if !column.use_current {
        return Ok(None);
    }
    let expression = match column.kind {
        ColumnType::DateTime { precision }
        | ColumnType::DateTimeTz { precision }
        | ColumnType::Timestamp { precision }
        | ColumnType::TimestampTz { precision } => return Ok(Some(current_time(precision))),
        ColumnType::Date => "(CURDATE())",
        ColumnType::Time { .. } | ColumnType::TimeTz { .. } => "(CURTIME())",
        ColumnType::Year => "(YEAR(CURDATE()))",
        _ => return Ok(None),
    };
    if !expression_defaults(ctx) {
        return Err(SchemaError::unsupported(
            D,
            format!(
                "`use_current` on {} column `{}` before 8.0.13",
                column.kind.name(),
                column.name
            ),
            "use a timestamp column, or upgrade MySQL",
        ));
    }
    Ok(Some(expression.to_owned()))
}

/// `` `name` TYPE [UNSIGNED] [CHARACTER SET] [COLLATE] [AS (...)] [NULL|NOT
/// NULL] [DEFAULT] [ON UPDATE] [INVISIBLE] [AUTO_INCREMENT [PRIMARY KEY]]
/// [COMMENT] [AFTER|FIRST] ``.
pub(crate) fn column_definition(
    blueprint: &Blueprint,
    column: &ColumnDefinition,
    has_primary: bool,
    ctx: &Context<'_>,
) -> SchemaResult<String> {
    let mut sql = format!("{} {}", ident(D, &column.name), type_sql(column, ctx)?);
    if column.unsigned && !matches!(column.kind, ColumnType::Raw { .. }) {
        sql.push_str(" UNSIGNED");
    }
    if let Some(charset) = &column.charset {
        sql.push_str(&format!(" CHARACTER SET {charset}"));
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
    if column.is_generated() {
        if column.nullable == Some(false) {
            sql.push_str(" NOT NULL");
        }
    } else {
        sql.push_str(if column.is_nullable() {
            " NULL"
        } else {
            " NOT NULL"
        });
    }
    if let Some(default) = effective_default(column, ctx)? {
        sql.push_str(&format!(" DEFAULT {default}"));
    }
    if let Some(on_update) = &column.on_update {
        sql.push_str(&format!(" ON UPDATE {on_update}"));
    } else if column.use_current_on_update {
        sql.push_str(&format!(
            " ON UPDATE {}",
            current_time(column.kind.precision())
        ));
    }
    if column.invisible {
        sql.push_str(" INVISIBLE");
    }
    if column.is_auto_increment() {
        let inline_primary = if column.change {
            column.primary.is_some()
        } else {
            !has_primary
        };
        sql.push_str(if inline_primary {
            " AUTO_INCREMENT PRIMARY KEY"
        } else {
            " AUTO_INCREMENT"
        });
    }
    if let Some(comment) = &column.comment {
        sql.push_str(&format!(" COMMENT {}", string(D, comment)));
    }
    // Placement only applies when adding or changing a column.
    if !blueprint.creating {
        if let Some(after) = &column.after {
            sql.push_str(&format!(" AFTER {}", ident(D, after)));
        }
        if column.first {
            sql.push_str(" FIRST");
        }
    }
    Ok(sql)
}

fn precision(p: Option<u8>) -> String {
    match p {
        Some(p) if p > 0 => format!("({p})"),
        _ => String::new(),
    }
}

/// The MySQL type of a column.
pub(crate) fn type_sql(column: &ColumnDefinition, ctx: &Context<'_>) -> SchemaResult<String> {
    let target = ctx.target;
    Ok(match &column.kind {
        ColumnType::Char { length } => format!("CHAR({length})"),
        ColumnType::String { length } => format!("VARCHAR({length})"),
        ColumnType::TinyText => "TINYTEXT".to_owned(),
        ColumnType::Text => "TEXT".to_owned(),
        ColumnType::MediumText => "MEDIUMTEXT".to_owned(),
        ColumnType::LongText => "LONGTEXT".to_owned(),
        ColumnType::TinyInteger => "TINYINT".to_owned(),
        ColumnType::SmallInteger => "SMALLINT".to_owned(),
        ColumnType::MediumInteger => "MEDIUMINT".to_owned(),
        ColumnType::Integer => "INT".to_owned(),
        ColumnType::BigInteger => "BIGINT".to_owned(),
        ColumnType::Float { precision } => format!("FLOAT({precision})"),
        ColumnType::Double => "DOUBLE".to_owned(),
        ColumnType::Decimal { precision, scale } => format!("DECIMAL({precision}, {scale})"),
        ColumnType::Boolean => "TINYINT(1)".to_owned(),
        ColumnType::Enum { values } => format!("ENUM({})", strings(D, values)),
        ColumnType::Set { values } => format!("SET({})", strings(D, values)),
        ColumnType::Json | ColumnType::Jsonb => "JSON".to_owned(),
        ColumnType::Date => "DATE".to_owned(),
        ColumnType::DateTime { precision: p } | ColumnType::DateTimeTz { precision: p } => {
            format!("DATETIME{}", precision(*p))
        }
        ColumnType::Time { precision: p } | ColumnType::TimeTz { precision: p } => {
            format!("TIME{}", precision(*p))
        }
        ColumnType::Timestamp { precision: p } | ColumnType::TimestampTz { precision: p } => {
            format!("TIMESTAMP{}", precision(*p))
        }
        ColumnType::Year => "YEAR".to_owned(),
        ColumnType::Binary { length, fixed } => match length {
            Some(n) if *fixed => format!("BINARY({n})"),
            Some(n) => format!("VARBINARY({n})"),
            None => "BLOB".to_owned(),
        },
        ColumnType::Uuid => {
            if target.is_mariadb() && target.at_least((10, 7)) {
                "UUID".to_owned()
            } else {
                "CHAR(36)".to_owned()
            }
        }
        ColumnType::IpAddress => "VARCHAR(45)".to_owned(),
        ColumnType::MacAddress => "VARCHAR(17)".to_owned(),
        ColumnType::Geometry { subtype, srid } => geometry(subtype.as_deref(), *srid, ctx),
        ColumnType::Geography { subtype, srid } => geometry(subtype.as_deref(), Some(*srid), ctx),
        ColumnType::Vector { dimensions } => {
            let supported = if target.is_mariadb() {
                target.at_least((11, 7))
            } else {
                target.at_least(9)
            };
            if !supported {
                return Err(SchemaError::unsupported(
                    D,
                    format!(
                        "vector columns on {} (column `{}`)",
                        target.describe(),
                        column.name
                    ),
                    "vectors need MySQL 9 or MariaDB 11.7",
                ));
            }
            match dimensions {
                Some(n) => format!("VECTOR({n})"),
                None => "VECTOR".to_owned(),
            }
        }
        ColumnType::TsVector => {
            return Err(SchemaError::unsupported(
                D,
                format!("tsvector columns (column `{}`)", column.name),
                "use a FULLTEXT index on the text columns (`t.fulltext([...])`)",
            ));
        }
        ColumnType::Raw { definition } => definition.clone(),
    })
}

fn geometry(subtype: Option<&str>, srid: Option<u32>, ctx: &Context<'_>) -> String {
    const SUBTYPES: [&str; 7] = [
        "point",
        "linestring",
        "polygon",
        "geometrycollection",
        "multipoint",
        "multilinestring",
        "multipolygon",
    ];
    let base = subtype
        .map(str::to_lowercase)
        .filter(|s| SUBTYPES.contains(&s.as_str()))
        .unwrap_or_else(|| "geometry".to_owned())
        .to_uppercase();
    match srid {
        Some(srid) if srid != 0 && ctx.target.is_mariadb() => {
            format!("{base} REF_SYSTEM_ID={srid}")
        }
        Some(srid) if srid != 0 => format!("{base} SRID {srid}"),
        _ => base,
    }
}

fn index_statement(
    table_name: &str,
    index: &IndexDefinition,
    ctx: &Context<'_>,
) -> SchemaResult<String> {
    let t = table(D, table_name);
    if index.predicate.is_some() {
        return Err(SchemaError::unsupported(
            D,
            format!("partial indexes (index `{}`)", index.name),
            "index every row, or add a generated column holding the condition and index it",
        ));
    }
    let parts = match &index.expression {
        Some(expression) => {
            if !ctx.target.is_mariadb() && !ctx.target.at_least((8, 0, 13)) {
                return Err(SchemaError::unsupported(
                    D,
                    format!("expression indexes before 8.0.13 (index `{}`)", index.name),
                    "index a generated column instead",
                ));
            }
            format!("({expression})")
        }
        None => columnize(D, &index.columns),
    };
    let using = index
        .algorithm
        .as_deref()
        .map(|a| format!(" USING {}", a.to_uppercase()))
        .unwrap_or_default();
    let mut sql = match index.kind {
        IndexKind::Primary => format!("ALTER TABLE {t} ADD PRIMARY KEY{using}({parts})"),
        kind => {
            let keyword = match kind {
                IndexKind::Unique => "UNIQUE",
                IndexKind::Fulltext => "FULLTEXT",
                IndexKind::Spatial => "SPATIAL INDEX",
                _ => "INDEX",
            };
            format!(
                "ALTER TABLE {t} ADD {keyword} {}{using}({parts})",
                ident(D, &index.name)
            )
        }
    };
    if index.inplace {
        sql.push_str(", ALGORITHM=INPLACE");
    }
    if let Some(lock) = &index.lock {
        sql.push_str(&format!(", LOCK={}", lock.to_uppercase()));
    }
    Ok(sql)
}

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
    sql
}
