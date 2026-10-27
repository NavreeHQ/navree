//! Reading a database's schema: tables, views, columns, indexes and
//! foreign keys, per dialect; the version and TimescaleDB detection; the
//! table state SQLite rebuilds and hypertable conversions need.

use std::collections::BTreeMap;

use sea_orm::{QueryResult, Statement, Value};
use serde::Serialize;

use crate::connection::SchemaConnection;
use crate::error::{SchemaError, SchemaResult};
use crate::grammar::timescale::KeyState;
use crate::grammar::{schema_of, table, unqualified};
use crate::index::ReferentialAction;
use crate::rebuild::{StateColumn, StateForeign, StateIndex, TableState};
use crate::target::{Dialect, Target, Version};

/// A table of the database.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TableInfo {
    /// The table name.
    pub name: String,
    /// Its schema (PostgreSQL), database (MySQL) or `main` (SQLite).
    pub schema: Option<String>,
    /// Its size in bytes, data and indexes, when the database reports it.
    pub size: Option<i64>,
    /// Its comment.
    pub comment: Option<String>,
    /// Its storage engine (MySQL).
    pub engine: Option<String>,
    /// Its collation (MySQL).
    pub collation: Option<String>,
}

/// A view of the database.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ViewInfo {
    /// The view name.
    pub name: String,
    /// Its schema.
    pub schema: Option<String>,
    /// Its query.
    pub definition: Option<String>,
}

/// How a generated column is computed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Generation {
    /// Stored (computed on write) rather than virtual (on read).
    pub stored: bool,
    /// The expression, when the database reports it.
    pub expression: Option<String>,
}

/// A column of a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ColumnInfo {
    /// The column name.
    pub name: String,
    /// The type's name: `varchar`, `int8`, `timestamptz`.
    pub type_name: String,
    /// The full type: `character varying(255)`, `bigint unsigned`.
    pub full_type: String,
    /// The collation.
    pub collation: Option<String>,
    /// Whether it accepts `NULL`.
    pub nullable: bool,
    /// The default, as SQL.
    pub default: Option<String>,
    /// Whether it auto-increments (serial, identity, `AUTO_INCREMENT`,
    /// SQLite's integer primary key).
    pub auto_increment: bool,
    /// Its comment.
    pub comment: Option<String>,
    /// How it is generated, for a generated column.
    pub generation: Option<Generation>,
}

/// An index of a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexInfo {
    /// The index name (lower-cased).
    pub name: String,
    /// The indexed columns, in order.
    pub columns: Vec<String>,
    /// The index method (`btree`, `gin`, `hash`, `fulltext`, ...), when
    /// known.
    pub kind: Option<String>,
    /// Whether it is unique.
    pub unique: bool,
    /// Whether it is the primary key.
    pub primary: bool,
}

/// A foreign key of a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForeignKeyInfo {
    /// The constraint name (SQLite foreign keys have none).
    pub name: Option<String>,
    /// The referencing columns.
    pub columns: Vec<String>,
    /// The referenced table's schema.
    pub foreign_schema: Option<String>,
    /// The referenced table.
    pub foreign_table: String,
    /// The referenced columns.
    pub foreign_columns: Vec<String>,
    /// The action on update.
    #[serde(serialize_with = "serialize_action")]
    pub on_update: Option<ReferentialAction>,
    /// The action on delete.
    #[serde(serialize_with = "serialize_action")]
    pub on_delete: Option<ReferentialAction>,
}

fn serialize_action<S: serde::Serializer>(
    action: &Option<ReferentialAction>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match action {
        Some(action) => serializer.serialize_some(action.sql()),
        None => serializer.serialize_none(),
    }
}

// ----- reading rows ----------------------------------------------------------

/// Column `column` of `row` as text, whatever the driver decoded it as.
pub(crate) fn text(row: &QueryResult, column: &str) -> Option<String> {
    if let Ok(value) = row.try_get::<Option<String>>("", column) {
        return value;
    }
    if let Ok(Some(bytes)) = row.try_get::<Option<Vec<u8>>>("", column) {
        return Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    if let Ok(Some(number)) = row.try_get::<Option<i64>>("", column) {
        return Some(number.to_string());
    }
    None
}

/// Column `column` of `row` as an integer.
pub(crate) fn int(row: &QueryResult, column: &str) -> Option<i64> {
    if let Ok(value) = row.try_get::<Option<i64>>("", column) {
        return value;
    }
    if let Ok(Some(value)) = row.try_get::<Option<i32>>("", column) {
        return Some(i64::from(value));
    }
    if let Ok(Some(value)) = row.try_get::<Option<bool>>("", column) {
        return Some(i64::from(value));
    }
    text(row, column).and_then(|t| t.trim().parse().ok())
}

/// Column `column` of `row` as a boolean (`false` when absent).
pub(crate) fn boolean(row: &QueryResult, column: &str) -> bool {
    if let Ok(Some(value)) = row.try_get::<Option<bool>>("", column) {
        return value;
    }
    if let Some(number) = int(row, column) {
        return number != 0;
    }
    text(row, column).is_some_and(|t| {
        matches!(
            t.trim().to_ascii_lowercase().as_str(),
            "yes" | "y" | "true" | "t" | "1"
        )
    })
}

fn split_list(value: Option<String>) -> Vec<String> {
    value
        .map(|v| {
            v.split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Runs a query, attaching it to the error.
pub(crate) async fn rows(
    conn: &dyn SchemaConnection,
    sql: &str,
    values: Vec<Value>,
) -> SchemaResult<Vec<QueryResult>> {
    let backend = conn.backend();
    let statement = if values.is_empty() {
        Statement::from_string(backend, sql)
    } else {
        Statement::from_sql_and_values(backend, sql, values)
    };
    conn.query(statement)
        .await
        .map_err(|source| SchemaError::database(sql, source))
}

/// A string parameter.
pub(crate) fn s(value: &str) -> Value {
    Value::String(Some(value.to_owned()))
}

/// `?` or `$n` placeholders per dialect.
fn p(dialect: Dialect, n: usize) -> String {
    match dialect {
        Dialect::Postgres => format!("${n}"),
        Dialect::MySql | Dialect::Sqlite => "?".to_owned(),
    }
}

// ----- target detection --------------------------------------------------------

/// Detects the database behind `conn`: dialect, version, MariaDB, and
/// TimescaleDB (installed, or installable).
pub(crate) async fn detect(conn: &dyn SchemaConnection) -> SchemaResult<Target> {
    let Some(dialect) = Dialect::of(conn.backend()) else {
        return Err(SchemaError::missing(format!(
            "the schema builder does not know the {:?} backend",
            conn.backend()
        )));
    };
    match dialect {
        Dialect::Postgres => {
            let sql = "SELECT current_setting('server_version_num') AS version_num, \
                (SELECT extversion FROM pg_extension WHERE extname = 'timescaledb') AS installed, \
                (SELECT default_version FROM pg_available_extensions WHERE name = 'timescaledb') AS available, \
                current_setting('shared_preload_libraries') AS preload, \
                (SELECT rolsuper FROM pg_roles WHERE rolname = current_user) AS superuser";
            let found = rows(conn, sql, Vec::new()).await?;
            let row = found.first();
            let number = row
                .and_then(|r| text(r, "version_num"))
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0);
            let version = if number >= 100_000 {
                Version::new(number / 10_000, number % 10_000, 0)
            } else {
                Version::new(number / 10_000, (number / 100) % 100, number % 100)
            };
            let mut target = Target::postgres(version);
            if let Some(row) = row {
                if let Some(installed) = text(row, "installed").and_then(|v| Version::parse(&v)) {
                    target = target.with_timescaledb(installed);
                } else if let Some(available) =
                    text(row, "available").and_then(|v| Version::parse(&v))
                {
                    let preloaded = text(row, "preload").is_some_and(|p| p.contains("timescaledb"));
                    if preloaded && boolean(row, "superuser") {
                        target = target.with_timescaledb_installable(available);
                    }
                }
            }
            Ok(target)
        }
        Dialect::MySql => {
            let found = rows(conn, "SELECT VERSION() AS version", Vec::new()).await?;
            let raw = found
                .first()
                .and_then(|r| text(r, "version"))
                .unwrap_or_default();
            let version = Version::parse(&raw).unwrap_or(Version::new(8, 0, 0));
            Ok(if raw.to_ascii_lowercase().contains("mariadb") {
                Target::mariadb(version)
            } else {
                Target::mysql(version)
            })
        }
        Dialect::Sqlite => {
            let found = rows(conn, "SELECT sqlite_version() AS version", Vec::new()).await?;
            let version = found
                .first()
                .and_then(|r| text(r, "version"))
                .and_then(|v| Version::parse(&v))
                .unwrap_or(Version::new(3, 35, 0));
            Ok(Target::sqlite(version))
        }
    }
}

// ----- tables and views --------------------------------------------------------

/// Whether `table_name` exists.
pub(crate) async fn has_table(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
    table_name: &str,
) -> SchemaResult<bool> {
    let name = unqualified(table_name);
    let schema = schema_of(table_name);
    let (sql, values) = match dialect {
        Dialect::Postgres => match schema {
            Some(schema) => (
                "SELECT EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')) AS present",
                vec![s(schema), s(name)],
            ),
            None => (
                "SELECT EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = ANY (current_schemas(false)) AND c.relname = $1 AND c.relkind IN ('r', 'p')) AS present",
                vec![s(name)],
            ),
        },
        Dialect::MySql => match schema {
            Some(schema) => (
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = ? \
                 AND table_name = ? AND table_type IN ('BASE TABLE', 'SYSTEM VERSIONED')) AS present",
                vec![s(schema), s(name)],
            ),
            None => (
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = schema() \
                 AND table_name = ? AND table_type IN ('BASE TABLE', 'SYSTEM VERSIONED')) AS present",
                vec![s(name)],
            ),
        },
        Dialect::Sqlite => {
            let master = sqlite_master(schema);
            let sql = format!(
                "SELECT EXISTS (SELECT 1 FROM {master} WHERE type = 'table' AND name = ? COLLATE NOCASE) AS present"
            );
            let found = rows(conn, &sql, vec![s(name)]).await?;
            return Ok(found.first().is_some_and(|r| boolean(r, "present")));
        }
    };
    let found = rows(conn, sql, values).await?;
    Ok(found.first().is_some_and(|r| boolean(r, "present")))
}

fn sqlite_master(schema: Option<&str>) -> String {
    match schema {
        Some(schema) => format!("{}.sqlite_master", table(Dialect::Sqlite, schema)),
        None => "sqlite_master".to_owned(),
    }
}

/// The tables of the current schema(s), by name.
pub(crate) async fn tables(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
) -> SchemaResult<Vec<TableInfo>> {
    let sql = match dialect {
        Dialect::Postgres => {
            "SELECT c.relname AS name, n.nspname AS schema, pg_total_relation_size(c.oid) AS size, \
             obj_description(c.oid, 'pg_class') AS comment FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind IN ('r', 'p') AND n.nspname = ANY (current_schemas(false)) ORDER BY n.nspname, c.relname"
        }
        Dialect::MySql => {
            "SELECT CAST(table_name AS CHAR) AS name, CAST(table_schema AS CHAR) AS `schema`, \
             CAST(data_length + index_length AS SIGNED) AS size, CAST(table_comment AS CHAR) AS comment, \
             CAST(engine AS CHAR) AS engine, CAST(table_collation AS CHAR) AS collation \
             FROM information_schema.tables WHERE table_type IN ('BASE TABLE', 'SYSTEM VERSIONED') \
             AND table_schema = schema() ORDER BY table_name"
        }
        Dialect::Sqlite => {
            "SELECT name, 'main' AS schema FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name"
        }
    };
    Ok(rows(conn, sql, Vec::new())
        .await?
        .iter()
        .map(|r| TableInfo {
            name: text(r, "name").unwrap_or_default(),
            schema: text(r, "schema"),
            size: int(r, "size"),
            comment: text(r, "comment").filter(|c| !c.is_empty()),
            engine: text(r, "engine"),
            collation: text(r, "collation"),
        })
        .collect())
}

/// The views of the current schema(s), by name.
pub(crate) async fn views(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
) -> SchemaResult<Vec<ViewInfo>> {
    let sql = match dialect {
        Dialect::Postgres => {
            "SELECT viewname AS name, schemaname AS schema, definition FROM pg_views \
             WHERE schemaname = ANY (current_schemas(false)) ORDER BY schemaname, viewname"
        }
        Dialect::MySql => {
            "SELECT CAST(table_name AS CHAR) AS name, CAST(table_schema AS CHAR) AS `schema`, \
             CAST(view_definition AS CHAR) AS definition FROM information_schema.views \
             WHERE table_schema = schema() ORDER BY table_name"
        }
        Dialect::Sqlite => {
            "SELECT name, 'main' AS schema, sql AS definition FROM sqlite_master WHERE type = 'view' ORDER BY name"
        }
    };
    Ok(rows(conn, sql, Vec::new())
        .await?
        .iter()
        .map(|r| ViewInfo {
            name: text(r, "name").unwrap_or_default(),
            schema: text(r, "schema"),
            definition: text(r, "definition"),
        })
        .collect())
}

/// The user-defined types of the current schema(s) (PostgreSQL), with
/// their kind (`e` enum, `d` domain, `c` composite, `r` range ...).
pub(crate) async fn postgres_types(
    conn: &dyn SchemaConnection,
) -> SchemaResult<Vec<(String, String)>> {
    let sql = "SELECT t.typname AS name, n.nspname AS schema, t.typtype AS kind \
        FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
        LEFT JOIN pg_class c ON c.oid = t.typrelid \
        WHERE (t.typrelid = 0 OR c.relkind = 'c') AND t.typtype IN ('e', 'd', 'c', 'r', 'm') \
        AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = t.oid AND d.deptype = 'e') \
        AND n.nspname = ANY (current_schemas(false)) ORDER BY t.typname";
    Ok(rows(conn, sql, Vec::new())
        .await?
        .iter()
        .map(|r| {
            let name = text(r, "name").unwrap_or_default();
            let schema = text(r, "schema").unwrap_or_default();
            (
                format!("{schema}.{name}"),
                text(r, "kind").unwrap_or_default(),
            )
        })
        .collect())
}

// ----- columns ---------------------------------------------------------------

/// The columns of `table_name`, in table order.
pub(crate) async fn columns(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
    table_name: &str,
) -> SchemaResult<Vec<ColumnInfo>> {
    let name = unqualified(table_name);
    let schema = schema_of(table_name);
    match dialect {
        Dialect::Postgres => {
            let schema_clause = match schema {
                Some(_) => format!("n.nspname = {}", p(dialect, 2)),
                None => "n.nspname = ANY (current_schemas(false))".to_owned(),
            };
            let sql = format!(
                "SELECT a.attname AS name, t.typname AS type_name, format_type(a.atttypid, a.atttypmod) AS full_type, \
                 (SELECT tc.collcollate FROM pg_catalog.pg_collation tc WHERE tc.oid = a.attcollation) AS collation, \
                 NOT a.attnotnull AS nullable, pg_get_expr(d.adbin, d.adrelid) AS column_default, \
                 a.attgenerated::text AS generated, a.attidentity::text AS identity, \
                 col_description(c.oid, a.attnum) AS comment \
                 FROM pg_attribute a JOIN pg_class c ON a.attrelid = c.oid JOIN pg_type t ON a.atttypid = t.oid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 WHERE c.relname = {} AND {schema_clause} AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                p(dialect, 1)
            );
            let mut values = vec![s(name)];
            if let Some(schema) = schema {
                values.push(s(schema));
            }
            Ok(rows(conn, &sql, values)
                .await?
                .iter()
                .map(|r| {
                    let default = text(r, "column_default");
                    let generated = text(r, "generated").filter(|g| !g.is_empty());
                    let identity = text(r, "identity").filter(|g| !g.is_empty());
                    ColumnInfo {
                        name: text(r, "name").unwrap_or_default(),
                        type_name: text(r, "type_name").unwrap_or_default(),
                        full_type: text(r, "full_type").unwrap_or_default(),
                        collation: text(r, "collation"),
                        nullable: boolean(r, "nullable"),
                        auto_increment: identity.is_some()
                            || default
                                .as_deref()
                                .is_some_and(|d| d.starts_with("nextval(")),
                        default: if generated.is_some() {
                            None
                        } else {
                            default.clone()
                        },
                        comment: text(r, "comment"),
                        generation: generated.map(|g| Generation {
                            stored: g == "s",
                            expression: default,
                        }),
                    }
                })
                .collect())
        }
        Dialect::MySql => {
            let schema_clause = match schema {
                Some(_) => "table_schema = ?",
                None => "table_schema = schema()",
            };
            let sql = format!(
                "SELECT CAST(column_name AS CHAR) AS name, CAST(data_type AS CHAR) AS type_name, \
                 CAST(column_type AS CHAR) AS full_type, CAST(collation_name AS CHAR) AS collation, \
                 CAST(is_nullable AS CHAR) AS nullable, CAST(column_default AS CHAR) AS column_default, \
                 CAST(column_comment AS CHAR) AS comment, CAST(generation_expression AS CHAR) AS expression, \
                 CAST(extra AS CHAR) AS extra FROM information_schema.columns \
                 WHERE {schema_clause} AND table_name = ? ORDER BY ordinal_position"
            );
            let mut values = Vec::new();
            if let Some(schema) = schema {
                values.push(s(schema));
            }
            values.push(s(name));
            Ok(rows(conn, &sql, values)
                .await?
                .iter()
                .map(|r| {
                    let extra = text(r, "extra").unwrap_or_default().to_ascii_lowercase();
                    let expression = text(r, "expression").filter(|e| !e.is_empty());
                    ColumnInfo {
                        name: text(r, "name").unwrap_or_default(),
                        type_name: text(r, "type_name").unwrap_or_default(),
                        full_type: text(r, "full_type").unwrap_or_default(),
                        collation: text(r, "collation"),
                        nullable: boolean(r, "nullable"),
                        default: text(r, "column_default"),
                        auto_increment: extra.contains("auto_increment"),
                        comment: text(r, "comment").filter(|c| !c.is_empty()),
                        generation: expression.map(|e| Generation {
                            stored: extra.contains("stored"),
                            expression: Some(e),
                        }),
                    }
                })
                .collect())
        }
        Dialect::Sqlite => {
            let sql = format!(
                "SELECT name, type, \"notnull\" AS not_null, dflt_value, pk, hidden FROM pragma_table_xinfo(?{}) ORDER BY cid",
                if schema.is_some() { ", ?" } else { "" }
            );
            let mut values = vec![s(name)];
            if let Some(schema) = schema {
                values.push(s(schema));
            }
            let found = rows(conn, &sql, values).await?;
            let create = sqlite_create_sql(conn, table_name)
                .await?
                .unwrap_or_default();
            let parsed = TableState::from_create_sql(&create);
            let primary_count = found
                .iter()
                .filter(|r| int(r, "pk").unwrap_or(0) > 0)
                .count();
            Ok(found
                .iter()
                .map(|r| {
                    let name = text(r, "name").unwrap_or_default();
                    let full_type = text(r, "type").unwrap_or_default();
                    let hidden = int(r, "hidden").unwrap_or(0);
                    let state = parsed.column(&name);
                    let definition = state.map(|c| c.definition.clone()).unwrap_or_default();
                    ColumnInfo {
                        type_name: full_type
                            .split('(')
                            .next()
                            .unwrap_or_default()
                            .trim()
                            .to_ascii_lowercase(),
                        collation: sqlite_collation(&definition),
                        nullable: int(r, "not_null").unwrap_or(0) == 0,
                        default: text(r, "dflt_value"),
                        auto_increment: primary_count == 1
                            && int(r, "pk").unwrap_or(0) == 1
                            && full_type.eq_ignore_ascii_case("integer"),
                        comment: None,
                        generation: (hidden == 2 || hidden == 3).then(|| Generation {
                            stored: hidden == 3,
                            expression: sqlite_generated_expression(&definition),
                        }),
                        full_type: full_type.to_ascii_lowercase(),
                        name,
                    }
                })
                .collect())
        }
    }
}

fn sqlite_collation(definition: &str) -> Option<String> {
    use crate::rebuild::{TokenKind, identifier, tokenize};
    let tokens = tokenize(definition);
    tokens.iter().enumerate().find_map(|(i, t)| {
        (t.kind == TokenKind::Word && definition[t.start..t.end].eq_ignore_ascii_case("COLLATE"))
            .then(|| tokens.get(i + 1))
            .flatten()
            .and_then(|next| match next.kind {
                TokenKind::String => Some(
                    definition[next.start + 1..next.end - 1]
                        .replace("''", "'")
                        .to_ascii_lowercase(),
                ),
                _ => identifier(definition, next).map(|s| s.to_ascii_lowercase()),
            })
    })
}

fn sqlite_generated_expression(definition: &str) -> Option<String> {
    use crate::rebuild::{TokenKind, tokenize};
    let tokens = tokenize(definition);
    let at = tokens.iter().position(|t| {
        t.kind == TokenKind::Word && definition[t.start..t.end].eq_ignore_ascii_case("AS")
    })?;
    let open = tokens.get(at + 1).filter(|t| t.kind == TokenKind::Open)?;
    let mut depth = 0;
    for token in &tokens[at + 1..] {
        match token.kind {
            TokenKind::Open => depth += 1,
            TokenKind::Close => {
                depth -= 1;
                if depth == 0 {
                    return Some(definition[open.end..token.start].trim().to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

async fn sqlite_create_sql(
    conn: &dyn SchemaConnection,
    table_name: &str,
) -> SchemaResult<Option<String>> {
    let master = sqlite_master(schema_of(table_name));
    let sql = format!("SELECT sql FROM {master} WHERE type = 'table' AND name = ? COLLATE NOCASE");
    let found = rows(conn, &sql, vec![s(unqualified(table_name))]).await?;
    Ok(found.first().and_then(|r| text(r, "sql")))
}

// ----- indexes ---------------------------------------------------------------

/// The indexes of `table_name`, the primary key included.
pub(crate) async fn indexes(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
    table_name: &str,
) -> SchemaResult<Vec<IndexInfo>> {
    let name = unqualified(table_name);
    let schema = schema_of(table_name);
    match dialect {
        Dialect::Postgres => {
            let schema_clause = match schema {
                Some(_) => "tn.nspname = $2".to_owned(),
                None => "tn.nspname = ANY (current_schemas(false))".to_owned(),
            };
            let sql = format!(
                "SELECT ic.relname AS name, string_agg(a.attname, ',' ORDER BY indseq.ord) AS columns, \
                 am.amname AS kind, i.indisunique AS is_unique, i.indisprimary AS is_primary \
                 FROM pg_index i JOIN pg_class tc ON tc.oid = i.indrelid \
                 JOIN pg_namespace tn ON tn.oid = tc.relnamespace JOIN pg_class ic ON ic.oid = i.indexrelid \
                 JOIN pg_am am ON am.oid = ic.relam \
                 JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS indseq(num, ord) ON TRUE \
                 LEFT JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = indseq.num \
                 WHERE tc.relname = $1 AND {schema_clause} \
                 GROUP BY ic.relname, am.amname, i.indisunique, i.indisprimary ORDER BY ic.relname"
            );
            let mut values = vec![s(name)];
            if let Some(schema) = schema {
                values.push(s(schema));
            }
            Ok(rows(conn, &sql, values)
                .await?
                .iter()
                .map(|r| IndexInfo {
                    name: text(r, "name").unwrap_or_default().to_lowercase(),
                    columns: split_list(text(r, "columns")),
                    kind: text(r, "kind").map(|k| k.to_lowercase()),
                    unique: boolean(r, "is_unique"),
                    primary: boolean(r, "is_primary"),
                })
                .collect())
        }
        Dialect::MySql => {
            let schema_clause = match schema {
                Some(_) => "table_schema = ?",
                None => "table_schema = schema()",
            };
            let sql = format!(
                "SELECT CAST(index_name AS CHAR) AS name, \
                 CAST(group_concat(column_name ORDER BY seq_in_index) AS CHAR) AS columns, \
                 CAST(index_type AS CHAR) AS kind, NOT non_unique AS is_unique \
                 FROM information_schema.statistics WHERE {schema_clause} AND table_name = ? \
                 GROUP BY index_name, index_type, non_unique ORDER BY index_name"
            );
            let mut values = Vec::new();
            if let Some(schema) = schema {
                values.push(s(schema));
            }
            values.push(s(name));
            Ok(rows(conn, &sql, values)
                .await?
                .iter()
                .map(|r| {
                    let name = text(r, "name").unwrap_or_default().to_lowercase();
                    IndexInfo {
                        primary: name == "primary",
                        columns: split_list(text(r, "columns")),
                        kind: text(r, "kind").map(|k| k.to_lowercase()),
                        unique: boolean(r, "is_unique"),
                        name,
                    }
                })
                .collect())
        }
        Dialect::Sqlite => {
            let state = sqlite_indexes(conn, table_name).await?;
            let columns = self::columns(conn, dialect, table_name).await?;
            let mut primary: Vec<(i64, String)> = Vec::new();
            let found = rows(
                conn,
                "SELECT name, pk FROM pragma_table_xinfo(?) WHERE pk > 0",
                vec![s(name)],
            )
            .await?;
            for row in &found {
                primary.push((
                    int(row, "pk").unwrap_or(0),
                    text(row, "name").unwrap_or_default(),
                ));
            }
            primary.sort();
            let _ = columns;
            let mut result = Vec::new();
            if !primary.is_empty() {
                result.push(IndexInfo {
                    name: "primary".to_owned(),
                    columns: primary.into_iter().map(|(_, c)| c).collect(),
                    kind: None,
                    unique: true,
                    primary: true,
                });
            }
            for (index, origin) in state {
                if origin == "pk" {
                    continue;
                }
                result.push(IndexInfo {
                    name: index.name.to_lowercase(),
                    columns: index.columns.into_iter().flatten().collect(),
                    kind: None,
                    unique: index.unique,
                    primary: false,
                });
            }
            Ok(result)
        }
    }
}

/// SQLite's indexes of `table_name` with their origin (`c` created, `u`
/// unique constraint, `pk` primary key).
async fn sqlite_indexes(
    conn: &dyn SchemaConnection,
    table_name: &str,
) -> SchemaResult<Vec<(StateIndex, String)>> {
    let name = unqualified(table_name);
    let master = sqlite_master(schema_of(table_name));
    let sql = format!(
        "SELECT il.name AS name, il.\"unique\" AS is_unique, il.origin AS origin, m.sql AS sql \
         FROM pragma_index_list(?) il LEFT JOIN {master} m ON m.type = 'index' AND m.name = il.name ORDER BY il.seq"
    );
    let list = rows(conn, &sql, vec![s(name)]).await?;
    let mut indexes = Vec::new();
    for row in list {
        let index_name = text(&row, "name").unwrap_or_default();
        let info = rows(
            conn,
            "SELECT name FROM pragma_index_info(?) ORDER BY seqno",
            vec![s(&index_name)],
        )
        .await?;
        let columns = info.iter().map(|r| text(r, "name")).collect();
        indexes.push((
            StateIndex {
                name: index_name,
                unique: boolean(&row, "is_unique"),
                columns,
                sql: text(&row, "sql"),
            },
            text(&row, "origin").unwrap_or_default(),
        ));
    }
    Ok(indexes)
}

// ----- foreign keys ------------------------------------------------------------

/// The foreign keys of `table_name`.
pub(crate) async fn foreign_keys(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
    table_name: &str,
) -> SchemaResult<Vec<ForeignKeyInfo>> {
    let name = unqualified(table_name);
    let schema = schema_of(table_name);
    match dialect {
        Dialect::Postgres => {
            let schema_clause = match schema {
                Some(_) => "tn.nspname = $2".to_owned(),
                None => "tn.nspname = ANY (current_schemas(false))".to_owned(),
            };
            let sql = format!(
                "SELECT c.conname AS name, string_agg(la.attname, ',' ORDER BY conseq.ord) AS columns, \
                 fn.nspname AS foreign_schema, fc.relname AS foreign_table, \
                 string_agg(fa.attname, ',' ORDER BY conseq.ord) AS foreign_columns, \
                 c.confupdtype::text AS on_update, c.confdeltype::text AS on_delete \
                 FROM pg_constraint c JOIN pg_class tc ON c.conrelid = tc.oid \
                 JOIN pg_namespace tn ON tn.oid = tc.relnamespace JOIN pg_class fc ON c.confrelid = fc.oid \
                 JOIN pg_namespace fn ON fn.oid = fc.relnamespace \
                 JOIN LATERAL unnest(c.conkey) WITH ORDINALITY AS conseq(num, ord) ON TRUE \
                 JOIN pg_attribute la ON la.attrelid = c.conrelid AND la.attnum = conseq.num \
                 JOIN pg_attribute fa ON fa.attrelid = c.confrelid AND fa.attnum = c.confkey[conseq.ord] \
                 WHERE c.contype = 'f' AND tc.relname = $1 AND {schema_clause} \
                 GROUP BY c.conname, fn.nspname, fc.relname, c.confupdtype, c.confdeltype ORDER BY c.conname"
            );
            let mut values = vec![s(name)];
            if let Some(schema) = schema {
                values.push(s(schema));
            }
            Ok(rows(conn, &sql, values)
                .await?
                .iter()
                .map(|r| ForeignKeyInfo {
                    name: text(r, "name"),
                    columns: split_list(text(r, "columns")),
                    foreign_schema: text(r, "foreign_schema"),
                    foreign_table: text(r, "foreign_table").unwrap_or_default(),
                    foreign_columns: split_list(text(r, "foreign_columns")),
                    on_update: text(r, "on_update").and_then(|a| ReferentialAction::parse(&a)),
                    on_delete: text(r, "on_delete").and_then(|a| ReferentialAction::parse(&a)),
                })
                .collect())
        }
        Dialect::MySql => {
            let schema_expr = if schema.is_some() { "?" } else { "schema()" };
            let sql = format!(
                "SELECT CAST(kc.constraint_name AS CHAR) AS name, \
                 CAST(group_concat(kc.column_name ORDER BY kc.ordinal_position) AS CHAR) AS columns, \
                 CAST(kc.referenced_table_schema AS CHAR) AS foreign_schema, \
                 CAST(kc.referenced_table_name AS CHAR) AS foreign_table, \
                 CAST(group_concat(kc.referenced_column_name ORDER BY kc.ordinal_position) AS CHAR) AS foreign_columns, \
                 CAST(rc.update_rule AS CHAR) AS on_update, CAST(rc.delete_rule AS CHAR) AS on_delete \
                 FROM information_schema.key_column_usage kc JOIN information_schema.referential_constraints rc \
                 ON kc.constraint_schema = rc.constraint_schema AND kc.constraint_name = rc.constraint_name \
                 WHERE kc.table_schema = {schema_expr} AND kc.table_name = ? AND kc.referenced_table_name IS NOT NULL \
                 GROUP BY kc.constraint_name, kc.referenced_table_schema, kc.referenced_table_name, rc.update_rule, rc.delete_rule \
                 ORDER BY kc.constraint_name"
            );
            let mut values = Vec::new();
            if let Some(schema) = schema {
                values.push(s(schema));
            }
            values.push(s(name));
            Ok(rows(conn, &sql, values)
                .await?
                .iter()
                .map(|r| ForeignKeyInfo {
                    name: text(r, "name"),
                    columns: split_list(text(r, "columns")),
                    foreign_schema: text(r, "foreign_schema"),
                    foreign_table: text(r, "foreign_table").unwrap_or_default(),
                    foreign_columns: split_list(text(r, "foreign_columns")),
                    on_update: text(r, "on_update").and_then(|a| ReferentialAction::parse(&a)),
                    on_delete: text(r, "on_delete").and_then(|a| ReferentialAction::parse(&a)),
                })
                .collect())
        }
        Dialect::Sqlite => {
            let grouped = sqlite_foreign_keys(conn, table_name).await?;
            Ok(grouped
                .into_iter()
                .map(|f| ForeignKeyInfo {
                    name: None,
                    columns: f.columns,
                    foreign_schema: Some("main".to_owned()),
                    foreign_table: f.table,
                    foreign_columns: f.references,
                    on_update: f.on_update,
                    on_delete: f.on_delete,
                })
                .collect())
        }
    }
}

async fn sqlite_foreign_keys(
    conn: &dyn SchemaConnection,
    table_name: &str,
) -> SchemaResult<Vec<StateForeign>> {
    let found = rows(
        conn,
        "SELECT id, seq, \"table\" AS foreign_table, \"from\" AS column_from, \"to\" AS column_to, \
         on_update, on_delete FROM pragma_foreign_key_list(?) ORDER BY id, seq",
        vec![s(unqualified(table_name))],
    )
    .await?;
    let mut by_id: BTreeMap<i64, StateForeign> = BTreeMap::new();
    for row in &found {
        let id = int(row, "id").unwrap_or(0);
        let entry = by_id.entry(id).or_insert_with(|| StateForeign {
            columns: Vec::new(),
            table: text(row, "foreign_table").unwrap_or_default(),
            references: Vec::new(),
            on_update: text(row, "on_update").and_then(|a| ReferentialAction::parse(&a)),
            on_delete: text(row, "on_delete").and_then(|a| ReferentialAction::parse(&a)),
        });
        entry
            .columns
            .push(text(row, "column_from").unwrap_or_default());
        if let Some(to) = text(row, "column_to") {
            entry.references.push(to);
        }
    }
    Ok(by_id.into_values().collect())
}

// ----- states ----------------------------------------------------------------

/// Everything a SQLite rebuild of `table_name` needs.
pub(crate) async fn sqlite_table_state(
    conn: &dyn SchemaConnection,
    table_name: &str,
) -> SchemaResult<TableState> {
    let Some(create) = sqlite_create_sql(conn, table_name).await? else {
        return Err(SchemaError::missing(format!(
            "table `{table_name}` does not exist"
        )));
    };
    let mut state = TableState::from_create_sql(&create);
    // Columns the parser could not read keep a definition from the pragma.
    let xinfo = rows(
        conn,
        "SELECT name, type, \"notnull\" AS not_null, dflt_value, pk, hidden FROM pragma_table_xinfo(?) ORDER BY cid",
        vec![s(unqualified(table_name))],
    )
    .await?;
    let mut columns = Vec::new();
    let mut primary: Vec<(i64, String)> = Vec::new();
    for row in &xinfo {
        let name = text(row, "name").unwrap_or_default();
        let pk = int(row, "pk").unwrap_or(0);
        if pk > 0 {
            primary.push((pk, name.clone()));
        }
        let hidden = int(row, "hidden").unwrap_or(0);
        if hidden == 1 {
            continue;
        }
        match state.column(&name) {
            Some(parsed) => columns.push(parsed.clone()),
            None => {
                let mut definition = text(row, "type").unwrap_or_default();
                if int(row, "not_null").unwrap_or(0) != 0 {
                    definition.push_str(" NOT NULL");
                }
                if let Some(default) = text(row, "dflt_value") {
                    definition.push_str(&format!(" DEFAULT ({default})"));
                }
                columns.push(StateColumn {
                    name,
                    definition: definition.trim().to_owned(),
                    generated: hidden == 2 || hidden == 3,
                    unique: false,
                });
            }
        }
    }
    state.columns = columns;
    if !primary.is_empty() {
        primary.sort();
        state.primary = primary.into_iter().map(|(_, c)| c).collect();
    }
    state.foreign_keys = sqlite_foreign_keys(conn, table_name).await?;
    state.indexes = sqlite_indexes(conn, table_name)
        .await?
        .into_iter()
        .filter(|(_, origin)| origin != "pk")
        .map(|(index, _)| index)
        .collect();
    let master = sqlite_master(schema_of(table_name));
    let triggers = rows(
        conn,
        &format!("SELECT sql FROM {master} WHERE type = 'trigger' AND tbl_name = ? COLLATE NOCASE"),
        vec![s(unqualified(table_name))],
    )
    .await?;
    state.triggers = triggers.iter().filter_map(|r| text(r, "sql")).collect();
    let enabled = rows(conn, "PRAGMA foreign_keys", Vec::new()).await?;
    state.foreign_keys_enabled = enabled.first().is_some_and(|r| boolean(r, "foreign_keys"));
    let referencing = rows(
        conn,
        "SELECT DISTINCT m.name AS name FROM sqlite_master m, pragma_foreign_key_list(m.name) f \
         WHERE m.type = 'table' AND f.\"table\" = ? COLLATE NOCASE AND m.name <> ? COLLATE NOCASE",
        vec![s(unqualified(table_name)), s(unqualified(table_name))],
    )
    .await?;
    state.referenced_by = referencing.iter().filter_map(|r| text(r, "name")).collect();
    state.has_rows = has_rows(conn, Dialect::Sqlite, table_name).await?;
    Ok(state)
}

/// Whether `table_name` has any rows.
pub(crate) async fn has_rows(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
    table_name: &str,
) -> SchemaResult<bool> {
    let sql = format!(
        "SELECT EXISTS (SELECT 1 FROM {}) AS present",
        table(dialect, table_name)
    );
    let found = rows(conn, &sql, Vec::new()).await?;
    Ok(found.first().is_some_and(|r| boolean(r, "present")))
}

/// The keys of `table_name`, read before converting it to a hypertable.
pub(crate) async fn key_state(
    conn: &dyn SchemaConnection,
    dialect: Dialect,
    table_name: &str,
) -> SchemaResult<KeyState> {
    let columns = columns(conn, dialect, table_name).await?;
    if columns.is_empty() {
        return Err(SchemaError::missing(format!(
            "table `{table_name}` does not exist"
        )));
    }
    let indexes = indexes(conn, dialect, table_name).await?;
    let mut state = KeyState {
        columns: columns
            .iter()
            .map(|c| (c.name.clone(), c.type_name.clone()))
            .collect(),
        ..KeyState::default()
    };
    for index in &indexes {
        state.indexes.push(index.columns.clone());
        if index.primary {
            state.primary = Some((index.name.clone(), index.columns.clone()));
        } else if index.unique {
            state
                .uniques
                .push((index.name.clone(), index.columns.clone()));
        }
    }
    let name = unqualified(table_name);
    state.referenced_by = match dialect {
        Dialect::Postgres => rows(
            conn,
            "SELECT c.conname AS name, cl.relname AS referencing FROM pg_constraint c \
             JOIN pg_class cl ON cl.oid = c.conrelid JOIN pg_class f ON f.oid = c.confrelid \
             JOIN pg_namespace n ON n.oid = f.relnamespace \
             WHERE c.contype = 'f' AND f.relname = $1 AND n.nspname = ANY (current_schemas(false)) AND c.conrelid <> c.confrelid",
            vec![s(name)],
        )
        .await?
        .iter()
        .map(|r| {
            (
                text(r, "name").unwrap_or_default(),
                text(r, "referencing").unwrap_or_default(),
            )
        })
        .collect(),
        Dialect::MySql => rows(
            conn,
            "SELECT CAST(constraint_name AS CHAR) AS name, CAST(table_name AS CHAR) AS referencing \
             FROM information_schema.referential_constraints \
             WHERE constraint_schema = schema() AND referenced_table_name = ? AND table_name <> ?",
            vec![s(name), s(name)],
        )
        .await?
        .iter()
        .map(|r| {
            (
                text(r, "name").unwrap_or_default(),
                text(r, "referencing").unwrap_or_default(),
            )
        })
        .collect(),
        Dialect::Sqlite => rows(
            conn,
            "SELECT DISTINCT m.name AS referencing FROM sqlite_master m, pragma_foreign_key_list(m.name) f \
             WHERE m.type = 'table' AND f.\"table\" = ? COLLATE NOCASE AND m.name <> ? COLLATE NOCASE",
            vec![s(name), s(name)],
        )
        .await?
        .iter()
        .map(|r| (String::new(), text(r, "referencing").unwrap_or_default()))
        .collect(),
    };
    state.has_rows = has_rows(conn, dialect, table_name).await?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sqlite_column_details_from_definitions() {
        assert_eq!(
            sqlite_collation("VARCHAR NOT NULL COLLATE NOCASE").as_deref(),
            Some("nocase")
        );
        assert_eq!(
            sqlite_collation("VARCHAR COLLATE 'BINARY'").as_deref(),
            Some("binary")
        );
        assert_eq!(sqlite_collation("VARCHAR NOT NULL"), None);
        assert_eq!(
            sqlite_generated_expression("INTEGER AS (price * (1 + tax)) STORED").as_deref(),
            Some("price * (1 + tax)")
        );
        assert_eq!(sqlite_generated_expression("INTEGER NOT NULL"), None);
    }
}
