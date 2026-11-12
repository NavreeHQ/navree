//! `describe_schema`: the database as it is.

use serde::Serialize;
use sqlx::postgres::PgConnection;
use sqlx::sqlite::SqliteConnection;
use sqlx::{AssertSqlSafe, Executor, Row};

use super::DatabaseError;

/// The table recording applied migrations (navree-db's `MIGRATIONS_TABLE`).
pub const MIGRATIONS_TABLE: &str = "navree_migrations";

/// The answer of `describe_schema`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Schema {
    /// The tables, by name.
    pub tables: Vec<Table>,
    /// The rows of `navree_migrations`, in the order they ran (empty when
    /// the table doesn't exist yet).
    pub applied_migrations: Vec<AppliedMigration>,
}

/// One table.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Table {
    /// The table name.
    pub name: String,
    /// Its columns, in order.
    pub columns: Vec<Column>,
    /// Its indexes (the primary key's included on PostgreSQL).
    pub indexes: Vec<Index>,
    /// Its foreign keys.
    pub foreign_keys: Vec<ForeignKey>,
}

/// One column.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Column {
    /// The column name.
    pub name: String,
    /// The declared type, as the database reports it.
    #[serde(rename = "type")]
    pub data_type: String,
    /// Whether it accepts NULL.
    pub nullable: bool,
    /// The default expression, if any.
    pub default: Option<String>,
    /// Whether it is (part of) the primary key.
    pub primary_key: bool,
}

/// One index.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Index {
    /// The index name.
    pub name: String,
    /// The indexed columns (or expressions), in order.
    pub columns: Vec<String>,
    /// Whether it is unique.
    pub unique: bool,
}

/// One foreign key.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ForeignKey {
    /// The referencing columns.
    pub columns: Vec<String>,
    /// The referenced table.
    pub references_table: String,
    /// The referenced columns.
    pub references_columns: Vec<String>,
    /// `ON DELETE` action.
    pub on_delete: String,
    /// `ON UPDATE` action.
    pub on_update: String,
}

/// One row of `navree_migrations`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AppliedMigration {
    /// The module that owns it (`crm`).
    pub module: String,
    /// The migration name.
    pub name: String,
    /// The batch it ran in.
    pub batch: i64,
    /// When it ran, as stored.
    pub applied_at: Option<String>,
}

impl AppliedMigration {
    /// `module/name`, as `migrate:status` prints it.
    pub fn label(&self) -> String {
        format!("{}/{}", self.module, self.name)
    }
}

/// `"name"` with embedded quotes doubled.
fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// `E'text'`: a PostgreSQL escape string with backslashes and quotes
/// doubled. An escape string reads a backslash as an escape whatever
/// `standard_conforming_strings` is set to, so no text can close it early.
fn literal(text: &str) -> String {
    format!("E'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

pub(super) async fn sqlite(
    conn: &mut SqliteConnection,
    only: Option<&str>,
) -> Result<Schema, DatabaseError> {
    let names: Vec<String> = conn
        .fetch_all(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .await?
        .iter()
        .map(|row| row.get::<String, _>(0))
        .collect();
    let mut schema = Schema::default();
    for name in names
        .iter()
        .filter(|name| only.is_none_or(|only| only.eq_ignore_ascii_case(name)))
    {
        let mut table = Table {
            name: name.clone(),
            ..Table::default()
        };
        for row in conn
            .fetch_all(AssertSqlSafe(format!("PRAGMA table_info({})", quote(name))))
            .await?
        {
            table.columns.push(Column {
                name: row.get("name"),
                data_type: row.get("type"),
                nullable: row.get::<i64, _>("notnull") == 0 && row.get::<i64, _>("pk") == 0,
                default: row.get("dflt_value"),
                primary_key: row.get::<i64, _>("pk") > 0,
            });
        }
        for row in conn
            .fetch_all(AssertSqlSafe(format!("PRAGMA index_list({})", quote(name))))
            .await?
        {
            let index: String = row.get("name");
            let columns = conn
                .fetch_all(AssertSqlSafe(format!(
                    "PRAGMA index_info({})",
                    quote(&index)
                )))
                .await?
                .iter()
                .map(|r| {
                    r.try_get::<Option<String>, _>("name")
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| "<expression>".to_owned())
                })
                .collect();
            table.indexes.push(Index {
                name: index,
                columns,
                unique: row.get::<i64, _>("unique") != 0,
            });
        }
        let mut keys: Vec<(i64, ForeignKey)> = Vec::new();
        for row in conn
            .fetch_all(AssertSqlSafe(format!(
                "PRAGMA foreign_key_list({})",
                quote(name)
            )))
            .await?
        {
            let id: i64 = row.get("id");
            let from: String = row.get("from");
            let to: Option<String> = row.get("to");
            match keys.iter_mut().find(|(key, _)| *key == id) {
                Some((_, key)) => {
                    key.columns.push(from);
                    key.references_columns.extend(to);
                }
                None => keys.push((
                    id,
                    ForeignKey {
                        columns: vec![from],
                        references_table: row.get("table"),
                        references_columns: to.into_iter().collect(),
                        on_delete: row.get("on_delete"),
                        on_update: row.get("on_update"),
                    },
                )),
            }
        }
        table.foreign_keys = keys.into_iter().map(|(_, key)| key).collect();
        schema.tables.push(table);
    }
    if names.iter().any(|n| n == MIGRATIONS_TABLE) {
        schema.applied_migrations = conn
            .fetch_all(AssertSqlSafe(format!(
                "SELECT module, name, batch, CAST(applied_at AS TEXT) AS applied_at FROM {MIGRATIONS_TABLE} ORDER BY batch, rowid"
            )))
            .await?
            .iter()
            .map(|row| AppliedMigration {
                module: row.get("module"),
                name: row.get("name"),
                batch: row.get("batch"),
                applied_at: row.get("applied_at"),
            })
            .collect();
    }
    Ok(schema)
}

pub(super) async fn postgres(
    conn: &mut PgConnection,
    only: Option<&str>,
) -> Result<Schema, DatabaseError> {
    // Text results (simple protocol), so every catalog type reads as a string.
    let text = |row: &sqlx::postgres::PgRow, i: usize| -> Option<String> {
        row.try_get_raw(i)
            .ok()
            .filter(|raw| !sqlx::ValueRef::is_null(raw))
            .and_then(|raw| raw.as_str().ok().map(str::to_owned))
    };
    // Every table, then the one asked for picked here: the name asked for never reaches the SQL.
    let tables: Vec<String> = conn
        .fetch_all(
            "SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = current_schema() AND c.relkind IN ('r', 'p') ORDER BY c.relname",
        )
        .await?
        .iter()
        .filter_map(|row| text(row, 0))
        .filter(|name| only.is_none_or(|only| only == name.as_str()))
        .collect();
    let mut schema = Schema::default();
    for name in &tables {
        let mut table = Table {
            name: name.clone(),
            ..Table::default()
        };
        // In this query and the three after it, `name` is a table name read from pg_class above,
        // written by `literal` as an escape string: the simple protocol these text results need
        // binds no parameters.
        // nosemgrep: navree-sql-built-from-strings
        let primary: Vec<String> = conn
            .fetch_all(AssertSqlSafe(format!(
                "SELECT a.attname FROM pg_index i \
                 JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
                 WHERE i.indrelid = (quote_ident(current_schema()) || '.' || quote_ident({}))::regclass AND i.indisprimary",
                literal(name)
            )))
            .await?
            .iter()
            .filter_map(|row| text(row, 0))
            .collect();
        // A catalog table name through `literal`, as above.
        // nosemgrep: navree-sql-built-from-strings
        for row in conn
            .fetch_all(AssertSqlSafe(format!(
                "SELECT column_name, data_type, is_nullable, column_default FROM information_schema.columns \
                 WHERE table_schema = current_schema() AND table_name = {} ORDER BY ordinal_position",
                literal(name)
            )))
            .await?
        {
            let column = text(&row, 0).unwrap_or_default();
            table.columns.push(Column {
                primary_key: primary.contains(&column),
                name: column,
                data_type: text(&row, 1).unwrap_or_default(),
                nullable: text(&row, 2).as_deref() == Some("YES"),
                default: text(&row, 3),
            });
        }
        // A catalog table name through `literal`, as above.
        // nosemgrep: navree-sql-built-from-strings
        for row in conn
            .fetch_all(AssertSqlSafe(format!(
                "SELECT ic.relname, i.indisunique, \
                 array_to_string(ARRAY(SELECT pg_get_indexdef(i.indexrelid, k, true) \
                 FROM generate_series(1, i.indnatts) AS k ORDER BY k), ',') \
                 FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid \
                 WHERE i.indrelid = (quote_ident(current_schema()) || '.' || quote_ident({}))::regclass ORDER BY ic.relname",
                literal(name)
            )))
            .await?
        {
            table.indexes.push(Index {
                name: text(&row, 0).unwrap_or_default(),
                unique: text(&row, 1).as_deref() == Some("t"),
                columns: text(&row, 2)
                    .unwrap_or_default()
                    .split(',')
                    .filter(|c| !c.is_empty())
                    .map(str::to_owned)
                    .collect(),
            });
        }
        // A catalog table name through `literal`, as above.
        // nosemgrep: navree-sql-built-from-strings
        for row in conn
            .fetch_all(AssertSqlSafe(format!(
                "SELECT \
                 array_to_string(ARRAY(SELECT a.attname FROM unnest(con.conkey) WITH ORDINALITY k(n, o) \
                   JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n ORDER BY k.o), ','), \
                 ref.relname, \
                 array_to_string(ARRAY(SELECT a.attname FROM unnest(con.confkey) WITH ORDINALITY k(n, o) \
                   JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.n ORDER BY k.o), ','), \
                 con.confdeltype, con.confupdtype \
                 FROM pg_constraint con JOIN pg_class ref ON ref.oid = con.confrelid \
                 WHERE con.contype = 'f' AND con.conrelid = (quote_ident(current_schema()) || '.' || quote_ident({}))::regclass \
                 ORDER BY con.conname",
                literal(name)
            )))
            .await?
        {
            let split = |s: Option<String>| -> Vec<String> {
                s.unwrap_or_default()
                    .split(',')
                    .filter(|c| !c.is_empty())
                    .map(str::to_owned)
                    .collect()
            };
            table.foreign_keys.push(ForeignKey {
                columns: split(text(&row, 0)),
                references_table: text(&row, 1).unwrap_or_default(),
                references_columns: split(text(&row, 2)),
                on_delete: fk_action(text(&row, 3).as_deref()),
                on_update: fk_action(text(&row, 4).as_deref()),
            });
        }
        schema.tables.push(table);
    }
    let has_migrations = conn
        .fetch_all(AssertSqlSafe(format!(
            "SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = current_schema() AND c.relname = '{MIGRATIONS_TABLE}'"
        )))
        .await?;
    if !has_migrations.is_empty() {
        schema.applied_migrations = conn
            .fetch_all(AssertSqlSafe(format!(
                "SELECT module, name, batch, applied_at::text FROM {MIGRATIONS_TABLE} ORDER BY batch, applied_at, name"
            )))
            .await?
            .iter()
            .map(|row| AppliedMigration {
                module: text(row, 0).unwrap_or_default(),
                name: text(row, 1).unwrap_or_default(),
                batch: text(row, 2).and_then(|b| b.parse().ok()).unwrap_or_default(),
                applied_at: text(row, 3),
            })
            .collect();
    }
    Ok(schema)
}

/// `pg_constraint.confdeltype` / `confupdtype` as SQL words.
fn fk_action(code: Option<&str>) -> String {
    match code {
        Some("a") => "NO ACTION",
        Some("r") => "RESTRICT",
        Some("c") => "CASCADE",
        Some("n") => "SET NULL",
        Some("d") => "SET DEFAULT",
        _ => "",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::literal;

    #[test]
    fn a_literal_doubles_backslashes_and_quotes_inside_an_escape_string() {
        for (text, written) in [
            ("deals", "E'deals'"),
            (r"\", r"E'\\'"),
            ("'", "E''''"),
            (r"\'", r"E'\\'''"),
            (r"\\''", r"E'\\\\'''''"),
            (r"\' OR 1=1 --", r"E'\\'' OR 1=1 --'"),
            ("सौदे ₹", "E'सौदे ₹'"),
        ] {
            assert_eq!(literal(text), written, "{text}");
        }
    }
}
