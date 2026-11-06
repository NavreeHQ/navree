//! MySQL, for real.

use navree_schema::{ContinuousAggregate, DumpOptions, IndexKind, ReferentialAction, Schema};

use crate::support::{ScratchDb, column, exec};

#[tokio::test]
async fn tables_columns_keys_and_alterations() {
    let Some(db) = ScratchDb::mysql().await else {
        return;
    };
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("companies", |t| {
            t.id();
            t.string("name").unique();
            t.timestamps();
        })
        .await
        .unwrap();
    schema
        .create("deals", |t| {
            t.id();
            t.foreign_id("company_id").constrained().cascade_on_delete();
            t.string("title").comment("what is being sold");
            t.decimal("amount", 12, 2).unsigned().default(0);
            t.enumeration("stage", ["lead", "won", "lost"])
                .default("lead");
            t.set("flags", ["hot", "cold"]).nullable();
            t.integer("cents").stored_as("amount * 100");
            t.integer("half").virtual_as("cents / 2");
            t.json("meta").default(serde_json::json!({"a": 1}));
            t.text("notes").fulltext();
            t.timestamp("seen_at").use_current().use_current_on_update();
            t.timestamps();
            t.soft_deletes();
            t.index(["stage", "created_at"]);
            t.comment("open and closed deals");
        })
        .await
        .unwrap();
    schema
        .create("samples", |t| {
            t.char("country", 2);
            t.tiny_text("note");
            t.medium_text("summary");
            t.long_text("archive");
            t.tiny_integer("rank");
            t.small_integer("score");
            t.medium_integer("views");
            t.unsigned_integer("hits");
            t.big_integer("total");
            t.float("ratio");
            t.double("weight");
            t.boolean("active").default(true);
            t.date("born_on");
            t.date_time_tz("ends_at").nullable();
            t.time("opens_at").precision(3);
            t.year("vintage");
            t.binary("avatar").nullable();
            t.binary("digest").length(32).fixed();
            t.uuid("external_id");
            t.ulid("public_id");
            t.ip_address("ip");
            t.mac_address("mac");
            t.geometry("spot")
                .subtype("point")
                .srid(4326)
                .spatial_index();
            t.integer("secret").invisible().default(0);
        })
        .await
        .unwrap();

    assert!(schema.has_table("deals").await.unwrap());
    assert!(
        schema
            .has_columns("deals", ["stage", "flags"])
            .await
            .unwrap()
    );
    let columns = schema.columns("deals").await.unwrap();
    assert!(columns[0].auto_increment);
    let half = columns.iter().find(|c| c.name == "half").unwrap();
    assert!(!half.generation.as_ref().unwrap().stored);
    assert_eq!(
        schema.column_type("deals", "amount", true).await.unwrap(),
        "decimal(12,2) unsigned"
    );
    assert!(
        schema
            .has_index("deals", ["notes"], Some(IndexKind::Fulltext))
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index("samples", ["spot"], Some(IndexKind::Spatial))
            .await
            .unwrap()
    );
    let foreign = schema.foreign_keys("deals").await.unwrap();
    assert_eq!(foreign[0].on_delete, Some(ReferentialAction::Cascade));

    exec(&conn, "INSERT INTO companies (name) VALUES ('Agra')").await;
    exec(
        &conn,
        "INSERT INTO deals (company_id, title, amount, notes) VALUES (1, 'Big', 12.5, 'hello world')",
    )
    .await;
    assert_eq!(
        column(&conn, "SELECT CAST(cents AS CHAR) FROM deals").await,
        ["1250"]
    );

    schema
        .table("deals", |t| {
            t.text("title").change();
            t.string("owner").nullable().after("title");
            t.string("code").nullable().first();
            t.rename_column("stage", "phase");
            t.drop_column("meta");
            t.rename_index("deals_stage_created_at_index", "deals_by_phase");
        })
        .await
        .unwrap();
    assert_eq!(
        schema.column_listing("deals").await.unwrap()[..4],
        ["code", "id", "company_id", "title"]
    );
    assert_eq!(
        schema.column_listing("deals").await.unwrap()[4],
        "owner",
        "placed after title"
    );
    assert!(
        schema
            .has_index("deals", "deals_by_phase", None)
            .await
            .unwrap()
    );
    assert_eq!(column(&conn, "SELECT title FROM deals").await, ["Big"]);

    schema
        .table("deals", |t| {
            t.drop_foreign(["company_id"]);
            t.drop_index("deals_by_phase");
        })
        .await
        .unwrap();
    assert!(schema.foreign_keys("deals").await.unwrap().is_empty());
    schema.rename("deals", "opportunities").await.unwrap();
    assert!(schema.has_table("opportunities").await.unwrap());
    schema.drop_all_tables().await.unwrap();
    assert!(schema.tables().await.unwrap().is_empty());
    conn.close().await.unwrap();
    db.drop().await;
}

#[tokio::test]
async fn hypertables_and_aggregates_fall_back_to_tables_and_views() {
    let Some(db) = ScratchDb::mysql().await else {
        return;
    };
    let conn = db.connect().await;
    exec(&conn, "SET time_zone = '+00:00'").await;
    let schema = Schema::new(&conn);
    schema
        .create_hypertable("readings", "taken_at", |t| {
            t.id();
            t.unsigned_big_integer("sensor_id");
            t.double("value");
            t.date_time("taken_at");
            t.hypertable("taken_at")
                .partition_by("sensor_id", 4)
                .retain_for("1 year");
        })
        .await
        .unwrap();
    assert!(
        schema
            .has_index(
                "readings",
                ["id", "taken_at", "sensor_id"],
                Some(IndexKind::Primary)
            )
            .await
            .unwrap(),
        "the same key as on TimescaleDB"
    );
    assert!(
        schema
            .has_index("readings", ["taken_at"], None)
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index("readings", ["sensor_id", "taken_at"], None)
            .await
            .unwrap()
    );
    schema
        .create_continuous_aggregate(
            ContinuousAggregate::over("readings_quarter_hourly", "readings")
                .time_bucket("15 minutes", "taken_at", "slot")
                .group_by("sensor_id")
                .aggregate("avg(value)", "mean"),
        )
        .await
        .unwrap();
    exec(
        &conn,
        "INSERT INTO readings (sensor_id, value, taken_at) VALUES \
         (1, 2, '2026-10-27 10:01:00'), (1, 4, '2026-10-27 10:14:59'), (1, 9, '2026-10-27 10:15:00')",
    )
    .await;
    assert_eq!(
        column(
            &conn,
            "SELECT CONCAT(slot, '=', mean) FROM readings_quarter_hourly ORDER BY slot"
        )
        .await,
        ["2026-10-27 10:00:00=3", "2026-10-27 10:15:00=9"]
    );
    conn.close().await.unwrap();
    db.drop().await;
}

#[tokio::test]
async fn dumps_load_into_an_empty_database_and_dump_the_same() {
    let Some(db) = ScratchDb::mysql().await else {
        return;
    };
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("companies", |t| {
            t.id();
            t.string("name").unique();
        })
        .await
        .unwrap();
    schema
        .create("deals", |t| {
            t.id();
            t.foreign_id("company_id").constrained().cascade_on_delete();
            t.string("title");
            t.integer("cents").stored_as("char_length(title) * 100");
        })
        .await
        .unwrap();
    schema
        .create("navree_migrations", |t| {
            t.string("module");
            t.string("name");
            t.big_integer("batch");
            t.date_time("applied_at");
            t.primary(["module", "name"]);
        })
        .await
        .unwrap();
    exec(
        &conn,
        "CREATE VIEW big_deals AS SELECT title FROM deals WHERE cents > 100",
    )
    .await;
    exec(
        &conn,
        "CREATE VIEW biggest_deals AS SELECT title FROM big_deals WHERE title <> ''",
    )
    .await;
    exec(
        &conn,
        "CREATE TRIGGER deals_title BEFORE INSERT ON deals FOR EACH ROW BEGIN IF NEW.title = '' THEN SET NEW.title = 'untitled'; END IF; END",
    )
    .await;
    exec(
        &conn,
        "CREATE PROCEDURE close_deal(IN deal BIGINT) BEGIN DELETE FROM deals WHERE id = deal; SELECT 1; END",
    )
    .await;
    exec(&conn, "INSERT INTO companies (name) VALUES ('Agra')").await;
    exec(
        &conn,
        "INSERT INTO navree_migrations VALUES ('crm', 'm0001_create_companies', 1, '2026-10-27 10:00:00')",
    )
    .await;

    let options = DumpOptions::default().with_data("navree_migrations");
    let dump = schema.dump(&options).await.unwrap();
    for expected in [
        "SET FOREIGN_KEY_CHECKS=0;",
        "DELIMITER ;;",
        "CREATE PROCEDURE `close_deal`",
        "CREATE TRIGGER `deals_title`",
        "INSERT INTO `navree_migrations` (`module`, `name`, `batch`, `applied_at`) VALUES ('crm', 'm0001_create_companies', '1', '2026-10-27 10:00:00.000000');",
    ] {
        assert!(dump.contains(expected), "missing `{expected}` in:\n{dump}");
    }
    assert!(!dump.contains("DEFINER"), "{dump}");
    assert!(!dump.contains("AUTO_INCREMENT=2"), "{dump}");
    assert!(
        dump.find("CREATE VIEW `big_deals`") < dump.find("CREATE VIEW `biggest_deals`"),
        "views after the views they read"
    );

    let copy = ScratchDb::mysql().await.unwrap();
    let copy_conn = copy.connect().await;
    let tx = copy_conn.begin().await.unwrap();
    Schema::new(&tx).load(&dump).await.unwrap();
    tx.commit().await.unwrap();
    let again = Schema::new(&copy_conn).dump(&options).await.unwrap();
    assert_eq!(again, dump);
    exec(&copy_conn, "INSERT INTO companies (name) VALUES ('Beta')").await;
    exec(
        &copy_conn,
        "INSERT INTO deals (company_id, title) VALUES (1, '')",
    )
    .await;
    assert_eq!(
        column(&copy_conn, "SELECT title FROM deals").await,
        ["untitled"],
        "the trigger was loaded"
    );
    copy_conn.close().await.unwrap();
    copy.drop().await;
    conn.close().await.unwrap();
    db.drop().await;
}

#[tokio::test]
async fn check_constraints_are_enforced() {
    let Some(db) = ScratchDb::mysql().await else {
        return;
    };
    let conn = db.connect().await;
    crate::support::checks_are_enforced(&conn).await;
    conn.close().await.unwrap();
    db.drop().await;
}
