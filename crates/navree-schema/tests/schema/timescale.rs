//! TimescaleDB: hypertables, their keys, policies and continuous
//! aggregates, compiled for TimescaleDB, and the fallbacks every other
//! database gets for the same migration.

use navree_schema::{
    Compression, ContinuousAggregate, Hypertable, KeyStrategy, RefreshPolicy, Schema,
    SchemaOptions, Target, TimescaleMode,
};

use crate::sql_for;
use crate::support::{everything, render, timescaledb};

#[tokio::test]
async fn a_hypertable_on_every_database() {
    let sql = sql_for!(everything(), |schema| {
        schema
            .create_hypertable("events", "occurred_at", |t| {
                t.uuid_primary();
                t.foreign_uuid("workspace_id");
                t.string("kind");
                t.timestamp_tz("occurred_at");
                t.jsonb("payload");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn chunks_partitions_and_policies() {
    let sql = sql_for!(everything(), |schema| {
        schema
            .create("events", |t| {
                t.uuid("id");
                t.uuid("workspace_id");
                t.timestamp_tz("occurred_at");
                t.integer("amount").default(0);
                t.primary(["id", "occurred_at", "workspace_id"]);
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
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn older_timescaledb_uses_the_positional_signature() {
    let sql = sql_for!(
        [(
            "timescaledb 2.11",
            Target::postgres(15).with_timescaledb((2, 11))
        )],
        |schema| {
            schema
                .create("readings", |t| {
                    t.big_integer("sensor_id");
                    t.timestamp_tz("taken_at");
                    t.double("value");
                    t.hypertable("taken_at")
                        .chunk_interval("12 hours")
                        .partition_by("sensor_id", 8)
                        .without_default_indexes();
                })
                .await
        }
    );
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn integer_time_columns_take_integer_intervals() {
    let sql = sql_for!(
        [
            ("timescaledb", timescaledb()),
            ("sqlite", Target::sqlite((3, 46)))
        ],
        |schema| {
            schema
                .create("ticks", |t| {
                    t.big_integer("epoch_ms");
                    t.integer("price");
                    t.hypertable("epoch_ms").chunk_interval("86400000");
                })
                .await
        }
    );
    insta::assert_snapshot!(sql);

    let schema = Schema::offline(timescaledb());
    let error = schema
        .create("ticks", |t| {
            t.big_integer("epoch_ms");
            t.hypertable("epoch_ms").chunk_interval("1 day");
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot make `ticks` a hypertable: time column `epoch_ms` is an integer, so the chunk interval must be a whole number of its units (got `1 day`)"
    );

    let error = schema
        .create("ticks", |t| {
            t.timestamp_tz("at");
            t.hypertable("at").chunk_interval("3600");
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot make `ticks` a hypertable: time column `at` holds dates or timestamps, so the chunk interval must be an interval such as `1 day` (got `3600`)"
    );
}

#[tokio::test]
async fn primary_keys_are_widened_with_the_time_column() {
    let sql = sql_for!(everything(), |schema| {
        schema
            .create("job_runs", |t| {
                t.id();
                t.string("job");
                t.timestamp_tz("started_at");
                t.hypertable("started_at");
            })
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn keys_the_builder_must_not_change_are_errors() {
    let schema = Schema::offline(timescaledb());
    let rejected = schema
        .create("events", |t| {
            t.uuid_primary();
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at").keys(KeyStrategy::Reject);
        })
        .await
        .unwrap_err();
    assert_eq!(
        rejected.to_string(),
        "cannot make `events` a hypertable: its primary key (id) lacks occurred_at: TimescaleDB enforces uniqueness per chunk, so the key must include the time column. Declare `t.primary([\"id\", \"occurred_at\"])`, or let the builder widen it (`hypertable_keys = \"extend\"`)"
    );

    let options = SchemaOptions {
        hypertable_keys: KeyStrategy::Reject,
        ..SchemaOptions::default()
    };
    let schema = Schema::offline(timescaledb()).with_options(options);
    let from_options = schema
        .create("events", |t| {
            t.uuid_primary();
            t.uuid("workspace_id");
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at").partition_by("workspace_id", 2);
        })
        .await
        .unwrap_err();
    assert_eq!(
        from_options.to_string(),
        "cannot make `events` a hypertable: its primary key (id) lacks occurred_at and workspace_id: TimescaleDB enforces uniqueness per chunk, so the key must include the time and partition column. Declare `t.primary([\"id\", \"occurred_at\", \"workspace_id\"])`, or let the builder widen it (`hypertable_keys = \"extend\"`)"
    );

    let schema = Schema::offline(Target::sqlite((3, 46)));
    let unique = schema
        .create("events", |t| {
            t.uuid_primary();
            t.string("dedupe_key").unique();
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at");
        })
        .await
        .unwrap_err();
    assert_eq!(
        unique.to_string(),
        "cannot make `events` a hypertable: unique index `events_dedupe_key_unique` (dedupe_key) does not include occurred_at: TimescaleDB enforces uniqueness per chunk, so every unique index of a hypertable must include its partitioning columns. Add occurred_at to the index, or drop it",
        "the same check on every database, so a migration that works in tests works in production"
    );
}

#[tokio::test]
async fn time_columns_must_exist_and_hold_time() {
    let schema = Schema::offline(timescaledb());
    let missing = schema
        .create("events", |t| {
            t.uuid_primary();
            t.hypertable("occurred_at");
        })
        .await
        .unwrap_err();
    assert_eq!(
        missing.to_string(),
        "cannot make `events` a hypertable: its time column `occurred_at` is not a column of the table; add it, e.g. `t.timestamp_tz(\"occurred_at\")`"
    );
    let text = schema
        .create("events", |t| {
            t.string("occurred_at");
            t.hypertable("occurred_at");
        })
        .await
        .unwrap_err();
    assert_eq!(
        text.to_string(),
        "cannot make `events` a hypertable: time column `occurred_at` is a string column; a hypertable is partitioned by a date, timestamp or integer column"
    );
    let partition = schema
        .create("events", |t| {
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at").partition_by("tenant_id", 4);
        })
        .await
        .unwrap_err();
    assert_eq!(
        partition.to_string(),
        "cannot make `events` a hypertable: its partition column `tenant_id` is not a column of the table"
    );
    let interval = schema
        .create("events", |t| {
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at").retain_for("ninety days");
        })
        .await
        .unwrap_err();
    assert!(
        interval
            .to_string()
            .starts_with("`ninety days` is not an interval"),
        "{interval}"
    );
}

#[tokio::test]
async fn the_extension_is_installed_when_allowed() {
    let installable = Target::postgres(17).with_timescaledb_installable((2, 17));
    let sql = sql_for!([("installable", installable.clone())], |schema| {
        schema
            .create_hypertable("metrics", "at", |t| {
                t.timestamp_tz("at");
                t.double("value");
            })
            .await?;
        schema
            .create_hypertable("samples", "at", |t| {
                t.timestamp_tz("at");
            })
            .await
    });
    insta::assert_snapshot!(sql);

    let options = SchemaOptions {
        create_extensions: false,
        ..SchemaOptions::default()
    };
    let schema = Schema::offline(installable).with_options(options);
    schema
        .create_hypertable("metrics", "at", |t| {
            t.timestamp_tz("at");
        })
        .await
        .unwrap();
    insta::assert_snapshot!(render("extensions off", &schema, Ok(())));
}

#[tokio::test]
async fn require_and_off_modes() {
    let require = SchemaOptions {
        timescaledb: TimescaleMode::Require,
        ..SchemaOptions::default()
    };
    let schema = Schema::offline(Target::postgres(17)).with_options(require.clone());
    let error = schema
        .create_hypertable("metrics", "at", |t| {
            t.timestamp_tz("at");
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot make `metrics` a hypertable: TimescaleDB is not installed in this PostgreSQL database, and `schema.timescaledb = \"require\"`: install TimescaleDB in PostgreSQL 17.0.0 (it must be in `shared_preload_libraries`, and a superuser must run `CREATE EXTENSION timescaledb`)"
    );
    // Other databases still fall back, so tests on SQLite run the same migration.
    let schema = Schema::offline(Target::sqlite((3, 46))).with_options(require);
    schema
        .create_hypertable("metrics", "at", |t| {
            t.timestamp_tz("at");
        })
        .await
        .unwrap();

    let off = SchemaOptions {
        timescaledb: TimescaleMode::Off,
        ..SchemaOptions::default()
    };
    let schema = Schema::offline(timescaledb()).with_options(off);
    schema
        .create_hypertable("metrics", "at", |t| {
            t.timestamp_tz("at");
            t.hypertable("at").retain_for("30 days");
        })
        .await
        .unwrap();
    insta::assert_snapshot!(render("off", &schema, Ok(())));
}

#[tokio::test]
async fn continuous_aggregates() {
    let sql = sql_for!(everything(), |schema| {
        schema
            .create_continuous_aggregate(
                ContinuousAggregate::over("events_daily", "events")
                    .time_bucket("1 day", "occurred_at", "bucket")
                    .group_by("workspace_id")
                    .aggregate("count(*)", "total")
                    .aggregate("sum(amount)", "amount")
                    .where_raw("kind <> 'noise'")
                    .refresh(
                        RefreshPolicy::every("1 hour")
                            .start_offset("3 days")
                            .end_offset("1 hour"),
                    ),
            )
            .await?;
        schema
            .create_continuous_aggregate(
                ContinuousAggregate::over("events_monthly", "events")
                    .time_bucket("1 month", "occurred_at", "month")
                    .aggregate("count(*)", "total")
                    .materialized_only(false),
            )
            .await?;
        schema
            .create_continuous_aggregate(
                ContinuousAggregate::over("events_hourly", "events")
                    .time_bucket("15 minutes", "occurred_at", "slot")
                    .aggregate("max(amount)", "peak"),
            )
            .await?;
        schema.drop_continuous_aggregate("events_hourly").await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn continuous_aggregates_from_sql() {
    let sql = sql_for!(
        [
            ("timescaledb", timescaledb()),
            ("postgres", Target::postgres(17))
        ],
        |schema| {
            schema
            .create_continuous_aggregate(
                ContinuousAggregate::new(
                    "readings_hourly",
                    "SELECT time_bucket(INTERVAL '1 hour', taken_at) AS hour, avg(value) AS mean \
                     FROM readings GROUP BY hour;",
                )
                .refresh(RefreshPolicy::every("30 minutes").end_offset("1 hour")),
            )
            .await
        }
    );
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn policies_on_existing_hypertables() {
    let sql = sql_for!(everything(), |schema| {
        let mut compression = Compression::new();
        compression.segment_by(["device_id"]).after("14 days");
        schema.compress("readings", &compression).await?;
        schema.remove_compression_policy("readings").await?;
        schema.add_compression_policy("readings", "30 days").await?;
        schema
            .add_continuous_aggregate_policy(
                "readings_hourly",
                &RefreshPolicy::every("1 hour").start_offset("1 day"),
            )
            .await?;
        schema
            .remove_continuous_aggregate_policy("readings_hourly")
            .await?;
        schema.add_retention_policy("readings", "1 year").await?;
        schema.remove_retention_policy("readings").await?;
        schema
            .add_reorder_policy("readings", "readings_device_id_taken_at_index")
            .await?;
        schema.remove_reorder_policy("readings").await?;
        schema.set_chunk_interval("readings", "2 days").await?;
        schema
            .refresh_continuous_aggregate("readings_hourly", Some("2 days"), None)
            .await
    });
    insta::assert_snapshot!(sql);
}

#[tokio::test]
async fn converting_a_table_needs_the_database() {
    let schema = Schema::offline(timescaledb());
    let mut audit = Hypertable::on("created_at");
    audit
        .chunk_interval("1 day")
        .partition_by("workspace_id", 4)
        .migrate_data();
    let error = schema
        .convert_to_hypertable("audit_logs", &audit)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "converting `audit_logs` to a hypertable needs a database connection; this schema is offline (it only records statements)"
    );
    // The one-expression form of the same description.
    let error = schema
        .convert_to_hypertable(
            "audit_logs",
            Hypertable::on("created_at")
                .chunk_interval("1 day")
                .partition_by("workspace_id", 4)
                .migrate_data(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("offline"), "{error}");
}

#[tokio::test]
async fn only_timescaledb_has_hypertables() {
    // Asking TimescaleDB reads its catalog: an offline schema cannot.
    let error = Schema::offline(timescaledb())
        .is_hypertable("events")
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "checking whether `events` is a hypertable needs a database connection; this schema is offline (it only records statements)"
    );
    // Every other database answers without one.
    for target in [
        Target::postgres(17),
        Target::postgres(17).with_timescaledb_installable((2, 17)),
        Target::mysql((8, 4)),
        Target::mariadb((11, 4)),
        Target::sqlite((3, 46)),
    ] {
        let schema = Schema::offline(target.clone());
        assert!(
            !schema.is_hypertable("events").await.unwrap(),
            "{}",
            target.describe()
        );
        assert_eq!(
            schema.time_dimension("events").await.unwrap(),
            None,
            "{}",
            target.describe()
        );
        assert!(schema.statements().is_empty());
    }
    let error = Schema::offline(timescaledb())
        .time_dimension("events")
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "reading the time dimension of `events` needs a database connection; this schema is offline (it only records statements)"
    );
}

#[tokio::test]
async fn a_configured_hypertable_can_be_passed_to_create_hypertable() {
    let sql = sql_for!([("timescaledb", timescaledb())], |schema| {
        schema
            .create_hypertable(
                "events",
                Hypertable::on("occurred_at").chunk_interval("6 hours"),
                |t| {
                    t.uuid("id");
                    t.timestamp_tz("occurred_at");
                    t.primary(["id", "occurred_at"]);
                },
            )
            .await
    });
    insta::assert_snapshot!(sql);
}
