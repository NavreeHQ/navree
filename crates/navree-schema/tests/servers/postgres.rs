//! PostgreSQL and TimescaleDB, for real.

use navree_schema::{
    ContinuousAggregate, DumpOptions, Hypertable, IndexKind, ReferentialAction, RefreshPolicy,
    Schema, SchemaOptions, TimescaleMode, raw,
};

use crate::support::{ScratchDb, column, exec};

#[tokio::test]
async fn tables_columns_keys_and_alterations() {
    let Some(db) = ScratchDb::postgres().await else {
        return;
    };
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("companies", |t| {
            t.id();
            t.string("name").unique();
            t.string("domain").nullable().collation("C");
            t.timestamps_tz();
        })
        .await
        .unwrap();
    schema
        .create("deals", |t| {
            t.id();
            t.foreign_id("company_id").constrained().cascade_on_delete();
            t.string("title").comment("what is being sold");
            t.decimal("amount", 12, 2).default(0);
            t.enumeration("stage", ["lead", "won", "lost"])
                .default("lead");
            t.integer("probability").default(10);
            t.integer("cents").stored_as("(amount * 100)::integer");
            t.jsonb("meta").default(serde_json::json!({}));
            t.timestamps();
            t.soft_deletes();
            t.index(["stage", "created_at"]);
            t.unique(["company_id", "title"]).nulls_not_distinct();
            t.comment("open and closed deals");
        })
        .await
        .unwrap();
    schema
        .create("samples", |t| {
            t.char("country", 2);
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
            t.float("ratio");
            t.float("rough").precision(24);
            t.double("weight");
            t.boolean("active").default(true);
            t.json("settings");
            t.date("born_on").use_current();
            t.date_time("starts_at");
            t.date_time_tz("ends_at");
            t.time("opens_at");
            t.time_tz("closes_at");
            t.timestamp("seen_at").use_current();
            t.timestamp_tz("synced_at").nullable();
            t.timestamp("legacy_at").precision(0).nullable();
            t.year("vintage");
            t.binary("avatar");
            t.uuid("external_id").default(raw("gen_random_uuid()"));
            t.ulid("public_id");
            t.ip_address("ip");
            t.mac_address("mac");
            t.tsvector("search");
            t.big_integer("ordinal")
                .generated_as_with("START WITH 1000 INCREMENT BY 10");
        })
        .await
        .unwrap();

    assert!(schema.has_table("deals").await.unwrap());
    assert!(schema.has_column("deals", "stage").await.unwrap());
    let columns = schema.columns("deals").await.unwrap();
    let id = &columns[0];
    assert!(id.auto_increment, "an identity column");
    let cents = columns.iter().find(|c| c.name == "cents").unwrap();
    assert!(cents.generation.as_ref().unwrap().stored);
    assert_eq!(
        columns
            .iter()
            .find(|c| c.name == "title")
            .unwrap()
            .comment
            .as_deref(),
        Some("what is being sold")
    );
    assert_eq!(
        schema
            .column_type("samples", "synced_at", true)
            .await
            .unwrap(),
        "timestamp with time zone"
    );
    assert_eq!(
        schema
            .column_type("samples", "legacy_at", true)
            .await
            .unwrap(),
        "timestamp(0) without time zone"
    );
    assert!(
        schema
            .has_index("deals", ["stage", "created_at"], Some(IndexKind::Index))
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index(
                "deals",
                "deals_company_id_title_unique",
                Some(IndexKind::Unique)
            )
            .await
            .unwrap()
    );
    let foreign = schema.foreign_keys("deals").await.unwrap();
    assert_eq!(foreign[0].name.as_deref(), Some("deals_company_id_foreign"));
    assert_eq!(foreign[0].on_delete, Some(ReferentialAction::Cascade));
    assert_eq!(
        schema.tables().await.unwrap()[1].comment.as_deref(),
        Some("open and closed deals")
    );

    exec(&conn, "INSERT INTO companies (name) VALUES ('Agra')").await;
    exec(
        &conn,
        "INSERT INTO deals (company_id, title, amount) VALUES (1, 'Big', 12.5)",
    )
    .await;
    assert_eq!(
        column(&conn, "SELECT cents::text FROM deals").await,
        ["1250"]
    );

    schema
        .table("deals", |t| {
            t.text("title").change();
            // (PostgreSQL refuses to change the type of `amount`: the
            // generated column `cents` uses it.)
            t.big_integer("probability").nullable().default(25).change();
            t.rename_column("stage", "phase");
            t.string("owner").nullable().after("title");
            t.drop_column("meta");
            t.drop_unique(["company_id", "title"]);
            t.rename_index("deals_stage_created_at_index", "deals_by_phase");
        })
        .await
        .unwrap();
    assert_eq!(
        schema.column_type("deals", "title", false).await.unwrap(),
        "text"
    );
    let probability = schema
        .columns("deals")
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.name == "probability")
        .unwrap();
    assert_eq!(probability.type_name, "int8");
    assert!(probability.nullable);
    assert_eq!(probability.default.as_deref(), Some("25"));
    assert!(schema.has_column("deals", "phase").await.unwrap());
    assert!(!schema.has_column("deals", "meta").await.unwrap());
    assert!(
        schema
            .has_index("deals", "deals_by_phase", None)
            .await
            .unwrap()
    );
    assert_eq!(
        column(&conn, "SELECT title FROM deals").await,
        ["Big"],
        "the data survived"
    );

    schema
        .table("deals", |t| {
            t.drop_foreign(["company_id"]);
            t.drop_index("deals_by_phase");
            t.drop_primary();
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
async fn hypertables_policies_and_continuous_aggregates() {
    let Some(db) = ScratchDb::postgres().await else {
        return;
    };
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    let target = schema.target().await.unwrap();
    if target.timescaledb().is_none() && target.timescaledb_installable().is_none() {
        eprintln!("TimescaleDB is not available on this server: skipping");
        conn.close().await.unwrap();
        db.drop().await;
        return;
    }
    // A server whose template database has the extension gives every new
    // database it already installed.
    let installed = target.timescaledb().is_some();
    schema
        .create_hypertable("events", "occurred_at", |t| {
            t.uuid_primary();
            t.uuid("workspace_id");
            t.string("kind");
            t.integer("amount").default(0);
            t.timestamp_tz("occurred_at");
            t.index(["workspace_id", "occurred_at"]);
            let events = t.hypertable("occurred_at");
            events
                .chunk_interval("1 day")
                .partition_by("workspace_id", 4)
                .retain_for("90 days")
                .reorder_by("events_workspace_id_occurred_at_index");
            events
                .compress()
                .segment_by(["workspace_id"])
                .order_by("occurred_at DESC")
                .after("7 days");
        })
        .await
        .unwrap();
    if installed {
        assert!(
            !schema
                .statements()
                .iter()
                .any(|statement| statement.starts_with("CREATE EXTENSION")),
            "an installed TimescaleDB is not installed again"
        );
    } else {
        assert_eq!(
            schema.statements()[0],
            "CREATE EXTENSION IF NOT EXISTS timescaledb",
            "the fresh database had TimescaleDB available, not installed"
        );
    }
    assert_eq!(
        column(
            &conn,
            "SELECT column_name::text || ':' || dimension_type || ':' || coalesce(time_interval::text, num_partitions::text) \
             FROM timescaledb_information.dimensions WHERE hypertable_name = 'events' ORDER BY dimension_number"
        )
        .await,
        ["occurred_at:Time:1 day", "workspace_id:Space:4"]
    );
    assert_eq!(
        column(
            &conn,
            "SELECT proc_name::text FROM timescaledb_information.jobs WHERE hypertable_name = 'events' ORDER BY proc_name"
        )
        .await,
        ["policy_compression", "policy_reorder", "policy_retention"]
    );
    assert!(
        schema
            .has_index(
                "events",
                ["id", "occurred_at", "workspace_id"],
                Some(IndexKind::Primary)
            )
            .await
            .unwrap(),
        "the primary key was widened with the time and partition columns"
    );

    schema
        .create_continuous_aggregate(
            ContinuousAggregate::over("events_daily", "events")
                .time_bucket("1 day", "occurred_at", "bucket")
                .group_by("workspace_id")
                .aggregate("count(*)", "total")
                .aggregate("sum(amount)", "amount")
                .refresh(
                    RefreshPolicy::every("1 hour")
                        .start_offset("3 days")
                        .end_offset("1 hour"),
                ),
        )
        .await
        .unwrap();
    exec(
        &conn,
        "INSERT INTO events (id, workspace_id, kind, amount, occurred_at) VALUES \
         (gen_random_uuid(), '0199a7a0-0000-7000-8000-000000000001', 'sale', 5, now() - INTERVAL '2 days'), \
         (gen_random_uuid(), '0199a7a0-0000-7000-8000-000000000001', 'sale', 7, now() - INTERVAL '2 days')",
    )
    .await;
    schema
        .refresh_continuous_aggregate("events_daily", None, None)
        .await
        .unwrap();
    assert_eq!(
        column(
            &conn,
            "SELECT total::text || '/' || amount::text FROM events_daily"
        )
        .await,
        ["2/12"]
    );

    // An existing table with rows: conversion widens the key and needs
    // `migrate_data`.
    schema
        .create("audit_logs", |t| {
            t.id();
            t.string("action");
            t.timestamp_tz("created_at").use_current();
        })
        .await
        .unwrap();
    exec(
        &conn,
        "INSERT INTO audit_logs (action) VALUES ('login'), ('logout')",
    )
    .await;
    let refused = schema
        .convert_to_hypertable("audit_logs", "created_at")
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("migrate_data"), "{refused}");
    schema
        .convert_to_hypertable(
            "audit_logs",
            Hypertable::on("created_at")
                .chunk_interval("1 week")
                .migrate_data(),
        )
        .await
        .unwrap();
    assert!(
        schema
            .has_index("audit_logs", ["id", "created_at"], Some(IndexKind::Primary))
            .await
            .unwrap()
    );
    assert_eq!(
        column(&conn, "SELECT count(*) FROM audit_logs").await,
        ["2"]
    );
    // The catalog knows which tables are hypertables, by plain or
    // schema-qualified name, and their time dimensions.
    for (table, expected) in [
        ("events", true),
        ("public.events", true),
        ("audit_logs", true),
        ("events_daily", false),
        ("other_schema.events", false),
        ("missing", false),
    ] {
        assert_eq!(
            schema.is_hypertable(table).await.unwrap(),
            expected,
            "{table}"
        );
    }
    let dimension = |table: &'static str| {
        let schema = &schema;
        async move {
            schema.time_dimension(table).await.unwrap().map(|d| {
                (
                    d.column.clone(),
                    d.column_type.clone(),
                    d.chunk_interval.clone(),
                )
            })
        }
    };
    let owned = |column: &str, kind: &str, interval: &str| {
        Some((column.to_owned(), kind.to_owned(), interval.to_owned()))
    };
    assert_eq!(
        dimension("events").await,
        owned("occurred_at", "timestamp with time zone", "1 day")
    );
    assert_eq!(
        dimension("public.audit_logs").await,
        owned("created_at", "timestamp with time zone", "7 days")
    );
    assert_eq!(dimension("events_daily").await, None);
    // An integer time column (Unix milliseconds) and its integer chunks.
    schema
        .create_hypertable("ticks", "at_ms", |t| {
            t.uuid_primary();
            t.big_integer("at_ms");
            t.hypertable("at_ms").chunk_interval("86400000");
        })
        .await
        .unwrap();
    assert_eq!(
        dimension("ticks").await,
        owned("at_ms", "bigint", "86400000")
    );

    // A table referenced by a foreign key cannot have its key widened.
    schema
        .create("notes", |t| {
            t.id();
            t.timestamp_tz("written_at");
        })
        .await
        .unwrap();
    schema
        .create("note_tags", |t| {
            t.id();
            t.foreign_id("note_id").constrained();
        })
        .await
        .unwrap();
    let blocked = schema
        .convert_to_hypertable("notes", "written_at")
        .await
        .unwrap_err();
    assert!(
        blocked
            .to_string()
            .contains("1 foreign key references it: `note_tags` (note_tags_note_id_foreign)"),
        "{blocked}"
    );

    // The whole thing dumps, loads into an empty database, and dumps the same.
    let dump = schema.dump(&DumpOptions::default()).await.unwrap();
    for expected in [
        "SELECT create_hypertable('public.events', by_range('occurred_at', INTERVAL '1 day'), create_default_indexes => FALSE)",
        "SELECT add_dimension('public.events', by_hash('workspace_id', 4))",
        "timescaledb.compress_segmentby = 'workspace_id'",
        "CREATE MATERIALIZED VIEW \"public\".\"events_daily\" WITH (timescaledb.continuous",
        "SELECT add_retention_policy('public.events', drop_after => INTERVAL '90 days')",
        "SELECT add_continuous_aggregate_policy('public.events_daily'",
    ] {
        assert!(dump.contains(expected), "missing `{expected}` in:\n{dump}");
    }
    assert!(
        !dump.contains("CREATE VIEW \"public\".\"events_daily\""),
        "the aggregate is not dumped as a plain view"
    );
    let copy = ScratchDb::postgres().await.unwrap();
    let copy_conn = copy.connect().await;
    let tx = copy_conn.begin().await.unwrap();
    Schema::new(&tx).load(&dump).await.unwrap();
    tx.commit().await.unwrap();
    let again = Schema::new(&copy_conn)
        .dump(&DumpOptions::default())
        .await
        .unwrap();
    assert_eq!(again, dump);
    copy_conn.close().await.unwrap();
    copy.drop().await;
    conn.close().await.unwrap();
    db.drop().await;
}

#[tokio::test]
async fn without_timescaledb_hypertables_are_regular_tables() {
    let Some(db) = ScratchDb::postgres().await else {
        return;
    };
    let conn = db.connect().await;
    exec(&conn, "SET TIME ZONE 'UTC'").await;
    let options = SchemaOptions {
        timescaledb: TimescaleMode::Off,
        ..SchemaOptions::default()
    };
    let schema = Schema::new(&conn).with_options(options);
    schema
        .create_hypertable("metrics", "at", |t| {
            t.timestamp_tz("at");
            t.double("value");
            t.hypertable("at").retain_for("30 days");
        })
        .await
        .unwrap();
    assert!(schema.has_index("metrics", ["at"], None).await.unwrap());
    assert!(!schema.is_hypertable("metrics").await.unwrap());
    assert_eq!(
        schema.notices(),
        [
            "TimescaleDB is turned off (`schema.timescaledb = \"off\"`): `metrics` is a regular table with an index on `at`; its retention (30 days) policy is skipped"
        ]
    );
    schema
        .create_continuous_aggregate(
            ContinuousAggregate::over("metrics_hourly", "metrics")
                .time_bucket("1 hour", "at", "hour")
                .aggregate("avg(value)", "mean"),
        )
        .await
        .unwrap();
    exec(
        &conn,
        "INSERT INTO metrics VALUES ('2026-10-27 10:15:00+00', 2), ('2026-10-27 10:45:00+00', 4)",
    )
    .await;
    assert_eq!(
        column(
            &conn,
            "SELECT hour::text || '=' || mean::text FROM metrics_hourly"
        )
        .await,
        ["2026-10-27 10:00:00+00=3"]
    );
    conn.close().await.unwrap();
    db.drop().await;
}

#[tokio::test]
async fn check_constraints_are_enforced() {
    let Some(db) = ScratchDb::postgres().await else {
        return;
    };
    let conn = db.connect().await;
    crate::support::checks_are_enforced(&conn).await;
    conn.close().await.unwrap();
    db.drop().await;
}
