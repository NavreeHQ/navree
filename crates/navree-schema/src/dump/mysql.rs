//! The MySQL and MariaDB dump: `SHOW CREATE` statements, made portable
//! (no definers, no `AUTO_INCREMENT` counters), with foreign key checks off
//! while the script loads.

use std::collections::{BTreeMap, BTreeSet};

use super::Script;
use crate::connection::SchemaConnection;
use crate::error::SchemaResult;
use crate::grammar::ident;
use crate::inspect::{rows, text};
use crate::target::Dialect;

const D: Dialect = Dialect::MySql;

pub(super) async fn dump(conn: &dyn SchemaConnection, script: &mut Script) -> SchemaResult {
    script.section(
        "Foreign keys are checked once every table exists",
        &["SET FOREIGN_KEY_CHECKS=0".to_owned()],
    );
    let objects = rows(
        conn,
        "SELECT CAST(table_name AS CHAR) AS name, CAST(table_type AS CHAR) AS kind \
         FROM information_schema.tables WHERE table_schema = DATABASE() ORDER BY table_name",
        Vec::new(),
    )
    .await?;
    let mut tables = Vec::new();
    let mut views = Vec::new();
    for row in &objects {
        let (Some(name), Some(kind)) = (text(row, "name"), text(row, "kind")) else {
            continue;
        };
        if kind.eq_ignore_ascii_case("VIEW") {
            views.push(name);
        } else {
            let found = rows(
                conn,
                &format!("SHOW CREATE TABLE {}", ident(D, &name)),
                Vec::new(),
            )
            .await?;
            if let Some(sql) = found.first().and_then(|r| text(r, "Create Table")) {
                tables.push(without_auto_increment(&sql));
            }
        }
    }
    script.section("Tables", &tables);

    let mut routines = Vec::new();
    let found = rows(
        conn,
        "SELECT CAST(routine_name AS CHAR) AS name, CAST(routine_type AS CHAR) AS kind \
         FROM information_schema.routines WHERE routine_schema = DATABASE() \
         ORDER BY routine_type, routine_name",
        Vec::new(),
    )
    .await?;
    for row in &found {
        let (Some(name), Some(kind)) = (text(row, "name"), text(row, "kind")) else {
            continue;
        };
        let kind = kind.to_ascii_uppercase();
        let column = if kind == "PROCEDURE" {
            "Create Procedure"
        } else {
            "Create Function"
        };
        let shown = rows(
            conn,
            &format!("SHOW CREATE {kind} {}", ident(D, &name)),
            Vec::new(),
        )
        .await?;
        if let Some(sql) = shown.first().and_then(|r| text(r, column)) {
            routines.push(without_definer(&sql));
        }
    }
    script.compound_section("Routines", &routines);

    let mut created_views = Vec::new();
    for name in in_dependency_order(conn, views).await {
        let shown = rows(
            conn,
            &format!("SHOW CREATE VIEW {}", ident(D, &name)),
            Vec::new(),
        )
        .await?;
        if let Some(sql) = shown.first().and_then(|r| text(r, "Create View")) {
            created_views.push(portable_view(&sql));
        }
    }
    script.section("Views", &created_views);

    let mut triggers = Vec::new();
    let found = rows(
        conn,
        "SELECT CAST(trigger_name AS CHAR) AS name FROM information_schema.triggers \
         WHERE trigger_schema = DATABASE() \
         ORDER BY event_object_table, action_timing, event_manipulation, action_order",
        Vec::new(),
    )
    .await?;
    for row in &found {
        let Some(name) = text(row, "name") else {
            continue;
        };
        let shown = rows(
            conn,
            &format!("SHOW CREATE TRIGGER {}", ident(D, &name)),
            Vec::new(),
        )
        .await?;
        if let Some(sql) = shown.first().and_then(|r| text(r, "SQL Original Statement")) {
            triggers.push(without_definer(&sql));
        }
    }
    script.compound_section("Triggers", &triggers);
    Ok(())
}

/// Views ordered so each comes after the views it reads
/// (`information_schema.view_table_usage`, MySQL 8.0.13+; by name where
/// the server does not have it).
async fn in_dependency_order(conn: &dyn SchemaConnection, views: Vec<String>) -> Vec<String> {
    let usage = rows(
        conn,
        "SELECT CAST(view_name AS CHAR) AS view_name, CAST(table_name AS CHAR) AS used \
         FROM information_schema.view_table_usage WHERE view_schema = DATABASE()",
        Vec::new(),
    )
    .await
    .unwrap_or_default();
    let names: BTreeSet<&str> = views.iter().map(String::as_str).collect();
    let mut waiting: BTreeMap<String, BTreeSet<String>> = views
        .iter()
        .map(|v| (v.clone(), BTreeSet::new()))
        .collect();
    for row in &usage {
        if let (Some(view), Some(used)) = (text(row, "view_name"), text(row, "used"))
            && view != used
            && names.contains(used.as_str())
            && let Some(deps) = waiting.get_mut(&view)
        {
            deps.insert(used);
        }
    }
    topological(waiting)
}

/// Kahn's algorithm over `waiting` (name → names it needs first), ties by
/// name; a cycle (impossible for views) keeps the rest in name order.
pub(super) fn topological(mut waiting: BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    let mut order = Vec::with_capacity(waiting.len());
    while let Some(next) = waiting
        .iter()
        .find(|(_, deps)| deps.is_empty())
        .map(|(name, _)| name.clone())
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

/// `CREATE TABLE ...` without the table option `AUTO_INCREMENT=<n>` (the
/// counter is data, not structure).
pub(super) fn without_auto_increment(sql: &str) -> String {
    let Some(body_end) = sql.rfind(')') else {
        return sql.to_owned();
    };
    let (body, options) = sql.split_at(body_end + 1);
    let options: Vec<&str> = options
        .split_whitespace()
        .filter(|option| !option.to_ascii_uppercase().starts_with("AUTO_INCREMENT="))
        .collect();
    if options.is_empty() {
        body.to_owned()
    } else {
        format!("{body} {}", options.join(" "))
    }
}

/// The statement without its `DEFINER=user@host` clause, so it loads
/// under any account.
pub(super) fn without_definer(sql: &str) -> String {
    let upper = sql.to_ascii_uppercase();
    let Some(at) = upper.find("DEFINER=") else {
        return sql.to_owned();
    };
    let bytes = sql.as_bytes();
    let mut end = at + "DEFINER=".len();
    // user@host, each part quoted (`...`, '...', "...") or bare.
    for _ in 0..2 {
        match bytes.get(end) {
            Some(&quote @ (b'`' | b'\'' | b'"')) => {
                end += 1;
                while end < bytes.len() && bytes[end] != quote {
                    end += 1;
                }
                end += 1;
            }
            _ => {
                while end < bytes.len() && !bytes[end].is_ascii_whitespace() && bytes[end] != b'@' {
                    end += 1;
                }
            }
        }
        if bytes.get(end) == Some(&b'@') {
            end += 1;
        } else {
            break;
        }
    }
    let rest = sql[end.min(sql.len())..].trim_start();
    format!("{} {rest}", sql[..at].trim_end())
}

/// `CREATE VIEW` as `SHOW CREATE VIEW` prints it, without the definer and
/// the default options (`ALGORITHM=UNDEFINED`, `SQL SECURITY DEFINER`).
pub(super) fn portable_view(sql: &str) -> String {
    let without = without_definer(sql);
    let upper = without.to_ascii_uppercase();
    let Some(view) = upper.find(" VIEW ") else {
        return without;
    };
    // What sits between CREATE and VIEW: ALGORITHM=..., SQL SECURITY ...
    let options = without[..view]
        .split_whitespace()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ")
        .replace("ALGORITHM=UNDEFINED", "")
        .replace("SQL SECURITY DEFINER", "");
    let kept = options.split_whitespace().collect::<Vec<_>>().join(" ");
    if kept.is_empty() {
        format!("CREATE{}", &without[view..])
    } else {
        format!("CREATE {kept}{}", &without[view..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_increment_counters_are_dropped() {
        assert_eq!(
            without_auto_increment(
                "CREATE TABLE `deals` (\n  `id` bigint unsigned NOT NULL AUTO_INCREMENT,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB AUTO_INCREMENT=42 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci"
            ),
            "CREATE TABLE `deals` (\n  `id` bigint unsigned NOT NULL AUTO_INCREMENT,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci",
            "the column's AUTO_INCREMENT stays"
        );
    }

    #[test]
    fn definers_are_dropped() {
        assert_eq!(
            without_definer(
                "CREATE DEFINER=`root`@`localhost` TRIGGER `t` BEFORE INSERT ON `a` FOR EACH ROW SET NEW.x = 1"
            ),
            "CREATE TRIGGER `t` BEFORE INSERT ON `a` FOR EACH ROW SET NEW.x = 1"
        );
        assert_eq!(
            without_definer("CREATE DEFINER=app@'%' PROCEDURE `p`() SELECT 1"),
            "CREATE PROCEDURE `p`() SELECT 1"
        );
        assert_eq!(without_definer("CREATE TABLE t (a INT)"), "CREATE TABLE t (a INT)");
    }

    #[test]
    fn views_keep_only_options_that_differ_from_the_defaults() {
        assert_eq!(
            portable_view(
                "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` SQL SECURITY DEFINER VIEW `open_deals` AS select `deals`.`id` AS `id` from `deals`"
            ),
            "CREATE VIEW `open_deals` AS select `deals`.`id` AS `id` from `deals`"
        );
        assert_eq!(
            portable_view(
                "CREATE ALGORITHM=MERGE DEFINER=`root`@`%` SQL SECURITY INVOKER VIEW `v` AS select 1 AS `1`"
            ),
            "CREATE ALGORITHM=MERGE SQL SECURITY INVOKER VIEW `v` AS select 1 AS `1`"
        );
    }

    #[test]
    fn dependencies_come_first() {
        let mut waiting = BTreeMap::new();
        waiting.insert("a".to_owned(), BTreeSet::from(["c".to_owned()]));
        waiting.insert("b".to_owned(), BTreeSet::new());
        waiting.insert("c".to_owned(), BTreeSet::from(["b".to_owned()]));
        assert_eq!(topological(waiting), ["b", "c", "a"]);
    }
}
