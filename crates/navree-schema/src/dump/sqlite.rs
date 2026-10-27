//! The SQLite dump: the statements SQLite keeps in `sqlite_master`.

use std::collections::HashSet;

use super::Script;
use crate::connection::SchemaConnection;
use crate::error::SchemaResult;
use crate::inspect::{rows, text};

pub(super) async fn dump(conn: &dyn SchemaConnection, script: &mut Script) -> SchemaResult {
    // Tables FTS5 and other virtual tables keep their data in: created by
    // their virtual table, never by hand.
    let shadow: HashSet<String> = rows(
        conn,
        "SELECT name FROM pragma_table_list WHERE type = 'shadow'",
        Vec::new(),
    )
    .await
    .map(|found| found.iter().filter_map(|r| text(r, "name")).collect())
    .unwrap_or_default();
    let found = rows(
        conn,
        "SELECT type, name, sql FROM sqlite_master \
         WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'index' THEN 1 WHEN 'view' THEN 2 ELSE 3 END, rowid",
        Vec::new(),
    )
    .await?;
    let mut sections: [Vec<String>; 4] = Default::default();
    for row in &found {
        let (Some(kind), Some(name), Some(sql)) =
            (text(row, "type"), text(row, "name"), text(row, "sql"))
        else {
            continue;
        };
        if shadow.contains(&name) {
            continue;
        }
        let slot = match kind.as_str() {
            "table" => 0,
            "index" => 1,
            "view" => 2,
            _ => 3,
        };
        sections[slot].push(sql);
    }
    let [tables, indexes, views, triggers] = sections;
    script.section("Tables", &tables);
    script.section("Indexes", &indexes);
    script.section("Views", &views);
    script.compound_section("Triggers", &triggers);
    Ok(())
}
