//! Every column type, modifier and helper, compiled for each database.

use navree_schema::{DefaultValue, raw};

use crate::sql_for;
use crate::support::{dialects, mariadb, mysql, postgres, sqlite};

#[tokio::test]
async fn every_column_type() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("samples", |t| {
                t.char("country", 2);
                t.string("name");
                t.string("code").length(16);
                t.tiny_text("note");
                t.text("body");
                t.medium_text("summary");
                t.long_text("archive");
                t.tiny_integer("rank");
                t.small_integer("score");
                t.medium_integer("views");
                t.integer("count");
                t.big_integer("total");
                t.unsigned_integer("hits");
                t.float("ratio");
                t.float("rough").precision(24);
                t.double("weight");
                t.decimal("amount", 12, 2);
                t.boolean("active");
                t.enumeration("stage", ["lead", "won", "lost"]);
                t.json("settings");
                t.jsonb("payload");
                t.date("born_on");
                t.date_time("starts_at");
                t.date_time_tz("ends_at");
                t.time("opens_at");
                t.time_tz("closes_at");
                t.timestamp("seen_at");
                t.timestamp_tz("synced_at");
                t.timestamp("legacy_at").precision(0);
                t.timestamp_tz("database_default_at").default_precision();
                t.year("vintage");
                t.binary("avatar");
                t.binary("digest").length(32).fixed();
                t.binary("token").length(64);
                t.uuid("external_id");
                t.ulid("public_id");
                t.ip_address("ip");
                t.mac_address("mac");
                t.raw_column("rank_check", "SMALLINT CHECK (rank_check > 0)");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn database_specific_types() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("places", |t| {
                t.uuid_primary();
                t.geometry("area");
                t.geometry("spot").subtype("point").srid(4326);
                t.geography("route").subtype("linestring");
            })
            .await
    });
    insta::assert_snapshot!(sql);

    let vectors = sql_for!(dialects(), |schema| {
        schema
            .create("embeddings", |t| {
                t.id();
                t.vector("embedding", 1536);
            })
            .await
    });
    insta::assert_snapshot!(vectors);

    let search = sql_for!(dialects(), |schema| {
        schema
            .create("documents", |t| {
                t.id();
                t.tsvector("search");
            })
            .await
    });
    insta::assert_snapshot!(search);

    let sets = sql_for!(dialects(), |schema| {
        schema
            .create("profiles", |t| {
                t.id();
                t.set("channels", ["mail", "sms", "push"]);
            })
            .await
    });
    insta::assert_snapshot!(sets);
}

#[tokio::test]
async fn column_modifiers() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("modifiers", |t| {
                t.id().starting_value(1000);
                t.string("nickname").nullable();
                t.string("stage").default("lead");
                t.string("quote").default("it's \\ fine");
                t.integer("attempts").default(0);
                t.unsigned_big_integer("budget").default(5_000_000_000_u64);
                t.double("ratio").default(1.5);
                t.double("whole").default(2.0);
                t.boolean("active").default(true);
                t.boolean("archived").default(false);
                t.json("settings")
                    .default(serde_json::json!({"theme": "dark"}));
                t.string("optional").nullable().default(DefaultValue::Null);
                t.timestamp_tz("created_at").use_current();
                t.timestamp("touched_at")
                    .use_current()
                    .use_current_on_update();
                t.date("on").use_current();
                t.date_time("checked_at").default(raw("CURRENT_TIMESTAMP"));
                t.string("title")
                    .comment("shown in lists")
                    .collation("utf8mb4_bin");
                t.string("legacy").charset("latin1");
                t.integer("price");
                t.integer("tax");
                t.integer("gross").stored_as("price + tax");
                t.integer("net").virtual_as("price - tax").nullable();
                t.string("secret").invisible().nullable();
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn generated_columns_need_postgres_18_for_virtual() {
    let sql = sql_for!(
        [
            ("postgres 17", postgres()),
            ("postgres 18", navree_schema::Target::postgres(18)),
        ],
        |schema| {
            schema
                .create("totals", |t| {
                    t.integer("price");
                    t.integer("double_price").virtual_as("price * 2");
                })
                .await
        }
    );
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn identity_columns() {
    let sql = sql_for!([("postgres", postgres())], |schema| {
        schema
            .create("tickets", |t| {
                t.big_integer("id").generated_as().always().primary();
                t.integer("sequence")
                    .generated_as_with("START WITH 10 INCREMENT BY 10");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn helpers() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("helpers", |t| {
                t.id();
                t.foreign_id("company_id").constrained().cascade_on_delete();
                t.foreign_uuid("owner_id")
                    .nullable()
                    .constrained_on("users", "id")
                    .null_on_delete();
                t.foreign_ulid("ticket_id")
                    .references("public_id")
                    .on("tickets");
                t.timestamps();
                t.timestamps_tz();
                t.soft_deletes_tz();
                t.remember_token();
                t.morphs("subject");
                t.nullable_uuid_morphs("target");
                t.ulid_morphs("source");
            })
            .await
    });
    insta::assert_snapshot!(sql);

    let increments = sql_for!(dialects(), |schema| {
        schema
            .create("counters", |t| {
                t.small_increments("id");
            })
            .await?;
        schema
            .create("codes", |t| {
                t.ulid_primary();
                t.datetimes();
                t.soft_deletes();
            })
            .await
    });
    insta::assert_snapshot!(increments);
}

#[tokio::test]
async fn morphs_follow_the_configured_key() {
    let options = navree_schema::SchemaOptions {
        morph_key: navree_schema::MorphKey::Uuid,
        ..Default::default()
    };
    let schema = navree_schema::Schema::offline(postgres()).with_options(options);
    schema
        .create("comments", |t| {
            t.uuid_primary();
            t.morphs("commentable");
            t.nullable_morphs("parent");
            t.morphs_with_index("about", "comments_about");
        })
        .await
        .unwrap();
    insta::assert_snapshot!(schema.statements().join(";\n"));
}

#[tokio::test]
async fn mysql_versions_shape_defaults_and_types() {
    let sql = sql_for!(
        [
            ("mysql 8.0.12", navree_schema::Target::mysql((8, 0, 12))),
            ("mysql 8.4", mysql()),
            ("mariadb 10.6", navree_schema::Target::mariadb((10, 6))),
            ("mariadb 11.4", mariadb()),
        ],
        |schema| {
            schema
                .create("versions", |t| {
                    t.uuid("id");
                    t.text("bio").default("none");
                })
                .await
        }
    );
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn sqlite_ignores_what_it_cannot_store() {
    let schema = navree_schema::Schema::offline(sqlite());
    schema
        .create("plain", |t| {
            t.string("title").comment("not stored").charset("utf8mb4");
            t.integer("count").unsigned().after("title");
        })
        .await
        .unwrap();
    assert_eq!(
        schema.statements(),
        ["CREATE TABLE \"plain\" (\"title\" VARCHAR NOT NULL, \"count\" INTEGER NOT NULL)"]
    );
}
