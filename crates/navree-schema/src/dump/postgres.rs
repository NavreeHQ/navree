//! The PostgreSQL dump, read from the catalogs: every object of the schemas
//! on the search path that no extension owns. With TimescaleDB, its
//! hypertables, compression, policies and continuous aggregates are
//! re-created through TimescaleDB's own functions (its internal schemas are
//! never dumped).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};


use super::Script;
use crate::connection::{SchemaConnection, SchemaRow};
use crate::error::SchemaResult;
use crate::grammar::timescale::regclass;
use crate::grammar::{ident, string};
use crate::inspect::{boolean, int, rows, text};
use crate::target::{Dialect, Target, Version};

const D: Dialect = Dialect::Postgres;

/// Objects that belong to an extension (created by `CREATE EXTENSION`).
fn not_extension_member(catalog: &str, oid: &str) -> String {
    format!(
        "NOT EXISTS (SELECT 1 FROM pg_depend e WHERE e.classid = '{catalog}'::regclass \
         AND e.objid = {oid} AND e.deptype = 'e')"
    )
}

/// `"schema"."name"`.
fn qualified(schema: &str, name: &str) -> String {
    format!("{}.{}", ident(D, schema), ident(D, name))
}

fn field(row: &SchemaRow, column: &str) -> String {
    text(row, column).unwrap_or_default()
}

async fn query(conn: &dyn SchemaConnection, sql: &str) -> SchemaResult<Vec<SchemaRow>> {
    rows(conn, sql, Vec::new()).await
}

/// A table being dumped.
struct Table {
    oid: i64,
    schema: String,
    name: String,
    comment: Option<String>,
    rls: bool,
    force_rls: bool,
}

impl Table {
    fn qualified(&self) -> String {
        qualified(&self.schema, &self.name)
    }
}

pub(super) async fn dump(
    conn: &dyn SchemaConnection,
    target: &Target,
    script: &mut Script,
) -> SchemaResult {
    let in_path = "n.nspname = ANY (current_schemas(false))";

    // Schemas and extensions.
    let schemas: Vec<String> = query(
        conn,
        &format!(
            "SELECT n.nspname AS name FROM pg_namespace n WHERE {in_path} AND n.nspname <> 'public' \
             ORDER BY n.nspname"
        ),
    )
    .await?
    .iter()
    .map(|r| format!("CREATE SCHEMA IF NOT EXISTS {}", ident(D, &field(r, "name"))))
    .collect();
    script.section("Schemas", &schemas);
    let extensions: Vec<String> = query(
        conn,
        "SELECT e.extname AS name, n.nspname AS schema FROM pg_extension e \
         JOIN pg_namespace n ON n.oid = e.extnamespace WHERE e.extname <> 'plpgsql' ORDER BY e.extname",
    )
    .await?
    .iter()
    .map(|r| {
        format!(
            "CREATE EXTENSION IF NOT EXISTS {} WITH SCHEMA {}",
            ident(D, &field(r, "name")),
            ident(D, &field(r, "schema"))
        )
    })
    .collect();
    script.section("Extensions", &extensions);

    script.section("Types", &types(conn, in_path).await?);

    let functions: Vec<String> = query(
        conn,
        &format!(
            "SELECT pg_get_functiondef(p.oid) AS definition FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE {in_path} AND p.prokind IN ('f', 'p', 'w') AND {} ORDER BY p.oid",
            not_extension_member("pg_proc", "p.oid")
        ),
    )
    .await?
    .iter()
    .map(|r| field(r, "definition"))
    .collect();
    script.section("Functions and procedures", &functions);

    // Sequences not created by identity columns; owned ones are tied to
    // their column once the tables exist.
    let sequence_rows = query(
        conn,
        &format!(
            "SELECT n.nspname AS schema, c.relname AS name, format_type(s.seqtypid, NULL) AS data_type, \
             s.seqstart::text AS start_value, s.seqincrement::text AS increment, s.seqmin::text AS min_value, \
             s.seqmax::text AS max_value, s.seqcache::text AS cache, s.seqcycle AS cycle, \
             (SELECT quote_ident(tn.nspname) || '.' || quote_ident(tc.relname) || '.' || quote_ident(a.attname) \
              FROM pg_depend d JOIN pg_class tc ON tc.oid = d.refobjid \
              JOIN pg_namespace tn ON tn.oid = tc.relnamespace \
              JOIN pg_attribute a ON a.attrelid = d.refobjid AND a.attnum = d.refobjsubid \
              WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid AND d.deptype = 'a' LIMIT 1) AS owned_by \
             FROM pg_sequence s JOIN pg_class c ON c.oid = s.seqrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE {in_path} AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_class'::regclass \
             AND d.objid = c.oid AND d.deptype IN ('i', 'e')) ORDER BY c.oid"
        ),
    )
    .await?;
    let mut sequences = Vec::new();
    let mut ownership = Vec::new();
    for row in &sequence_rows {
        let name = qualified(&field(row, "schema"), &field(row, "name"));
        sequences.push(format!(
            "CREATE SEQUENCE {name} AS {} INCREMENT BY {} MINVALUE {} MAXVALUE {} START WITH {} CACHE {}{}",
            field(row, "data_type"),
            field(row, "increment"),
            field(row, "min_value"),
            field(row, "max_value"),
            field(row, "start_value"),
            field(row, "cache"),
            if boolean(row, "cycle") { " CYCLE" } else { "" }
        ));
        if let Some(owner) = text(row, "owned_by") {
            ownership.push(format!("ALTER SEQUENCE {name} OWNED BY {owner}"));
        }
    }
    script.section("Sequences", &sequences);

    // Tables, then what hangs off them.
    let tables = tables(conn, target, in_path, script).await?;
    script.section("Sequence ownership", &ownership);
    let by_oid: HashMap<i64, &Table> = tables.iter().map(|t| (t.oid, t)).collect();
    let constraint_rows = query(
        conn,
        &format!(
            "SELECT co.conrelid::bigint AS table_oid, co.conname AS name, co.contype::text AS kind, \
             pg_get_constraintdef(co.oid) AS definition FROM pg_constraint co \
             JOIN pg_class c ON c.oid = co.conrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE co.contype IN ('p', 'u', 'x', 'c', 'f') AND co.conislocal AND {in_path} \
             ORDER BY co.conrelid, CASE co.contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 WHEN 'x' THEN 2 \
             WHEN 'c' THEN 3 ELSE 4 END, co.conname"
        ),
    )
    .await?;
    let mut keys = Vec::new();
    let mut foreign_keys = Vec::new();
    for row in &constraint_rows {
        let Some(table) = int(row, "table_oid").and_then(|oid| by_oid.get(&oid)) else {
            continue;
        };
        let statement = format!(
            "ALTER TABLE {} ADD CONSTRAINT {} {}",
            table.qualified(),
            ident(D, &field(row, "name")),
            field(row, "definition")
        );
        if field(row, "kind") == "f" {
            foreign_keys.push(statement);
        } else {
            keys.push(statement);
        }
    }
    script.section("Keys and checks", &keys);

    let index_rows = query(
        conn,
        &format!(
            "SELECT i.indrelid::bigint AS table_oid, tc.relkind::text AS table_kind, \
             pg_get_indexdef(i.indexrelid) AS definition FROM pg_index i \
             JOIN pg_class ic ON ic.oid = i.indexrelid JOIN pg_class tc ON tc.oid = i.indrelid \
             JOIN pg_namespace n ON n.oid = tc.relnamespace \
             WHERE tc.relkind IN ('r', 'p', 'm') AND {in_path} \
             AND NOT EXISTS (SELECT 1 FROM pg_constraint co WHERE co.conindid = i.indexrelid \
             AND co.contype IN ('p', 'u', 'x')) \
             AND NOT EXISTS (SELECT 1 FROM pg_inherits h WHERE h.inhrelid = i.indexrelid) \
             ORDER BY i.indrelid, ic.relname"
        ),
    )
    .await?;
    let mut table_indexes = Vec::new();
    let mut view_indexes = Vec::new();
    for row in &index_rows {
        let definition = field(row, "definition");
        if field(row, "table_kind") == "m" {
            view_indexes.push(definition);
        } else if int(row, "table_oid").is_some_and(|oid| by_oid.contains_key(&oid)) {
            table_indexes.push(definition);
        }
    }
    script.section("Indexes", &table_indexes);

    let timescale = match target.timescaledb() {
        Some(version) => Some(Timescale::read(conn, in_path, version).await?),
        None => None,
    };
    if let Some(timescale) = &timescale {
        script.section("Hypertables", &timescale.hypertables());
    }
    script.section("Foreign keys", &foreign_keys);

    let skipped: HashSet<(String, String)> = timescale
        .as_ref()
        .map(|t| {
            t.aggregates
                .iter()
                .map(|a| (a.schema.clone(), a.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let (views, view_comments) = views(conn, in_path, &skipped).await?;
    script.section("Views", &views);
    script.section("Indexes of materialized views", &view_indexes);
    if let Some(timescale) = &timescale {
        script.section("Continuous aggregates", &timescale.continuous_aggregates());
        let (policies, notes) = timescale.policies();
        script.section("TimescaleDB policies", &policies);
        script.notes(&notes);
    }

    let mut triggers_sql = format!(
        "SELECT t.tgrelid::bigint AS table_oid, pg_get_triggerdef(t.oid) AS definition FROM pg_trigger t \
         JOIN pg_class c ON c.oid = t.tgrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_proc p ON p.oid = t.tgfoid JOIN pg_namespace pn ON pn.oid = p.pronamespace \
         WHERE NOT t.tgisinternal AND {in_path} AND pn.nspname NOT LIKE '\\_timescaledb%' ESCAPE '\\' \
         AND {}",
        not_extension_member("pg_trigger", "t.oid")
    );
    if target.version() >= Version::from(13) {
        triggers_sql.push_str(" AND t.tgparentid = 0");
    }
    triggers_sql.push_str(" ORDER BY c.oid, t.tgname");
    let triggers: Vec<String> = query(conn, &triggers_sql)
        .await?
        .iter()
        .map(|r| field(r, "definition"))
        .collect();
    script.section("Triggers", &triggers);

    script.section("Row-level security", &row_security(conn, &tables).await?);

    let mut comments = Vec::new();
    for table in &tables {
        if let Some(comment) = &table.comment {
            comments.push(format!(
                "COMMENT ON TABLE {} IS {}",
                table.qualified(),
                string(D, comment)
            ));
        }
    }
    comments.extend(column_comments(conn, in_path, &by_oid).await?);
    comments.extend(view_comments);
    script.section("Comments", &comments);
    Ok(())
}

/// Enum, range, domain and composite types, in that order (a domain may
/// be over an enum, a composite may use a domain).
async fn types(conn: &dyn SchemaConnection, in_path: &str) -> SchemaResult<Vec<String>> {
    let member = not_extension_member("pg_type", "t.oid");
    let mut statements = Vec::new();
    for row in query(
        conn,
        &format!(
            "SELECT n.nspname AS schema, t.typname AS name, \
             (SELECT string_agg(quote_literal(e.enumlabel), ', ' ORDER BY e.enumsortorder) \
              FROM pg_enum e WHERE e.enumtypid = t.oid) AS labels \
             FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
             WHERE t.typtype = 'e' AND {in_path} AND {member} ORDER BY t.oid"
        ),
    )
    .await?
    {
        statements.push(format!(
            "CREATE TYPE {} AS ENUM ({})",
            qualified(&field(&row, "schema"), &field(&row, "name")),
            field(&row, "labels")
        ));
    }
    for row in query(
        conn,
        &format!(
            "SELECT n.nspname AS schema, t.typname AS name, format_type(r.rngsubtype, NULL) AS subtype \
             FROM pg_range r JOIN pg_type t ON t.oid = r.rngtypid \
             JOIN pg_namespace n ON n.oid = t.typnamespace WHERE {in_path} AND {member} ORDER BY t.oid"
        ),
    )
    .await?
    {
        statements.push(format!(
            "CREATE TYPE {} AS RANGE (SUBTYPE = {})",
            qualified(&field(&row, "schema"), &field(&row, "name")),
            field(&row, "subtype")
        ));
    }
    for row in query(
        conn,
        &format!(
            "SELECT n.nspname AS schema, t.typname AS name, format_type(t.typbasetype, t.typtypmod) AS base, \
             t.typnotnull AS not_null, t.typdefault AS default_expr, \
             (SELECT quote_ident(co.collname) FROM pg_collation co WHERE co.oid = t.typcollation \
              AND t.typcollation <> (SELECT b.typcollation FROM pg_type b WHERE b.oid = t.typbasetype)) AS collation, \
             (SELECT string_agg(' CONSTRAINT ' || quote_ident(c.conname) || ' ' || pg_get_constraintdef(c.oid), '' \
              ORDER BY c.conname) FROM pg_constraint c WHERE c.contypid = t.oid AND c.contype = 'c') AS checks \
             FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
             WHERE t.typtype = 'd' AND {in_path} AND {member} ORDER BY t.oid"
        ),
    )
    .await?
    {
        let mut statement = format!(
            "CREATE DOMAIN {} AS {}",
            qualified(&field(&row, "schema"), &field(&row, "name")),
            field(&row, "base")
        );
        if let Some(collation) = text(&row, "collation") {
            statement.push_str(&format!(" COLLATE {collation}"));
        }
        if let Some(default) = text(&row, "default_expr") {
            statement.push_str(&format!(" DEFAULT {default}"));
        }
        if boolean(&row, "not_null") {
            statement.push_str(" NOT NULL");
        }
        statement.push_str(&field(&row, "checks"));
        statements.push(statement);
    }
    for row in query(
        conn,
        &format!(
            "SELECT n.nspname AS schema, t.typname AS name, \
             (SELECT string_agg(quote_ident(a.attname) || ' ' || format_type(a.atttypid, a.atttypmod), ', ' \
              ORDER BY a.attnum) FROM pg_attribute a WHERE a.attrelid = t.typrelid AND a.attnum > 0 \
              AND NOT a.attisdropped) AS attributes \
             FROM pg_type t JOIN pg_class c ON c.oid = t.typrelid JOIN pg_namespace n ON n.oid = t.typnamespace \
             WHERE t.typtype = 'c' AND c.relkind = 'c' AND {in_path} AND {member} ORDER BY t.oid"
        ),
    )
    .await?
    {
        statements.push(format!(
            "CREATE TYPE {} AS ({})",
            qualified(&field(&row, "schema"), &field(&row, "name")),
            field(&row, "attributes")
        ));
    }
    Ok(statements)
}

/// Writes the `CREATE TABLE` section and returns the tables.
async fn tables(
    conn: &dyn SchemaConnection,
    target: &Target,
    in_path: &str,
    script: &mut Script,
) -> SchemaResult<Vec<Table>> {
    let table_rows = query(
        conn,
        &format!(
            "SELECT c.oid::bigint AS oid, n.nspname AS schema, c.relname AS name, c.relkind::text AS kind, \
             c.relpersistence::text AS persistence, c.relispartition AS is_partition, \
             (SELECT quote_ident(pn.nspname) || '.' || quote_ident(pc.relname) FROM pg_inherits i \
              JOIN pg_class pc ON pc.oid = i.inhparent JOIN pg_namespace pn ON pn.oid = pc.relnamespace \
              WHERE i.inhrelid = c.oid AND c.relispartition) AS parent, \
             pg_get_expr(c.relpartbound, c.oid) AS bound, \
             CASE WHEN c.relkind = 'p' THEN pg_get_partkeydef(c.oid) END AS partition_key, \
             array_to_string(c.reloptions, ', ') AS options, c.relrowsecurity AS rls, \
             c.relforcerowsecurity AS force_rls, obj_description(c.oid, 'pg_class') AS comment \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind IN ('r', 'p') AND {in_path} AND {} ORDER BY c.relispartition, c.oid",
            not_extension_member("pg_class", "c.oid")
        ),
    )
    .await?;
    let column_rows = query(
        conn,
        &format!(
            "SELECT a.attrelid::bigint AS table_oid, a.attname AS name, \
             format_type(a.atttypid, a.atttypmod) AS data_type, a.attnotnull AS not_null, \
             pg_get_expr(d.adbin, d.adrelid) AS default_expr, a.attidentity::text AS identity, \
             a.attgenerated::text AS generated, \
             (SELECT CASE WHEN cn.nspname = 'pg_catalog' THEN quote_ident(co.collname) \
              ELSE quote_ident(cn.nspname) || '.' || quote_ident(co.collname) END \
              FROM pg_collation co JOIN pg_namespace cn ON cn.oid = co.collnamespace \
              WHERE co.oid = a.attcollation AND a.attcollation <> t.typcollation) AS collation, \
             CASE WHEN a.attidentity <> '' THEN (SELECT s.seqstart::text || ' ' || s.seqincrement::text || ' ' \
              || s.seqmin::text || ' ' || s.seqmax::text || ' ' || s.seqcache::text || ' ' || s.seqcycle::text \
              FROM pg_sequence s WHERE s.seqrelid = pg_get_serial_sequence(\
              quote_ident(n.nspname) || '.' || quote_ident(c.relname), a.attname)::regclass) END AS identity_options \
             FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_type t ON t.oid = a.atttypid \
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             WHERE c.relkind IN ('r', 'p') AND {in_path} AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attrelid, a.attnum"
        ),
    )
    .await?;
    let mut columns: HashMap<i64, Vec<String>> = HashMap::new();
    for row in &column_rows {
        let Some(table_oid) = int(row, "table_oid") else {
            continue;
        };
        columns
            .entry(table_oid)
            .or_default()
            .push(column_definition(row, target));
    }

    let mut tables = Vec::new();
    let mut statements = Vec::new();
    for row in &table_rows {
        let Some(oid) = int(row, "oid") else {
            continue;
        };
        let table = Table {
            oid,
            schema: field(row, "schema"),
            name: field(row, "name"),
            comment: text(row, "comment"),
            rls: boolean(row, "rls"),
            force_rls: boolean(row, "force_rls"),
        };
        let unlogged = if field(row, "persistence") == "u" {
            "UNLOGGED "
        } else {
            ""
        };
        let mut statement = match (boolean(row, "is_partition"), text(row, "parent")) {
            (true, Some(parent)) => format!(
                "CREATE {unlogged}TABLE {} PARTITION OF {parent} {}",
                table.qualified(),
                field(row, "bound")
            ),
            _ => format!(
                "CREATE {unlogged}TABLE {} ({})",
                table.qualified(),
                columns.remove(&oid).unwrap_or_default().join(", ")
            ),
        };
        if let Some(key) = text(row, "partition_key") {
            statement.push_str(&format!(" PARTITION BY {key}"));
        }
        if let Some(options) = text(row, "options").filter(|o| !o.is_empty()) {
            statement.push_str(&format!(" WITH ({options})"));
        }
        statements.push(statement);
        tables.push(table);
    }
    script.section("Tables", &statements);
    Ok(tables)
}

/// `"name" type [COLLATE] [GENERATED ...] [DEFAULT ...] [NOT NULL]`.
fn column_definition(row: &SchemaRow, target: &Target) -> String {
    let data_type = field(row, "data_type");
    let mut sql = format!("{} {data_type}", ident(D, &field(row, "name")));
    if let Some(collation) = text(row, "collation") {
        sql.push_str(&format!(" COLLATE {collation}"));
    }
    let identity = field(row, "identity");
    let generated = field(row, "generated");
    if !identity.is_empty() {
        sql.push_str(if identity == "a" {
            " GENERATED ALWAYS AS IDENTITY"
        } else {
            " GENERATED BY DEFAULT AS IDENTITY"
        });
        if let Some(options) =
            text(row, "identity_options").and_then(|o| identity_options(&o, &data_type))
        {
            sql.push_str(&format!(" ({options})"));
        }
    } else if !generated.is_empty() {
        let kind = if generated == "v" && target.version() >= Version::from(18) {
            "VIRTUAL"
        } else {
            "STORED"
        };
        sql.push_str(&format!(
            " GENERATED ALWAYS AS ({}) {kind}",
            field(row, "default_expr")
        ));
    } else if let Some(default) = text(row, "default_expr") {
        sql.push_str(&format!(" DEFAULT {default}"));
    }
    if boolean(row, "not_null") {
        sql.push_str(" NOT NULL");
    }
    sql
}

/// The options of an identity column's sequence that differ from the
/// defaults (`start increment min max cache cycle`, space-separated).
fn identity_options(options: &str, data_type: &str) -> Option<String> {
    let parts: Vec<&str> = options.split_whitespace().collect();
    let [start, increment, min, max, cache, cycle] = parts.as_slice() else {
        return None;
    };
    let type_max = match data_type {
        "smallint" => "32767",
        "integer" => "2147483647",
        _ => "9223372036854775807",
    };
    let ascending = !increment.starts_with('-');
    let mut kept = Vec::new();
    if *start != "1" || !ascending {
        kept.push(format!("START WITH {start}"));
    }
    if *increment != "1" {
        kept.push(format!("INCREMENT BY {increment}"));
    }
    if ascending && *min != "1" {
        kept.push(format!("MINVALUE {min}"));
    }
    if ascending && *max != type_max {
        kept.push(format!("MAXVALUE {max}"));
    }
    if *cache != "1" {
        kept.push(format!("CACHE {cache}"));
    }
    if matches!(*cycle, "true" | "t") {
        kept.push("CYCLE".to_owned());
    }
    (!kept.is_empty()).then(|| kept.join(" "))
}

/// Views and materialized views, each after the relations it reads; and
/// their comments.
async fn views(
    conn: &dyn SchemaConnection,
    in_path: &str,
    skipped: &HashSet<(String, String)>,
) -> SchemaResult<(Vec<String>, Vec<String>)> {
    let view_rows = query(
        conn,
        &format!(
            "SELECT c.oid::bigint AS oid, n.nspname AS schema, c.relname AS name, c.relkind::text AS kind, \
             pg_get_viewdef(c.oid) AS definition, array_to_string(c.reloptions, ', ') AS options, \
             obj_description(c.oid, 'pg_class') AS comment FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind IN ('v', 'm') AND {in_path} AND {} ORDER BY c.oid",
            not_extension_member("pg_class", "c.oid")
        ),
    )
    .await?;
    let dependency_rows = query(
        conn,
        "SELECT DISTINCT r.ev_class::bigint AS view_oid, d.refobjid::bigint AS used_oid \
         FROM pg_depend d JOIN pg_rewrite r ON r.oid = d.objid \
         WHERE d.classid = 'pg_rewrite'::regclass AND d.refclassid = 'pg_class'::regclass \
         AND d.refobjid <> r.ev_class",
    )
    .await?;
    let mut by_oid: BTreeMap<i64, &SchemaRow> = BTreeMap::new();
    for row in &view_rows {
        let key = (field(row, "schema"), field(row, "name"));
        if skipped.contains(&key) {
            continue;
        }
        if let Some(oid) = int(row, "oid") {
            by_oid.insert(oid, row);
        }
    }
    let mut waiting: BTreeMap<i64, BTreeSet<i64>> =
        by_oid.keys().map(|oid| (*oid, BTreeSet::new())).collect();
    for row in &dependency_rows {
        if let (Some(view), Some(used)) = (int(row, "view_oid"), int(row, "used_oid"))
            && by_oid.contains_key(&used)
            && let Some(deps) = waiting.get_mut(&view)
        {
            deps.insert(used);
        }
    }
    let mut statements = Vec::new();
    let mut comments = Vec::new();
    for oid in topological(waiting) {
        let row = by_oid[&oid];
        let name = qualified(&field(row, "schema"), &field(row, "name"));
        let definition = field(row, "definition");
        let definition = definition.trim().trim_end_matches(';');
        let materialized = field(row, "kind") == "m";
        let options = text(row, "options")
            .filter(|o| !o.is_empty())
            .map(|o| format!(" WITH ({o})"))
            .unwrap_or_default();
        statements.push(if materialized {
            format!("CREATE MATERIALIZED VIEW {name}{options} AS {definition} WITH NO DATA")
        } else {
            format!("CREATE VIEW {name}{options} AS {definition}")
        });
        if let Some(comment) = text(row, "comment") {
            comments.push(format!(
                "COMMENT ON {} {name} IS {}",
                if materialized {
                    "MATERIALIZED VIEW"
                } else {
                    "VIEW"
                },
                string(D, &comment)
            ));
        }
    }
    Ok((statements, comments))
}

/// Kahn's algorithm over oids (creation order breaks ties).
fn topological(mut waiting: BTreeMap<i64, BTreeSet<i64>>) -> Vec<i64> {
    let mut order = Vec::with_capacity(waiting.len());
    while let Some(next) = waiting
        .iter()
        .find(|(_, deps)| deps.is_empty())
        .map(|(oid, _)| *oid)
    {
        waiting.remove(&next);
        for deps in waiting.values_mut() {
            deps.remove(&next);
        }
        order.push(next);
    }
    order.extend(waiting.into_keys());
    order
}

async fn column_comments(
    conn: &dyn SchemaConnection,
    in_path: &str,
    tables: &HashMap<i64, &Table>,
) -> SchemaResult<Vec<String>> {
    let found = query(
        conn,
        &format!(
            "SELECT a.attrelid::bigint AS table_oid, a.attname AS name, \
             col_description(a.attrelid, a.attnum) AS comment FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind IN ('r', 'p') AND {in_path} AND a.attnum > 0 AND NOT a.attisdropped \
             AND col_description(a.attrelid, a.attnum) IS NOT NULL ORDER BY a.attrelid, a.attnum"
        ),
    )
    .await?;
    Ok(found
        .iter()
        .filter_map(|row| {
            let table = int(row, "table_oid").and_then(|oid| tables.get(&oid))?;
            Some(format!(
                "COMMENT ON COLUMN {}.{} IS {}",
                table.qualified(),
                ident(D, &field(row, "name")),
                string(D, &text(row, "comment")?)
            ))
        })
        .collect())
}

/// `ENABLE` / `FORCE ROW LEVEL SECURITY` and the policies of the tables.
async fn row_security(conn: &dyn SchemaConnection, tables: &[Table]) -> SchemaResult<Vec<String>> {
    let mut statements = Vec::new();
    for table in tables {
        if table.rls {
            statements.push(format!(
                "ALTER TABLE {} ENABLE ROW LEVEL SECURITY",
                table.qualified()
            ));
        }
        if table.force_rls {
            statements.push(format!(
                "ALTER TABLE {} FORCE ROW LEVEL SECURITY",
                table.qualified()
            ));
        }
    }
    let by_oid: HashMap<i64, &Table> = tables.iter().map(|t| (t.oid, t)).collect();
    let policies = query(
        conn,
        "SELECT pol.polrelid::bigint AS table_oid, pol.polname AS name, pol.polcmd::text AS command, \
         pol.polpermissive AS permissive, \
         CASE WHEN pol.polroles = '{0}' THEN 'PUBLIC' ELSE (SELECT string_agg(quote_ident(r.rolname), ', ' \
          ORDER BY r.rolname) FROM pg_roles r WHERE r.oid = ANY (pol.polroles)) END AS roles, \
         pg_get_expr(pol.polqual, pol.polrelid) AS using_expr, \
         pg_get_expr(pol.polwithcheck, pol.polrelid) AS check_expr \
         FROM pg_policy pol ORDER BY pol.polrelid, pol.polname",
    )
    .await?;
    for row in &policies {
        let Some(table) = int(row, "table_oid").and_then(|oid| by_oid.get(&oid)) else {
            continue;
        };
        let command = match field(row, "command").as_str() {
            "r" => "SELECT",
            "a" => "INSERT",
            "w" => "UPDATE",
            "d" => "DELETE",
            _ => "ALL",
        };
        let mut statement = format!(
            "CREATE POLICY {} ON {}{} FOR {command} TO {}",
            ident(D, &field(row, "name")),
            table.qualified(),
            if boolean(row, "permissive") {
                ""
            } else {
                " AS RESTRICTIVE"
            },
            text(row, "roles").unwrap_or_else(|| "PUBLIC".to_owned())
        );
        if let Some(using) = text(row, "using_expr") {
            statement.push_str(&format!(" USING ({using})"));
        }
        if let Some(check) = text(row, "check_expr") {
            statement.push_str(&format!(" WITH CHECK ({check})"));
        }
        statements.push(statement);
    }
    Ok(statements)
}

/// TimescaleDB's objects, read from `timescaledb_information`.
struct Timescale {
    version: Version,
    dimensions: Vec<Dimension>,
    compression: BTreeMap<(String, String), Settings>,
    aggregates: Vec<Aggregate>,
    jobs: Vec<Job>,
}

struct Dimension {
    schema: String,
    table: String,
    number: i64,
    column: String,
    space: bool,
    interval: Option<String>,
    integer_now: Option<String>,
    partitions: Option<i64>,
}

#[derive(Default)]
struct Settings {
    segment_by: Vec<(i64, String)>,
    order_by: Vec<(i64, String)>,
}

struct Aggregate {
    schema: String,
    name: String,
    definition: String,
    materialized_only: bool,
    compressed: bool,
    materialization: String,
}

struct Job {
    id: i64,
    procedure: String,
    schema: String,
    table: String,
    config: String,
    schedule: String,
}

impl Timescale {
    async fn read(
        conn: &dyn SchemaConnection,
        in_path: &str,
        version: Version,
    ) -> SchemaResult<Timescale> {
        let in_path_of = |column: &str| in_path.replace("n.nspname", column);
        let mut dimensions = Vec::new();
        for row in query(
            conn,
            &format!(
                "SELECT hypertable_schema::text AS schema, hypertable_name::text AS table_name, \
                 dimension_number::bigint AS number, column_name::text AS column_name, \
                 dimension_type::text AS kind, time_interval::text AS time_interval, \
                 integer_interval::text AS integer_interval, integer_now_func::text AS integer_now, \
                 num_partitions::bigint AS partitions FROM timescaledb_information.dimensions \
                 WHERE {} ORDER BY hypertable_schema, hypertable_name, dimension_number",
                in_path_of("hypertable_schema")
            ),
        )
        .await?
        {
            dimensions.push(Dimension {
                schema: field(&row, "schema"),
                table: field(&row, "table_name"),
                number: int(&row, "number").unwrap_or(1),
                column: field(&row, "column_name"),
                space: field(&row, "kind").eq_ignore_ascii_case("space"),
                interval: text(&row, "time_interval")
                    .map(|i| format!("INTERVAL {}", string(D, &i)))
                    .or_else(|| text(&row, "integer_interval")),
                integer_now: text(&row, "integer_now"),
                partitions: int(&row, "partitions"),
            });
        }
        let mut compression: BTreeMap<(String, String), Settings> = BTreeMap::new();
        for row in query(
            conn,
            &format!(
                "SELECT hypertable_schema::text AS schema, hypertable_name::text AS table_name \
                 FROM timescaledb_information.hypertables WHERE compression_enabled AND {}",
                in_path_of("hypertable_schema")
            ),
        )
        .await?
        {
            compression.insert(
                (field(&row, "schema"), field(&row, "table_name")),
                Settings::default(),
            );
        }
        for row in query(
            conn,
            &format!(
                "SELECT hypertable_schema::text AS schema, hypertable_name::text AS table_name, \
                 attname::text AS column_name, segmentby_column_index::bigint AS segment_index, \
                 orderby_column_index::bigint AS order_index, orderby_asc AS ascending, \
                 orderby_nullsfirst AS nulls_first FROM timescaledb_information.compression_settings \
                 WHERE {}",
                in_path_of("hypertable_schema")
            ),
        )
        .await?
        {
            let Some(settings) =
                compression.get_mut(&(field(&row, "schema"), field(&row, "table_name")))
            else {
                continue;
            };
            let column = setting_name(&field(&row, "column_name"));
            if let Some(index) = int(&row, "segment_index") {
                settings.segment_by.push((index, column));
            } else if let Some(index) = int(&row, "order_index") {
                let ascending = boolean(&row, "ascending");
                let nulls_first = boolean(&row, "nulls_first");
                let mut order = column;
                if !ascending {
                    order.push_str(" DESC");
                }
                match (ascending, nulls_first) {
                    (true, true) => order.push_str(" NULLS FIRST"),
                    (false, false) => order.push_str(" NULLS LAST"),
                    _ => {}
                }
                settings.order_by.push((index, order));
            }
        }
        let mut aggregates = Vec::new();
        for row in query(
            conn,
            &format!(
                "SELECT view_schema::text AS schema, view_name::text AS name, view_definition AS definition, \
                 materialized_only, compression_enabled, materialization_hypertable_name::text AS materialization \
                 FROM timescaledb_information.continuous_aggregates WHERE {}",
                in_path_of("view_schema")
            ),
        )
        .await?
        {
            aggregates.push(Aggregate {
                schema: field(&row, "schema"),
                name: field(&row, "name"),
                definition: field(&row, "definition"),
                materialized_only: boolean(&row, "materialized_only"),
                compressed: boolean(&row, "compression_enabled"),
                materialization: field(&row, "materialization"),
            });
        }
        // Aggregates over aggregates come after the ones they read: by the
        // id in their materialization hypertable's name.
        aggregates.sort_by_key(|a| {
            a.materialization
                .rsplit('_')
                .next()
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(u64::MAX)
        });
        let mut jobs = Vec::new();
        for row in query(
            conn,
            "SELECT job_id::bigint AS id, proc_schema::text AS proc_schema, proc_name::text AS proc_name, \
             hypertable_schema::text AS schema, hypertable_name::text AS table_name, config::text AS config, \
             schedule_interval::text AS schedule FROM timescaledb_information.jobs \
             WHERE job_id >= 1000 ORDER BY job_id",
        )
        .await?
        {
            jobs.push(Job {
                id: int(&row, "id").unwrap_or_default(),
                procedure: format!("{}.{}", field(&row, "proc_schema"), field(&row, "proc_name")),
                schema: field(&row, "schema"),
                table: field(&row, "table_name"),
                config: field(&row, "config"),
                schedule: field(&row, "schedule"),
            });
        }
        Ok(Timescale {
            version,
            dimensions,
            compression,
            aggregates,
            jobs,
        })
    }

    fn by_range(&self) -> bool {
        self.version >= Version::new(2, 13, 0)
    }

    /// `create_hypertable` (without default indexes: the dump has every
    /// index), extra dimensions, integer-now functions and compression
    /// settings.
    fn hypertables(&self) -> Vec<String> {
        let mut statements = Vec::new();
        for dimension in &self.dimensions {
            let relation = regclass(&format!("{}.{}", dimension.schema, dimension.table));
            let column = string(D, &dimension.column);
            let interval = dimension.interval.clone();
            statements.push(match (dimension.number, dimension.space, self.by_range()) {
                (1, _, true) => format!(
                    "SELECT create_hypertable({relation}, by_range({column}{}), create_default_indexes => FALSE)",
                    interval.map(|i| format!(", {i}")).unwrap_or_default()
                ),
                (1, _, false) => format!(
                    "SELECT create_hypertable({relation}, {column}{}, create_default_indexes => FALSE)",
                    interval
                        .map(|i| format!(", chunk_time_interval => {i}"))
                        .unwrap_or_default()
                ),
                (_, true, true) => format!(
                    "SELECT add_dimension({relation}, by_hash({column}, {}))",
                    dimension.partitions.unwrap_or(1)
                ),
                (_, true, false) => format!(
                    "SELECT add_dimension({relation}, {column}, number_partitions => {})",
                    dimension.partitions.unwrap_or(1)
                ),
                (_, false, true) => format!(
                    "SELECT add_dimension({relation}, by_range({column}{}))",
                    interval.map(|i| format!(", {i}")).unwrap_or_default()
                ),
                (_, false, false) => format!(
                    "SELECT add_dimension({relation}, {column}{})",
                    interval
                        .map(|i| format!(", chunk_time_interval => {i}"))
                        .unwrap_or_default()
                ),
            });
            if let Some(function) = &dimension.integer_now {
                statements.push(format!(
                    "SELECT set_integer_now_func({relation}, {})",
                    string(D, function)
                ));
            }
        }
        for ((schema, table), settings) in &self.compression {
            statements.push(compression_statement(
                &format!("ALTER TABLE {}", qualified(schema, table)),
                settings,
            ));
        }
        statements
    }

    fn continuous_aggregates(&self) -> Vec<String> {
        let mut statements = Vec::new();
        for aggregate in &self.aggregates {
            let name = qualified(&aggregate.schema, &aggregate.name);
            statements.push(format!(
                "CREATE MATERIALIZED VIEW {name} WITH (timescaledb.continuous, timescaledb.materialized_only = {}) AS {} WITH NO DATA",
                if aggregate.materialized_only {
                    "TRUE"
                } else {
                    "FALSE"
                },
                aggregate.definition.trim().trim_end_matches(';')
            ));
            if aggregate.compressed {
                statements.push(format!(
                    "ALTER MATERIALIZED VIEW {name} SET (timescaledb.compress = TRUE)"
                ));
            }
        }
        statements
    }

    /// The compression, retention, reorder and refresh policies, and notes
    /// on the app's own jobs (their procedures are the app's to re-create).
    fn policies(&self) -> (Vec<String>, Vec<String>) {
        let aggregates: HashMap<&str, String> = self
            .aggregates
            .iter()
            .map(|a| {
                (
                    a.materialization.as_str(),
                    format!("{}.{}", a.schema, a.name),
                )
            })
            .collect();
        let mut statements = Vec::new();
        let mut notes = Vec::new();
        for job in &self.jobs {
            let relation = match aggregates.get(job.table.as_str()) {
                Some(aggregate) => regclass(aggregate),
                None => regclass(&format!("{}.{}", job.schema, job.table)),
            };
            let config: serde_json::Value =
                serde_json::from_str(&job.config).unwrap_or(serde_json::Value::Null);
            let interval = |key: &str| config.get(key).map(interval_literal);
            let statement = match job.procedure.rsplit('.').next().unwrap_or_default() {
                "policy_compression" | "policy_columnstore" => {
                    match (
                        interval("compress_after"),
                        interval("compress_created_before"),
                    ) {
                        (Some(after), _) => Some(format!(
                            "SELECT add_compression_policy({relation}, compress_after => {after})"
                        )),
                        (None, Some(before)) => Some(format!(
                            "SELECT add_compression_policy({relation}, compress_created_before => {before})"
                        )),
                        (None, None) => None,
                    }
                }
                "policy_retention" => {
                    match (interval("drop_after"), interval("drop_created_before")) {
                        (Some(after), _) => Some(format!(
                            "SELECT add_retention_policy({relation}, drop_after => {after})"
                        )),
                        (None, Some(before)) => Some(format!(
                            "SELECT add_retention_policy({relation}, drop_created_before => {before})"
                        )),
                        (None, None) => None,
                    }
                }
                "policy_reorder" => config
                    .get("index_name")
                    .and_then(serde_json::Value::as_str)
                    .map(|index| {
                        format!(
                            "SELECT add_reorder_policy({relation}, {})",
                            string(D, index)
                        )
                    }),
                "policy_refresh_continuous_aggregate" => Some(format!(
                    "SELECT add_continuous_aggregate_policy({relation}, start_offset => {}, end_offset => {}, schedule_interval => INTERVAL {})",
                    interval("start_offset").unwrap_or_else(|| "NULL".to_owned()),
                    interval("end_offset").unwrap_or_else(|| "NULL".to_owned()),
                    string(D, &job.schedule)
                )),
                _ => None,
            };
            match statement {
                Some(statement) => statements.push(statement),
                None => notes.push(format!(
                    "job {} ({}) is not dumped: it is the app's own job; re-create it with add_job",
                    job.id, job.procedure
                )),
            }
        }
        (statements, notes)
    }
}

/// A column name inside a TimescaleDB setting (`compress_segmentby`): bare
/// when it needs no quotes, the way migrations write it.
fn setting_name(column: &str) -> String {
    let mut chars = column.chars();
    let simple = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if simple {
        column.to_owned()
    } else {
        ident(D, column)
    }
}

fn compression_statement(prefix: &str, settings: &Settings) -> String {
    let mut options = vec!["timescaledb.compress".to_owned()];
    let mut segment_by = settings.segment_by.clone();
    segment_by.sort();
    if !segment_by.is_empty() {
        let columns: Vec<String> = segment_by.into_iter().map(|(_, c)| c).collect();
        options.push(format!(
            "timescaledb.compress_segmentby = {}",
            string(D, &columns.join(", "))
        ));
    }
    let mut order_by = settings.order_by.clone();
    order_by.sort();
    if !order_by.is_empty() {
        let columns: Vec<String> = order_by.into_iter().map(|(_, c)| c).collect();
        options.push(format!(
            "timescaledb.compress_orderby = {}",
            string(D, &columns.join(", "))
        ));
    }
    format!("{prefix} SET ({})", options.join(", "))
}

/// A policy setting from a job's JSON config: a number for integer time,
/// otherwise an interval literal; `null` is `NULL`.
fn interval_literal(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::String(text) => format!("INTERVAL {}", string(D, text)),
        _ => "NULL".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_options_keep_only_what_differs() {
        assert_eq!(
            identity_options("1 1 1 9223372036854775807 1 false", "bigint"),
            None
        );
        assert_eq!(
            identity_options("1000 10 1 2147483647 1 false", "integer").as_deref(),
            Some("START WITH 1000 INCREMENT BY 10")
        );
        assert_eq!(
            identity_options("1 1 1 32767 20 true", "smallint").as_deref(),
            Some("CACHE 20 CYCLE")
        );
    }

    #[test]
    fn compression_settings_render_in_column_order() {
        let settings = Settings {
            segment_by: vec![(2, setting_name("Kind")), (1, setting_name("workspace_id"))],
            order_by: vec![(1, format!("{} DESC", setting_name("occurred_at")))],
        };
        assert_eq!(
            compression_statement("ALTER TABLE \"public\".\"events\"", &settings),
            "ALTER TABLE \"public\".\"events\" SET (timescaledb.compress, timescaledb.compress_segmentby = 'workspace_id, \"Kind\"', timescaledb.compress_orderby = 'occurred_at DESC')"
        );
    }

    #[test]
    fn policy_settings_become_literals() {
        assert_eq!(
            interval_literal(&serde_json::json!("7 days")),
            "INTERVAL '7 days'"
        );
        assert_eq!(interval_literal(&serde_json::json!(86400)), "86400");
        assert_eq!(interval_literal(&serde_json::Value::Null), "NULL");
    }
}
