//! The examples of `content/docs/database/timescaledb.mdx`, written as an
//! app writes them: the migrations compiled for TimescaleDB (offline) and
//! run on SQLite, pruning and the time-bucket queries run on SQLite. The
//! facade's `time_series` suite runs Navree's own tables on real servers.

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use navree::db::{Db, DbOptions, prune_time_series};
use navree::db::{Executor as _, Statement};
use navree::orm::{Dialect, Expr, col, table};
use navree::schema::{
    ContinuousAggregate, Hypertable, RefreshPolicy, Schema, SchemaResult, Target,
};

/// "Creating hypertables": the `events` migration.
mod m20261027_090000_create_events_table {
    use navree::schema::prelude::*;

    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create_hypertable("events", "occurred_at", |t| {
                    t.uuid_primary();
                    t.foreign_uuid("workspace_id").index();
                    t.string("kind");
                    t.jsonb("payload");
                    t.timestamp_tz("occurred_at");

                    let events = t.hypertable("occurred_at");
                    events.chunk_interval("1 day").retain_for("90 days");
                    events
                        .compress()
                        .segment_by(["workspace_id"])
                        .order_by("occurred_at DESC")
                        .after("7 days");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("events").await
        }
    }
}

/// "Navree's own tables": tuning them in a migration of your own.
mod m20261101_090000_tune_time_series {
    use navree::schema::prelude::*;

    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            // A busy app: a day per audit chunk, from the next chunk on.
            schema.set_chunk_interval("audit_logs", "1 day").await?;
            // Compress Lighthouse's samples after two days.
            let mut samples = Compression::new();
            samples
                .segment_by(["kind", "name"])
                .order_by("at DESC")
                .after("2 days");
            schema.compress("lighthouse_samples", &samples).await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.remove_compression_policy("lighthouse_samples").await
        }
    }
}

/// "Pruning in code".
mod prune {
    use chrono::{TimeDelta, Utc};
    use navree::db::{Db, DbError, prune_time_series};

    /// Deletes the events older than `days`; returns how many.
    pub async fn prune_events(db: &Db, days: i64) -> Result<u64, DbError> {
        let cutoff = Utc::now() - TimeDelta::days(days);
        db.transaction(|uow| async move {
            prune_time_series(&uow, "events", "occurred_at", cutoff).await
        })
        .await
    }
}

fn timescaledb() -> Target {
    Target::postgres(17).with_timescaledb((2, 31))
}

async fn sqlite(dir: &tempfile::TempDir) -> Db {
    Db::connect(&DbOptions {
        url: format!("sqlite://{}?mode=rwc", dir.path().join("docs.db").display()),
        ..DbOptions::default()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn the_events_migration_compiles_for_timescaledb_and_runs_on_sqlite() {
    use navree::schema::Migration as _;

    let schema = Schema::offline(timescaledb());
    m20261027_090000_create_events_table::Migration
        .up(&schema)
        .await
        .unwrap();
    let statements = schema.statements();
    assert!(statements[0].starts_with("CREATE TABLE \"events\" ("));
    assert!(
        statements[0].ends_with("PRIMARY KEY (\"id\", \"occurred_at\"))"),
        "{}",
        statements[0]
    );
    for expected in [
        "SELECT create_hypertable('events', by_range('occurred_at', INTERVAL '1 day'))",
        "CREATE INDEX \"events_workspace_id_index\" ON \"events\" (\"workspace_id\")",
        "ALTER TABLE \"events\" SET (timescaledb.compress, timescaledb.compress_segmentby = 'workspace_id', timescaledb.compress_orderby = 'occurred_at DESC')",
        "SELECT add_compression_policy('events', INTERVAL '7 days')",
        "SELECT add_retention_policy('events', INTERVAL '90 days')",
    ] {
        assert!(
            statements.iter().any(|s| s == expected),
            "missing `{expected}` in {statements:#?}"
        );
    }

    let dir = tempfile::tempdir().unwrap();
    let db = sqlite(&dir).await;
    let schema = Schema::new(&db);
    m20261027_090000_create_events_table::Migration
        .up(&schema)
        .await
        .unwrap();
    assert_eq!(
        schema.notices().last().map(String::as_str),
        Some(
            "SQLite does not support TimescaleDB: `events` is a regular table with an index on `occurred_at`; its compression and retention (90 days) policies are skipped"
        )
    );
    assert!(!schema.is_hypertable("events").await.unwrap());
    assert_eq!(schema.time_dimension("events").await.unwrap(), None);
}

#[tokio::test]
async fn tuning_navree_s_tables() {
    use navree::schema::Migration as _;

    let schema = Schema::offline(timescaledb());
    m20261101_090000_tune_time_series::Migration
        .up(&schema)
        .await
        .unwrap();
    assert_eq!(
        schema.statements(),
        [
            "SELECT set_chunk_time_interval('audit_logs', INTERVAL '1 day')",
            "ALTER TABLE \"lighthouse_samples\" SET (timescaledb.compress, timescaledb.compress_segmentby = 'kind, name', timescaledb.compress_orderby = 'at DESC')",
            "SELECT add_compression_policy('lighthouse_samples', INTERVAL '2 days')",
        ]
    );
    // Without TimescaleDB both report a notice and change nothing.
    let sqlite = Schema::offline(Target::sqlite((3, 46)));
    m20261101_090000_tune_time_series::Migration
        .up(&sqlite)
        .await
        .unwrap();
    assert!(sqlite.statements().is_empty());
    assert_eq!(sqlite.notices().len(), 2);
}

#[tokio::test]
async fn converting_an_existing_table() {
    let dir = tempfile::tempdir().unwrap();
    let db = sqlite(&dir).await;
    let schema = Schema::new(&db);
    schema
        .create("visits", |t| {
            t.uuid_primary();
            t.uuid("workspace_id");
            t.timestamp_tz("created_at");
        })
        .await
        .unwrap();
    let convert = || async {
        let mut audit = Hypertable::on("created_at");
        audit
            .chunk_interval("1 week")
            .migrate_data()
            .retain_for("365 days");
        audit
            .compress()
            .segment_by(["workspace_id"])
            .after("30 days");
        schema.convert_to_hypertable("visits", &audit).await
    };
    convert().await.unwrap();
    assert!(
        schema
            .has_index("visits", ["created_at"], None)
            .await
            .unwrap()
    );
    if !schema.is_hypertable("visits").await.unwrap() {
        // SQLite: converting again adds nothing.
        let before = schema.statements().len();
        convert().await.unwrap();
        assert_eq!(schema.statements().len(), before);
    }
}

#[tokio::test]
async fn reading_the_time_dimension() -> SchemaResult {
    let dir = tempfile::tempdir().unwrap();
    let db = sqlite(&dir).await;
    let mut shown = Vec::new();
    if let Some(time) = Schema::new(&db).time_dimension("events").await? {
        shown.push(format!(
            "{} ({}), chunks of {}",
            time.column, time.column_type, time.chunk_interval
        ));
    }
    assert!(shown.is_empty(), "no hypertables on SQLite");
    Ok(())
}

#[tokio::test]
async fn pruning_in_code() {
    use navree::schema::Migration as _;

    let dir = tempfile::tempdir().unwrap();
    let db = sqlite(&dir).await;
    m20261027_090000_create_events_table::Migration
        .up(&Schema::new(&db))
        .await
        .unwrap();
    for days in [120, 100, 10] {
        db.execute(&Statement::portable(
            "INSERT INTO events (id, workspace_id, kind, payload, occurred_at) VALUES (?, ?, ?, ?, ?)",
            [
                uuid::Uuid::now_v7().into(),
                uuid::Uuid::now_v7().into(),
                "page_view".into(),
                serde_json::json!({}).into(),
                (Utc::now() - TimeDelta::days(days)).into(),
            ],
        ))
        .await
        .unwrap();
    }
    assert_eq!(prune::prune_events(&db, 90).await.unwrap(), 2);
    assert_eq!(prune::prune_events(&db, 90).await.unwrap(), 0);
    // The helper the example wraps, on its own.
    assert_eq!(
        prune_time_series(&db, "events", "occurred_at", Utc::now())
            .await
            .unwrap(),
        1
    );
}

/// `readings`: one row every 10 minutes for an hour and a half from 08:00,
/// at 20.0 °C and up a degree each time.
async fn readings(db: &Db) -> DateTime<Utc> {
    Schema::new(&db)
        .create_hypertable("readings", "taken_at", |t| {
            t.id();
            t.double("celsius");
            t.timestamp_tz("taken_at");
        })
        .await
        .unwrap();
    let start = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
    for step in 0..9_i32 {
        db.execute(&Statement::portable(
            "INSERT INTO readings (celsius, taken_at) VALUES (?, ?)",
            [
                (20.0 + f64::from(step)).into(),
                (start + TimeDelta::minutes(10 * i64::from(step))).into(),
            ],
        ))
        .await
        .unwrap();
    }
    start
}

#[tokio::test]
async fn time_bucket_queries() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir().unwrap();
    let db = sqlite(&dir).await;
    let start = readings(&db).await;

    let since = start - TimeDelta::days(1);
    let points = table("readings")
        .select_time_bucket("15 minutes", "taken_at", "bucket")
        .select_expr(Expr::avg(col("celsius")), "average")
        .select_expr(Expr::count_all(), "samples")
        .where_(col("taken_at").gte(since))
        .group_by(["bucket"])
        .order_by("bucket")
        .get(&db)
        .await?;
    let mut shown = Vec::new();
    for point in &points {
        let bucket: DateTime<Utc> = point.get("bucket")?;
        let average: f64 = point.get("average")?;
        let samples: i64 = point.get("samples")?;
        shown.push(format!("{} {average:.1} {samples}", bucket.format("%H:%M")));
    }
    assert_eq!(
        shown,
        [
            "08:00 20.5 2",
            "08:15 22.0 1",
            "08:30 23.5 2",
            "08:45 25.0 1",
            "09:00 26.5 2",
            "09:15 28.0 1"
        ]
    );

    let days = table("readings")
        .select_time_bucket("1 day", "taken_at", "day")
        .select_first("celsius", "taken_at", "opening")
        .select_last("celsius", "taken_at", "closing")
        .group_by(["day"])
        .order_by("day");
    let sql = days.clone().to_sql(Dialect::Postgres)?.sql;
    assert!(sql.contains("time_bucket(CAST("), "{sql}");
    assert!(sql.contains("first(\"celsius\", \"taken_at\")"), "{sql}");
    let error = days.get(&db).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "first(value, time) (a TimescaleDB aggregate) is not supported on SQLite"
    );

    // Buckets from another origin: 08:05.
    let origin = start + TimeDelta::minutes(5);
    let shifted = table("readings")
        .select_expr(
            Expr::time_bucket_from("1 hour", col("taken_at"), Expr::val(origin)),
            "hour",
        )
        .select_expr(Expr::count_all(), "samples")
        .group_by(["hour"])
        .order_by("hour")
        .get(&db)
        .await?;
    let counts: Vec<i64> = shifted
        .iter()
        .map(|row| row.get::<i64>("samples"))
        .collect::<Result<_, _>>()?;
    assert_eq!(counts, [1, 6, 2], "07:05, 08:05 and 09:05");
    Ok(())
}

#[tokio::test]
async fn the_continuous_aggregate_compiles_for_timescaledb() -> SchemaResult {
    let schema = Schema::offline(timescaledb());
    schema
        .create_continuous_aggregate(
            ContinuousAggregate::over("events_daily", "events")
                .time_bucket("1 day", "occurred_at", "day")
                .group_by("workspace_id")
                .aggregate("count(*)", "total")
                .aggregate("count(DISTINCT kind)", "kinds")
                .where_raw("kind <> 'heartbeat'")
                .refresh(
                    RefreshPolicy::every("1 hour")
                        .start_offset("3 days")
                        .end_offset("1 hour"),
                ),
        )
        .await?;
    let statements = schema.statements();
    assert!(
        statements[0]
            .starts_with("CREATE MATERIALIZED VIEW \"events_daily\" WITH (timescaledb.continuous"),
        "{statements:#?}"
    );
    assert!(statements[1].starts_with("SELECT add_continuous_aggregate_policy('events_daily'"));
    Ok(())
}
