//! Navree's own time-series tables, as an app gets them: the audit log, the
//! webhook delivery log, Fleet's job history and metrics, the scheduler's
//! runs and Lighthouse's events are TimescaleDB hypertables, and regular
//! tables with time indexes on every other database.
//!
//! The SQLite test always runs. The TimescaleDB and MySQL ones run against
//! real servers: build with `--features postgres,mysql` and set
//! `NAVREE_TEST_POSTGRES_URL` and `NAVREE_TEST_MYSQL_URL` to URLs whose users
//! may create databases (each test works in a database of its own):
//!
//! ```text
//! NAVREE_TEST_POSTGRES_URL=postgres://postgres:secret@127.0.0.1:55432/postgres \
//! NAVREE_TEST_MYSQL_URL=mysql://root:secret@127.0.0.1:53306/test \
//! cargo test -p navree --features postgres,mysql --test time_series
//! ```

use navree::db::navree_schema::Schema;
use navree::db::{Db, MIGRATIONS_TABLE};
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder};
use sea_orm::{ConnectionTrait, Statement};

/// Every time-series table of the framework, with its time column.
const TIME_SERIES: [(&str, &str); 9] = [
    ("audit_logs", "created_at"),
    ("webhook_deliveries", "created_at"),
    ("webhook_attempts", "created_at"),
    ("fleet_jobs", "finished_at"),
    ("fleet_metrics", "at"),
    ("schedule_runs", "tick"),
    ("lighthouse_samples", "at"),
    ("lighthouse_occurrences", "at"),
    ("lighthouse_aggregates", "bucket"),
];

/// The conversion migrations, one per module, by name.
const CONVERSIONS: [&str; 5] = [
    "m20261027_000001_convert_schedule_runs_to_hypertable",
    "m20261027_000001_convert_webhook_deliveries_to_hypertables",
    "m20261027_000002_convert_audit_logs_to_hypertable",
    "m20261027_000002_convert_fleet_history_to_hypertables",
    "m20261027_000002_convert_lighthouse_events_to_hypertables",
];

/// An app with every module that keeps time series, migrated at boot (the
/// database comes from the caller).
fn app() -> TestAppBuilder {
    TestApp::builder()
        .config("database.migrate_on_boot", true)
        .config("audit.routes", false)
        .config("webhooks.routes", false)
        .config("fleet.dashboard", false)
        .config("lighthouse.dashboard", false)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(QueueModule::from_config())
        .module(HttpClientModule::from_config())
        .module(AuditModule::from_config())
        .module(WebhooksModule::from_config())
        .module(FleetModule::from_config())
        .module(ScheduleModule::for_root())
        .module(LighthouseModule::from_config())
}

/// The names of the applied migrations that convert tables.
async fn conversions_applied(db: &Db) -> Vec<String> {
    let mut names: Vec<String> = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            format!("SELECT name FROM {MIGRATIONS_TABLE}"),
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .filter(|name| name.contains("_to_hypertable"))
        .collect();
    names.sort();
    names
}

/// Every table is a regular table, indexed by its time column.
async fn assert_regular_tables(db: &Db) {
    let schema = Schema::new(&db);
    for (table, time) in TIME_SERIES {
        assert!(!schema.is_hypertable(table).await.unwrap(), "{table}");
        assert!(
            schema.has_index(table, [time], None).await.unwrap(),
            "{table} is indexed by {time}"
        );
    }
}

/// Attempts reference their endpoint (with `ON DELETE CASCADE`), not their
/// delivery.
async fn assert_attempts_reference_endpoints(db: &Db) {
    let keys = Schema::new(&db)
        .foreign_keys("webhook_attempts")
        .await
        .unwrap();
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert_eq!(keys[0].columns, ["endpoint_id"]);
    assert_eq!(keys[0].foreign_table, "webhook_endpoints");
}

#[tokio::test]
async fn sqlite_keeps_regular_tables_with_time_indexes() {
    let app = app().temp_database().build().await;
    let db = app.get::<Db>().await;
    assert_eq!(conversions_applied(&db).await, CONVERSIONS);
    assert_regular_tables(&db).await;
    assert_attempts_reference_endpoints(&db).await;
    app.shutdown().await.unwrap();
}

/// Databases of their own on the test servers.
#[cfg(any(feature = "postgres", feature = "mysql"))]
mod scratch {
    use std::sync::atomic::{AtomicU32, Ordering};

    use sea_orm::{ConnectionTrait, Database};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A database created for one test; [`drop`](Scratch::drop) removes it.
    pub struct Scratch {
        admin: String,
        name: String,
        pub url: String,
        mysql: bool,
    }

    impl Scratch {
        /// A fresh database on the server of `variable`, or `None` (with a
        /// notice) when it is unset.
        pub async fn new(variable: &str, mysql: bool) -> Option<Scratch> {
            let Ok(admin) = std::env::var(variable) else {
                eprintln!("{variable} is not set: skipping this real-server test");
                return None;
            };
            let name = format!(
                "navree_time_series_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            );
            let conn = Database::connect(&admin).await.expect("connects");
            conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
                .await
                .expect("the test user may create databases");
            conn.close().await.expect("closes");
            let (base, query) = match admin.split_once('?') {
                Some((base, query)) => (base.to_owned(), format!("?{query}")),
                None => (admin.clone(), String::new()),
            };
            let (server, _) = base.rsplit_once('/').expect("a URL with a database");
            let url = format!("{server}/{name}{query}");
            Some(Scratch {
                admin,
                name,
                url,
                mysql,
            })
        }

        /// Drops the database.
        pub async fn drop(self) {
            let conn = Database::connect(&self.admin).await.expect("connects");
            let sql = if self.mysql {
                format!("DROP DATABASE IF EXISTS {}", self.name)
            } else {
                format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)
            };
            conn.execute_unprepared(&sql).await.expect("drops");
            conn.close().await.expect("closes");
        }
    }
}

#[cfg(feature = "postgres")]
mod timescaledb {
    use chrono::{TimeDelta, Utc};
    use navree::db::navree_schema::{IndexKind, Schema};
    use navree::db::{Db, MIGRATIONS_TABLE};
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use super::scratch::Scratch;
    use super::{CONVERSIONS, app, assert_attempts_reference_endpoints, conversions_applied};

    async fn number(db: &Db, sql: &str) -> i64 {
        db.conn()
            .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .unwrap()
            .try_get_by_index::<i64>(0)
            .unwrap()
    }

    /// The webhook tables as the release before the delivery log became
    /// time series created them: attempts reference their delivery.
    const PREVIOUS_WEBHOOK_TABLES: &str = r#"
CREATE TABLE "webhook_endpoints" ("id" uuid NOT NULL PRIMARY KEY, "workspace_id" uuid NULL, "url" text NOT NULL, "description" varchar NULL, "events" json NOT NULL, "headers" json NOT NULL, "secret" text NOT NULL, "previous_secret" text NULL, "previous_secret_expires_at" timestamp with time zone NULL, "enabled" bool NOT NULL, "disabled_at" timestamp with time zone NULL, "disabled_reason" varchar NULL, "failing_since" timestamp with time zone NULL, "created_at" timestamp with time zone NOT NULL, "updated_at" timestamp with time zone NOT NULL);
CREATE INDEX "webhook_endpoints_workspace_id_index" ON "webhook_endpoints" ("workspace_id");
CREATE TABLE "webhook_deliveries" ("id" uuid NOT NULL PRIMARY KEY, "endpoint_id" uuid NOT NULL, "workspace_id" uuid NULL, "message_id" varchar NOT NULL, "event_type" varchar NOT NULL, "payload" text NOT NULL, "status" varchar NOT NULL, "attempts" integer NOT NULL, "max_attempts" integer NOT NULL, "next_attempt_at" timestamp with time zone NULL, "last_status_code" integer NULL, "last_error" text NULL, "created_at" timestamp with time zone NOT NULL, "updated_at" timestamp with time zone NOT NULL, FOREIGN KEY ("endpoint_id") REFERENCES "webhook_endpoints" ("id") ON DELETE CASCADE);
CREATE INDEX "webhook_deliveries_endpoint_created_index" ON "webhook_deliveries" ("endpoint_id", "created_at");
CREATE INDEX "webhook_deliveries_status_index" ON "webhook_deliveries" ("status");
CREATE INDEX "webhook_deliveries_created_at_index" ON "webhook_deliveries" ("created_at");
CREATE TABLE "webhook_attempts" ("id" uuid NOT NULL PRIMARY KEY, "delivery_id" uuid NOT NULL, "endpoint_id" uuid NOT NULL, "attempt" integer NOT NULL, "status_code" integer NULL, "succeeded" bool NOT NULL, "response_body" text NULL, "error" text NULL, "duration_ms" bigint NOT NULL, "created_at" timestamp with time zone NOT NULL, FOREIGN KEY ("delivery_id") REFERENCES "webhook_deliveries" ("id") ON DELETE CASCADE);
CREATE INDEX "webhook_attempts_delivery_id_index" ON "webhook_attempts" ("delivery_id");
CREATE TABLE "webhook_receipts" ("id" uuid NOT NULL PRIMARY KEY, "source" varchar NOT NULL, "message_id" varchar NOT NULL, "received_at" timestamp with time zone NOT NULL);
CREATE UNIQUE INDEX "webhook_receipts_source_message_unique" ON "webhook_receipts" ("source", "message_id");
CREATE INDEX "webhook_receipts_received_at_index" ON "webhook_receipts" ("received_at");
INSERT INTO "navree_migrations" ("module", "name", "batch", "applied_at") VALUES ('webhooks', 'm20261026_000001_create_webhook_tables', 1, now());
INSERT INTO "webhook_endpoints" VALUES ('0199a7a0-0000-7000-8000-00000000000e', NULL, 'https://hooks.example.com/in', NULL, '["*"]', '{}', 'whsec_test', NULL, NULL, TRUE, NULL, NULL, NULL, now(), now());
INSERT INTO "webhook_deliveries" VALUES
  ('0199a7a0-0000-7000-8000-0000000000d1', '0199a7a0-0000-7000-8000-00000000000e', NULL, 'msg_1', 'deal.won', '{}', 'delivered', 1, 8, NULL, 200, NULL, now() - INTERVAL '2 days', now()),
  ('0199a7a0-0000-7000-8000-0000000000d2', '0199a7a0-0000-7000-8000-00000000000e', NULL, 'msg_2', 'deal.won', '{}', 'delivered', 2, 8, NULL, 200, NULL, now() - INTERVAL '1 day', now());
INSERT INTO "webhook_attempts" VALUES
  (gen_random_uuid(), '0199a7a0-0000-7000-8000-0000000000d1', '0199a7a0-0000-7000-8000-00000000000e', 1, 200, TRUE, NULL, NULL, 12, now() - INTERVAL '2 days'),
  (gen_random_uuid(), '0199a7a0-0000-7000-8000-0000000000d2', '0199a7a0-0000-7000-8000-00000000000e', 1, 500, FALSE, NULL, NULL, 15, now() - INTERVAL '1 day'),
  (gen_random_uuid(), '0199a7a0-0000-7000-8000-0000000000d2', '0199a7a0-0000-7000-8000-00000000000e', 2, 200, TRUE, NULL, NULL, 11, now() - INTERVAL '23 hours');
"#;

    /// An app the previous release migrated, with deliveries and attempts:
    /// `migrate --pretend` plans the conversion and changes nothing, and
    /// `migrate` makes both tables hypertables with every row kept.
    #[tokio::test]
    async fn upgrading_converts_an_existing_delivery_log() {
        use navree::prelude::*;
        use navree::testing::TestApp;

        let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", false).await else {
            return;
        };
        // Every other module migrated as usual; the webhook tables as the
        // previous release left them.
        let before = TestApp::builder()
            .config("database.url", scratch.url.clone())
            .config("database.migrate_on_boot", true)
            .config("audit.routes", false)
            .config("fleet.dashboard", false)
            .config("lighthouse.dashboard", false)
            .module(DatabaseModule::from_config())
            .module(EventsModule::for_root())
            .module(QueueModule::from_config())
            .module(AuditModule::from_config())
            .module(FleetModule::from_config())
            .module(ScheduleModule::for_root())
            .module(LighthouseModule::from_config())
            .build()
            .await;
        let db = before.get::<Db>().await;
        if Schema::new(&db)
            .target()
            .await
            .unwrap()
            .timescaledb()
            .is_none()
        {
            eprintln!("TimescaleDB is not available on this server: skipping");
            before.shutdown().await.unwrap();
            scratch.drop().await;
            return;
        }
        db.conn()
            .execute_unprepared(PREVIOUS_WEBHOOK_TABLES)
            .await
            .unwrap();
        before.shutdown().await.unwrap();

        let app = app()
            .config("database.url", scratch.url.clone())
            .config("database.migrate_on_boot", false)
            .build()
            .await;
        let db = app.get::<Db>().await;
        let attempt_keys = || async {
            Schema::new(&db)
                .foreign_keys("webhook_attempts")
                .await
                .unwrap()
                .into_iter()
                .map(|key| key.foreign_table)
                .collect::<Vec<_>>()
        };
        app.run_command("migrate", &["--pretend"]).await.unwrap();
        assert_eq!(
            attempt_keys().await,
            ["webhook_deliveries"],
            "pretending changes nothing"
        );
        assert!(
            !Schema::new(&db)
                .is_hypertable("webhook_attempts")
                .await
                .unwrap()
        );
        assert!(
            !conversions_applied(&db)
                .await
                .iter()
                .any(|name| name.contains("webhook"))
        );

        app.run_command("migrate", &[]).await.unwrap();
        assert_eq!(conversions_applied(&db).await, CONVERSIONS);
        assert_eq!(attempt_keys().await, ["webhook_endpoints"]);
        let schema = Schema::new(&db);
        for table in ["webhook_deliveries", "webhook_attempts"] {
            assert!(schema.is_hypertable(table).await.unwrap(), "{table}");
        }
        assert_eq!(
            number(&db, "SELECT count(*) FROM webhook_deliveries").await,
            2
        );
        assert_eq!(
            number(
                &db,
                "SELECT count(*) FROM webhook_attempts WHERE delivery_id = '0199a7a0-0000-7000-8000-0000000000d2'"
            )
            .await,
            2,
            "the rows moved into chunks, links intact"
        );
        assert_eq!(
            number(
                &db,
                "SELECT count(*) FROM show_chunks('webhook_deliveries')"
            )
            .await,
            2
        );
        app.shutdown().await.unwrap();
        scratch.drop().await;
    }

    /// With `lighthouse.database_url`, Lighthouse creates its tables in that
    /// database at boot and makes the event tables hypertables there.
    #[tokio::test]
    async fn lighthouse_s_own_database_gets_hypertables_too() {
        use navree::db::DbOptions;

        let Some(app_db) = Scratch::new("NAVREE_TEST_POSTGRES_URL", false).await else {
            return;
        };
        let monitoring = Scratch::new("NAVREE_TEST_POSTGRES_URL", false)
            .await
            .expect("the variable is set");
        let lighthouse_db = Db::connect(&DbOptions {
            url: monitoring.url.clone(),
            ..DbOptions::default()
        })
        .await
        .unwrap();
        let target = Schema::new(&lighthouse_db).target().await.unwrap();
        if target.timescaledb().is_none() && target.timescaledb_installable().is_none() {
            eprintln!("TimescaleDB is not available on this server: skipping");
            app_db.drop().await;
            monitoring.drop().await;
            return;
        }
        let first = app()
            .config("database.url", app_db.url.clone())
            .config("lighthouse.database_url", monitoring.url.clone())
            .build()
            .await;
        let schema = Schema::new(&lighthouse_db);
        for (table, column) in [
            ("lighthouse_samples", "at"),
            ("lighthouse_occurrences", "at"),
            ("lighthouse_aggregates", "bucket"),
        ] {
            let dimension = schema
                .time_dimension(table)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{table} is a hypertable in Lighthouse's database"));
            assert_eq!(dimension.column, column, "{table}");
            assert_eq!(dimension.chunk_interval, "1 day", "{table}");
        }
        assert!(!schema.is_hypertable("lighthouse_issues").await.unwrap());
        first.shutdown().await.unwrap();
        // Booting again finds the hypertables and leaves them as they are.
        let again = app()
            .config("database.url", app_db.url.clone())
            .config("lighthouse.database_url", monitoring.url.clone())
            .build()
            .await;
        assert_eq!(
            number(
                &lighthouse_db,
                "SELECT count(*) FROM timescaledb_information.dimensions WHERE hypertable_name LIKE 'lighthouse%'"
            )
            .await,
            3
        );
        again.shutdown().await.unwrap();
        lighthouse_db.conn().clone().close().await.unwrap();
        app_db.drop().await;
        monitoring.drop().await;
    }

    /// The time-bucket queries of the TimescaleDB guide, on TimescaleDB:
    /// the same buckets as on SQLite, and `first` / `last`.
    #[tokio::test]
    async fn the_guide_s_time_bucket_queries() {
        use chrono::{DateTime, TimeZone};
        use navree::db::DbOptions;
        use navree::orm::{Expr, col, table};

        let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", false).await else {
            return;
        };
        let db = Db::connect(&DbOptions {
            url: scratch.url.clone(),
            ..DbOptions::default()
        })
        .await
        .unwrap();
        let schema = Schema::new(&db);
        let target = schema.target().await.unwrap();
        if target.timescaledb().is_none() && target.timescaledb_installable().is_none() {
            eprintln!("TimescaleDB is not available on this server: skipping");
            scratch.drop().await;
            return;
        }
        schema
            .create_hypertable("readings", "taken_at", |t| {
                t.id();
                t.double("celsius");
                t.timestamp_tz("taken_at");
            })
            .await
            .unwrap();
        assert!(schema.is_hypertable("readings").await.unwrap());
        let start = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
        for step in 0..9_i32 {
            db.conn()
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "INSERT INTO readings (celsius, taken_at) VALUES ($1, $2)",
                    [
                        (20.0 + f64::from(step)).into(),
                        (start + TimeDelta::minutes(10 * i64::from(step))).into(),
                    ],
                ))
                .await
                .unwrap();
        }
        let since = start - TimeDelta::days(1);
        let points = table("readings")
            .select_time_bucket("15 minutes", "taken_at", "bucket")
            .select_expr(Expr::avg(col("celsius")), "average")
            .select_expr(Expr::count_all(), "samples")
            .where_(col("taken_at").gte(since))
            .group_by(["bucket"])
            .order_by("bucket")
            .get(&db)
            .await
            .unwrap();
        let shown: Vec<String> = points
            .iter()
            .map(|point| {
                let bucket: DateTime<Utc> = point.get("bucket").unwrap();
                let average: f64 = point.get("average").unwrap();
                let samples: i64 = point.get("samples").unwrap();
                format!("{} {average:.1} {samples}", bucket.format("%H:%M"))
            })
            .collect();
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
            .order_by("day")
            .get(&db)
            .await
            .unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].get::<f64>("opening").unwrap(), 20.0);
        assert_eq!(days[0].get::<f64>("closing").unwrap(), 28.0);
        scratch.drop().await;
    }

    #[tokio::test]
    async fn every_time_series_table_is_a_hypertable() {
        let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", false).await else {
            return;
        };
        let app = app()
            .config("database.url", scratch.url.clone())
            .build()
            .await;
        let db = app.get::<Db>().await;
        let schema = Schema::new(&db);
        let target = schema.target().await.unwrap();
        if target.timescaledb().is_none() {
            eprintln!("TimescaleDB is not available on this server: skipping");
            app.shutdown().await.unwrap();
            scratch.drop().await;
            return;
        }
        assert_eq!(conversions_applied(&db).await, CONVERSIONS);
        let timestamp = "timestamp with time zone";
        for (table, column, kind, interval) in [
            ("audit_logs", "created_at", timestamp, "7 days"),
            ("webhook_deliveries", "created_at", timestamp, "1 day"),
            ("webhook_attempts", "created_at", timestamp, "1 day"),
            ("fleet_jobs", "finished_at", "bigint", "3600000"),
            ("fleet_metrics", "at", "bigint", "86400000"),
            ("schedule_runs", "tick", timestamp, "1 day"),
            ("lighthouse_samples", "at", timestamp, "1 day"),
            ("lighthouse_occurrences", "at", timestamp, "1 day"),
            ("lighthouse_aggregates", "bucket", timestamp, "1 day"),
        ] {
            let dimension = schema
                .time_dimension(table)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{table} is a hypertable"));
            assert_eq!(
                (
                    dimension.column.as_str(),
                    dimension.column_type.as_str(),
                    dimension.chunk_interval.as_str()
                ),
                (column, kind, interval),
                "{table}"
            );
        }
        // Unique keys gained the time column; the runs' key already had it.
        for (table, key) in [
            ("audit_logs", &["id", "created_at"][..]),
            ("webhook_deliveries", &["id", "created_at"][..]),
            ("webhook_attempts", &["id", "created_at"][..]),
            ("fleet_jobs", &["id", "finished_at"][..]),
            ("schedule_runs", &["task", "tick"][..]),
            ("lighthouse_aggregates", &["id", "bucket"][..]),
        ] {
            assert!(
                schema
                    .has_index(table, key, Some(IndexKind::Primary))
                    .await
                    .unwrap(),
                "{table}: PRIMARY KEY ({})",
                key.join(", ")
            );
        }
        assert_attempts_reference_endpoints(&db).await;
        for table in [
            "webhook_endpoints",
            "jobs",
            "lighthouse_issues",
            "fleet_masters",
        ] {
            assert!(!schema.is_hypertable(table).await.unwrap(), "{table}");
        }

        // The conversions run again harmlessly (after a rollback, say).
        let names = CONVERSIONS
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ");
        db.conn()
            .execute_unprepared(&format!(
                "DELETE FROM {MIGRATIONS_TABLE} WHERE name IN ({names})"
            ))
            .await
            .unwrap();
        app.run_command("migrate", &[]).await.unwrap();
        assert_eq!(conversions_applied(&db).await, CONVERSIONS);
        assert_eq!(
            number(&db, "SELECT count(*) FROM timescaledb_information.dimensions WHERE hypertable_name = 'audit_logs'").await,
            1
        );

        // `audit:prune` drops the weeks before the cutoff whole.
        let now = Utc::now();
        for weeks in [10, 9, 8, 1] {
            db.conn()
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "INSERT INTO audit_logs (id, kind, action, metadata, created_at) \
                     VALUES (gen_random_uuid(), 'auth', 'auth.login', '{}', $1)",
                    [(now - TimeDelta::weeks(weeks)).into()],
                ))
                .await
                .unwrap();
        }
        let chunks = || number(&db, "SELECT count(*) FROM show_chunks('audit_logs')");
        assert_eq!(chunks().await, 4);
        app.run_command("audit:prune", &["--days", "30"])
            .await
            .unwrap();
        assert_eq!(number(&db, "SELECT count(*) FROM audit_logs").await, 1);
        assert_eq!(chunks().await, 1, "three old weeks dropped whole");
        // The other prunes run on hypertables too.
        app.run_command("webhooks:prune", &[]).await.unwrap();
        app.run_command("lighthouse:prune", &[]).await.unwrap();

        app.shutdown().await.unwrap();
        scratch.drop().await;
    }
}

#[cfg(feature = "mysql")]
mod mysql {
    use navree::db::Db;

    use super::scratch::Scratch;
    use super::{
        CONVERSIONS, app, assert_attempts_reference_endpoints, assert_regular_tables,
        conversions_applied,
    };

    #[tokio::test]
    async fn mysql_keeps_regular_tables_with_time_indexes() {
        let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", true).await else {
            return;
        };
        let app = app()
            .config("database.url", scratch.url.clone())
            .build()
            .await;
        let db = app.get::<Db>().await;
        assert_eq!(conversions_applied(&db).await, CONVERSIONS);
        assert_regular_tables(&db).await;
        assert_attempts_reference_endpoints(&db).await;
        app.run_command("audit:prune", &[]).await.unwrap();
        app.run_command("webhooks:prune", &[]).await.unwrap();
        app.shutdown().await.unwrap();
        scratch.drop().await;
    }
}
