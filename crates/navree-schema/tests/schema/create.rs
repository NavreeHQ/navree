//! Creating tables: keys, indexes, foreign keys and table options, for
//! each database.

use navree_schema::ReferentialAction;

use crate::sql_for;
use crate::support::{dialects, postgres};

#[tokio::test]
async fn the_deals_example() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("deals", |t| {
                t.id();
                t.foreign_id("company_id").constrained().cascade_on_delete();
                t.string("title");
                t.decimal("amount", 12, 2);
                t.timestamps();
                t.soft_deletes();
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn keys_and_indexes() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("memberships", |t| {
                t.uuid("workspace_id");
                t.uuid("user_id");
                t.string("email").unique();
                t.string("role").index();
                t.string("nickname").nullable().unique_named("memberships_nick");
                t.text("bio").nullable();
                t.timestamp_tz("deleted_at").nullable();
                t.primary(["workspace_id", "user_id"]);
                t.unique(["workspace_id", "email"]).name("memberships_workspace_email");
                t.index(["workspace_id", "role"]).algorithm("btree");
                t.index("deleted_at").where_raw("deleted_at IS NULL");
                t.raw_index("lower(email)", "memberships_email_lower_index");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn postgres_index_options() {
    let sql = sql_for!(vec![("postgres", postgres())], |schema| {
        schema
            .create("articles", |t| {
                t.id();
                t.string("slug");
                t.string("code").nullable();
                t.text("title");
                t.text("body");
                t.geometry("location");
                t.unique("slug").deferrable().initially_deferred();
                t.unique("code").nulls_not_distinct();
                t.index("title").online();
                t.unique("title").online();
                t.fulltext(["title", "body"]).language("simple");
                t.spatial_index("location");
                t.index("slug").operator_class("text_pattern_ops");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn fulltext_and_spatial_indexes() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("posts", |t| {
                t.id();
                t.text("title");
                t.text("body");
                t.fulltext(["title", "body"]);
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn foreign_keys() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("line_items", |t| {
                t.uuid_primary();
                t.uuid("order_id");
                t.uuid("product_id").nullable();
                t.string("currency").length(3);
                t.foreign("order_id")
                    .references("id")
                    .on("orders")
                    .cascade_on_delete()
                    .cascade_on_update();
                t.foreign("product_id")
                    .references("id")
                    .on("catalog.products")
                    .on_delete(ReferentialAction::SetNull)
                    .deferrable()
                    .initially_deferred();
                t.foreign("currency")
                    .references("code")
                    .on("currencies")
                    .name("line_items_currency")
                    .restrict_on_delete()
                    .no_action_on_update()
                    .not_valid();
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn table_options() {
    let sql = sql_for!(dialects(), |schema| {
        schema
            .create("caches", |t| {
                t.string("key").primary();
                t.text("value");
                t.engine("MEMORY");
                t.charset("ascii");
                t.collation("ascii_bin");
                t.if_not_exists();
                t.comment("process cache");
            })
            .await?;
        schema
            .create("scratch", |t| {
                t.temporary();
                t.integer("n");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn schema_qualified_tables() {
    let sql = sql_for!(vec![("postgres", postgres())], |schema| {
        schema
            .create("analytics.events", |t| {
                t.id();
                t.string("name").index();
            })
            .await?;
        schema
            .table("analytics.events", |t| {
                t.drop_index(["name"]);
                t.rename_index("analytics_events_id_index", "events_id");
                t.rename("happenings");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn mysql_options_come_from_schema_options() {
    let options = navree_schema::SchemaOptions {
        mysql_charset: Some("utf8mb4".into()),
        mysql_collation: Some("utf8mb4_unicode_ci".into()),
        mysql_engine: Some("InnoDB".into()),
        default_string_length: 191,
        default_time_precision: None,
        ..Default::default()
    };
    let schema = navree_schema::Schema::offline(crate::support::mysql()).with_options(options);
    schema
        .create("users", |t| {
            t.id();
            t.string("email").unique();
            t.timestamps();
        })
        .await
        .unwrap();
    insta::assert_snapshot!(schema.statements().join(";\n"));
}
