//! Altering tables (offline): added, changed, renamed and dropped columns,
//! indexes and foreign keys, for each database. SQLite's rebuilds run
//! against real databases in `sqlite.rs`.

use crate::sql_for;
use crate::support::{dialects, mariadb, mysql, postgres};

#[tokio::test]
async fn adding_columns() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .table("deals", |t| {
                t.string("stage").default("lead").index();
                t.string("source").nullable().after("title");
                t.after("amount", |t| {
                    t.string("currency").length(3).default("USD");
                    t.boolean("taxed").default(false);
                });
                t.integer("position").default(0).first();
                t.foreign_uuid("owner_id")
                    .nullable()
                    .constrained_on("users", "id");
                t.json("tags").nullable().comment("free-form labels");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn adding_an_auto_increment_key() {
    let sql = sql_for!([("postgres", postgres()), ("mysql", mysql())], |schema| {
        schema
            .table("legacy", |t| {
                t.id();
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn changing_columns() {
    let sql = sql_for!(
        [
            ("postgres", postgres()),
            ("mysql", mysql()),
            ("mariadb", mariadb())
        ],
        |schema| {
            schema
                .table("deals", |t| {
                    t.string("title").length(500).change();
                    t.decimal("amount", 14, 4)
                        .nullable()
                        .default(0)
                        .using("amount::numeric(14, 4)")
                        .change();
                    t.text("notes").comment("internal").change();
                    t.string("email").unique().change();
                    t.string("code").without_unique().change();
                    t.big_increments("id").change();
                })
                .await
        }
    );
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn changing_identity_and_generated_columns_on_postgres() {
    let sql = sql_for!(
        [
            ("postgres 16", navree_schema::Target::postgres(16)),
            ("postgres 17", postgres()),
        ],
        |schema| {
            schema
                .table("totals", |t| {
                    t.big_integer("sequence").generated_as().change();
                    t.integer("gross").stored_as("price + tax * 2").change();
                })
                .await
        }
    );
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn renaming_and_dropping() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .table("deals", |t| {
                t.rename_column("value", "amount");
                t.drop_column(["legacy", "unused"]);
                t.drop_unique(["email"]);
                t.drop_index("deals_stage_index");
                t.drop_timestamps();
                t.drop_soft_deletes();
                t.drop_morphs("subject");
            })
            .await?;
        schema.rename("deals", "opportunities").await?;
        schema.drop("archived_deals").await?;
        schema.drop_if_exists("old_deals").await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn dropping_keys() {
    let sql = sql_for!([("postgres", postgres()), ("mysql", mysql())], |schema| {
        schema
            .table("deals", |t| {
                t.drop_primary();
                t.drop_foreign(["company_id"]);
                t.drop_constrained_foreign_id("owner_id");
                t.drop_fulltext(["title", "body"]);
                t.rename_index("deals_title_index", "deals_title");
                t.primary(["id", "created_at"]);
                t.comment("pipeline deals");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn mysql_before_8_0_3_cannot_rename_columns() {
    let schema = navree_schema::Schema::offline(navree_schema::Target::mysql((5, 7, 44)));
    let error = schema
        .table("deals", |t| t.rename_column("value", "amount"))
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "MySQL does not support RENAME COLUMN on MySQL 5.7.44 (renaming `value`): upgrade to MySQL 8.0.3 or MariaDB 10.5.2, or redefine the column under its new name with `.change()` and raw SQL"
    );
}

#[tokio::test]
async fn sqlite_rebuilds_need_the_database() {
    let schema = navree_schema::Schema::offline(crate::support::sqlite());
    let error = schema
        .table("deals", |t| {
            t.string("title").length(500).change();
        })
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "changing column `title` on SQLite (it rebuilds `deals` from its current columns, indexes and foreign keys) needs a database connection; this schema is offline (it only records statements)"
    );
}
