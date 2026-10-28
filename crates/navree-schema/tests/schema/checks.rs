//! Table-level `CHECK` constraints: created with the table, added to and
//! dropped from an existing one, for each database. SQLite adds and drops
//! them by rebuilding the table (end to end in `sqlite.rs`).

use crate::sql_for;
use crate::support::{dialects, mariadb, mysql, postgres};

#[tokio::test]
async fn checks_are_created_with_the_table() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("templates", |t| {
                t.uuid("id").primary();
                t.string("scope").length(16);
                t.uuid("workspace_id").nullable();
                t.integer("position").default(0);
                t.check(
                    "templates_owner_check",
                    "(scope = 'global' AND workspace_id IS NULL) OR (scope = 'workspace' AND workspace_id IS NOT NULL)",
                );
                t.check("templates_position_check", "position >= 0");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn checks_are_added_to_and_dropped_from_a_table() {
    let sql = sql_for!(
        [
            ("postgres", postgres()),
            ("mysql", mysql()),
            ("mariadb", mariadb()),
        ],
        |schema| {
            schema
                .table("templates", |t| {
                    t.check("templates_position_check", "position >= 0");
                    t.drop_check("templates_owner_check");
                })
                .await
        }
    );
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn mysql_before_8_0_16_is_told_its_checks_are_not_enforced() {
    let old = navree_schema::Target::mysql((8, 0, 15));
    let sql = sql_for!([("mysql 8.0.15", old)], |schema| {
        schema
            .create("templates", |t| {
                t.integer("position");
                t.check("templates_position_check", "position >= 0");
            })
            .await
    });
    assert!(
        sql.contains("ALTER TABLE `templates` ADD CONSTRAINT `templates_position_check` CHECK (position >= 0);"),
        "{sql}"
    );
    assert!(
        sql.contains(
            "-- notice: MySQL 8.0.15 parses CHECK constraints but enforces them only from 8.0.16"
        ),
        "{sql}"
    );
}

#[tokio::test]
async fn a_check_needs_a_name_and_an_expression() {
    let sql = sql_for!([("postgres", postgres())], |schema| {
        schema
            .create("templates", |t| {
                t.integer("position");
                t.check(" ", "position >= 0");
            })
            .await
    });
    assert!(sql.contains("-- error:"), "{sql}");
    assert!(sql.contains("a CHECK constraint needs a name"), "{sql}");

    let sql = sql_for!([("postgres", postgres())], |schema| {
        schema
            .create("templates", |t| {
                t.integer("position");
                t.check("templates_position_check", "  ");
            })
            .await
    });
    assert!(
        sql.contains("CHECK constraint `templates_position_check` has no expression"),
        "{sql}"
    );

    let sql = sql_for!([("postgres", postgres())], |schema| {
        schema
            .table("templates", |t| {
                t.drop_check("");
            })
            .await
    });
    assert!(sql.contains("a CHECK constraint needs a name"), "{sql}");
}
